//! Instruction dispatch loop.
//!
//! The `execute` function is the main bytecode interpreter loop. It fetches
//! instructions from the current Proto's code array, decodes them, and
//! dispatches to the appropriate handler.
//!
//! Phase 3: no metamethods. Type mismatches produce runtime errors.
//! Metamethod dispatch is added in Phase 4.
//!
//! Reference: `lvm.c` `luaV_execute` in PUC-Rio Lua 5.1.1.

use crate::error::{LuaError, LuaResult, RuntimeError};

use crate::check_interrupted;

use super::callinfo::{CallInfo, LUA_MULTRET};
use super::closure::{Closure, LuaClosure, Upvalue};
use super::debug_info;
use super::gc::arena::GcRef;
use super::instructions::{Instruction, LFIELDS_PER_FLUSH, OpCode, index_k, is_k};
use super::metatable::{MAXTAGLOOP, TMS, get_comp_tm, gettmbyobj, val_raw_equal};
use super::proto::ProtoRef;
use super::proto::{Proto, VARARG_ISVARARG, VARARG_NEEDSARG};
use super::state::{
    Gc, LUA_MINSTACK, LuaState, MASK_CALL, MASK_COUNT, MASK_LINE, MASK_RET, MAXCALLS, MAXCCALLS,
};
use super::table::Table;
use super::value::{Userdata, Val};

use crate::platform::{localeconv, strcoll, strtod};

/// Calls libc `strtod` on a NUL-terminated buffer.
///
/// Returns `Some((value, bytes_consumed))` on success, `None` if no
/// conversion was performed (endptr == nptr).
#[allow(unsafe_code)]
pub(crate) fn libc_strtod(s: &[u8]) -> Option<(f64, usize)> {
    // strtod requires NUL-terminated input.
    let mut buf = Vec::with_capacity(s.len() + 1);
    buf.extend_from_slice(s);
    buf.push(0);

    let mut endptr: *mut u8 = std::ptr::null_mut();
    // SAFETY: buf is a valid NUL-terminated C string, endptr is a valid pointer.
    let result = unsafe { strtod(buf.as_ptr(), &raw mut endptr) };
    let consumed = endptr as usize - buf.as_ptr() as usize;
    if consumed == 0 {
        return None;
    }
    Some((result, consumed))
}

/// Returns the locale's decimal point character (from `localeconv()`).
/// Falls back to `'.'` if unavailable.
#[allow(unsafe_code)]
pub(crate) fn locale_decimal_point() -> u8 {
    // SAFETY: localeconv() returns a pointer to a static struct.
    let lc = unsafe { localeconv() };
    if lc.is_null() {
        return b'.';
    }
    let dp = unsafe { (*lc).decimal_point };
    if dp.is_null() {
        return b'.';
    }
    let ch = unsafe { *dp };
    if ch == 0 { b'.' } else { ch }
}

// ---------------------------------------------------------------------------
// Helper: RK resolution
// ---------------------------------------------------------------------------

/// Resolves an RK field: if bit 256 is set, returns the constant at the
/// masked index; otherwise returns the register value.
#[inline]
fn rk(stack: &[Val], base: usize, constants: &[Val], field: u32) -> Val {
    if is_k(field) {
        constants[index_k(field) as usize]
    } else {
        stack[base + field as usize]
    }
}

// ---------------------------------------------------------------------------
// Helper: numeric coercion
// ---------------------------------------------------------------------------

/// Attempts to coerce a value to a number (for arithmetic).
///
/// Numbers pass through. Strings are parsed via Lua 5.1 rules
/// (decimal, hex with `0x` prefix, leading/trailing whitespace OK).
/// Returns `None` if the value is not coercible.
pub(crate) fn coerce_to_number(val: Val, gc: &Gc) -> Option<f64> {
    match val {
        Val::Num(n) => Some(n),
        Val::Str(r) => {
            let s = gc.string_arena.get(r)?;
            str_to_number(s.data())
        }
        _ => None,
    }
}

/// Parse a byte slice as a Lua number (matching PUC-Rio's `luaO_str2d`).
///
/// Uses libc `strtod` for locale-aware decimal parsing. Supports hex
/// (`0x`/`0X` prefix) via `strtoul`, and leading/trailing whitespace.
pub(crate) fn str_to_number(data: &[u8]) -> Option<f64> {
    // Skip leading whitespace.
    let start = data.iter().position(|&b| !b.is_ascii_whitespace())?;
    let trimmed = &data[start..];
    if trimmed.is_empty() {
        return None;
    }

    // Use strtod (locale-aware).
    let (result, consumed) = libc_strtod(trimmed)?;

    // Check for hex constant: strtod may have stopped at 'x'/'X'.
    let rest = &trimmed[consumed..];
    if !rest.is_empty() && (rest[0] == b'x' || rest[0] == b'X') {
        // Hex: parse with strtoul (PUC-Rio fallback).
        let hex_str = std::str::from_utf8(trimmed).ok()?;
        let hex_trimmed = hex_str.trim();
        let (sign, hex_part) = if let Some(rest) = hex_trimmed.strip_prefix('-') {
            (-1.0_f64, rest)
        } else if let Some(rest) = hex_trimmed.strip_prefix('+') {
            (1.0, rest)
        } else {
            (1.0, hex_trimmed)
        };
        let hex_part = hex_part
            .strip_prefix("0x")
            .or_else(|| hex_part.strip_prefix("0X"))?;
        let val = u64::from_str_radix(hex_part, 16).ok()?;
        return Some(sign * val as f64);
    }

    // Check that remaining characters are only whitespace.
    if rest.iter().all(u8::is_ascii_whitespace) {
        Some(result)
    } else {
        None
    }
}

