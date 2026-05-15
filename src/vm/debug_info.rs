//! Debug name resolution: maps registers to variable names for error messages.
//!
//! Ports PUC-Rio's `ldebug.c` name resolution functions (`getobjname`,
//! `symbexec`, `kname`, `getfuncname`, `luaF_getlocalname`) to provide
//! descriptive error messages like `"attempt to call local 'x' (a number value)"`.

use super::callinfo::CallInfo;
use super::closure::Closure;
use super::gc::arena::Arena;
use super::instructions::{Instruction, NO_REG, OpCode, index_k, is_k};
use super::proto::Proto;
use super::state::{Gc, LuaState};
use super::string::LuaString;
use super::value::Val;

/// Returns the name of local variable `local_number` (1-based) active at `pc`.
///
/// Matches PUC-Rio's `luaF_getlocalname(f, local_number, pc)`.
/// Walks `proto.local_vars` in order, decrementing the counter for each
/// variable whose range `[startpc, endpc)` contains `pc`.
pub fn get_local_name(proto: &Proto, local_number: usize, pc: usize) -> Option<&str> {
    let mut remaining = local_number;
    #[allow(clippy::cast_possible_truncation)]
    let pc_u32 = pc as u32;
    for var in &proto.local_vars {
        if var.start_pc > pc_u32 {
            break;
        }
        if pc_u32 < var.end_pc {
            remaining -= 1;
            if remaining == 0 {
                return Some(&var.name);
            }
        }
    }
    None
}

/// Simplified symbolic execution: walks instructions `0..lastpc`, tracking
/// the last instruction that wrote to `reg`. Returns the raw u32 instruction.
///
/// Matches PUC-Rio's `symbexec` from `ldebug.c`. We omit the verification
/// checks (those are for the bytecode verifier) and focus only on the
/// register-tracking logic needed by `getobjname`.
pub fn symbexec(proto: &Proto, lastpc: usize, reg: u32) -> u32 {
    // Default: points to the final RETURN (a "neutral" instruction).
    let mut last = if proto.code.is_empty() {
        0
    } else {
        proto.code.len() - 1
    };

    let mut pc: usize = 0;
    while pc < lastpc {
        let raw = proto.code[pc];
        let instr = Instruction::from_raw(raw);
        let op = instr.opcode();
        let a = instr.a();

        // If this opcode writes to register A and A matches our target, mark it.
        if op.sets_register_a() && a == reg {
            last = pc;
        }

        match op {
            OpCode::LoadNil => {
                // LOADNIL writes registers a..=b
                let b = instr.b();
                if a <= reg && reg <= b {
                    last = pc;
                }
            }

            OpCode::OpSelf if reg == a + 1 => {
                // SELF also writes to a+1
                last = pc;
            }

            OpCode::Call | OpCode::TailCall if reg >= a => {
                // Overwrites all registers >= a
                last = pc;
            }

            OpCode::TForLoop if reg >= a + 2 => {
                // Writes to registers >= a+2
                last = pc;
            }

            OpCode::Jmp | OpCode::ForLoop | OpCode::ForPrep => {
                // Follow forward jumps that don't skip past lastpc.
                // PUC-Rio: `if (reg != NO_REG && pc < dest && dest <= lastpc)`
                let sbx = instr.sbx();
                #[allow(clippy::cast_possible_wrap)]
                let dest = pc as i64 + 1 + i64::from(sbx);
                #[allow(clippy::cast_sign_loss)]
                if reg != NO_REG && dest > pc as i64 && (dest as usize) <= lastpc {
                    pc = dest as usize;
                    continue; // Skip the normal pc increment
                }
            }

            OpCode::SetList => {
                // If C == 0, next instruction is the real count; skip it.
                let c = instr.c();
                if c == 0 {
                    pc += 1;
                }
            }

            OpCode::Closure => {
                // Skip the MOVE/GETUPVAL pseudo-instructions for upvalues.
                if let Some(child) = proto.protos.get(instr.bx() as usize) {
                    pc += usize::from(child.num_upvalues);
                }
            }

            _ => {}
        }

        pc += 1;
    }

    proto.code.get(last).copied().unwrap_or(0)
}

/// Returns the name of a constant key from an RK field value.
///
/// If the field has the ISK bit set (>= 256), looks up the constant at
/// `index_k(c)`. If that constant is a string, returns the string content.
/// Otherwise returns `"?"`.
///
/// Matches PUC-Rio's `kname` from `ldebug.c`.
pub fn kname(proto: &Proto, c: u32, string_arena: &Arena<LuaString>) -> String {
    if is_k(c) {
        let idx = index_k(c) as usize;
        if let Some(Val::Str(r)) = proto.constants.get(idx)
            && let Some(s) = string_arena.get(*r)
        {
            return String::from_utf8_lossy(s.data()).into_owned();
        }
    }
    "?".to_string()
}

