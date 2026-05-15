//! Bytecode listing output matching PUC-Rio's `print.c`.
//!
//! Produces human-readable bytecode listings in the same format as
//! `luac -l` (summary) and `luac -l -l` (full with constants, locals,
//! upvalues).

use std::fmt::Write;

use super::instructions::{Instruction, OpArgMask, OpCode, OpMode, index_k, is_k};
use super::proto::{Proto, ProtoRef, VARARG_ISVARARG};
use super::value::Val;

/// Prints a string with Lua-style quoting and escaping.
///
/// Matches PUC-Rio's `PrintString` from `print.c`.
fn format_string(s: &[u8], out: &mut String) {
    out.push('"');
    for &b in s {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\x07' => out.push_str("\\a"),
            b'\x08' => out.push_str("\\b"),
            b'\x0C' => out.push_str("\\f"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'\x0B' => out.push_str("\\v"),
            c if c.is_ascii_graphic() || c == b' ' => out.push(c as char),
            c => {
                let _ = write!(out, "\\{c:03}");
            }
        }
    }
    out.push('"');
}

/// Formats a single constant value for display.
///
/// Matches PUC-Rio's `PrintConstant` from `print.c`.
fn format_constant(proto: &Proto, idx: usize, out: &mut String) {
    if idx >= proto.constants.len() {
        out.push_str("(invalid constant)");
        return;
    }

    // Check string_pool first: unpatched protos store string constants
    // as Val::Nil placeholders with raw bytes in string_pool.
    for (sidx, bytes) in &proto.string_pool {
        if *sidx as usize == idx {
            format_string(bytes, out);
            return;
        }
    }

    match &proto.constants[idx] {
        Val::Nil => out.push_str("nil"),
        Val::Bool(b) => {
            if *b {
                out.push_str("true");
            } else {
                out.push_str("false");
            }
        }
        Val::Num(n) => {
            // Uses Val::Display which implements %.14g formatting.
            let _ = write!(out, "{}", Val::Num(*n));
        }
        Val::Str(_) => {
            // Already-patched string: we can't recover the bytes from
            // a Val::Str(GcRef) without GC access, so show placeholder.
            out.push_str("(string)");
        }
        _ => {
            out.push('?');
        }
    }
}

/// Formats a constant value without string quoting.
///
/// Used for GETGLOBAL/SETGLOBAL comments where PUC-Rio uses `svalue()`
/// directly (raw string, no quotes).
fn format_constant_raw(proto: &Proto, idx: usize, out: &mut String) {
    if idx >= proto.constants.len() {
        out.push_str("(invalid constant)");
        return;
    }

    // Check string_pool first (unpatched proto).
    for (sidx, bytes) in &proto.string_pool {
        if *sidx as usize == idx {
            // Print string bytes directly without quoting.
            out.push_str(&String::from_utf8_lossy(bytes));
            return;
        }
    }

    // Fall back to format_constant for non-string types.
    format_constant(proto, idx, out);
}

/// Prints the function header line.
///
/// Matches PUC-Rio's `PrintHeader` from `print.c`.
/// Strips the `@` or `=` prefix from a source name for display.
///
/// PUC-Rio's `getstr(f->source)` returns the raw source, but
/// `PrintFunction` uses `getstr(f->source)` which still has the prefix.
/// However, PUC-Rio's `luac -l` output shows the source without `@`
/// but keeps `=`. We match this: `@foo.lua` -> `foo.lua`, `=stdin` -> `=stdin`.
fn display_source(source: &str) -> &str {
    source.strip_prefix('@').unwrap_or(source)
}

fn format_header(proto: &Proto, proto_ptr: usize, out: &mut String) {
    // "main" or "function" designation.
    if proto.line_defined == 0 {
        out.push_str("main");
    } else {
        out.push_str("function");
    }

    let _ = writeln!(
        out,
        " <{source}:{start},{end}> ({n_code} instruction{plural}, {n_bytes} bytes at {ptr:#x})",
        source = display_source(&proto.source),
        start = proto.line_defined,
        end = proto.last_line_defined,
        n_code = proto.code.len(),
        plural = if proto.code.len() == 1 { "" } else { "s" },
        n_bytes = proto.code.len() * 4,
        ptr = proto_ptr,
    );

    let _ = writeln!(
        out,
        "{params}{vararg} param{pp}, {slots} slot{sp}, {upvals} upvalue{up}, \
         {locals} local{lp}, {constants} constant{cp}, {functions} function{fp}",
        params = proto.num_params,
        vararg = if proto.is_vararg & VARARG_ISVARARG != 0 {
            "+"
        } else {
            ""
        },
        pp = if proto.num_params == 1 { "" } else { "s" },
        slots = proto.max_stack_size,
        sp = if proto.max_stack_size == 1 { "" } else { "s" },
        upvals = proto.num_upvalues,
        up = if proto.num_upvalues == 1 { "" } else { "s" },
        locals = proto.local_vars.len(),
        lp = if proto.local_vars.len() == 1 { "" } else { "s" },
        constants = proto.constants.len(),
        cp = if proto.constants.len() == 1 { "" } else { "s" },
        functions = proto.protos.len(),
        fp = if proto.protos.len() == 1 { "" } else { "s" },
    );
}