/// Coerces a value to a string for concatenation.
///
/// Numbers are formatted using Lua's %.14g rules. Strings pass through.
/// Returns `None` for other types.
#[allow(dead_code)] // Used in Phase 4b (tostring metamethod support)
fn coerce_to_string(val: Val, gc: &mut Gc) -> Option<Val> {
    match val {
        Val::Str(_) => Some(val),
        Val::Num(_) => {
            // Format the number as a string.
            let formatted = format!("{val}");
            let r = gc.intern_string(formatted.as_bytes());
            Some(Val::Str(r))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helper: runtime error construction
// ---------------------------------------------------------------------------

/// Creates a runtime error with source location from the current proto.
fn runtime_error(proto: &Proto, pc: usize, message: &str) -> LuaError {
    let line = if pc > 0 && pc <= proto.line_info.len() {
        proto.line_info[pc - 1]
    } else if !proto.line_info.is_empty() {
        proto.line_info[0]
    } else {
        0
    };
    let source = chunkid(&proto.source);
    LuaError::Runtime(RuntimeError {
        message: format!("{source}:{line}: {message}"),
        level: 0,
        traceback: vec![],
    })
}

/// Type error with variable name resolution.
///
/// Matches PUC-Rio's `luaG_typeerror`. Uses `getobjname` to find the
/// variable name and kind (local/global/field/upvalue/method) for the
/// value at register `reg` (relative to base, so the stack position
/// is `base + reg`). Produces messages like:
/// - `"attempt to call local 'x' (a number value)"`
/// - `"attempt to index a nil value"`
fn type_error(
    state: &LuaState,
    proto: &Proto,
    pc: usize,
    base: usize,
    reg: usize,
    opname: &str,
) -> LuaError {
    let val = state.stack_get(base + reg);
    let type_name = val.type_name();
    // pc is already incremented past the current instruction, so use pc-1.
    let current_pc = pc.saturating_sub(1);
    let name_info = debug_info::getobjname(
        proto,
        current_pc,
        #[allow(clippy::cast_possible_truncation)]
        (reg as u32),
        &state.gc.string_arena,
    );
    let message = if let Some((kind, name)) = name_info {
        format!("attempt to {opname} {kind} '{name}' (a {type_name} value)")
    } else {
        format!("attempt to {opname} a {type_name} value")
    };
    runtime_error(proto, pc, &message)
}

/// Arithmetic type error with RK-aware operand resolution.
///
/// Matches PUC-Rio's `luaG_aritherror`: identifies which operand
/// caused the error (first non-numeric), then calls `type_error`
/// if the operand is in a register (not a constant).
#[allow(clippy::too_many_arguments)]
fn arith_error(
    state: &LuaState,
    proto: &Proto,
    pc: usize,
    base: usize,
    lhs: Val,
    rhs: Val,
    b_rk: u32,
    c_rk: u32,
) -> LuaError {
    // PUC-Rio: if first can't convert, error on first operand.
    let rk = if coerce_to_number(lhs, &state.gc).is_none() {
        b_rk
    } else {
        c_rk
    };
    // If the problematic operand is a constant, we can't resolve a variable
    // name. Otherwise use type_error for register-based name resolution.
    if is_k(rk) {
        let val = if coerce_to_number(lhs, &state.gc).is_none() {
            lhs
        } else {
            rhs
        };
        runtime_error(
            proto,
            pc,
            &format!(
                "attempt to perform arithmetic on a {} value",
                val.type_name()
            ),
        )
    } else {
        type_error(state, proto, pc, base, rk as usize, "perform arithmetic on")
    }
}

/// Type error for comparison (no variable names — PUC-Rio doesn't use them).
fn compare_error(proto: &Proto, pc: usize, left: Val, right: Val) -> LuaError {
    let message = if left.type_name() == right.type_name() {
        format!("attempt to compare two {} values", left.type_name())
    } else {
        format!(
            "attempt to compare {} with {}",
            left.type_name(),
            right.type_name()
        )
    };
    runtime_error(proto, pc, &message)
}

/// luaO_fb2int: convert a "floating point byte" to an integer.
///
/// Used by NEWTABLE to decode size hints. The format encodes
/// `(x & 7 + 8) << ((x >> 3) - 1)` for non-zero exponent,
/// or `x` directly when the exponent is zero.
pub(crate) fn fb2int(x: u32) -> u32 {
    let e = (x >> 3) & 31;
    if e == 0 { x } else { ((x & 7) + 8) << (e - 1) }
}

// ---------------------------------------------------------------------------
// Call machinery
// ---------------------------------------------------------------------------

/// Result of `precall`: what should the caller do next?
pub enum CallResult {
    /// The called function is a Lua function. The caller should enter
    /// or re-enter the execute loop.
    Lua,
    /// The called function was a Rust function and has already returned.
    /// Results are in place on the stack.
    Rust,
}

impl LuaState {
    /// Checks `call_depth` against the two-threshold stack overflow model.
    ///
    /// PUC-Rio `luaD_call` uses two thresholds:
    /// - `LUAI_MAXCCALLS` (200): throw recoverable "stack overflow"
    /// - `LUAI_MAXCCALLS + LUAI_MAXCCALLS/8` (225): unrecoverable error
    ///   Calls between 201-224 are allowed as headroom for error handlers.
    fn check_stack_overflow(&self) -> LuaResult<()> {
        if self.call_depth >= MAXCCALLS {
            let hard_limit = MAXCCALLS + (MAXCCALLS >> 3);
            if self.call_depth >= hard_limit {
                return Err(runtime_error_simple("stack overflow"));
            }
            if self.call_depth == MAXCCALLS {
                let where_prefix = get_where(self, 0);
                return Err(LuaError::Runtime(RuntimeError {
                    message: format!("{where_prefix}stack overflow"),
                    level: 0,
                    traceback: vec![],
                }));
            }
        }
        Ok(())
    }

    /// Calls a function at `func_idx` with call depth and yield boundary tracking.
    ///
    /// This is the rilua equivalent of PUC-Rio's `luaD_call()`. It wraps
    /// `precall` + `execute` with depth increment/decrement. Used by
    /// stdlib functions (pcall, table.sort, etc.) and metamethods to call
    /// Lua code.
    ///
    /// Increments both counters:
    /// - `call_depth`: stack overflow detection (checked by `check_stack_overflow`)
    /// - `n_ccalls`: yield boundary (`coroutine.yield()` blocked when > 0)
    pub fn call_function(&mut self, func_idx: usize, num_results: i32) -> LuaResult<()> {
        self.n_ccalls += 1;
        self.call_depth += 1;
        let result = (|| {
            self.check_stack_overflow()?;
            match self.precall(func_idx, num_results)? {
                CallResult::Lua => execute(self),
                CallResult::Rust => Ok(()),
            }
        })();
        self.call_depth -= 1;
        self.n_ccalls -= 1;
        result
    }

    /// Invokes the active hook function with (event_name, line_or_nil).
    ///
    /// Matches PUC-Rio's `luaD_callhook()` in `ldo.c`:
    /// - Saves and restores `top` and `ci.top`
    /// - Sets `allow_hook = false` to prevent recursive hook calls
    /// - Pushes the hook function, event string, and line number (or nil)
    /// - Calls the hook with 2 arguments and 0 results
    pub fn callhook(&mut self, event: &str, line: i32) -> LuaResult<()> {
        if !self.hook.allow_hook {
            return Ok(());
        }
        let hook_func = self.hook.hook_func;
        if hook_func.is_nil() {
            return Ok(());
        }

        // Save top and ci.top (hook may modify the stack).
        let saved_top = self.top;
        let saved_ci_top = self.call_stack[self.ci].top;

        // Ensure minimum stack for the hook call.
        self.ensure_stack(self.top + LUA_MINSTACK);
        self.call_stack[self.ci].top = self.top + LUA_MINSTACK;

        // Disable hooks during the callback (PUC-Rio: L->allowhook = 0).
        self.hook.allow_hook = false;

        let call_base = self.top;
        let event_ref = self.gc.intern_string(event.as_bytes());
        self.stack_set(call_base, hook_func);
        self.stack_set(call_base + 1, Val::Str(event_ref));
        if line >= 0 {
            #[allow(clippy::cast_precision_loss)]
            self.stack_set(call_base + 2, Val::Num(f64::from(line)));
        } else {
            self.stack_set(call_base + 2, Val::Nil);
        }
        self.top = call_base + 3;

        let result = self.call_function(call_base, 0);

        // Restore state (PUC-Rio: L->allowhook = 1, restore top/ci.top).
        self.hook.allow_hook = true;
        self.call_stack[self.ci].top = saved_ci_top;
        self.top = saved_top;

        result
    }

    /// Sets up a call frame for the function at `func_idx`.
    ///
    /// For Lua functions: pushes a new CallInfo and returns `CallResult::Lua`.
    /// For Rust functions: executes the function, calls `poscall`, and
    /// returns `CallResult::Rust`.
    ///
    /// `num_results` is the number of results the caller expects, or
    /// `LUA_MULTRET` (-1) for all results.
    pub fn precall(&mut self, func_idx: usize, num_results: i32) -> LuaResult<CallResult> {
        // Check total call depth (Lua + Rust). Matches PUC-Rio's `growCI`:
        // - At MAXCALLS (20,000): throw recoverable "stack overflow"
        //   (error handlers like debug.traceback can still run)
        // - Above MAXCALLS: allow calls (headroom for error handling)
        // - At 2*MAXCALLS (40,000): unrecoverable overflow
        //
        // PUC-Rio achieves this by doubling the CI array: first overflow
        // at 20k doubles to 40k capacity. Second overflow at 40k is
        // unrecoverable. We track with `ci_overflow`: false below limit,
        // true once past MAXCALLS. Cleared when pcall/xpcall restores ci.
        let next_ci = self.ci + 1;
        if self.ci_overflow {
            // Already in overflow. Allow headroom but cap at 2*MAXCALLS.
            if next_ci >= MAXCALLS * 2 {
                return Err(runtime_error_simple("stack overflow"));
            }
        } else if next_ci >= MAXCALLS {
            // First overflow. Allow the CI to grow past MAXCALLS for
            // error handlers, but throw a recoverable error.
            self.ci_overflow = true;
            let where_prefix = get_where(self, 0);
            return Err(LuaError::Runtime(RuntimeError {
                message: format!("{where_prefix}stack overflow"),
                level: 0,
                traceback: vec![],
            }));
        }

        let func_val = self.stack_get(func_idx);
        let closure_ref = if let Val::Function(r) = func_val {
            r
        } else {
            // Try __call metamethod.
            let tm = get_tm_for_val(&self.gc, func_val, TMS::Call);
            match tm {
                Some(tm_val) if matches!(tm_val, Val::Function(_)) => {
                    // Shift stack up to insert __call at func position.
                    // The original value becomes the first argument.
                    let top = self.top;
                    self.ensure_stack(top + 1);
                    let mut p = top;
                    while p > func_idx {
                        let v = self.stack_get(p - 1);
                        self.stack_set(p, v);
                        p -= 1;
                    }
                    self.top = top + 1;
                    self.stack_set(func_idx, tm_val);
                    // Now func_idx points to __call, original value is at func_idx+1.
                    match tm_val {
                        Val::Function(r) => r,
                        _ => unreachable!(),
                    }
                }
                _ => {
                    // Try to resolve the variable name from the caller frame.
                    let ci = &self.call_stack[self.ci];
                    let caller_func = self.stack_get(ci.func);
                    let err = if let Val::Function(r) = caller_func {
                        if let Some(Closure::Lua(lcl)) = self.gc.closures.get(r) {
                            let reg = func_idx - ci.base;
                            type_error(self, &lcl.proto, ci.saved_pc, ci.base, reg, "call")
                        } else {
                            LuaError::Runtime(RuntimeError {
                                message: format!(
                                    "attempt to call a {} value",
                                    func_val.type_name()
                                ),
                                level: 0,
                                traceback: vec![],
                            })
                        }
                    } else {
                        LuaError::Runtime(RuntimeError {
                            message: format!("attempt to call a {} value", func_val.type_name()),
                            level: 0,
                            traceback: vec![],
                        })
                    };
                    return Err(err);
                }
            }
        };

        // Save caller's PC.
        let saved_pc = self.call_stack[self.ci].saved_pc;
        let _ = saved_pc; // used for restoration in poscall

        let closure = self
            .gc
            .closures
            .get(closure_ref)
            .ok_or_else(|| runtime_error_simple("invalid function reference"))?;

        match closure {
            Closure::Lua(lua_cl) => {
                let proto = ProtoRef::clone(&lua_cl.proto);
                let num_params = proto.num_params as usize;
                let max_stack = proto.max_stack_size as usize;
                let is_vararg = proto.is_vararg & VARARG_ISVARARG != 0;

                // Ensure enough stack space.
                self.ensure_stack(func_idx + max_stack + 1);

                let nargs = self.get_nargs(func_idx);

                let new_base;
                if is_vararg {
                    new_base = self.adjust_varargs(&proto, nargs, func_idx);
                } else {
                    new_base = func_idx + 1;
                    // Trim excess args or pad with nil.
                    if nargs > num_params {
                        self.top = new_base + num_params;
                    } else {
                        // Pad with nil for missing params.
                        while self.top < new_base + num_params {
                            self.push(Val::Nil);
                        }
                    }
                }

                // Initialize remaining locals to nil.
                let ci_top = new_base + max_stack;
                for i in self.top..ci_top {
                    self.stack_set(i, Val::Nil);
                }
                self.top = ci_top;

                // Push new CallInfo.
                let mut ci = CallInfo::new(func_idx, new_base, ci_top, num_results);
                ci.is_lua = true;
                self.push_ci(ci);
                self.base = new_base;

                // Fire call hook (PUC-Rio: luaD_precall lines 299-303).
                // Hooks expect savedpc to point past the first instruction.
                if self.hook.hook_mask & MASK_CALL != 0 {
                    self.call_stack[self.ci].saved_pc += 1;
                    self.callhook("call", -1)?;
                    self.call_stack[self.ci].saved_pc =
                        self.call_stack[self.ci].saved_pc.saturating_sub(1);
                }

                Ok(CallResult::Lua)
            }
            Closure::Rust(rust_cl) => {
                let func = rust_cl.func;
                // All Rust functions in rilua are built-in library functions.

                // Ensure minimum stack for Rust functions.
                self.ensure_stack(self.top + LUA_MINSTACK);

                let ci_top = self.top + LUA_MINSTACK;
                let ci = CallInfo::new(func_idx, func_idx + 1, ci_top, num_results);
                self.push_ci(ci);
                self.base = func_idx + 1;

                // Note: n_ccalls is NOT incremented here. The C-call boundary
                // counter is managed by call_function() (the luaD_call equivalent).
                // This matches PUC-Rio where luaD_precall does not touch nCcalls.

                // Fire call hook (PUC-Rio: luaD_precall lines 316-317).
                if self.hook.hook_mask & MASK_CALL != 0 {
                    self.callhook("call", -1)?;
                }
                // Execute the Rust function.
                let my_ci = self.ci;
                let n_results = match func(self) {
                    Ok(n) => n,
                    // PUC-Rio's library functions raise errors through
                    // `luaL_error`, which prefixes the position of the
                    // calling Lua function (`luaL_where(L, 1)`). Do the same
                    // for errors that originate in a built-in function
                    // (frame still current, no thrown error object, not a
                    // VM-style "attempt to ..." error).
                    Err(LuaError::Runtime(mut e))
                        if self.ci == my_ci
                            && self.error_object.is_none()
                            && !e.message.starts_with("attempt to ")
                            && !e.message.starts_with("loop in ") =>
                    {
                        let where_prefix = get_where(self, 1);
                        if !where_prefix.is_empty() && !e.message.starts_with(&where_prefix) {
                            e.message.insert_str(0, &where_prefix);
                        }
                        return Err(LuaError::Runtime(e));
                    }
                    Err(e) => return Err(e),
                };
                // Move results into place.
                let first_result = self.top - n_results as usize;
                self.poscall(first_result);

                Ok(CallResult::Rust)
            }
        }
    }

    /// Unwinds a call frame and moves results to the caller's frame.
    ///
    /// `first_result` is the stack index of the first return value.
    /// Returns `true` if the caller is a Lua function (execution should
    /// continue in the caller's frame).
    pub fn poscall(&mut self, mut first_result: usize) -> bool {
        // Fire return hook before unwinding (PUC-Rio: luaD_poscall line 346).
        // callrethooks fires LUA_HOOKRET, then LUA_HOOKTAILRET for each
        // elided tail call. The hook may reallocate the stack, so
        // first_result is saved/restored as an offset.
        if self.hook.hook_mask & MASK_RET != 0 {
            let fr_offset = first_result;
            let _ = self.callhook("return", -1);
            // Handle tail return hooks (PUC-Rio: callrethooks lines 335-336).
            let tail_calls = self.call_stack[self.ci].tail_calls;
            for _ in 0..tail_calls {
                let _ = self.callhook("tail return", -1);
            }
            first_result = fr_offset;
        }

        // Pop the current CallInfo.
        let ci_func = self.call_stack[self.ci].func;
        let wanted = self.call_stack[self.ci].num_results;
        self.pop_ci();

        // Restore caller's base.
        self.base = self.call_stack[self.ci].base;

        // Move results to where the function was.
        let mut res = ci_func;
        if wanted == LUA_MULTRET {
            // Move all available results.
            let mut src = first_result;
            while src < self.top {
                self.stack_set(res, self.stack_get(src));
                res += 1;
                src += 1;
            }
        } else {
            let mut moved = 0i32;
            let mut src = first_result;
            while moved < wanted && src < self.top {
                self.stack_set(res, self.stack_get(src));
                res += 1;
                src += 1;
                moved += 1;
            }
            // Pad with nil if not enough results.
            while moved < wanted {
                self.stack_set(res, Val::Nil);
                res += 1;
                moved += 1;
            }
        }
        self.top = res;

        // Return true if the caller requested fixed results (not MULTRET).
        // Matches PUC-Rio: `return (L->nresults - LUA_MULTRET)` which is
        // non-zero when nresults != LUA_MULTRET. The caller uses this to
        // decide whether to reset top to the frame's max (fixed results)
        // or leave it as-is (MULTRET, so the next operation can read the
        // actual result count from top).
        wanted != LUA_MULTRET
    }

    /// Adjusts the stack for a vararg function call.
    ///
    /// Matches PUC-Rio's `adjust_varargs` in `ldo.c`:
    /// 1. Pads actual args with nil if fewer than num_params
    /// 2. Copies fixed params from their original positions to above the vararg area
    /// 3. Nils out the original fixed param positions
    ///
    /// Returns the new base (pointing to the first fixed param copy).
    fn adjust_varargs(&mut self, proto: &Proto, nargs: usize, func_idx: usize) -> usize {
        let num_params = proto.num_params as usize;

        // Pad with nil if actual < num_params.
        let mut actual = nargs;
        while actual < num_params {
            self.push(Val::Nil);
            actual += 1;
        }

        // LUA_COMPAT_VARARG: create 'arg' table if NEEDSARG is set.
        // The table contains the extra (vararg) arguments with an 'n' field.
        let arg_table = if proto.is_vararg & VARARG_NEEDSARG != 0 {
            let nvar = actual - num_params; // Number of extra arguments.
            let fixed = self.top - actual;
            let mut tbl = Table::new();
            for i in 0..nvar {
                let val = self.stack_get(fixed + num_params + i);
                #[allow(clippy::cast_precision_loss)]
                let _ = tbl.raw_set(Val::Num((i + 1) as f64), val, &self.gc.string_arena);
            }
            let n_key = self.gc.intern_string(b"n");
            #[allow(clippy::cast_precision_loss)]
            let _ = tbl.raw_set(
                Val::Str(n_key),
                Val::Num(nvar as f64),
                &self.gc.string_arena,
            );
            Some(self.gc.alloc_table(tbl))
        } else {
            None
        };

        // `fixed` = first original argument position.
        let fixed = self.top - actual;
        // `base` = where the fixed param copies will start (above all args).
        let new_base = self.top;

        // Ensure enough stack space for the copies + optional 'arg' table.
        self.ensure_stack(new_base + num_params + 1);

        // Copy fixed params to above varargs, nil out originals.
        for i in 0..num_params {
            let val = self.stack_get(fixed + i);
            self.stack_set(self.top, val);
            self.top += 1;
            self.stack_set(fixed + i, Val::Nil);
        }

        // Push 'arg' table as an extra parameter if needed.
        if let Some(tbl_ref) = arg_table {
            self.stack_set(self.top, Val::Table(tbl_ref));
            self.top += 1;
        }

        // Stack layout now:
        // [func] [nil...] [vararg1] [vararg2] ... [param1] [param2] [arg?]
        //         ^ fixed positions nilled out       ^ base (new_base)
        let _ = func_idx; // func_idx not needed; `fixed` is derived from top

        new_base
    }

    /// Finds or creates an open upvalue for the given stack index.
    ///
    /// The open_upvalues list is maintained sorted by stack index
    /// (descending). If an upvalue for this index already exists,
    /// it is reused.
    pub fn find_upvalue(&mut self, stack_index: usize) -> super::gc::arena::GcRef<Upvalue> {
        // Search for existing open upvalue at this stack index.
        for &uv_ref in &self.open_upvalues {
            if let Some(uv) = self.gc.upvalues.get(uv_ref)
                && let Some(idx) = uv.stack_index()
            {
                if idx == stack_index {
                    return uv_ref;
                }
                if idx < stack_index {
                    break; // List is sorted descending; won't find it.
                }
            }
        }

        // Create new upvalue and insert in sorted position.
        let uv = Upvalue::new_open(stack_index);
        let uv_ref = self.gc.alloc_upvalue(uv);

        // Find insertion point (maintain descending order by stack_index).
        let pos = self
            .open_upvalues
            .iter()
            .position(|&r| {
                self.gc
                    .upvalues
                    .get(r)
                    .and_then(super::closure::Upvalue::stack_index)
                    .is_none_or(|idx| idx < stack_index)
            })
            .unwrap_or(self.open_upvalues.len());

        self.open_upvalues.insert(pos, uv_ref);
        uv_ref
    }

    /// Closes all open upvalues at or above the given stack level.
    ///
    /// Copies the value from the stack into the upvalue's own storage,
    /// transitioning it from Open to Closed state.
    pub fn close_upvalues(&mut self, level: usize) {
        while let Some(&uv_ref) = self.open_upvalues.first() {
            let should_close = self
                .gc
                .upvalues
                .get(uv_ref)
                .and_then(super::closure::Upvalue::stack_index)
                .is_some_and(|idx| idx >= level);

            if !should_close {
                break;
            }

            // Close the upvalue: captures current stack value.
            if let Some(uv) = self.gc.upvalues.get_mut(uv_ref) {
                uv.close(&self.stack);
            }

            // Write barrier: upvalue now holds the captured value.
            // Read back the closed value for the barrier check.
            let captured_val = self
                .gc
                .upvalues
                .get(uv_ref)
                .map_or(Val::Nil, |uv| uv.get(&self.stack));
            let uv_color = self
                .gc
                .upvalues
                .color(uv_ref)
                .unwrap_or(crate::vm::gc::Color::White0);
            self.gc.barrier_forward_val(uv_color, captured_val);

            self.open_upvalues.remove(0);
        }
    }
}

/// Simple runtime error without source location.
fn runtime_error_simple(message: &str) -> LuaError {
    LuaError::Runtime(RuntimeError {
        message: message.to_string(),
        level: 0,
        traceback: vec![],
    })
}

use crate::error::chunkid;

/// Gets "source:line: " for a given call stack level.
///
/// Level 0 is the currently running function, level 1 is the caller, etc.
/// Returns an empty string if the level doesn't exist or has no source info.
///
/// Matches PUC-Rio's `luaL_where`.
pub(crate) fn get_where(state: &LuaState, level: u32) -> String {
    let level = level as usize;
    if state.ci < level {
        return String::new();
    }

    let target_ci = state.ci - level;
    let ci = &state.call_stack[target_ci];
    let func_val = state.stack_get(ci.func);

    if let Val::Function(r) = func_val
        && let Some(Closure::Lua(lcl)) = state.gc.closures.get(r)
    {
        let proto = &lcl.proto;
        let pc = ci.saved_pc;
        let line = if pc > 0 && pc <= proto.line_info.len() {
            proto.line_info[pc - 1]
        } else if !proto.line_info.is_empty() {
            proto.line_info[0]
        } else {
            return String::new();
        };
        let short_src = chunkid(&proto.source);
        return format!("{short_src}:{line}: ");
    }

    String::new()
}

// ---------------------------------------------------------------------------
// Metamethod helpers
// ---------------------------------------------------------------------------

/// Looks up a metamethod for the given value.
///
/// Returns the metamethod value if found, or `None`.
fn get_tm_for_val(gc: &Gc, val: Val, event: TMS) -> Option<Val> {
    gettmbyobj(
        val,
        event,
        &gc.tables,
        &gc.string_arena,
        &gc.type_metatables,
        &gc.tm_names,
        &gc.userdata,
    )
}

/// Call a metamethod with 2 arguments, storing 1 result.
///
/// Used by arithmetic and comparison metamethods.
/// Stack layout: push tm, push arg1, push arg2, call, read result.
fn call_tm_res(
    state: &mut LuaState,
    tm: Val,
    arg1: Val,
    arg2: Val,
    result_reg: usize,
) -> LuaResult<()> {
    let call_base = state.top;
    state.ensure_stack(call_base + 4);
    state.stack_set(call_base, tm);
    state.stack_set(call_base + 1, arg1);
    state.stack_set(call_base + 2, arg2);
    state.top = call_base + 3;

    state.call_function(call_base, 1)?;

    // Read the result (poscall placed it at call_base).
    let result = state.stack_get(call_base);
    state.stack_set(result_reg, result);
    Ok(())
}

/// Call a metamethod with 3 arguments, no result stored.
///
/// Used by `__newindex` metamethod.
fn call_tm_void(state: &mut LuaState, tm: Val, arg1: Val, arg2: Val, arg3: Val) -> LuaResult<()> {
    let call_base = state.top;
    state.ensure_stack(call_base + 5);
    state.stack_set(call_base, tm);
    state.stack_set(call_base + 1, arg1);
    state.stack_set(call_base + 2, arg2);
    state.stack_set(call_base + 3, arg3);
    state.top = call_base + 4;

    state.call_function(call_base, 0)?;

    Ok(())
}

/// Try a binary metamethod on left operand, then right.
///
/// Looks up `event` in lhs's metatable. If not found, tries rhs's.
/// Calls the found metamethod with (lhs, rhs) and stores the result.
/// Returns an error if neither side has the metamethod.
///
/// `b_rk` and `c_rk` are the raw B/C instruction fields, used to
/// resolve variable names in error messages via `arith_error`.
#[allow(clippy::too_many_arguments)]
fn call_bin_tm(
    state: &mut LuaState,
    lhs: Val,
    rhs: Val,
    result_reg: usize,
    event: TMS,
    proto: &Proto,
    pc: usize,
    base: usize,
    b_rk: u32,
    c_rk: u32,
) -> LuaResult<()> {
    let tm =
        get_tm_for_val(&state.gc, lhs, event).or_else(|| get_tm_for_val(&state.gc, rhs, event));

    match tm {
        Some(tm_val) => call_tm_res(state, tm_val, lhs, rhs, result_reg),
        None => Err(arith_error(state, proto, pc, base, lhs, rhs, b_rk, c_rk)),
    }
}

/// Try an order (comparison) metamethod.
///
/// Matches PUC-Rio's `call_orderTM`: looks up the event on the left
/// operand first, then checks that the right operand has the SAME
/// metamethod (raw equality). Returns `None` if no matching metamethod
/// exists, `Some(bool)` with the truthiness of the call result.
fn call_order_tm(state: &mut LuaState, lhs: Val, rhs: Val, event: TMS) -> LuaResult<Option<bool>> {
    let tm1 = get_tm_for_val(&state.gc, lhs, event);
    let Some(tm1_val) = tm1 else {
        return Ok(None);
    };

    let tm2 = get_tm_for_val(&state.gc, rhs, event);
    let tm2_val = tm2.unwrap_or(Val::Nil);

    // Both operands must have the same metamethod (raw equality).
    if !val_raw_equal(tm1_val, tm2_val, &state.gc.tables, &state.gc.string_arena) {
        return Ok(None);
    }

    let call_base = state.top;
    state.ensure_stack(call_base + 4);
    state.stack_set(call_base, tm1_val);
    state.stack_set(call_base + 1, lhs);
    state.stack_set(call_base + 2, rhs);
    state.top = call_base + 3;

    state.call_depth += 1;
    let cmp_result = (|| {
        state.check_stack_overflow()?;
        match state.precall(call_base, 1)? {
            CallResult::Lua => execute(state),
            CallResult::Rust => Ok(()),
        }
    })();
    state.call_depth -= 1;
    cmp_result?;

    let result = state.stack_get(call_base);
    Ok(Some(result.is_truthy()))
}

// ---------------------------------------------------------------------------
// String concatenation with metamethod support
// ---------------------------------------------------------------------------

/// Concatenate registers `base+b` through `base+c`, storing the result
/// in `base+b`. Matches PUC-Rio's `luaV_concat`.
///
/// Processes pairs right-to-left. For each pair, tries to coerce both to
/// strings. If coercion fails, tries the `__concat` metamethod. Coalesces
/// consecutive string/number values into a single buffer for efficiency.
fn vm_concat(
    state: &mut LuaState,
    base: usize,
    b: usize,
    c: usize,
    proto: &Proto,
    pc: usize,
) -> LuaResult<()> {
    let mut total = c - b + 1; // number of values remaining
    let mut last = c; // rightmost index (relative to base)

    while total > 1 {
        let top = base + last + 1;
        let lhs = state.stack_get(top - 2);
        let rhs = state.stack_get(top - 1);

        if !is_string_or_number(lhs, &state.gc) || !is_string_or_number(rhs, &state.gc) {
            // Try __concat metamethod on the pair.
            let tm = get_tm_for_val(&state.gc, lhs, TMS::Concat)
                .or_else(|| get_tm_for_val(&state.gc, rhs, TMS::Concat));
            if let Some(tm_val) = tm {
                call_tm_res(state, tm_val, lhs, rhs, top - 2)?;
            } else {
                // No metamethod -- find which operand is the problem.
                // lhs is at top-2 (register last-1), rhs at top-1 (register last).
                let reg = if is_string_or_number(lhs, &state.gc) {
                    last
                } else {
                    last - 1
                };
                return Err(type_error(state, proto, pc, base, reg, "concatenate"));
            }
            total -= 1;
            last -= 1;
        } else {
            // Both are string/number. Coalesce as many as possible.
            let mut n = 2;
            while n < total && is_string_or_number(state.stack_get(top - n - 1), &state.gc) {
                n += 1;
            }
            // Check total length before allocating (PUC-Rio lvm.c:295).
            let mut tl: usize = 0;
            for i in (0..n).rev() {
                let l = val_string_len(state.stack_get(top - 1 - i), &state.gc);
                if l >= MAX_STRING_SIZE - tl {
                    return Err(runtime_error(proto, pc, "string length overflow"));
                }
                tl += l;
            }
            // Collect all n values into a single buffer.
            let mut buffer = Vec::with_capacity(tl);
            for i in (0..n).rev() {
                let val = state.stack_get(top - 1 - i);
                val_to_string_bytes(val, &state.gc, &mut buffer);
            }
            let r = state.gc.intern_string(&buffer);
            state.stack_set(top - n, Val::Str(r));
            total -= n - 1;
            last -= n - 1;
        }
    }
    Ok(())
}

/// Maximum string size. PUC-Rio uses MAX_SIZET which is ~4GB on 32-bit.
/// We use u32::MAX - 2 to match PUC-Rio 32-bit behavior on all platforms,
/// ensuring the PUC-Rio test suite passes regardless of host word size.
const MAX_STRING_SIZE: usize = (u32::MAX - 2) as usize;

/// Check if a value is a string or number (coercible for concatenation).
fn is_string_or_number(val: Val, gc: &Gc) -> bool {
    matches!(val, Val::Num(_)) || {
        if let Val::Str(r) = val {
            gc.string_arena.get(r).is_some()
        } else {
            false
        }
    }
}

/// Returns the byte length of a value when coerced to string for concatenation.
fn val_string_len(val: Val, gc: &Gc) -> usize {
    match val {
        Val::Str(r) => gc.string_arena.get(r).map_or(0, |s| s.data().len()),
        Val::Num(_) => format!("{val}").len(),
        _ => 0,
    }
}

/// Append the string representation of a value to a buffer.
fn val_to_string_bytes(val: Val, gc: &Gc, buffer: &mut Vec<u8>) {
    match val {
        Val::Str(r) => {
            if let Some(s) = gc.string_arena.get(r) {
                buffer.extend_from_slice(s.data());
            }
        }
        Val::Num(_) => {
            let formatted = format!("{val}");
            buffer.extend_from_slice(formatted.as_bytes());
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Main execute loop
// ---------------------------------------------------------------------------

/// Executes the current Lua function in the given state.
///
/// Assumes a Lua closure has been set up via `precall` and the current
/// CallInfo points to its frame. Runs until `OP_RETURN` drops back to
/// a non-Lua caller (ci == 0 or a Rust frame).
///
/// Returns when the outermost Lua call returns.
pub fn execute(state: &mut LuaState) -> LuaResult<()> {
    // PUC-Rio's `nexeccalls` pattern: tracks how many Lua functions were
    // entered via OP_CALL within this execute() invocation. Starts at 1
    // (the function we were called to run). OP_CALL increments it for
    // Lua callees; OP_RETURN decrements it and returns when it hits 0.
    // This avoids recursive execute() calls for Lua-to-Lua calls,
    // matching PUC-Rio's `goto reentry` model.
    let mut nexeccalls: u32 = 1;

    loop {
        // Cache values from current frame.
        let ci_func = state.call_stack[state.ci].func;
        let mut base = state.base;

        // Get the current closure and proto.
        let Val::Function(closure_ref) = state.stack_get(ci_func) else {
            return Err(runtime_error_simple("not a function"));
        };

        let (proto, env) = {
            let cl = state
                .gc
                .closures
                .get(closure_ref)
                .ok_or_else(|| runtime_error_simple("invalid closure"))?;
            match cl {
                Closure::Lua(lcl) => (ProtoRef::clone(&lcl.proto), lcl.env),
                Closure::Rust(_) => {
                    return Err(runtime_error_simple("expected Lua closure in execute"));
                }
            }
        };

        let mut pc = state.call_stack[state.ci].saved_pc;

        // Inner dispatch loop for this frame.
        loop {
            if pc >= proto.code.len() {
                return Err(runtime_error(&proto, pc, "bytecode overrun"));
            }

            // Read instruction and advance pc BEFORE hook check.
            // PUC-Rio: `const Instruction i = *pc++;` then traceexec(L, pc).
            // After this, pc points one past the instruction being executed,
            // matching PUC-Rio's convention where pcRel(pc, p) = (pc - code) - 1
            // gives the current instruction's index.
            let instr = Instruction::from_raw(proto.code[pc]);
            pc += 1;

            // Interrupt check: abort if the embedder's signal handler set the flag.
            if check_interrupted() {
                return Err(runtime_error(&proto, pc, "interrupted!"));
            }

            // Hook check: line and count hooks (PUC-Rio: lvm.c lines 388-396).
            // The decrement runs every instruction when line or count hooks
            // are set. traceexec fires when counter reaches zero OR line
            // hooks are active.
            if (state.hook.hook_mask & (MASK_LINE | MASK_COUNT)) != 0 {
                state.hook.hook_count -= 1;
                if state.hook.hook_count == 0 || (state.hook.hook_mask & MASK_LINE) != 0 {
                    // npc = index of the current instruction (pc was advanced).
                    // Equivalent to PUC-Rio's pcRel(pc, p) = (pc - code) - 1.
                    let npc = pc - 1;

                    // Save pc for the hook callback (getinfo reads saved_pc).
                    let old_pc = state.call_stack[state.ci].saved_pc;
                    state.call_stack[state.ci].saved_pc = pc;

                    // Count hook: fire when hookcount reaches zero
                    // (PUC-Rio: traceexec lines 64-68).
                    if state.hook.hook_mask > MASK_LINE && state.hook.hook_count == 0 {
                        state.hook.hook_count = state.hook.base_hook_count;
                        if state.hook.yield_on_hook {
                            state.call_stack[state.ci].saved_pc = npc;
                            state.yielded_in_hook = true;
                            return Err(LuaError::Yield(0));
                        }
                        state.callhook("count", -1)?;
                    }

                    // Line hook: fire on new function entry, jump back,
                    // or new source line (PUC-Rio: traceexec lines 70-78).
                    if (state.hook.hook_mask & MASK_LINE) != 0 {
                        #[allow(clippy::cast_possible_wrap)]
                        let newline = proto.line_info.get(npc).copied().unwrap_or(0) as i32;
                        let should_fire = npc == 0 || pc <= old_pc || {
                            let old_npc = old_pc.wrapping_sub(1);
                            #[allow(clippy::cast_possible_wrap)]
                            let oldline = proto.line_info.get(old_npc).copied().unwrap_or(0) as i32;
                            newline != oldline
                        };
                        if should_fire {
                            if state.hook.yield_on_hook {
                                state.call_stack[state.ci].saved_pc = npc;
                                state.yielded_in_hook = true;
                                return Err(LuaError::Yield(0));
                            }
                            state.callhook("line", newline)?;
                        }
                    }

                    // Hook may have changed base via coroutine ops.
                    base = state.base;
                }
            }

            let op = instr.opcode();
            let a = instr.a() as usize;
            let ra = base + a;

            // Debug tracing (compile-time enabled).
            if option_env!("LUA_DEBUG_VM").is_some() {
                eprintln!(
                    "  [{pc:>4}] {op:<12} A={a} B={} C={} (base={base}, top={})",
                    instr.b(),
                    instr.c(),
                    state.top
                );
            }

            match op {
                // ----- Data movement -----
                OpCode::Move => {
                    let b = instr.b() as usize;
                    let val = state.stack_get(base + b);
                    state.stack_set(ra, val);
                }

                OpCode::LoadK => {
                    let bx = instr.bx() as usize;
                    let val = proto.constants[bx];
                    state.stack_set(ra, val);
                }

                OpCode::LoadBool => {
                    let b = instr.b();
                    state.stack_set(ra, Val::Bool(b != 0));
                    if instr.c() != 0 {
                        pc += 1; // skip next instruction
                    }
                }

                OpCode::LoadNil => {
                    let b = instr.b() as usize;
                    // Set registers A through B to nil.
                    for i in a..=b {
                        state.stack_set(base + i, Val::Nil);
                    }
                }

                // ----- Globals -----
                OpCode::GetGlobal => {
                    let bx = instr.bx() as usize;
                    let key = proto.constants[bx];
                    state.call_stack[state.ci].saved_pc = pc;
                    vm_gettable(state, Val::Table(env), key, ra, &proto, pc, base, None)?;
                }

                OpCode::SetGlobal => {
                    let bx = instr.bx() as usize;
                    let key = proto.constants[bx];
                    let val = state.stack_get(ra);
                    state.call_stack[state.ci].saved_pc = pc;
                    vm_settable(state, Val::Table(env), key, val, &proto, pc, base, None)?;
                }

                // ----- Table access -----
                OpCode::GetTable => {
                    let b = instr.b() as usize;
                    let table_val = state.stack_get(base + b);
                    let key = rk(&state.stack, base, &proto.constants, instr.c());
                    state.call_stack[state.ci].saved_pc = pc;
                    vm_gettable(state, table_val, key, ra, &proto, pc, base, Some(b))?;
                }

                OpCode::SetTable => {
                    let table_val = state.stack_get(ra);
                    let key = rk(&state.stack, base, &proto.constants, instr.b());
                    let val = rk(&state.stack, base, &proto.constants, instr.c());
                    state.call_stack[state.ci].saved_pc = pc;
                    vm_settable(state, table_val, key, val, &proto, pc, base, Some(a))?;
                }

                OpCode::NewTable => {
                    state.gc_check()?;
                    let narray = fb2int(instr.b()) as usize;
                    let nhash = fb2int(instr.c()) as usize;
                    let t = state.gc.alloc_table(Table::with_sizes(narray, nhash));
                    state.stack_set(ra, Val::Table(t));
                }

                OpCode::OpSelf => {
                    let b = instr.b() as usize;
                    let table_val = state.stack_get(base + b);
                    // R(A+1) := R(B) -- save table for method call
                    state.stack_set(ra + 1, table_val);
                    let key = rk(&state.stack, base, &proto.constants, instr.c());
                    state.call_stack[state.ci].saved_pc = pc;
                    vm_gettable(state, table_val, key, ra, &proto, pc, base, Some(b))?;
                }

                // ----- Arithmetic -----
                OpCode::Add => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    if let (Some(nb), Some(nc)) = (
                        coerce_to_number(b_val, &state.gc),
                        coerce_to_number(c_val, &state.gc),
                    ) {
                        state.stack_set(ra, Val::Num(nb + nc));
                    } else {
                        state.call_stack[state.ci].saved_pc = pc;
                        call_bin_tm(
                            state,
                            b_val,
                            c_val,
                            ra,
                            TMS::Add,
                            &proto,
                            pc,
                            base,
                            instr.b(),
                            instr.c(),
                        )?;
                    }
                }

                OpCode::Sub => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    if let (Some(nb), Some(nc)) = (
                        coerce_to_number(b_val, &state.gc),
                        coerce_to_number(c_val, &state.gc),
                    ) {
                        state.stack_set(ra, Val::Num(nb - nc));
                    } else {
                        state.call_stack[state.ci].saved_pc = pc;
                        call_bin_tm(
                            state,
                            b_val,
                            c_val,
                            ra,
                            TMS::Sub,
                            &proto,
                            pc,
                            base,
                            instr.b(),
                            instr.c(),
                        )?;
                    }
                }

                OpCode::Mul => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    if let (Some(nb), Some(nc)) = (
                        coerce_to_number(b_val, &state.gc),
                        coerce_to_number(c_val, &state.gc),
                    ) {
                        state.stack_set(ra, Val::Num(nb * nc));
                    } else {
                        state.call_stack[state.ci].saved_pc = pc;
                        call_bin_tm(
                            state,
                            b_val,
                            c_val,
                            ra,
                            TMS::Mul,
                            &proto,
                            pc,
                            base,
                            instr.b(),
                            instr.c(),
                        )?;
                    }
                }

                OpCode::Div => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    if let (Some(nb), Some(nc)) = (
                        coerce_to_number(b_val, &state.gc),
                        coerce_to_number(c_val, &state.gc),
                    ) {
                        state.stack_set(ra, Val::Num(nb / nc));
                    } else {
                        state.call_stack[state.ci].saved_pc = pc;
                        call_bin_tm(
                            state,
                            b_val,
                            c_val,
                            ra,
                            TMS::Div,
                            &proto,
                            pc,
                            base,
                            instr.b(),
                            instr.c(),
                        )?;
                    }
                }

                OpCode::Mod => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    if let (Some(nb), Some(nc)) = (
                        coerce_to_number(b_val, &state.gc),
                        coerce_to_number(c_val, &state.gc),
                    ) {
                        // Lua mod: a - floor(a/b)*b
                        state.stack_set(ra, Val::Num((nb / nc).floor().mul_add(-nc, nb)));
                    } else {
                        state.call_stack[state.ci].saved_pc = pc;
                        call_bin_tm(
                            state,
                            b_val,
                            c_val,
                            ra,
                            TMS::Mod,
                            &proto,
                            pc,
                            base,
                            instr.b(),
                            instr.c(),
                        )?;
                    }
                }

                OpCode::Pow => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    if let (Some(nb), Some(nc)) = (
                        coerce_to_number(b_val, &state.gc),
                        coerce_to_number(c_val, &state.gc),
                    ) {
                        state.stack_set(ra, Val::Num(nb.powf(nc)));
                    } else {
                        state.call_stack[state.ci].saved_pc = pc;
                        call_bin_tm(
                            state,
                            b_val,
                            c_val,
                            ra,
                            TMS::Pow,
                            &proto,
                            pc,
                            base,
                            instr.b(),
                            instr.c(),
                        )?;
                    }
                }

                OpCode::Unm => {
                    let b = instr.b() as usize;
                    let b_val = state.stack_get(base + b);
                    if let Some(nb) = coerce_to_number(b_val, &state.gc) {
                        state.stack_set(ra, Val::Num(-nb));
                    } else {
                        // __unm: try on the single operand only
                        let tm = get_tm_for_val(&state.gc, b_val, TMS::Unm);
                        match tm {
                            Some(tm_val) => {
                                state.call_stack[state.ci].saved_pc = pc;
                                call_tm_res(state, tm_val, b_val, b_val, ra)?;
                            }
                            None => {
                                return Err(type_error(
                                    state,
                                    &proto,
                                    pc,
                                    base,
                                    b,
                                    "perform arithmetic on",
                                ));
                            }
                        }
                    }
                }

                OpCode::Not => {
                    let b = instr.b() as usize;
                    let b_val = state.stack_get(base + b);
                    state.stack_set(ra, Val::Bool(!b_val.is_truthy()));
                }

                // ----- String operations -----
                OpCode::Len => {
                    let b = instr.b() as usize;
                    let b_val = state.stack_get(base + b);
                    match b_val {
                        Val::Str(r) => {
                            let s =
                                state.gc.string_arena.get(r).ok_or_else(|| {
                                    runtime_error_simple("invalid string reference")
                                })?;
                            #[allow(clippy::cast_precision_loss)]
                            state.stack_set(ra, Val::Num(s.len() as f64));
                        }
                        Val::Table(r) => {
                            // Lua 5.1.1: tables always use raw length (no __len).
                            let t =
                                state.gc.tables.get(r).ok_or_else(|| {
                                    runtime_error_simple("invalid table reference")
                                })?;
                            #[allow(clippy::cast_precision_loss)]
                            let len = t.len(&state.gc.string_arena) as f64;
                            state.stack_set(ra, Val::Num(len));
                        }
                        _ => {
                            // Try __len metamethod for other types.
                            let tm = get_tm_for_val(&state.gc, b_val, TMS::Len);
                            if let Some(tm_val) = tm {
                                state.call_stack[state.ci].saved_pc = pc;
                                call_tm_res(state, tm_val, b_val, Val::Nil, ra)?;
                            } else {
                                return Err(type_error(
                                    state,
                                    &proto,
                                    pc,
                                    base,
                                    b,
                                    "get length of",
                                ));
                            }
                        }
                    }
                }

                OpCode::Concat => {
                    state.gc_check()?;
                    let b = instr.b() as usize;
                    let c = instr.c() as usize;
                    state.call_stack[state.ci].saved_pc = pc;
                    vm_concat(state, base, b, c, &proto, pc)?;
                    // The result is in R(base+b). Copy to R(A) if needed.
                    if ra != base + b {
                        let val = state.stack_get(base + b);
                        state.stack_set(ra, val);
                    }
                }

                // ----- Comparison -----
                OpCode::Eq => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    let equal = if val_equal(b_val, c_val, &state.gc) {
                        // Raw-equal succeeded.
                        true
                    } else if std::mem::discriminant(&b_val) != std::mem::discriminant(&c_val) {
                        // Different types are never equal (no metamethod).
                        false
                    } else {
                        // Same type, not raw-equal. Try __eq metamethod.
                        // Only tables and userdata support __eq.
                        let tm = match (b_val, c_val) {
                            (Val::Table(r1), Val::Table(r2)) => {
                                let mt1 = state.gc.tables.get(r1).and_then(Table::metatable);
                                let mt2 = state.gc.tables.get(r2).and_then(Table::metatable);
                                get_comp_tm(
                                    &state.gc.tables,
                                    &state.gc.string_arena,
                                    mt1,
                                    mt2,
                                    TMS::Eq,
                                    &state.gc.tm_names,
                                )
                            }
                            (Val::Userdata(r1), Val::Userdata(r2)) => {
                                let mt1 = state.gc.userdata.get(r1).and_then(Userdata::metatable);
                                let mt2 = state.gc.userdata.get(r2).and_then(Userdata::metatable);
                                get_comp_tm(
                                    &state.gc.tables,
                                    &state.gc.string_arena,
                                    mt1,
                                    mt2,
                                    TMS::Eq,
                                    &state.gc.tm_names,
                                )
                            }
                            _ => None,
                        };
                        if let Some(tm_val) = tm {
                            state.call_stack[state.ci].saved_pc = pc;
                            let res = state.top;
                            call_tm_res(state, tm_val, b_val, c_val, res)?;
                            // PUC-Rio reads from L->top after callTMres
                            // decrements it. We saved the result position.
                            state.stack_get(res).is_truthy()
                        } else {
                            false
                        }
                    };
                    // PUC-Rio: if (result == A) then dojump; pc++.
                    // dojump adds sbx to pc (can be negative). We combine
                    // with the +1 to avoid usize underflow on the intermediate.
                    let expected = a != 0;
                    if equal == expected {
                        let jump_instr = Instruction::from_raw(proto.code[pc]);
                        pc = ((pc as i64) + i64::from(jump_instr.sbx()) + 1) as usize;
                    } else {
                        pc += 1;
                    }
                }

                OpCode::Lt => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    state.call_stack[state.ci].saved_pc = pc;
                    let result = val_less_than(b_val, c_val, state, &proto, pc)?;
                    let expected = a != 0;
                    if result == expected {
                        let jump_instr = Instruction::from_raw(proto.code[pc]);
                        pc = ((pc as i64) + i64::from(jump_instr.sbx()) + 1) as usize;
                    } else {
                        pc += 1;
                    }
                }

                OpCode::Le => {
                    let b_val = rk(&state.stack, base, &proto.constants, instr.b());
                    let c_val = rk(&state.stack, base, &proto.constants, instr.c());
                    state.call_stack[state.ci].saved_pc = pc;
                    let result = val_less_equal(b_val, c_val, state, &proto, pc)?;
                    let expected = a != 0;
                    if result == expected {
                        let jump_instr = Instruction::from_raw(proto.code[pc]);
                        pc = ((pc as i64) + i64::from(jump_instr.sbx()) + 1) as usize;
                    } else {
                        pc += 1;
                    }
                }

                // ----- Logic / test -----
                OpCode::Test => {
                    let val = state.stack_get(ra);
                    let c = instr.c() != 0;
                    if val.is_truthy() == c {
                        let jump_instr = Instruction::from_raw(proto.code[pc]);
                        pc = ((pc as i64) + i64::from(jump_instr.sbx()) + 1) as usize;
                    } else {
                        pc += 1;
                    }
                }

                OpCode::TestSet => {
                    let b = instr.b() as usize;
                    let rb = state.stack_get(base + b);
                    let c = instr.c() != 0;
                    if rb.is_truthy() == c {
                        state.stack_set(ra, rb);
                        let jump_instr = Instruction::from_raw(proto.code[pc]);
                        pc = ((pc as i64) + i64::from(jump_instr.sbx()) + 1) as usize;
                    } else {
                        pc += 1;
                    }
                }

                // ----- Control flow -----
                OpCode::Jmp => {
                    let sbx = instr.sbx();
                    pc = ((pc as i64) + i64::from(sbx)) as usize;
                }

                OpCode::ForPrep => {
                    let init = state.stack_get(ra);
                    let limit = state.stack_get(ra + 1);
                    let step = state.stack_get(ra + 2);

                    let n_init = coerce_to_number(init, &state.gc).ok_or_else(|| {
                        runtime_error(&proto, pc, "'for' initial value must be a number")
                    })?;
                    let n_limit = coerce_to_number(limit, &state.gc)
                        .ok_or_else(|| runtime_error(&proto, pc, "'for' limit must be a number"))?;
                    let n_step = coerce_to_number(step, &state.gc)
                        .ok_or_else(|| runtime_error(&proto, pc, "'for' step must be a number"))?;

                    // R(A) -= step (so the first FORLOOP adds it back).
                    state.stack_set(ra, Val::Num(n_init - n_step));
                    state.stack_set(ra + 1, Val::Num(n_limit));
                    state.stack_set(ra + 2, Val::Num(n_step));

                    let sbx = instr.sbx();
                    pc = ((pc as i64) + i64::from(sbx)) as usize;
                }

                OpCode::ForLoop => {
                    let Val::Num(step) = state.stack_get(ra + 2) else {
                        return Err(runtime_error(&proto, pc, "'for' step is not a number"));
                    };
                    let Val::Num(idx) = state.stack_get(ra) else {
                        return Err(runtime_error(&proto, pc, "'for' index is not a number"));
                    };
                    let idx = idx + step;
                    let Val::Num(limit) = state.stack_get(ra + 1) else {
                        return Err(runtime_error(&proto, pc, "'for' limit is not a number"));
                    };

                    let continue_loop = if step > 0.0 {
                        idx <= limit
                    } else {
                        limit <= idx
                    };

                    if continue_loop {
                        let sbx = instr.sbx();
                        pc = ((pc as i64) + i64::from(sbx)) as usize;
                        state.stack_set(ra, Val::Num(idx)); // internal index
                        state.stack_set(ra + 3, Val::Num(idx)); // external index
                    }
                }

                OpCode::TForLoop => {
                    // Generic for: call iterator function.
                    let cb = ra + 3;
                    // Set up: R(A+3) = R(A), R(A+4) = R(A+1), R(A+5) = R(A+2)
                    state.stack_set(cb + 2, state.stack_get(ra + 2));
                    state.stack_set(cb + 1, state.stack_get(ra + 1));
                    state.stack_set(cb, state.stack_get(ra));
                    state.top = cb + 3;

                    // Save pc before call.
                    state.call_stack[state.ci].saved_pc = pc;

                    let c = instr.c() as i32;
                    state.call_depth += 1;
                    let tfor_result = (|| {
                        state.check_stack_overflow()?;
                        match state.precall(cb, c)? {
                            CallResult::Lua => execute(state),
                            CallResult::Rust => Ok(()),
                        }
                    })();
                    state.call_depth -= 1;
                    tfor_result?;

                    // Restore top to frame top.
                    state.top = state.call_stack[state.ci].top;

                    // Check: if R(A+3) is not nil, continue loop.
                    let result_base = ra + 3;
                    let control = state.stack_get(result_base);
                    if !control.is_nil() {
                        // Save control variable: R(A+2) = R(A+3)
                        state.stack_set(ra + 2, control);
                        // Jump back.
                        let jump_instr = Instruction::from_raw(proto.code[pc]);
                        pc = ((pc as i64) + i64::from(jump_instr.sbx())) as usize;
                    }
                    pc += 1;
                }

                // ----- Calls and returns -----
                OpCode::Call => {
                    let b = instr.b();
                    let c = instr.c();

                    if b != 0 {
                        state.top = ra + b as usize;
                    }
                    // else: B==0 means top is already set (MULTRET args)

                    let num_results = if c == 0 { LUA_MULTRET } else { c as i32 - 1 };

                    // Save our pc before the call.
                    state.call_stack[state.ci].saved_pc = pc;

                    match state.precall(ra, num_results)? {
                        CallResult::Lua => {
                            // Lua-to-Lua call: no recursive execute(), no
                            // call_depth increment. Just break to the outer
                            // loop to re-read the new frame's proto/base.
                            // This matches PUC-Rio's `goto reentry` pattern.
                            nexeccalls += 1;
                        }
                        CallResult::Rust => {
                            // Rust function already completed in precall.
                            // Restore top for fixed results.
                            if c != 0 {
                                state.top = state.call_stack[state.ci].top;
                            }
                            // Break to outer loop to re-read env -- Rust
                            // functions like module() can change the running
                            // closure's environment via setfenv.
                        }
                    }
                    break;
                }

                OpCode::TailCall => {
                    let b = instr.b();

                    if b != 0 {
                        state.top = ra + b as usize;
                    }

                    // Save pc before the call (matches PUC-Rio).
                    state.call_stack[state.ci].saved_pc = pc;

                    // PUC-Rio calls precall FIRST, then only does the
                    // tail call optimization for Lua-to-Lua calls. For
                    // Lua-to-C calls, the C function has already completed
                    // and the caller's frame stays on the stack.
                    match state.precall(ra, LUA_MULTRET)? {
                        CallResult::Lua => {
                            // Tail call optimization: put the new Lua frame
                            // in place of the previous one.
                            let prev_ci = state.ci - 1;
                            let old_func = state.call_stack[prev_ci].func;
                            let new_func = state.call_stack[state.ci].func;

                            // Close upvalues at the old base.
                            state.close_upvalues(state.call_stack[prev_ci].base);

                            // Adjust previous frame's base.
                            let base_offset = state.call_stack[state.ci].base - new_func;
                            state.call_stack[prev_ci].base = old_func + base_offset;
                            state.base = state.call_stack[prev_ci].base;

                            // Move the new frame's stack down over the old
                            // frame.
                            let mut aux = 0;
                            while new_func + aux < state.top {
                                state.stack_set(old_func + aux, state.stack_get(new_func + aux));
                                aux += 1;
                            }
                            state.top = old_func + aux;
                            state.call_stack[prev_ci].top = state.top;

                            // Update saved_pc from the new frame.
                            state.call_stack[prev_ci].saved_pc =
                                state.call_stack[state.ci].saved_pc;
                            state.call_stack[prev_ci].tail_calls += 1;

                            // Remove the new frame -- the previous frame
                            // now holds the callee's data.
                            state.pop_ci();
                        }
                        CallResult::Rust => {
                            // Rust tail-call completed. poscall already
                            // unwound the Rust frame and placed results.
                            // Do NOT restore top to ci.top here -- poscall
                            // set top to reflect the actual result count,
                            // and the subsequent RETURN (B=0) uses top for
                            // MULTRET. Matches PUC-Rio: case PCRC just
                            // refreshes base and continues.
                            base = state.call_stack[state.ci].base;
                            // Continue executing in the current frame.
                            continue;
                        }
                    }
                    break; // Re-enter outer loop for Lua tail call.
                }

                OpCode::Return => {
                    let b = instr.b();
                    let first_result = ra;

                    // Close upvalues at current base.
                    state.close_upvalues(base);

                    if b != 0 {
                        state.top = first_result + (b as usize) - 1;
                    }
                    // else: B==0, use everything up to current top.

                    let fixed_results = state.poscall(first_result);

                    nexeccalls -= 1;
                    if nexeccalls == 0 {
                        // The function that execute() was called to run has
                        // returned. Exit the execute loop.
                        return Ok(());
                    }
                    // The callee returned but there are still Lua callers
                    // within this execute() invocation. Restore top if
                    // fixed results, then re-enter the outer loop.
                    if fixed_results {
                        state.top = state.call_stack[state.ci].top;
                    }
                    break; // goto reentry
                }

                // ----- Upvalues -----
                OpCode::GetUpval => {
                    let b = instr.b() as usize;
                    let cl = state
                        .gc
                        .closures
                        .get(closure_ref)
                        .ok_or_else(|| runtime_error_simple("invalid closure"))?;
                    if let Closure::Lua(lcl) = cl {
                        if b < lcl.upvalues.len() {
                            let uv_ref = lcl.upvalues[b];
                            let val = state
                                .gc
                                .upvalues
                                .get(uv_ref)
                                .map_or(Val::Nil, |uv| uv.get(&state.stack));
                            state.stack_set(ra, val);
                        } else {
                            state.stack_set(ra, Val::Nil);
                        }
                    }
                }

                OpCode::SetUpval => {
                    let b = instr.b() as usize;
                    let val = state.stack_get(ra);
                    let cl = state
                        .gc
                        .closures
                        .get(closure_ref)
                        .ok_or_else(|| runtime_error_simple("invalid closure"))?;
                    if let Closure::Lua(lcl) = cl
                        && b < lcl.upvalues.len()
                    {
                        let uv_ref = lcl.upvalues[b];
                        let uv_color = state.gc.upvalues.color(uv_ref);
                        if let Some(uv) = state.gc.upvalues.get_mut(uv_ref) {
                            uv.set(&mut state.stack, val);
                        }
                        // Forward barrier: mark child if upvalue is black.
                        if let Some(color) = uv_color {
                            state.gc.barrier_forward_val(color, val);
                        }
                    }
                }

                // ----- Closure creation -----
                OpCode::Closure => {
                    state.gc_check()?;
                    let bx = instr.bx() as usize;
                    let child_proto = ProtoRef::clone(&proto.protos[bx]);
                    let nups = child_proto.num_upvalues as usize;

                    let mut new_cl = LuaClosure::new(child_proto, env);

                    // Process pseudo-instructions for upvalue capture.
                    for _ in 0..nups {
                        let pseudo = Instruction::from_raw(proto.code[pc]);
                        pc += 1;

                        match pseudo.opcode() {
                            OpCode::Move => {
                                // Capture local from current frame.
                                let local_reg = pseudo.b() as usize;
                                let stack_slot = base + local_reg;
                                let uv_ref = state.find_upvalue(stack_slot);
                                new_cl.upvalues.push(uv_ref);
                            }
                            OpCode::GetUpval => {
                                // Share parent's upvalue.
                                let parent_uv_idx = pseudo.b() as usize;
                                let cl = state
                                    .gc
                                    .closures
                                    .get(closure_ref)
                                    .ok_or_else(|| runtime_error_simple("invalid closure"))?;
                                if let Closure::Lua(lcl) = cl
                                    && parent_uv_idx < lcl.upvalues.len()
                                {
                                    new_cl.upvalues.push(lcl.upvalues[parent_uv_idx]);
                                }
                            }
                            _ => {
                                return Err(runtime_error(
                                    &proto,
                                    pc,
                                    "invalid pseudo-instruction after CLOSURE",
                                ));
                            }
                        }
                    }

                    let cl_ref = state.gc.alloc_closure(Closure::Lua(new_cl));
                    state.stack_set(ra, Val::Function(cl_ref));
                }

                OpCode::Close => {
                    state.close_upvalues(ra);
                }

                // ----- Varargs -----
                OpCode::VarArg => {
                    let b = instr.b() as i32;
                    let ci_func = state.call_stack[state.ci].func;
                    // Varargs are stored between ci.func+1 and base.
                    let vararg_start = ci_func + 1 + proto.num_params as usize;
                    let num_varargs = base.saturating_sub(vararg_start);

                    let wanted = if b == 0 {
                        // B==0: copy all varargs, adjust top.
                        num_varargs
                    } else {
                        (b - 1) as usize
                    };

                    // Ensure stack space.
                    state.ensure_stack(ra + wanted);

                    for i in 0..wanted {
                        if i < num_varargs {
                            let val = state.stack_get(vararg_start + i);
                            state.stack_set(ra + i, val);
                        } else {
                            state.stack_set(ra + i, Val::Nil);
                        }
                    }

                    if b == 0 {
                        state.top = ra + wanted;
                    }
                }

                // ----- SETLIST -----
                OpCode::SetList => {
                    let mut n = instr.b() as usize;
                    let mut c = instr.c() as usize;

                    if n == 0 {
                        n = state.top - ra - 1;
                        state.top = state.call_stack[state.ci].top;
                    }
                    if c == 0 {
                        // Next instruction contains the real C value.
                        c = proto.code[pc] as usize;
                        pc += 1;
                    }

                    let Val::Table(table_ref) = state.stack_get(ra) else {
                        return Err(type_error(state, &proto, pc, base, a, "index"));
                    };

                    let offset = (c - 1) * LFIELDS_PER_FLUSH as usize;
                    let last = offset + n;

                    // Pre-allocate array to exact size needed (PUC-Rio
                    // luaH_resizearray). Without this, each insert beyond
                    // the current array size triggers rehash, which rounds
                    // up to a power of 2.
                    if let Some(table) = state.gc.tables.get_mut(table_ref) {
                        let mem_before = table.estimated_memory();
                        table.ensure_array_capacity(last);
                        let mem_after = table.estimated_memory();
                        if mem_after > mem_before {
                            state.gc.gc_state.total_bytes += mem_after - mem_before;
                        }
                    }
                    if state.gc.gc_state.total_bytes > state.gc.gc_state.alloc_limit {
                        return Err(crate::LuaError::Memory);
                    }

                    for i in 1..=n {
                        let val = state.stack_get(ra + i);
                        let key = Val::Num((offset + i) as f64);
                        table_set(state, table_ref, key, val)?;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Table access helpers (raw, no metamethods)
// ---------------------------------------------------------------------------

/// Raw table get.
#[allow(dead_code)] // Used by rawget stdlib function (Phase 4c)
fn table_get(state: &LuaState, table_ref: GcRef<Table>, key: Val) -> LuaResult<Val> {
    let table = state
        .gc
        .tables
        .get(table_ref)
        .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
    Ok(table.get(key, &state.gc.string_arena))
}

/// Raw table set with write barrier and memory tracking.
fn table_set(state: &mut LuaState, table_ref: GcRef<Table>, key: Val, value: Val) -> LuaResult<()> {
    let table = state
        .gc
        .tables
        .get_mut(table_ref)
        .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
    let mem_before = table.estimated_memory();
    table.raw_set(key, value, &state.gc.string_arena)?;
    let mem_after = table.estimated_memory();
    if mem_after > mem_before {
        state.gc.gc_state.total_bytes += mem_after - mem_before;
    }
    // Check alloc limit after table growth.
    if state.gc.gc_state.total_bytes > state.gc.gc_state.alloc_limit {
        return Err(crate::LuaError::Memory);
    }
    state.gc.barrier_back(table_ref);
    Ok(())
}

// ---------------------------------------------------------------------------
// Table access with metamethods
// ---------------------------------------------------------------------------

/// Lua table get with `__index` metamethod support.
///
/// Matches PUC-Rio's `luaV_gettable`:
/// 1. If `t` is a table, rawget `key`. If not nil, return it.
/// 2. Look up `__index` in `t`'s metatable.
/// 3. If `__index` is a function, call it with (t, key) and return result.
/// 4. If `__index` is a table, repeat the lookup on that table.
/// 5. Loop up to `MAXTAGLOOP` times to prevent infinite chains.
#[allow(clippy::too_many_arguments)]
fn vm_gettable(
    state: &mut LuaState,
    t: Val,
    key: Val,
    result_reg: usize,
    proto: &Proto,
    pc: usize,
    base: usize,
    obj_reg: Option<usize>,
) -> LuaResult<()> {
    let mut current = t;
    for _ in 0..MAXTAGLOOP {
        if let Val::Table(table_ref) = current {
            // Try raw table get.
            let table = state
                .gc
                .tables
                .get(table_ref)
                .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
            let result = table.get(key, &state.gc.string_arena);

            if !result.is_nil() {
                // Key found -- return it.
                state.stack_set(result_reg, result);
                return Ok(());
            }

            // Key not found. Check for __index metamethod.
            let tm = {
                let mt = table.metatable();
                match mt {
                    Some(mt_ref) => get_tm_for_table(&state.gc, mt_ref, TMS::Index),
                    None => None,
                }
            };

            match tm {
                None => {
                    // No metamethod -- return nil.
                    state.stack_set(result_reg, Val::Nil);
                    return Ok(());
                }
                Some(tm_val) if matches!(tm_val, Val::Function(_)) => {
                    // __index is a function: call it with (table, key).
                    call_tm_res(state, tm_val, current, key, result_reg)?;
                    return Ok(());
                }
                Some(tm_val) => {
                    // __index is a table (or other value): loop.
                    current = tm_val;
                }
            }
        } else {
            // Non-table value: check type metatable for __index.
            let tm = get_tm_for_val(&state.gc, current, TMS::Index);
            match tm {
                None => {
                    if let Some(reg) = obj_reg {
                        return Err(type_error(state, proto, pc, base, reg, "index"));
                    }
                    return Err(runtime_error(
                        proto,
                        pc,
                        &format!("attempt to index a {} value", current.type_name()),
                    ));
                }
                Some(tm_val) if matches!(tm_val, Val::Function(_)) => {
                    call_tm_res(state, tm_val, current, key, result_reg)?;
                    return Ok(());
                }
                Some(tm_val) => {
                    current = tm_val;
                }
            }
        }
    }
    Err(runtime_error_simple("loop in gettable"))
}

/// Lua table set with `__newindex` metamethod support.
///
/// Matches PUC-Rio's `luaV_settable`:
/// 1. If `t` is a table, rawget `key`. If exists (not nil), rawset and return.
/// 2. Look up `__newindex` in `t`'s metatable.
/// 3. If `__newindex` is a function, call it with (t, key, value). Return.
/// 4. If `__newindex` is a table, repeat the set on that table.
/// 5. If no `__newindex`, rawset on original table.
#[allow(clippy::too_many_arguments)]
fn vm_settable(
    state: &mut LuaState,
    t: Val,
    key: Val,
    value: Val,
    proto: &Proto,
    pc: usize,
    base: usize,
    obj_reg: Option<usize>,
) -> LuaResult<()> {
    let mut current = t;
    for _ in 0..MAXTAGLOOP {
        if let Val::Table(table_ref) = current {
            // Check if key already exists (rawget).
            let existing = {
                let table = state
                    .gc
                    .tables
                    .get(table_ref)
                    .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
                table.get(key, &state.gc.string_arena)
            };

            if !existing.is_nil() {
                // Key exists -- rawset directly.
                let table = state
                    .gc
                    .tables
                    .get_mut(table_ref)
                    .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
                table.raw_set(key, value, &state.gc.string_arena)?;
                state.gc.barrier_back(table_ref);
                return Ok(());
            }

            // Key not found. Check for __newindex metamethod.
            let tm = {
                let table = state
                    .gc
                    .tables
                    .get(table_ref)
                    .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
                let mt = table.metatable();
                match mt {
                    Some(mt_ref) => get_tm_for_table(&state.gc, mt_ref, TMS::NewIndex),
                    None => None,
                }
            };

            match tm {
                None => {
                    // No metamethod -- rawset on this table.
                    let table = state
                        .gc
                        .tables
                        .get_mut(table_ref)
                        .ok_or_else(|| runtime_error_simple("invalid table reference"))?;
                    let mem_before = table.estimated_memory();
                    table.raw_set(key, value, &state.gc.string_arena)?;
                    let mem_after = table.estimated_memory();
                    if mem_after > mem_before {
                        state.gc.gc_state.total_bytes += mem_after - mem_before;
                    }
                    if state.gc.gc_state.total_bytes > state.gc.gc_state.alloc_limit {
                        return Err(crate::LuaError::Memory);
                    }
                    state.gc.barrier_back(table_ref);
                    return Ok(());
                }
                Some(tm_val) if matches!(tm_val, Val::Function(_)) => {
                    // __newindex is a function: call with (table, key, value).
                    call_tm_void(state, tm_val, current, key, value)?;
                    return Ok(());
                }
                Some(tm_val) => {
                    // __newindex is a table: loop.
                    current = tm_val;
                }
            }
        } else {
            // Non-table value: check type metatable for __newindex.
            let tm = get_tm_for_val(&state.gc, current, TMS::NewIndex);
            match tm {
                None => {
                    if let Some(reg) = obj_reg {
                        return Err(type_error(state, proto, pc, base, reg, "index"));
                    }
                    return Err(runtime_error(
                        proto,
                        pc,
                        &format!("attempt to index a {} value", current.type_name()),
                    ));
                }
                Some(tm_val) if matches!(tm_val, Val::Function(_)) => {
                    call_tm_void(state, tm_val, current, key, value)?;
                    return Ok(());
                }
                Some(tm_val) => {
                    current = tm_val;
                }
            }
        }
    }
    Err(runtime_error_simple("loop in settable"))
}

/// Look up a metamethod in a specific metatable (fast path for tables).
fn get_tm_for_table(gc: &Gc, mt_ref: GcRef<Table>, event: TMS) -> Option<Val> {
    use super::metatable::fasttm;
    fasttm(&gc.tables, &gc.string_arena, mt_ref, event, &gc.tm_names)
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// Lua raw equality comparison (no metamethods).
///
/// Delegates to `val_raw_equal` in metatable.rs.
fn val_equal(a: Val, b: Val, gc: &Gc) -> bool {
    val_raw_equal(a, b, &gc.tables, &gc.string_arena)
}

// ---------------------------------------------------------------------------
// Locale-aware string comparison (PUC-Rio's l_strcmp)
// ---------------------------------------------------------------------------

/// Compare two Lua strings using `strcoll` (locale-aware), matching
/// PUC-Rio's `l_strcmp` in `lvm.c`. Handles embedded null bytes by
/// iterating over null-terminated segments.
#[allow(unsafe_code)]
pub(crate) fn l_strcmp(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    let mut l = left;
    let mut r = right;
    loop {
        // Create null-terminated copies for the current segment.
        // strcoll reads until '\0', so we find the first '\0' in each slice
        // (or use the whole slice if no '\0' exists).
        let l_nul = l.iter().position(|&b| b == 0).unwrap_or(l.len());
        let r_nul = r.iter().position(|&b| b == 0).unwrap_or(r.len());

        // Build null-terminated buffers for strcoll.
        let mut l_buf = Vec::with_capacity(l_nul + 1);
        l_buf.extend_from_slice(&l[..l_nul]);
        l_buf.push(0);
        let mut r_buf = Vec::with_capacity(r_nul + 1);
        r_buf.extend_from_slice(&r[..r_nul]);
        r_buf.push(0);

        // SAFETY: both buffers are null-terminated.
        let temp = unsafe { strcoll(l_buf.as_ptr(), r_buf.as_ptr()) };
        if temp != 0 {
            return if temp < 0 {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
        }
        // Strings are equal up to a '\0'. Check if either is finished.
        // The first '\0' in both is at position l_nul/r_nul.
        // strlen of the segment = l_nul (since that's where the first '\0' is,
        // or the end of the slice if no '\0').
        if r_nul >= r.len() {
            // r is finished at this segment
            return if l_nul >= l.len() {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Greater
            };
        } else if l_nul >= l.len() {
            // l is finished but r continues
            return std::cmp::Ordering::Less;
        }
        // Both strings continue past the '\0'. Skip past it.
        let skip = l_nul + 1;
        l = &l[skip..];
        r = &r[skip..];
    }
}

/// Lua less-than comparison with metamethod support.
///
/// Matches PUC-Rio's `luaV_lessthan`: numbers and strings use raw
/// comparison; same-type non-comparable values try `__lt` metamethod
/// (both operands must share the same TM); different types error.
fn val_less_than(
    a: Val,
    b: Val,
    state: &mut LuaState,
    proto: &Proto,
    pc: usize,
) -> LuaResult<bool> {
    match (&a, &b) {
        (Val::Num(x), Val::Num(y)) => Ok(x < y),
        (Val::Str(x), Val::Str(y)) => {
            let sx = state
                .gc
                .string_arena
                .get(*x)
                .ok_or_else(|| compare_error(proto, pc, a, b))?;
            let sy = state
                .gc
                .string_arena
                .get(*y)
                .ok_or_else(|| compare_error(proto, pc, a, b))?;
            Ok(l_strcmp(sx.data(), sy.data()) == std::cmp::Ordering::Less)
        }
        _ => {
            if std::mem::discriminant(&a) != std::mem::discriminant(&b) {
                return Err(compare_error(proto, pc, a, b));
            }
            match call_order_tm(state, a, b, TMS::Lt)? {
                Some(result) => Ok(result),
                None => Err(compare_error(proto, pc, a, b)),
            }
        }
    }
}

/// Lua less-or-equal comparison with metamethod support.
///
/// Matches PUC-Rio's `lessequal`: numbers and strings use raw
/// comparison; same-type non-comparable values try `__le` first,
/// then fall back to `NOT __lt(rhs, lhs)` with reversed arguments.
fn val_less_equal(
    a: Val,
    b: Val,
    state: &mut LuaState,
    proto: &Proto,
    pc: usize,
) -> LuaResult<bool> {
    match (&a, &b) {
        (Val::Num(x), Val::Num(y)) => Ok(x <= y),
        (Val::Str(x), Val::Str(y)) => {
            let sx = state
                .gc
                .string_arena
                .get(*x)
                .ok_or_else(|| compare_error(proto, pc, a, b))?;
            let sy = state
                .gc
                .string_arena
                .get(*y)
                .ok_or_else(|| compare_error(proto, pc, a, b))?;
            Ok(l_strcmp(sx.data(), sy.data()) != std::cmp::Ordering::Greater)
        }
        _ => {
            if std::mem::discriminant(&a) != std::mem::discriminant(&b) {
                return Err(compare_error(proto, pc, a, b));
            }
            // Try __le first.
            if let Some(result) = call_order_tm(state, a, b, TMS::Le)? {
                return Ok(result);
            }
            // Fallback: try NOT __lt(rhs, lhs) (reversed arguments).
            if let Some(result) = call_order_tm(state, b, a, TMS::Lt)? {
                return Ok(!result);
            }
            Err(compare_error(proto, pc, a, b))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::approx_constant
)]
mod tests {
    use super::*;
    use crate::vm::instructions::{Instruction, OpCode, rk_as_k};

    /// Helper: create a LuaState with a proto loaded as the current function.
    fn setup_state(proto: Proto) -> LuaState {
        let mut state = LuaState::new();
        let proto_rc = ProtoRef::new(proto);
        let env = state.global;

        let cl = LuaClosure::new(proto_rc, env);
        let cl_ref = state.gc.alloc_closure(Closure::Lua(cl));

        // Place closure at stack[0], set up frame.
        state.stack_set(0, Val::Function(cl_ref));
        state.base = 1;
        state.top = 1;
        state.call_stack[0] = CallInfo::new(0, 1, 41, LUA_MULTRET);

        state
    }

    /// Helper: create a minimal proto with given instructions.
    fn make_proto(code: Vec<u32>, constants: Vec<Val>) -> Proto {
        let mut p = Proto::new("test");
        let n = code.len();
        p.code = code;
        p.constants = constants;
        p.line_info = vec![1; n];
        p.max_stack_size = 20;
        p.is_vararg = VARARG_ISVARARG;
        p
    }

    // ----- Data movement tests -----

    #[test]
    fn op_move() {
        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 0).raw(), // R(0) = 42.0
            Instruction::abc(OpCode::Move, 1, 0, 0).raw(), // R(1) = R(0)
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(42.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base + 1), Val::Num(42.0));
    }

    #[test]
    fn op_loadk() {
        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 0).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(3.14)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(3.14));
    }

    #[test]
    fn op_loadbool() {
        let code = vec![
            Instruction::abc(OpCode::LoadBool, 0, 1, 0).raw(), // R(0) = true
            Instruction::abc(OpCode::LoadBool, 1, 0, 0).raw(), // R(1) = false
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let mut state = setup_state(make_proto(code, vec![]));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Bool(true));
        assert_eq!(state.stack_get(state.base + 1), Val::Bool(false));
    }

    #[test]
    fn op_loadbool_skip() {
        let code = vec![
            Instruction::abc(OpCode::LoadBool, 0, 1, 1).raw(), // R(0) = true; skip
            Instruction::abc(OpCode::LoadBool, 0, 0, 0).raw(), // skipped
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let mut state = setup_state(make_proto(code, vec![]));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Bool(true));
    }

    #[test]
    fn op_loadnil() {
        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 0).raw(), // R(0) = 1
            Instruction::a_bx(OpCode::LoadK, 1, 0).raw(), // R(1) = 1
            Instruction::a_bx(OpCode::LoadK, 2, 0).raw(), // R(2) = 1
            Instruction::abc(OpCode::LoadNil, 0, 2, 0).raw(), // R(0..2) = nil
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(1.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert!(state.stack_get(state.base).is_nil());
        assert!(state.stack_get(state.base + 1).is_nil());
        assert!(state.stack_get(state.base + 2).is_nil());
    }

    // ----- Arithmetic tests -----

    #[test]
    fn op_add() {
        let code = vec![
            Instruction::abc(OpCode::Add, 0, rk_as_k(0), rk_as_k(1)).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(10.0), Val::Num(20.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(30.0));
    }

    #[test]
    fn op_sub() {
        let code = vec![
            Instruction::abc(OpCode::Sub, 0, rk_as_k(0), rk_as_k(1)).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(50.0), Val::Num(30.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(20.0));
    }

    #[test]
    fn op_mul() {
        let code = vec![
            Instruction::abc(OpCode::Mul, 0, rk_as_k(0), rk_as_k(1)).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(6.0), Val::Num(7.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(42.0));
    }

    #[test]
    fn op_div() {
        let code = vec![
            Instruction::abc(OpCode::Div, 0, rk_as_k(0), rk_as_k(1)).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(10.0), Val::Num(4.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(2.5));
    }

    #[test]
    fn op_mod() {
        let code = vec![
            Instruction::abc(OpCode::Mod, 0, rk_as_k(0), rk_as_k(1)).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(10.0), Val::Num(3.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(1.0));
    }

    #[test]
    fn op_pow() {
        let code = vec![
            Instruction::abc(OpCode::Pow, 0, rk_as_k(0), rk_as_k(1)).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(2.0), Val::Num(10.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Num(1024.0));
    }

    #[test]
    fn op_unm() {
        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 0).raw(),
            Instruction::abc(OpCode::Unm, 1, 0, 0).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(42.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base + 1), Val::Num(-42.0));
    }

    #[test]
    fn op_not() {
        let code = vec![
            Instruction::abc(OpCode::LoadBool, 0, 1, 0).raw(), // R(0) = true
            Instruction::abc(OpCode::Not, 1, 0, 0).raw(),      // R(1) = not R(0)
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let mut state = setup_state(make_proto(code, vec![]));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base + 1), Val::Bool(false));
    }

    // ----- Comparison tests -----

    #[test]
    fn op_eq_numbers_equal() {
        // if 1 == 1 then R(0) = true else R(0) = false
        let code = vec![
            Instruction::abc(OpCode::Eq, 1, rk_as_k(0), rk_as_k(0)).raw(), // if (K0 == K0) == 1
            Instruction::a_sbx(OpCode::Jmp, 0, 1).raw(),                   // jump over next
            Instruction::a_sbx(OpCode::Jmp, 0, 1).raw(),                   // skip to false
            Instruction::abc(OpCode::LoadBool, 0, 1, 1).raw(),             // R(0) = true; skip
            Instruction::abc(OpCode::LoadBool, 0, 0, 0).raw(),             // R(0) = false
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(1.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Bool(true));
    }

    // ----- Control flow tests -----

    #[test]
    fn op_jmp() {
        let code = vec![
            Instruction::a_sbx(OpCode::Jmp, 0, 1).raw(), // skip next
            Instruction::abc(OpCode::LoadBool, 0, 0, 0).raw(), // skipped
            Instruction::abc(OpCode::LoadBool, 0, 1, 0).raw(), // R(0) = true
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let mut state = setup_state(make_proto(code, vec![]));
        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base), Val::Bool(true));
    }

    #[test]
    fn op_forloop() {
        // for i=1,3 do end -- R(0)=init, R(1)=limit, R(2)=step, R(3)=i
        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 0).raw(), // R(0) = 1 (init)
            Instruction::a_bx(OpCode::LoadK, 1, 1).raw(), // R(1) = 3 (limit)
            Instruction::a_bx(OpCode::LoadK, 2, 0).raw(), // R(2) = 1 (step)
            Instruction::a_sbx(OpCode::ForPrep, 0, 0).raw(), // R(0) -= step; jmp +0
            Instruction::a_sbx(OpCode::ForLoop, 0, -1).raw(), // loop back
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Num(1.0), Val::Num(3.0)];
        let mut state = setup_state(make_proto(code, constants));
        execute(&mut state).ok();
        // After loop, R(0) should be 3 (last successful index)
        // and R(3) should be 3.
        assert_eq!(state.stack_get(state.base + 3), Val::Num(3.0));
    }

    // ----- Table tests -----

    #[test]
    fn op_newtable_and_settable_gettable() {
        let mut state = LuaState::new();
        let key_ref = state.gc.intern_string(b"x");

        let code = vec![
            Instruction::abc(OpCode::NewTable, 0, 0, 0).raw(), // R(0) = {}
            Instruction::abc(OpCode::SetTable, 0, rk_as_k(0), rk_as_k(1)).raw(), // R(0)["x"] = 42
            Instruction::abc(OpCode::GetTable, 1, 0, rk_as_k(0)).raw(), // R(1) = R(0)["x"]
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Str(key_ref), Val::Num(42.0)];

        let proto_rc = ProtoRef::new(make_proto(code, constants));
        let env = state.global;
        let cl = LuaClosure::new(proto_rc, env);
        let cl_ref = state.gc.alloc_closure(Closure::Lua(cl));
        state.stack_set(0, Val::Function(cl_ref));
        state.base = 1;
        state.top = 1;
        state.call_stack[0] = CallInfo::new(0, 1, 41, LUA_MULTRET);

        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base + 1), Val::Num(42.0));
    }

    // ----- Globals tests -----

    #[test]
    fn op_setglobal_getglobal() {
        let mut state = LuaState::new();
        let key_ref = state.gc.intern_string(b"myvar");

        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 1).raw(), // R(0) = 99
            Instruction::a_bx(OpCode::SetGlobal, 0, 0).raw(), // _G["myvar"] = R(0)
            Instruction::a_bx(OpCode::GetGlobal, 1, 0).raw(), // R(1) = _G["myvar"]
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Str(key_ref), Val::Num(99.0)];

        let proto_rc = ProtoRef::new(make_proto(code, constants));
        let env = state.global;
        let cl = LuaClosure::new(proto_rc, env);
        let cl_ref = state.gc.alloc_closure(Closure::Lua(cl));
        state.stack_set(0, Val::Function(cl_ref));
        state.base = 1;
        state.top = 1;
        state.call_stack[0] = CallInfo::new(0, 1, 41, LUA_MULTRET);

        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base + 1), Val::Num(99.0));
    }

    // ----- Len tests -----

    #[test]
    fn op_len_string() {
        let mut state = LuaState::new();
        let s_ref = state.gc.intern_string(b"hello");

        let code = vec![
            Instruction::a_bx(OpCode::LoadK, 0, 0).raw(),
            Instruction::abc(OpCode::Len, 1, 0, 0).raw(),
            Instruction::abc(OpCode::Return, 0, 1, 0).raw(),
        ];
        let constants = vec![Val::Str(s_ref)];

        let proto_rc = ProtoRef::new(make_proto(code, constants));
        let env = state.global;
        let cl = LuaClosure::new(proto_rc, env);
        let cl_ref = state.gc.alloc_closure(Closure::Lua(cl));
        state.stack_set(0, Val::Function(cl_ref));
        state.base = 1;
        state.top = 1;
        state.call_stack[0] = CallInfo::new(0, 1, 41, LUA_MULTRET);

        execute(&mut state).ok();
        assert_eq!(state.stack_get(state.base + 1), Val::Num(5.0));
    }

    // ----- str_to_number tests -----

    #[test]
    fn str_to_number_basic() {
        assert_eq!(str_to_number(b"42"), Some(42.0));
        assert_eq!(str_to_number(b"3.14"), Some(3.14));
        assert_eq!(str_to_number(b"  42  "), Some(42.0));
        assert_eq!(str_to_number(b""), None);
        assert_eq!(str_to_number(b"abc"), None);
    }

    #[test]
    fn str_to_number_hex() {
        assert_eq!(str_to_number(b"0xff"), Some(255.0));
        assert_eq!(str_to_number(b"0xFF"), Some(255.0));
        assert_eq!(str_to_number(b"0x10"), Some(16.0));
    }

    // ----- fb2int tests -----

    #[test]
    fn fb2int_values() {
        assert_eq!(fb2int(0), 0);
        assert_eq!(fb2int(1), 1);
        assert_eq!(fb2int(7), 7); // exponent 0
        assert_eq!(fb2int(8), 8); // exponent 1: (0+8) << 0 = 8
    }

    // ----- Helper tests -----

    #[test]
    fn val_equal_basics() {
        let gc = Gc::default();
        assert!(val_equal(Val::Nil, Val::Nil, &gc));
        assert!(!val_equal(Val::Nil, Val::Bool(false), &gc));
        assert!(val_equal(Val::Num(1.0), Val::Num(1.0), &gc));
        assert!(!val_equal(Val::Num(1.0), Val::Num(2.0), &gc));
        assert!(val_equal(Val::Bool(true), Val::Bool(true), &gc));
    }
}