/// Returns the kind and name of the object at `stackpos` (0-based register)
/// in the current call frame.
///
/// Returns `Some(("local"|"global"|"field"|"upvalue"|"method", name))`.
///
/// Matches PUC-Rio's `getobjname` from `ldebug.c`.
pub fn getobjname(
    proto: &Proto,
    pc: usize,
    stackpos: u32,
    string_arena: &Arena<LuaString>,
) -> Option<(&'static str, String)> {
    // 1. Check if it's a local variable.
    if let Some(name) = get_local_name(proto, (stackpos + 1) as usize, pc) {
        return Some(("local", name.to_string()));
    }

    // 2. Symbolic execution: find the last instruction that wrote to this register.
    let raw = symbexec(proto, pc, stackpos);
    let instr = Instruction::from_raw(raw);
    let op = instr.opcode();

    match op {
        OpCode::GetGlobal => {
            let bx = instr.bx() as usize;
            if let Some(Val::Str(r)) = proto.constants.get(bx)
                && let Some(s) = string_arena.get(*r)
            {
                return Some(("global", String::from_utf8_lossy(s.data()).into_owned()));
            }
        }

        OpCode::Move => {
            let a = instr.a();
            let b = instr.b();
            // Recurse on the source register (only if b < a to avoid loops).
            if b < a {
                return getobjname(proto, pc, b, string_arena);
            }
        }

        OpCode::GetTable => {
            let c = instr.c();
            let name = kname(proto, c, string_arena);
            return Some(("field", name));
        }

        OpCode::GetUpval => {
            let b = instr.b() as usize;
            if let Some(name) = proto.upvalue_names.get(b) {
                return Some(("upvalue", name.clone()));
            }
            return Some(("upvalue", "?".to_string()));
        }

        OpCode::OpSelf => {
            let c = instr.c();
            let name = kname(proto, c, string_arena);
            return Some(("method", name));
        }

        _ => {}
    }

    None
}

/// Returns the kind and name of the function being called at `ci_idx`.
///
/// Looks at the caller's frame to find the CALL/TAILCALL/TFORLOOP instruction
/// that invoked the function, then uses `getobjname` to resolve the name of
/// the function register.
///
/// Matches PUC-Rio's `getfuncname` from `ldebug.c`.
pub fn getfuncname(
    state: &LuaState,
    ci_idx: usize,
    string_arena: &Arena<LuaString>,
) -> Option<(&'static str, String)> {
    if ci_idx == 0 {
        return None;
    }

    let ci = &state.call_stack[ci_idx];

    // If this frame has tail calls, we can't determine the caller.
    if ci.tail_calls > 0 {
        return None;
    }

    // Check that the caller (ci_idx - 1) is a Lua function.
    let caller_ci = &state.call_stack[ci_idx - 1];
    let caller_func = state.stack_get(caller_ci.func);
    let caller_proto = match caller_func {
        Val::Function(r) => {
            let cl = state.gc.closures.get(r)?;
            match cl {
                Closure::Lua(lcl) => crate::vm::proto::ProtoRef::clone(&lcl.proto),
                Closure::Rust(_) => return None,
            }
        }
        _ => return None,
    };

    // The caller's saved_pc points past the CALL instruction.
    let caller_pc = caller_ci.saved_pc;
    if caller_pc == 0 {
        return None;
    }

    let call_pc = caller_pc - 1;
    if call_pc >= caller_proto.code.len() {
        return None;
    }

    let call_instr = Instruction::from_raw(caller_proto.code[call_pc]);
    let call_op = call_instr.opcode();

    match call_op {
        OpCode::Call | OpCode::TailCall => {
            let a = call_instr.a();
            // a is relative to the caller's base
            getobjname(&caller_proto, call_pc, a, string_arena)
        }
        OpCode::TForLoop => {
            // The iterator function is at A (relative to caller's base).
            let a = call_instr.a();
            getobjname(&caller_proto, call_pc, a, string_arena)
        }
        _ => None,
    }
}