/// Prints the instruction listing.
///
/// Matches PUC-Rio's `PrintCode` from `print.c`.
#[allow(clippy::too_many_lines)]
fn format_code(proto: &Proto, out: &mut String) {
    let n = proto.code.len();
    for pc in 0..n {
        let instr = Instruction::from_raw(proto.code[pc]);
        let op = instr.opcode();
        let a = instr.a();
        let b = instr.b();
        let c = instr.c();
        let bx = instr.bx();
        let sbx = instr.sbx();

        // Line number.
        let line = if pc < proto.line_info.len() {
            i64::from(proto.line_info[pc])
        } else {
            -1
        };

        // Instruction index (1-based) and line.
        let _ = write!(out, "\t{pc1}\t", pc1 = pc + 1);
        if line >= 0 {
            let _ = write!(out, "[{line}]\t");
        } else {
            out.push_str("[-]\t");
        }

        // Opcode name, padded to 9 chars.
        let _ = write!(out, "{name:<9}\t", name = op.name());

        // Operands vary by instruction format.
        match op.mode() {
            OpMode::IABC => {
                let _ = write!(out, "{a}");
                if op.b_mode() != OpArgMask::N {
                    if is_k(b) {
                        let _ = write!(out, " {}", -(1 + i64::from(index_k(b))));
                    } else {
                        let _ = write!(out, " {b}");
                    }
                }
                if op.c_mode() != OpArgMask::N {
                    if is_k(c) {
                        let _ = write!(out, " {}", -(1 + i64::from(index_k(c))));
                    } else {
                        let _ = write!(out, " {c}");
                    }
                }
            }
            OpMode::IABx => {
                if op.b_mode() == OpArgMask::K {
                    let _ = write!(out, "{a} {}", -(1 + i64::from(bx)));
                } else {
                    let _ = write!(out, "{a} {bx}");
                }
            }
            OpMode::IAsBx => {
                if op == OpCode::Jmp {
                    let _ = write!(out, "{sbx}");
                } else {
                    let _ = write!(out, "{a} {sbx}");
                }
            }
        }

        // Comment section.
        match op {
            OpCode::LoadK => {
                out.push_str("\t; ");
                format_constant(proto, bx as usize, out);
            }
            OpCode::GetUpval | OpCode::SetUpval => {
                out.push_str("\t; ");
                if (b as usize) < proto.upvalue_names.len() {
                    out.push_str(&proto.upvalue_names[b as usize]);
                } else {
                    out.push('-');
                }
            }
            OpCode::GetGlobal | OpCode::SetGlobal => {
                // PUC-Rio uses svalue() directly (no quoting) for globals.
                out.push_str("\t; ");
                format_constant_raw(proto, bx as usize, out);
            }
            OpCode::GetTable | OpCode::OpSelf if is_k(c) => {
                out.push_str("\t; ");
                format_constant(proto, index_k(c) as usize, out);
            }
            OpCode::SetTable
            | OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Eq
            | OpCode::Lt
            | OpCode::Le
                if is_k(b) || is_k(c) =>
            {
                out.push_str("\t; ");
                if is_k(b) {
                    format_constant(proto, index_k(b) as usize, out);
                } else {
                    out.push('-');
                }
                out.push(' ');
                if is_k(c) {
                    format_constant(proto, index_k(c) as usize, out);
                } else {
                    out.push('-');
                }
            }
            OpCode::Jmp | OpCode::ForLoop | OpCode::ForPrep => {
                let _ = write!(out, "\t; to {}", sbx + pc as i32 + 2);
            }
            OpCode::Closure if (bx as usize) < proto.protos.len() => {
                let child_ptr = ProtoRef::as_ptr(&proto.protos[bx as usize]) as usize;
                let _ = write!(out, "\t; {child_ptr:#x}");
            }
            OpCode::SetList => {
                if c == 0 {
                    // Next instruction holds the real count.
                    if pc + 1 < n {
                        let extra = proto.code[pc + 1];
                        let _ = write!(out, "\t; {extra}");
                    }
                } else {
                    let _ = write!(out, "\t; {c}");
                }
            }
            OpCode::VarArg => {
                let _ = write!(out, "\t; is_vararg");
            }
            _ => {}
        }

        out.push('\n');
    }
}