/// Like `getfuncname` but works with raw fields (for coroutine threads).
pub fn getfuncname_raw(
    call_stack: &[CallInfo],
    stack: &[Val],
    gc: &Gc,
    ci_idx: usize,
    string_arena: &Arena<LuaString>,
) -> Option<(&'static str, String)> {
    if ci_idx == 0 {
        return None;
    }

    let ci = &call_stack[ci_idx];
    if ci.tail_calls > 0 {
        return None;
    }

    let caller_ci = &call_stack[ci_idx - 1];
    let caller_func = if caller_ci.func < stack.len() {
        stack[caller_ci.func]
    } else {
        return None;
    };
    let caller_proto = match caller_func {
        Val::Function(r) => {
            let cl = gc.closures.get(r)?;
            match cl {
                Closure::Lua(lcl) => crate::vm::proto::ProtoRef::clone(&lcl.proto),
                Closure::Rust(_) => return None,
            }
        }
        _ => return None,
    };

    let caller_pc = caller_ci.saved_pc;
    if caller_pc == 0 {
        return None;
    }

    let call_pc = caller_pc - 1;
    if call_pc >= caller_proto.code.len() {
        return None;
    }

    let call_instr = Instruction::from_raw(caller_proto.code[call_pc]);
    let call_op = call_instr.opcode();

    match call_op {
        OpCode::Call | OpCode::TailCall | OpCode::TForLoop => {
            let a = call_instr.a();
            getobjname(&caller_proto, call_pc, a, string_arena)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::instructions::OpCode;
    use crate::vm::proto::LocalVar;

    #[test]
    fn get_local_name_basic() {
        let proto = Proto {
            local_vars: vec![
                LocalVar {
                    name: "x".into(),
                    start_pc: 0,
                    end_pc: 5,
                },
                LocalVar {
                    name: "y".into(),
                    start_pc: 1,
                    end_pc: 5,
                },
            ],
            ..Proto::new("test")
        };
        // At pc=0, only "x" is active (local #1)
        assert_eq!(get_local_name(&proto, 1, 0), Some("x"));
        // At pc=0, "y" is not yet active (startpc=1)
        assert_eq!(get_local_name(&proto, 2, 0), None);
        // At pc=1, both are active
        assert_eq!(get_local_name(&proto, 1, 1), Some("x"));
        assert_eq!(get_local_name(&proto, 2, 1), Some("y"));
        // At pc=5, both are past endpc
        assert_eq!(get_local_name(&proto, 1, 5), None);
    }

    #[test]
    fn get_local_name_not_found() {
        let proto = Proto::new("test");
        assert_eq!(get_local_name(&proto, 1, 0), None);
    }

    #[test]
    fn symbexec_move() {
        // MOVE R0, R1
        let move_instr = Instruction::abc(OpCode::Move, 0, 1, 0);
        // RETURN 0, 1
        let ret = Instruction::abc(OpCode::Return, 0, 1, 0);
        let proto = Proto {
            code: vec![move_instr.raw(), ret.raw()],
            ..Proto::new("test")
        };
        // symbexec(proto, 1, 0) should return the MOVE instruction
        let result = symbexec(&proto, 1, 0);
        let instr = Instruction::from_raw(result);
        assert_eq!(instr.opcode(), OpCode::Move);
    }

    #[test]
    fn symbexec_getglobal() {
        // GETGLOBAL R0, K0
        let getglobal = Instruction::a_bx(OpCode::GetGlobal, 0, 0);
        let ret = Instruction::abc(OpCode::Return, 0, 1, 0);
        let proto = Proto {
            code: vec![getglobal.raw(), ret.raw()],
            ..Proto::new("test")
        };
        let result = symbexec(&proto, 1, 0);
        let instr = Instruction::from_raw(result);
        assert_eq!(instr.opcode(), OpCode::GetGlobal);
    }

    #[test]
    fn symbexec_loadnil() {
        // LOADNIL R0, R2 (sets R0, R1, R2 to nil)
        let loadnil = Instruction::abc(OpCode::LoadNil, 0, 2, 0);
        let ret = Instruction::abc(OpCode::Return, 0, 1, 0);
        let proto = Proto {
            code: vec![loadnil.raw(), ret.raw()],
            ..Proto::new("test")
        };
        // Register 1 should be tracked (within a..=b range)
        let result = symbexec(&proto, 1, 1);
        let instr = Instruction::from_raw(result);
        assert_eq!(instr.opcode(), OpCode::LoadNil);
    }

    #[test]
    fn kname_with_constant() {
        use crate::vm::gc::Color;
        use crate::vm::gc::arena::Arena;
        use crate::vm::instructions::rk_as_k;

        let mut arena = Arena::new();
        let r = arena.alloc(LuaString::new(b"hello", 0), Color::White0);
        let proto = Proto {
            constants: vec![Val::Str(r)],
            ..Proto::new("test")
        };
        let name = kname(&proto, rk_as_k(0), &arena);
        assert_eq!(name, "hello");
    }

    #[test]
    fn kname_with_register() {
        let arena = Arena::new();
        let proto = Proto::new("test");
        // Not a constant (no ISK bit) — returns "?"
        let name = kname(&proto, 5, &arena);
        assert_eq!(name, "?");
    }
}