/// Formats the constants table (full listing mode).
fn format_constants(proto: &Proto, proto_ptr: usize, out: &mut String) {
    let _ = writeln!(
        out,
        "constants ({n}) for {ptr:#x}:",
        n = proto.constants.len(),
        ptr = proto_ptr,
    );
    for (i, _) in proto.constants.iter().enumerate() {
        let _ = write!(out, "\t{idx}\t", idx = i + 1);
        format_constant(proto, i, out);
        out.push('\n');
    }
}

/// Formats the local variables table (full listing mode).
fn format_locals(proto: &Proto, proto_ptr: usize, out: &mut String) {
    let _ = writeln!(
        out,
        "locals ({n}) for {ptr:#x}:",
        n = proto.local_vars.len(),
        ptr = proto_ptr,
    );
    for (i, var) in proto.local_vars.iter().enumerate() {
        let _ = writeln!(
            out,
            "\t{idx}\t{name}\t{start}\t{end}",
            idx = i,
            name = var.name,
            start = var.start_pc + 1,
            end = var.end_pc + 1,
        );
    }
}

/// Formats the upvalues table (full listing mode).
fn format_upvalues(proto: &Proto, proto_ptr: usize, out: &mut String) {
    let _ = writeln!(
        out,
        "upvalues ({n}) for {ptr:#x}:",
        n = proto.upvalue_names.len(),
        ptr = proto_ptr,
    );
    for (i, name) in proto.upvalue_names.iter().enumerate() {
        let _ = writeln!(out, "\t{i}\t{name}");
    }
}

/// Produces a complete bytecode listing for a function prototype.
///
/// If `full` is true, includes constants, locals, and upvalue tables
/// after the code listing (matches `luac -l -l`).
///
/// Recursively lists nested function prototypes.
pub fn list_function(proto: &Proto, full: bool) -> String {
    let mut out = String::new();
    list_function_recursive(proto, full, &mut out);
    out
}

fn list_function_recursive(proto: &Proto, full: bool, out: &mut String) {
    let proto_ptr = std::ptr::from_ref(proto) as usize;

    format_header(proto, proto_ptr, out);
    format_code(proto, out);

    if full {
        format_constants(proto, proto_ptr, out);
        format_locals(proto, proto_ptr, out);
        format_upvalues(proto, proto_ptr, out);
    }

    // Recursively list nested function prototypes.
    for child in &proto.protos {
        out.push('\n');
        list_function_recursive(child, full, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler;

    #[test]
    fn print_header_main() {
        let proto =
            compiler::compile(b"print('hello')", "=test").unwrap_or_else(|_| unreachable!());
        let listing = list_function(&proto, false);
        // Should start with "main".
        assert!(listing.starts_with("main <"), "got: {listing}");
        assert!(listing.contains("=test:0,0"), "got: {listing}");
        assert!(listing.contains("instruction"), "got: {listing}");
    }

    #[test]
    fn print_header_nested() {
        let proto = compiler::compile(b"local function f() end", "=test")
            .unwrap_or_else(|_| unreachable!());
        let listing = list_function(&proto, false);
        // Should contain "function" for the nested proto.
        assert!(listing.contains("function <"), "got: {listing}");
    }

    #[test]
    fn print_code_simple() {
        let proto = compiler::compile(b"local x = 1", "=test").unwrap_or_else(|_| unreachable!());
        let listing = list_function(&proto, false);
        // Should contain LOADK instruction.
        assert!(listing.contains("LOADK"), "got: {listing}");
        // Should contain the constant comment.
        assert!(listing.contains("; 1"), "got: {listing}");
    }

    #[test]
    fn print_full_listing() {
        let proto = compiler::compile(b"local x = 42\nreturn x", "=test")
            .unwrap_or_else(|_| unreachable!());
        let listing = list_function(&proto, true);
        // Full listing includes constants section.
        assert!(listing.contains("constants ("), "got: {listing}");
        assert!(listing.contains("locals ("), "got: {listing}");
        assert!(listing.contains("upvalues ("), "got: {listing}");
    }

    #[test]
    fn print_global_access() {
        let proto =
            compiler::compile(b"print('hello')", "=test").unwrap_or_else(|_| unreachable!());
        let listing = list_function(&proto, false);
        // GETGLOBAL should have a comment with the global name.
        assert!(listing.contains("GETGLOBAL"), "got: {listing}");
    }

    #[test]
    fn print_jump_target() {
        let proto = compiler::compile(b"local x = 1\nif x then return end", "=test")
            .unwrap_or_else(|_| unreachable!());
        let listing = list_function(&proto, false);
        // JMP should show "; to N".
        assert!(listing.contains("; to "), "got: {listing}");
    }
}
