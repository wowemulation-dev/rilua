//! String library: pattern matching, formatting, string manipulation.
//!
//! Reference: `lstrlib.c` in PUC-Rio Lua 5.1.1.

use crate::error::{LuaError, LuaResult, RuntimeError};
use crate::vm::state::LuaState;
use crate::vm::value::Val;

// ---------------------------------------------------------------------------
// Argument helpers (same pattern as base.rs)
// ---------------------------------------------------------------------------

#[inline]
fn nargs(state: &LuaState) -> usize {
    state.top.saturating_sub(state.base)
}

#[inline]
fn arg(state: &LuaState, n: usize) -> Val {
    let idx = state.base + n;
    if idx < state.top {
        state.stack_get(idx)
    } else {
        Val::Nil
    }
}

fn bad_argument(name: &str, n: usize, msg: &str) -> LuaError {
    LuaError::Runtime(RuntimeError {
        message: format!("bad argument #{n} to '{name}' ({msg})"),
        level: 0,
        traceback: vec![],
    })
}

/// Relative string position: negative means back from end.
/// PUC-Rio's `posrelat`.
fn posrelat(pos: i64, len: usize) -> i64 {
    if pos >= 0 { pos } else { pos + len as i64 + 1 }
}

// ---------------------------------------------------------------------------
// string.len
// ---------------------------------------------------------------------------

/// `string.len(s)` -- Returns the length of a string.
pub fn str_len(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    #[allow(clippy::cast_precision_loss)]
    let len = s.len() as f64;
    state.push_number(len);
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.byte
// ---------------------------------------------------------------------------

/// `string.byte(s [, i [, j]])` -- Returns byte values of characters.
pub fn str_byte(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let len = s.len();

    let posi = state.opt_integer(2, 1).map(|i| posrelat(i, len))?;
    let pose = state.opt_integer(3, posi).map(|j| posrelat(j, len))?;

    let posi = posi.max(1) as usize;
    let pose = pose.min(len as i64).max(0) as usize;

    if posi > pose {
        return Ok(0);
    }

    let n = pose - posi + 1;
    state.ensure_stack(state.top + n);

    for i in posi..=pose {
        state.push(Val::Num(f64::from(s[i - 1])));
    }

    Ok(n as u32)
}

// ---------------------------------------------------------------------------
// string.char
// ---------------------------------------------------------------------------

/// `string.char(...)` -- Returns a string of characters from byte values.
pub fn str_char(state: &mut LuaState) -> LuaResult<u32> {
    let n = nargs(state);
    let mut buf = Vec::with_capacity(n);

    for i in 1..=n {
        let c = state.check_number(i)?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let byte = c as u32;
        if byte > 255 {
            return Err(state.arg_error(i, "invalid value"));
        }
        #[allow(clippy::cast_possible_truncation)]
        buf.push(byte as u8);
    }

    let r = state.gc.intern_string(&buf);
    state.push(Val::Str(r));
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.sub
// ---------------------------------------------------------------------------

/// `string.sub(s, i [, j])` -- Returns a substring.
pub fn str_sub(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let len = s.len();

    let start = posrelat(state.check_integer(2)?, len);
    #[allow(clippy::cast_possible_truncation)]
    let end = state.opt_integer(3, len as i64).map(|j| posrelat(j, len))?;

    let start = start.max(1) as usize;
    let end = end.min(len as i64).max(0) as usize;

    if start <= end {
        let r = state.gc.intern_string(&s[start - 1..end]);
        state.push(Val::Str(r));
    } else {
        let r = state.gc.intern_string(b"");
        state.push(Val::Str(r));
    }
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.rep
// ---------------------------------------------------------------------------

/// PUC-Rio 32-bit limit: ~4GB. Use same limit for cross-platform consistency.
const MAX_STRING_SIZE: usize = (u32::MAX - 2) as usize;

/// `string.rep(s, n)` -- Returns a string repeated n times.
pub fn str_rep(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let n = state.check_integer(2)?;

    if n <= 0 {
        let r = state.gc.intern_string(b"");
        state.push(Val::Str(r));
    } else {
        let total = s.len().saturating_mul(n as usize);
        if total > MAX_STRING_SIZE {
            return Err(LuaError::Runtime(crate::RuntimeError {
                message: "string length overflow".to_string(),
                level: 0,
                traceback: vec![],
            }));
        }
        let mut buf = Vec::with_capacity(total);
        for _ in 0..n {
            buf.extend_from_slice(&s);
        }
        let r = state.gc.intern_string(&buf);
        state.push(Val::Str(r));
    }
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.reverse
// ---------------------------------------------------------------------------

/// `string.reverse(s)` -- Returns the string reversed.
pub fn str_reverse(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let mut reversed = s;
    reversed.reverse();
    let r = state.gc.intern_string(&reversed);
    state.push(Val::Str(r));
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.lower
// ---------------------------------------------------------------------------

/// `string.lower(s)` -- Returns the string with all uppercase letters lowered.
/// Uses libc `tolower` for locale-aware conversion (matching PUC-Rio).
#[allow(unsafe_code)]
pub fn str_lower(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let lowered: Vec<u8> = s
        .iter()
        .map(|&c| unsafe { tolower(i32::from(c)) as u8 })
        .collect();
    let r = state.gc.intern_string(&lowered);
    state.push(Val::Str(r));
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.upper
// ---------------------------------------------------------------------------

/// `string.upper(s)` -- Returns the string with all lowercase letters raised.
/// Uses libc `toupper` for locale-aware conversion (matching PUC-Rio).
#[allow(unsafe_code)]
pub fn str_upper(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let uppered: Vec<u8> = s
        .iter()
        .map(|&c| unsafe { toupper(i32::from(c)) as u8 })
        .collect();
    let r = state.gc.intern_string(&uppered);
    state.push(Val::Str(r));
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.format
// ---------------------------------------------------------------------------

/// Maximum number of captures for pattern matching.
const LUA_MAXCAPTURES: usize = 32;

/// `string.format(formatstring, ...)` -- Formats values into a string.
///
/// Supports: %d, %i, %u, %f, %e, %E, %g, %G, %o, %x, %X, %c, %s, %q, %%.
/// Width, precision, and flags (-, +, space, 0, #) are supported.
///
/// Reference: `str_format` in `lstrlib.c`.
pub fn str_format(state: &mut LuaState) -> LuaResult<u32> {
    let fmt = state.check_string(1)?;
    let mut result = Vec::new();
    let mut arg_idx = 1usize; // Next argument index (0 = format string itself).
    let mut i = 0;

    while i < fmt.len() {
        if fmt[i] != b'%' {
            result.push(fmt[i]);
            i += 1;
            continue;
        }
        i += 1; // Skip '%'.
        if i >= fmt.len() {
            return Err(bad_argument(
                "string.format",
                1,
                "invalid format string (ends with '%')",
            ));
        }

        // %% escape.
        if fmt[i] == b'%' {
            result.push(b'%');
            i += 1;
            continue;
        }

        // Parse flags.
        let spec_start = i - 1;
        while i < fmt.len() && b"-+ #0".contains(&fmt[i]) {
            i += 1;
        }

        // Parse width.
        while i < fmt.len() && fmt[i].is_ascii_digit() {
            i += 1;
        }

        // Parse precision.
        if i < fmt.len() && fmt[i] == b'.' {
            i += 1;
            while i < fmt.len() && fmt[i].is_ascii_digit() {
                i += 1;
            }
        }

        if i >= fmt.len() {
            return Err(bad_argument("string.format", 1, "invalid format string"));
        }

        let spec_char = fmt[i];
        let spec = &fmt[spec_start..=i];
        i += 1;

        match spec_char {
            b'd' | b'i' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let n = coerce_to_number_err(state, val, "string.format", arg_idx)?;
                #[allow(clippy::cast_possible_truncation)]
                let int_val = n as i64;
                let formatted = format_with_spec(spec, &FormatArg::Int(int_val));
                result.extend_from_slice(formatted.as_bytes());
            }
            b'u' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let n = coerce_to_number_err(state, val, "string.format", arg_idx)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let uint_val = n as u64;
                let formatted = format_with_spec(spec, &FormatArg::Uint(uint_val));
                result.extend_from_slice(formatted.as_bytes());
            }
            b'o' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let n = coerce_to_number_err(state, val, "string.format", arg_idx)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let uint_val = n as u64;
                let formatted = format_with_spec(spec, &FormatArg::Oct(uint_val));
                result.extend_from_slice(formatted.as_bytes());
            }
            b'x' | b'X' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let n = coerce_to_number_err(state, val, "string.format", arg_idx)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let uint_val = n as u64;
                let formatted = if spec_char == b'x' {
                    format_with_spec(spec, &FormatArg::Hex(uint_val))
                } else {
                    format_with_spec(spec, &FormatArg::HexUpper(uint_val))
                };
                result.extend_from_slice(formatted.as_bytes());
            }
            b'f' | b'e' | b'E' | b'g' | b'G' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let n = coerce_to_number_err(state, val, "string.format", arg_idx)?;
                let formatted = format_with_spec(spec, &FormatArg::Float(n));
                result.extend_from_slice(formatted.as_bytes());
            }
            b'c' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let n = coerce_to_number_err(state, val, "string.format", arg_idx)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let ch = (n as u32 & 0xFF) as u8;
                result.push(ch);
            }
            b's' => {
                let val = arg(state, arg_idx);
                arg_idx += 1;
                // Lua strings are byte arrays, not UTF-8. We must work
                // with raw bytes to avoid replacing high bytes with the
                // UTF-8 replacement character (U+FFFD).
                let s_bytes: Vec<u8> = match val {
                    Val::Str(r) => state
                        .gc
                        .string_arena
                        .get(r)
                        .map(|s| s.data().to_vec())
                        .unwrap_or_default(),
                    Val::Nil => b"nil".to_vec(),
                    Val::Bool(b) => {
                        if b {
                            b"true".to_vec()
                        } else {
                            b"false".to_vec()
                        }
                    }
                    _ => format!("{val}").into_bytes(),
                };
                let formatted = format_string_with_spec_bytes(spec, &s_bytes);
                result.extend_from_slice(&formatted);
            }
            b'q' => {
                // %q: quoted string (adds quotes and escapes special chars).
                let val = arg(state, arg_idx);
                arg_idx += 1;
                let Val::Str(r) = val else {
                    return Err(bad_argument("string.format", arg_idx, "string expected"));
                };
                let s = state
                    .gc
                    .string_arena
                    .get(r)
                    .map(|s| s.data().to_vec())
                    .unwrap_or_default();
                result.push(b'"');
                for &byte in &s {
                    match byte {
                        b'\\' => result.extend_from_slice(b"\\\\"),
                        b'"' => result.extend_from_slice(b"\\\""),
                        // PUC-Rio: newline is escaped as backslash + actual newline.
                        b'\n' => {
                            result.push(b'\\');
                            result.push(b'\n');
                        }
                        b'\r' => result.extend_from_slice(b"\\r"),
                        // PUC-Rio: null byte is escaped as \000 (3-digit octal).
                        b'\0' => result.extend_from_slice(b"\\000"),
                        _ => result.push(byte),
                    }
                }
                result.push(b'"');
            }
            _ => {
                return Err(bad_argument(
                    "string.format",
                    1,
                    &format!("invalid option '%{}'", char::from(spec_char)),
                ));
            }
        }
    }

    let r = state.gc.intern_string(&result);
    state.push(Val::Str(r));
    Ok(1)
}

/// Coerce a value to f64 or return an error.
fn coerce_to_number_err(state: &LuaState, val: Val, name: &str, arg_n: usize) -> LuaResult<f64> {
    match val {
        Val::Num(n) => Ok(n),
        Val::Str(r) => {
            let s = state
                .gc
                .string_arena
                .get(r)
                .map(|s| String::from_utf8_lossy(s.data()).to_string())
                .unwrap_or_default();
            s.trim()
                .parse::<f64>()
                .map_err(|_| bad_argument(name, arg_n, "number expected"))
        }
        _ => Err(bad_argument(name, arg_n, "number expected")),
    }
}

/// Format argument types for the spec formatter.
enum FormatArg {
    Int(i64),
    Uint(u64),
    Oct(u64),
    Hex(u64),
    HexUpper(u64),
    Float(f64),
}

/// Formats a value using a C-style format specifier.
///
/// We parse the spec manually to extract flags, width, precision, then
/// use Rust's formatting to approximate C printf behavior.
fn format_with_spec(spec: &[u8], arg: &FormatArg) -> String {
    let spec_str = String::from_utf8_lossy(spec);

    // Parse flags, width, precision from spec like "%-10.3f"
    let mut flags = String::new();
    let mut width: Option<usize> = None;
    let mut precision: Option<usize> = None;

    let chars: Vec<char> = spec_str.chars().collect();
    let mut idx = 1; // Skip '%'

    // Parse flags.
    while idx < chars.len() && "-+ #0".contains(chars[idx]) {
        flags.push(chars[idx]);
        idx += 1;
    }

    // Parse width.
    let width_start = idx;
    while idx < chars.len() && chars[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx > width_start {
        width = chars[width_start..idx]
            .iter()
            .collect::<String>()
            .parse()
            .ok();
    }

    // Parse precision.
    if idx < chars.len() && chars[idx] == '.' {
        idx += 1;
        let prec_start = idx;
        while idx < chars.len() && chars[idx].is_ascii_digit() {
            idx += 1;
        }
        precision = if idx > prec_start {
            chars[prec_start..idx]
                .iter()
                .collect::<String>()
                .parse()
                .ok()
        } else {
            Some(0)
        };
    }

    let left_align = flags.contains('-');
    let pad_zero = flags.contains('0') && !left_align;
    let plus_sign = flags.contains('+');
    let space_sign = flags.contains(' ');
    let alt = flags.contains('#');
    let w = width.unwrap_or(0);

    match *arg {
        FormatArg::Int(n) => {
            let sign = if n < 0 {
                "-"
            } else if plus_sign {
                "+"
            } else if space_sign {
                " "
            } else {
                ""
            };
            let abs = n.unsigned_abs();
            let digits = format!("{abs}");
            pad_number(sign, &digits, w, left_align, pad_zero)
        }
        FormatArg::Uint(n) => {
            let digits = format!("{n}");
            pad_number("", &digits, w, left_align, pad_zero)
        }
        FormatArg::Oct(n) => {
            let digits = format!("{n:o}");
            let prefix = if alt && n != 0 { "0" } else { "" };
            pad_number(prefix, &digits, w, left_align, pad_zero)
        }
        FormatArg::Hex(n) => {
            let digits = format!("{n:x}");
            let prefix = if alt { "0x" } else { "" };
            pad_number(prefix, &digits, w, left_align, pad_zero)
        }
        FormatArg::HexUpper(n) => {
            let digits = format!("{n:X}");
            let prefix = if alt { "0X" } else { "" };
            pad_number(prefix, &digits, w, left_align, pad_zero)
        }
        FormatArg::Float(n) => format_float(
            n, &chars, w, precision, left_align, pad_zero, plus_sign, space_sign,
        ),
    }
}

/// Pads a number with sign/prefix, digits, and appropriate padding.
fn pad_number(sign: &str, digits: &str, width: usize, left_align: bool, pad_zero: bool) -> String {
    let total = sign.len() + digits.len();
    if total >= width {
        return format!("{sign}{digits}");
    }
    let padding = width - total;
    if left_align {
        format!("{sign}{digits}{:padding$}", "")
    } else if pad_zero {
        format!("{sign}{:0>padding$}{digits}", "")
    } else {
        format!("{:>padding$}{sign}{digits}", "")
    }
}

/// Normalizes the exponent in scientific notation strings to match C printf:
/// - Always includes a sign (`e+10` not `e10`)
/// - Pads to at least 2 digits (`e+05` not `e+5`)
fn normalize_exponent_sign(s: &str) -> String {
    if let Some(e_pos) = s.find(['e', 'E']) {
        let (before, exp_part) = s.split_at(e_pos);
        let e_char = &exp_part[..1];
        let exp_digits = &exp_part[1..];
        // Separate sign and digits.
        let (sign, digits) = if let Some(rest) = exp_digits.strip_prefix('-') {
            ("-", rest)
        } else if let Some(rest) = exp_digits.strip_prefix('+') {
            ("+", rest)
        } else {
            ("+", exp_digits)
        };
        // C printf pads exponent to at least 2 digits.
        if digits.len() < 2 {
            format!("{before}{e_char}{sign}0{digits}")
        } else {
            format!("{before}{e_char}{sign}{digits}")
        }
    } else {
        s.to_string()
    }
}

/// Formats a float value with the given spec.
///
/// The parameter count mirrors PUC-Rio's format spec fields: value, spec chars,
/// width, precision, and four boolean flags. Grouping into a struct would add
/// indirection without reducing complexity since this is the only call site.
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn format_float(
    n: f64,
    chars: &[char],
    width: usize,
    precision: Option<usize>,
    left_align: bool,
    pad_zero: bool,
    plus_sign: bool,
    space_sign: bool,
) -> String {
    let spec_char = chars[chars.len() - 1];
    let prec = precision.unwrap_or(6);

    // Handle NaN and infinity specially to match C printf behavior.
    if n.is_nan() || n.is_infinite() {
        let upper = matches!(spec_char, 'E' | 'G');
        let mut formatted = if n.is_nan() {
            if upper {
                "nan".to_uppercase()
            } else {
                "nan".to_string()
            }
        } else if upper {
            "inf".to_uppercase()
        } else {
            "inf".to_string()
        };
        // Sign handling.
        if n.is_sign_negative() {
            formatted.insert(0, '-');
        } else if plus_sign {
            formatted.insert(0, '+');
        } else if space_sign {
            formatted.insert(0, ' ');
        }
        // Pad to width.
        if formatted.len() >= width {
            return formatted;
        }
        let padding = width - formatted.len();
        return if left_align {
            format!("{formatted}{:padding$}", "")
        } else {
            format!("{:>padding$}{formatted}", "")
        };
    }

    let mut formatted = match spec_char {
        'f' => format!("{n:.prec$}"),
        'e' => normalize_exponent_sign(&format!("{n:.prec$e}")),
        'E' => normalize_exponent_sign(&format!("{n:.prec$E}")),
        'g' | 'G' => {
            // %g: use %e if exponent < -4 or >= precision, else %f.
            // Remove trailing zeros from fractional part.
            let effective_prec = if prec == 0 { 1 } else { prec };
            format_g(n, effective_prec, spec_char == 'G')
        }
        _ => format!("{n}"),
    };

    // Add sign if needed.
    if n >= 0.0 && !n.is_nan() {
        if plus_sign {
            formatted.insert(0, '+');
        } else if space_sign {
            formatted.insert(0, ' ');
        }
    }

    // Pad to width.
    if formatted.len() >= width {
        formatted
    } else {
        let padding = width - formatted.len();
        if left_align {
            format!("{formatted}{:padding$}", "")
        } else if pad_zero {
            // Insert zeros after sign.
            if formatted.starts_with('-')
                || formatted.starts_with('+')
                || formatted.starts_with(' ')
            {
                let (sign, rest) = formatted.split_at(1);
                format!("{sign}{:0>padding$}{rest}", "")
            } else {
                format!("{:0>padding$}{formatted}", "")
            }
        } else {
            format!("{:>padding$}{formatted}", "")
        }
    }
}

/// Format a float using %g/%G rules.
fn format_g(n: f64, prec: usize, upper: bool) -> String {
    // Special cases: NaN and infinity are handled by format_float's sign logic,
    // but we need to handle them here for standalone %g calls.
    if n.is_nan() {
        // Match C printf: lowercase, with sign.
        return if upper {
            if n.is_sign_negative() {
                "-NAN".to_string()
            } else {
                "NAN".to_string()
            }
        } else if n.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        };
    }
    if n.is_infinite() {
        return if upper {
            if n < 0.0 {
                "-INF".to_string()
            } else {
                "INF".to_string()
            }
        } else if n < 0.0 {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }
    if n == 0.0 {
        // Preserve negative zero.
        return if n.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    // Use scientific notation to determine which format to use.
    let exp = if n == 0.0 {
        0
    } else {
        n.abs().log10().floor() as i32
    };

    if exp < -4 || exp >= prec as i32 {
        // Use %e format, then strip trailing zeros.
        let e_prec = prec.saturating_sub(1);
        let formatted = if upper {
            format!("{n:.e_prec$E}")
        } else {
            format!("{n:.e_prec$e}")
        };
        strip_trailing_zeros_scientific(&formatted)
    } else {
        // Use %f format with adjusted precision, then strip trailing zeros.
        // prec = total significant digits, exp+1 = digits before decimal point.
        // When exp is negative, we need MORE fractional digits, not fewer.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let f_prec = (prec as i32 - (exp + 1)).max(0) as usize;
        let formatted = format!("{n:.f_prec$}");
        strip_trailing_zeros_fixed(&formatted)
    }
}

/// Strip trailing zeros from a fixed-point formatted number.
fn strip_trailing_zeros_fixed(s: &str) -> String {
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0');
        let trimmed = trimmed.trim_end_matches('.');
        trimmed.to_string()
    } else {
        s.to_string()
    }
}

/// Strip trailing zeros from a scientific notation number.
/// Also normalizes the exponent to always include a sign (C printf compat).
fn strip_trailing_zeros_scientific(s: &str) -> String {
    // Split at 'e' or 'E'.
    if let Some(e_pos) = s.find(['e', 'E']) {
        let (mantissa, exp_part) = s.split_at(e_pos);
        let trimmed = strip_trailing_zeros_fixed(mantissa);
        // Recombine with normalized exponent sign.
        normalize_exponent_sign(&format!("{trimmed}{exp_part}"))
    } else {
        s.to_string()
    }
}

/// Format a string with width/precision from a `%s` specifier.
/// Lua strings are byte arrays; we must avoid any UTF-8 conversion so that
/// raw bytes (like 0xed) pass through unchanged.
fn format_string_with_spec_bytes(spec: &[u8], s: &[u8]) -> Vec<u8> {
    let mut idx = 1; // Skip '%'.

    let mut left_align = false;
    while idx < spec.len() && b"-+ #0".contains(&spec[idx]) {
        if spec[idx] == b'-' {
            left_align = true;
        }
        idx += 1;
    }

    let mut width: Option<usize> = None;
    let width_start = idx;
    while idx < spec.len() && spec[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx > width_start {
        width = std::str::from_utf8(&spec[width_start..idx])
            .ok()
            .and_then(|s| s.parse().ok());
    }

    let precision = if idx < spec.len() && spec[idx] == b'.' {
        idx += 1;
        let prec_start = idx;
        while idx < spec.len() && spec[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx > prec_start {
            std::str::from_utf8(&spec[prec_start..idx])
                .ok()
                .and_then(|s| s.parse().ok())
        } else {
            Some(0)
        }
    } else {
        None
    };

    // Apply precision (truncate string at byte level).
    let truncated = if let Some(prec) = precision {
        if prec < s.len() { &s[..prec] } else { s }
    } else {
        s
    };

    // Apply width.
    let w = width.unwrap_or(0);
    if truncated.len() >= w {
        truncated.to_vec()
    } else {
        let padding = w - truncated.len();
        let mut out = Vec::with_capacity(w);
        if left_align {
            out.extend_from_slice(truncated);
            out.resize(out.len() + padding, b' ');
        } else {
            out.resize(padding, b' ');
            out.extend_from_slice(truncated);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Pattern matching engine
// ---------------------------------------------------------------------------

/// Match state for pattern matching operations.
struct MatchState<'a> {
    /// Source string being searched.
    src: &'a [u8],
    /// Pattern string.
    pat: &'a [u8],
    /// Captures.
    captures: Vec<Capture>,
    /// Number of open (incomplete) captures.
    level: usize,
    /// Recursion depth counter for match_ function.
    depth: u32,
}

/// A pattern capture result.
#[derive(Clone, Copy)]
struct Capture {
    /// Start position in source string.
    start: usize,
    /// Length of capture. `CAP_UNFINISHED` = not yet closed,
    /// `CAP_POSITION` = position capture.
    len: CaptureLen,
}

#[derive(Clone, Copy)]
enum CaptureLen {
    Finished(usize),
    Unfinished,
    Position,
}

/// Maximum recursion depth for pattern matching.
const MAXCCALLS: u32 = 200;

enum Quantifier {
    Greedy,   // *
    Plus,     // +
    Lazy,     // -
    Optional, // ?
    None,
}

// Locale-aware character classification and case conversion.
// On native targets, these are libc functions that respect the current
// C locale (set via os.setlocale). On WASM, ASCII-only stubs are used.
// All declarations live in platform.rs.
use crate::platform::{
    isalnum, isalpha, iscntrl, isdigit, islower, ispunct, isspace, isupper, isxdigit, tolower,
    toupper,
};

/// Match a character against a character class letter.
/// Uses libc functions for locale-aware classification (matching PUC-Rio).
#[allow(unsafe_code)]
fn matchclass(ch: u8, class: u8) -> bool {
    let c = i32::from(ch);
    let lower_class = class.to_ascii_lowercase();
    // SAFETY: isalpha et al. are standard C functions that accept any int
    // value; unsigned char values (0-255) are always valid arguments.
    let result = match lower_class {
        b'a' => unsafe { isalpha(c) != 0 },
        b'c' => unsafe { iscntrl(c) != 0 },
        b'd' => unsafe { isdigit(c) != 0 },
        b'l' => unsafe { islower(c) != 0 },
        b'p' => unsafe { ispunct(c) != 0 },
        b's' => unsafe { isspace(c) != 0 },
        b'u' => unsafe { isupper(c) != 0 },
        b'w' => unsafe { isalnum(c) != 0 },
        b'x' => unsafe { isxdigit(c) != 0 },
        b'z' => ch == 0,         // PUC-Rio: case 'z': res = (c == 0)
        _ => return ch == class, // Literal match for non-class escapes.
    };
    // Uppercase class means complement.
    if class.is_ascii_uppercase() {
        !result
    } else {
        result
    }
}

impl<'a> MatchState<'a> {
    fn new(src: &'a [u8], pat: &'a [u8]) -> Self {
        Self {
            src,
            pat,
            captures: Vec::new(),
            level: 0,
            depth: 0,
        }
    }

    /// Main pattern matching function. Tries to match pattern starting at
    /// `pat_pos` against source starting at `src_pos`.
    ///
    /// Returns `Some(end_pos)` where end_pos is the position past the match
    /// in the source, or `None` if no match.
    fn match_(&mut self, src_pos: usize, pat_pos: usize) -> LuaResult<Option<usize>> {
        self.depth += 1;
        if self.depth > MAXCCALLS {
            return Err(LuaError::Runtime(RuntimeError {
                message: "pattern too complex".into(),
                level: 0,
                traceback: vec![],
            }));
        }
        let result = self.match_inner(src_pos, pat_pos);
        self.depth -= 1;
        result
    }

    fn match_inner(&mut self, mut src_pos: usize, mut pat_pos: usize) -> LuaResult<Option<usize>> {
        loop {
            if pat_pos >= self.pat.len() {
                return Ok(Some(src_pos));
            }

            match self.pat[pat_pos] {
                b'(' => {
                    if pat_pos + 1 < self.pat.len() && self.pat[pat_pos + 1] == b')' {
                        // Position capture.
                        return self.match_position_capture(src_pos, pat_pos);
                    }
                    return self.match_open_capture(src_pos, pat_pos);
                }
                b')' => {
                    return self.match_close_capture(src_pos, pat_pos);
                }
                b'$' if pat_pos + 1 >= self.pat.len() => {
                    // End anchor.
                    return Ok(if src_pos == self.src.len() {
                        Some(src_pos)
                    } else {
                        None
                    });
                }
                _ => {}
            }

            // Check for class followed by quantifier.
            let (class_end, quantifier) = self.get_quantifier(pat_pos);

            match quantifier {
                Quantifier::Greedy => {
                    // '*' - match 0 or more, greedy.
                    return self.match_greedy(src_pos, pat_pos, class_end + 1);
                }
                Quantifier::Lazy => {
                    // '-' - match 0 or more, lazy.
                    return self.match_lazy(src_pos, pat_pos, class_end + 1);
                }
                Quantifier::Plus => {
                    // '+' - match 1 or more, greedy.
                    if src_pos < self.src.len() && self.singlematch(src_pos, pat_pos) {
                        return self.match_greedy(src_pos + 1, pat_pos, class_end + 1);
                    }
                    return Ok(None);
                }
                Quantifier::Optional => {
                    // '?' - match 0 or 1.
                    if src_pos < self.src.len()
                        && self.singlematch(src_pos, pat_pos)
                        && let Some(end) = self.match_(src_pos + 1, class_end + 1)?
                    {
                        return Ok(Some(end));
                    }
                    pat_pos = class_end + 1;
                }
                Quantifier::None => {
                    // No quantifier. Single match.
                    if pat_pos < self.pat.len()
                        && self.pat[pat_pos] == b'%'
                        && pat_pos + 1 < self.pat.len()
                    {
                        match self.pat[pat_pos + 1] {
                            b'b' => {
                                return self.match_balance(src_pos, pat_pos);
                            }
                            b'f' => {
                                return self.match_frontier(src_pos, pat_pos);
                            }
                            c if c.is_ascii_digit() => {
                                return self.match_backref(src_pos, pat_pos);
                            }
                            _ => {}
                        }
                    }

                    if src_pos < self.src.len() && self.singlematch(src_pos, pat_pos) {
                        src_pos += 1;
                        pat_pos = class_end;
                        continue;
                    }
                    return Ok(None);
                }
            }
        }
    }

    /// Greedy match: match as many as possible, then backtrack.
    fn match_greedy(
        &mut self,
        src_pos: usize,
        pat_class_start: usize,
        pat_rest: usize,
    ) -> LuaResult<Option<usize>> {
        let mut count = 0;
        while src_pos + count < self.src.len() && self.singlematch(src_pos + count, pat_class_start)
        {
            count += 1;
        }
        // Try from longest match down.
        while count >= 0_i64 as usize {
            if let Some(end) = self.match_(src_pos + count, pat_rest)? {
                return Ok(Some(end));
            }
            if count == 0 {
                break;
            }
            count -= 1;
        }
        Ok(None)
    }

    /// Lazy match: match as few as possible, grow.
    fn match_lazy(
        &mut self,
        src_pos: usize,
        pat_class_start: usize,
        pat_rest: usize,
    ) -> LuaResult<Option<usize>> {
        let mut count = 0;
        loop {
            if let Some(end) = self.match_(src_pos + count, pat_rest)? {
                return Ok(Some(end));
            }
            if src_pos + count < self.src.len()
                && self.singlematch(src_pos + count, pat_class_start)
            {
                count += 1;
            } else {
                return Ok(None);
            }
        }
    }

    /// Match a single character against the pattern class at `pat_pos`.
    fn singlematch(&self, src_pos: usize, pat_pos: usize) -> bool {
        if src_pos >= self.src.len() {
            return false;
        }
        let ch = self.src[src_pos];
        let p = self.pat[pat_pos];
        match p {
            b'.' => true, // Match any character.
            b'%' => {
                if pat_pos + 1 < self.pat.len() {
                    matchclass(ch, self.pat[pat_pos + 1])
                } else {
                    false
                }
            }
            b'[' => {
                // Bracket class.
                let (matches, _end) = self.matchbracketclass(ch, pat_pos);
                matches
            }
            _ => ch == p,
        }
    }

    /// Match a character against a bracket class `[set]`.
    /// PUC-Rio: `matchbracketclass(c, p, ec)` where p points to `[` and ec to `]`.
    fn matchbracketclass(&self, ch: u8, pat_pos: usize) -> (bool, usize) {
        // Find the closing ']' first (class_end handles ] after [^ correctly).
        let class_end_pos = self.class_end(pat_pos);
        // ec is the position of ']', class_end returns one past it.
        let ec = class_end_pos - 1;

        let mut pos = pat_pos + 1; // Skip '['.
        let complement = pos < ec && self.pat[pos] == b'^';
        if complement {
            pos += 1;
        }

        // PUC-Rio: while (++p < ec) — pre-increment processes chars between
        // (the opening `[`/`^`) and `ec` (the closing `]`).
        let mut matched = false;
        while pos < ec {
            if self.pat[pos] == b'%' && pos + 1 < ec {
                pos += 1;
                if matchclass(ch, self.pat[pos]) {
                    matched = true;
                }
                pos += 1;
            } else if pos + 2 < ec && self.pat[pos + 1] == b'-' {
                // Range a-z.
                if ch >= self.pat[pos] && ch <= self.pat[pos + 2] {
                    matched = true;
                }
                pos += 3;
            } else {
                if ch == self.pat[pos] {
                    matched = true;
                }
                pos += 1;
            }
        }

        (if complement { !matched } else { matched }, class_end_pos)
    }

    /// Returns the end position of the pattern class and what quantifier follows.
    fn get_quantifier(&self, pat_pos: usize) -> (usize, Quantifier) {
        let class_end = self.class_end(pat_pos);
        if class_end < self.pat.len() {
            match self.pat[class_end] {
                b'*' => (class_end, Quantifier::Greedy),
                b'+' => (class_end, Quantifier::Plus),
                b'-' => (class_end, Quantifier::Lazy),
                b'?' => (class_end, Quantifier::Optional),
                _ => (class_end, Quantifier::None),
            }
        } else {
            (class_end, Quantifier::None)
        }
    }

    /// Returns the position past the class pattern at `pat_pos`.
    fn class_end(&self, pat_pos: usize) -> usize {
        if pat_pos >= self.pat.len() {
            return pat_pos;
        }
        match self.pat[pat_pos] {
            b'%' => {
                if pat_pos + 1 < self.pat.len() {
                    pat_pos + 2
                } else {
                    pat_pos + 1
                }
            }
            b'[' => {
                let mut pos = pat_pos + 1;
                // Handle ^.
                if pos < self.pat.len() && self.pat[pos] == b'^' {
                    pos += 1;
                }
                // PUC-Rio uses do { ... } while (*p != ']') which processes
                // at least one character before checking for ']'. This handles
                // `]` immediately after `[` or `[^` as a literal.
                loop {
                    if pos >= self.pat.len() {
                        return pos;
                    }
                    if self.pat[pos] == b'%' && pos + 1 < self.pat.len() {
                        pos += 1; // Skip escaped char in bracket.
                    }
                    pos += 1;
                    // Check for closing ']' AFTER processing the character.
                    if pos < self.pat.len() && self.pat[pos] == b']' {
                        return pos + 1;
                    }
                }
            }
            _ => pat_pos + 1,
        }
    }

    /// Opens a new capture group.
    fn match_open_capture(&mut self, src_pos: usize, pat_pos: usize) -> LuaResult<Option<usize>> {
        if self.captures.len() >= LUA_MAXCAPTURES {
            return Err(LuaError::Runtime(RuntimeError {
                message: "too many captures".into(),
                level: 0,
                traceback: vec![],
            }));
        }
        let cap_idx = self.captures.len();
        self.captures.push(Capture {
            start: src_pos,
            len: CaptureLen::Unfinished,
        });
        self.level += 1;

        let result = self.match_(src_pos, pat_pos + 1)?;
        if result.is_none() {
            // Backtrack: remove the capture.
            self.captures.truncate(cap_idx);
            self.level -= 1;
        }
        Ok(result)
    }

    /// Handles a position capture `()`.
    fn match_position_capture(
        &mut self,
        src_pos: usize,
        pat_pos: usize,
    ) -> LuaResult<Option<usize>> {
        if self.captures.len() >= LUA_MAXCAPTURES {
            return Err(LuaError::Runtime(RuntimeError {
                message: "too many captures".into(),
                level: 0,
                traceback: vec![],
            }));
        }
        let cap_idx = self.captures.len();
        self.captures.push(Capture {
            start: src_pos,
            len: CaptureLen::Position,
        });
        self.level += 1;

        let result = self.match_(src_pos, pat_pos + 2)?;
        if result.is_none() {
            self.captures.truncate(cap_idx);
            self.level -= 1;
        }
        Ok(result)
    }

    /// Closes the most recent open capture.
    fn match_close_capture(&mut self, src_pos: usize, pat_pos: usize) -> LuaResult<Option<usize>> {
        // Find the most recent unfinished capture.
        let cap_idx = self.find_open_capture()?;
        let start = self.captures[cap_idx].start;
        self.captures[cap_idx].len = CaptureLen::Finished(src_pos - start);
        self.level -= 1;

        let result = self.match_(src_pos, pat_pos + 1)?;
        if result.is_none() {
            // Backtrack: reopen.
            self.captures[cap_idx].len = CaptureLen::Unfinished;
            self.level += 1;
        }
        Ok(result)
    }

    /// Finds the most recent unfinished capture index.
    fn find_open_capture(&self) -> LuaResult<usize> {
        for i in (0..self.captures.len()).rev() {
            if matches!(self.captures[i].len, CaptureLen::Unfinished) {
                return Ok(i);
            }
        }
        Err(LuaError::Runtime(RuntimeError {
            message: "invalid pattern capture".into(),
            level: 0,
            traceback: vec![],
        }))
    }

    /// Match `%bxy`: balanced match for delimiters x and y.
    fn match_balance(&mut self, src_pos: usize, pat_pos: usize) -> LuaResult<Option<usize>> {
        if pat_pos + 3 >= self.pat.len() {
            return Err(LuaError::Runtime(RuntimeError {
                message: "unbalanced pattern".into(),
                level: 0,
                traceback: vec![],
            }));
        }
        let open = self.pat[pat_pos + 2];
        let close = self.pat[pat_pos + 3];

        if src_pos >= self.src.len() || self.src[src_pos] != open {
            return Ok(None);
        }

        let mut count = 1i32;
        let mut pos = src_pos + 1;
        while pos < self.src.len() {
            // PUC-Rio checks close first, so when open==close,
            // each occurrence decrements (matching nearest pair).
            if self.src[pos] == close {
                count -= 1;
                if count == 0 {
                    return self.match_(pos + 1, pat_pos + 4);
                }
            } else if self.src[pos] == open {
                count += 1;
            }
            pos += 1;
        }
        Ok(None)
    }

    /// Match `%f[set]`: frontier pattern.
    fn match_frontier(&mut self, src_pos: usize, pat_pos: usize) -> LuaResult<Option<usize>> {
        if pat_pos + 2 >= self.pat.len() || self.pat[pat_pos + 2] != b'[' {
            return Err(LuaError::Runtime(RuntimeError {
                message: "missing '[' after '%f' in pattern".into(),
                level: 0,
                traceback: vec![],
            }));
        }

        let prev = if src_pos > 0 {
            self.src[src_pos - 1]
        } else {
            b'\0'
        };
        let curr = if src_pos < self.src.len() {
            self.src[src_pos]
        } else {
            b'\0'
        };

        let (prev_matches, bracket_end) = self.matchbracketclass(prev, pat_pos + 2);
        let (curr_matches, _) = self.matchbracketclass(curr, pat_pos + 2);

        if !prev_matches && curr_matches {
            self.match_(src_pos, bracket_end)
        } else {
            Ok(None)
        }
    }

    /// Match `%N` back-reference (1-9).
    fn match_backref(&mut self, src_pos: usize, pat_pos: usize) -> LuaResult<Option<usize>> {
        // PUC-Rio's check_capture: l = digit - '1'; error if l < 0 || l >= level || unfinished
        let n = (self.pat[pat_pos + 1] - b'0') as usize;
        if n == 0 || n > self.captures.len() {
            return Err(LuaError::Runtime(RuntimeError {
                message: "invalid capture index".into(),
                level: 0,
                traceback: vec![],
            }));
        }
        let cap = self.captures[n - 1];
        let CaptureLen::Finished(len) = cap.len else {
            return Err(LuaError::Runtime(RuntimeError {
                message: "invalid capture index".into(),
                level: 0,
                traceback: vec![],
            }));
        };

        if src_pos + len > self.src.len() {
            return Ok(None);
        }
        if self.src[src_pos..src_pos + len] == self.src[cap.start..cap.start + len] {
            self.match_(src_pos + len, pat_pos + 2)
        } else {
            Ok(None)
        }
    }
}

// ---------------------------------------------------------------------------
// string.find
// ---------------------------------------------------------------------------

/// `string.find(s, pattern [, init [, plain]])` -- Find first match.
///
/// Returns start and end indices (1-based) plus captures, or nil.
pub fn str_find(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let pat = state.check_string(2)?;

    let plain_val = arg(state, 3);

    let init = state.opt_integer(3, 1).map(|i| posrelat(i, s.len()))?;
    let init = (init.max(1) as usize).saturating_sub(1); // Convert to 0-based.
    let plain = plain_val.is_truthy();

    if plain {
        // Plain string search.
        if let Some(pos) = find_plain(&s[init..], &pat) {
            let start = init + pos + 1; // 1-based.
            let end = start + pat.len() - 1;
            #[allow(clippy::cast_precision_loss)]
            {
                state.push(Val::Num(start as f64));
                state.push(Val::Num(end as f64));
            }
            Ok(2)
        } else {
            state.push(Val::Nil);
            Ok(1)
        }
    } else {
        // Pattern search.
        let anchor = !pat.is_empty() && pat[0] == b'^';
        let pat_start = usize::from(anchor);
        let pattern = &pat[pat_start..];

        let mut pos = init;
        loop {
            let mut ms = MatchState::new(&s, pattern);
            if let Some(end_pos) = ms.match_(pos, 0)? {
                #[allow(clippy::cast_precision_loss)]
                {
                    state.push(Val::Num((pos + 1) as f64));
                    state.push(Val::Num(end_pos as f64));
                }
                // Push captures.
                let n_caps = push_captures(state, &ms, &s)?;
                return Ok(2 + n_caps);
            }
            pos += 1;
            if anchor || pos > s.len() {
                break;
            }
        }
        state.push(Val::Nil);
        Ok(1)
    }
}

/// Plain substring search.
fn find_plain(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Push captures from a match state onto the Lua stack.
fn push_captures(state: &mut LuaState, ms: &MatchState<'_>, src: &[u8]) -> LuaResult<u32> {
    let n = ms.captures.len();
    if n == 0 {
        return Ok(0);
    }

    state.ensure_stack(state.top + n);
    for cap in &ms.captures {
        match cap.len {
            CaptureLen::Position => {
                #[allow(clippy::cast_precision_loss)]
                state.push(Val::Num((cap.start + 1) as f64));
            }
            CaptureLen::Finished(len) => {
                let r = state.gc.intern_string(&src[cap.start..cap.start + len]);
                state.push(Val::Str(r));
            }
            CaptureLen::Unfinished => {
                return Err(LuaError::Runtime(RuntimeError {
                    message: "unfinished capture".into(),
                    level: 0,
                    traceback: vec![],
                }));
            }
        }
    }
    Ok(n as u32)
}

// ---------------------------------------------------------------------------
// string.match
// ---------------------------------------------------------------------------

/// `string.match(s, pattern [, init])` -- Extract captures from first match.
pub fn str_match(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let pat = state.check_string(2)?;

    let init_raw = state.opt_integer(3, 1)?;
    let init = posrelat(init_raw, s.len());
    let init = (init.max(1) as usize).saturating_sub(1);

    let anchor = !pat.is_empty() && pat[0] == b'^';
    let pat_start = usize::from(anchor);
    let pattern = &pat[pat_start..];

    let mut pos = init;
    loop {
        let mut ms = MatchState::new(&s, pattern);
        if let Some(end_pos) = ms.match_(pos, 0)? {
            if ms.captures.is_empty() {
                // No captures: return the whole match.
                let r = state.gc.intern_string(&s[pos..end_pos]);
                state.push(Val::Str(r));
                return Ok(1);
            }
            return push_captures(state, &ms, &s).map(|n| if n == 0 { 1 } else { n });
        }
        pos += 1;
        if anchor || pos > s.len() {
            break;
        }
    }
    state.push(Val::Nil);
    Ok(1)
}

// ---------------------------------------------------------------------------
// string.gmatch
// ---------------------------------------------------------------------------

/// `string.gmatch(s, pattern)` -- Returns an iterator for all matches.
///
/// We implement this by creating a Rust closure that holds state.
pub fn str_gmatch(state: &mut LuaState) -> LuaResult<u32> {
    // Validate that the first two arguments exist and are convertible to strings.
    let _ = state.check_string(1)?;
    let _ = state.check_string(2)?;
    let s_val = arg(state, 0);
    let pat_val = arg(state, 1);

    // Store the source string and pattern as upvalues, plus the current position.
    let closure = crate::vm::closure::Closure::Rust(crate::vm::closure::RustClosure {
        func: gmatch_aux,
        upvalues: vec![s_val, pat_val, Val::Num(0.0)],
        name: "gmatch_aux".to_string(),
        env: None,
    });
    let closure_ref = state.gc.alloc_closure(closure);
    state.push(Val::Function(closure_ref));
    Ok(1)
}

/// Iterator function for gmatch. Upvalues: [source_string, pattern, position].
fn gmatch_aux(state: &mut LuaState) -> LuaResult<u32> {
    // Read upvalues from the closure.
    let func_idx = state.call_stack[state.ci].func;
    let func_val = state.stack_get(func_idx);
    let Val::Function(closure_ref) = func_val else {
        return Ok(0);
    };

    let (s_val, pat_val, pos_val) = {
        let cl = state.gc.closures.get(closure_ref).ok_or_else(|| {
            LuaError::Runtime(RuntimeError {
                message: "gmatch: invalid closure".into(),
                level: 0,
                traceback: vec![],
            })
        })?;
        let upvalues = match cl {
            crate::vm::closure::Closure::Rust(rc) => &rc.upvalues,
            crate::vm::closure::Closure::Lua(_) => {
                return Ok(0);
            }
        };
        if upvalues.len() < 3 {
            return Ok(0);
        }
        (upvalues[0], upvalues[1], upvalues[2])
    };

    let s = match s_val {
        Val::Str(r) => state
            .gc
            .string_arena
            .get(r)
            .map(|s| s.data().to_vec())
            .unwrap_or_default(),
        _ => return Ok(0),
    };
    let pat = match pat_val {
        Val::Str(r) => state
            .gc
            .string_arena
            .get(r)
            .map(|s| s.data().to_vec())
            .unwrap_or_default(),
        _ => return Ok(0),
    };
    let Val::Num(pos_f) = pos_val else {
        return Ok(0);
    };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut pos = pos_f as usize;

    let anchor = !pat.is_empty() && pat[0] == b'^';
    let pat_start = usize::from(anchor);
    let pattern = &pat[pat_start..];

    while pos <= s.len() {
        let mut ms = MatchState::new(&s, pattern);
        if let Some(end_pos) = ms.match_(pos, 0)? {
            // Update position upvalue. Ensure we advance at least 1 char
            // for empty matches to avoid infinite loops.
            let new_pos = if end_pos == pos { end_pos + 1 } else { end_pos };

            // Update the upvalue in the closure.
            if let Some(cl) = state.gc.closures.get_mut(closure_ref)
                && let crate::vm::closure::Closure::Rust(rc) = cl
            {
                #[allow(clippy::cast_precision_loss)]
                {
                    rc.upvalues[2] = Val::Num(new_pos as f64);
                }
            }

            if ms.captures.is_empty() {
                let r = state.gc.intern_string(&s[pos..end_pos]);
                state.push(Val::Str(r));
                return Ok(1);
            }
            return push_captures(state, &ms, &s);
        }
        pos += 1;
        if anchor {
            break;
        }
    }
    Ok(0)
}

// ---------------------------------------------------------------------------
// string.gsub
// ---------------------------------------------------------------------------

/// `string.gsub(s, pattern, repl [, n])` -- Global substitution.
///
/// `repl` can be a string, table, or function.
pub fn str_gsub(state: &mut LuaState) -> LuaResult<u32> {
    let s = state.check_string(1)?;
    let pat = state.check_string(2)?;
    // gsub.repl can be string, table, or function — fetch the raw Val.
    if nargs(state) < 3 {
        return Err(state.type_error(3, "string/function/table"));
    }
    let repl_val = arg(state, 2);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let max_replacements = state.opt_integer(4, i64::MAX)?.max(0) as usize;

    let anchor = !pat.is_empty() && pat[0] == b'^';
    let pat_start = usize::from(anchor);
    let pattern = &pat[pat_start..];

    let mut result = Vec::new();
    let mut pos = 0usize;
    let mut count = 0usize;

    while count < max_replacements && pos <= s.len() {
        let mut ms = MatchState::new(&s, pattern);
        let match_result = ms.match_(pos, 0)?;

        if let Some(end_pos) = match_result {
            count += 1;

            // Get the replacement.
            let replacement = get_gsub_replacement(state, &ms, &s, pos, end_pos, repl_val)?;
            result.extend_from_slice(&replacement);

            // Advance past the match. For empty matches, advance by 1 char.
            if end_pos == pos {
                if pos < s.len() {
                    result.push(s[pos]);
                }
                pos += 1;
            } else {
                pos = end_pos;
            }
        } else {
            if pos < s.len() {
                result.push(s[pos]);
            }
            pos += 1;
        }

        if anchor {
            break;
        }
    }

    // Append remaining unmatched portion.
    if pos <= s.len() {
        result.extend_from_slice(&s[pos..]);
    }

    let r = state.gc.intern_string(&result);
    state.push(Val::Str(r));
    #[allow(clippy::cast_precision_loss)]
    state.push(Val::Num(count as f64));
    Ok(2)
}

/// Gets the replacement string for a gsub match.
fn get_gsub_replacement(
    state: &mut LuaState,
    ms: &MatchState<'_>,
    src: &[u8],
    match_start: usize,
    match_end: usize,
    repl: Val,
) -> LuaResult<Vec<u8>> {
    match repl {
        Val::Str(r) => {
            // String replacement with capture references (%0-%9).
            let repl_bytes = state
                .gc
                .string_arena
                .get(r)
                .map(|s| s.data().to_vec())
                .unwrap_or_default();

            let mut result = Vec::new();
            let mut i = 0;
            while i < repl_bytes.len() {
                if repl_bytes[i] == b'%' && i + 1 < repl_bytes.len() {
                    let next = repl_bytes[i + 1];
                    if next.is_ascii_digit() {
                        let n = (next - b'0') as usize;
                        if n == 0 {
                            // %0 = whole match.
                            result.extend_from_slice(&src[match_start..match_end]);
                        } else {
                            // PUC-Rio push_onecapture: index = n-1
                            let cap_idx = n - 1;
                            if cap_idx >= ms.captures.len() {
                                // i >= level: if i == 0 (cap_idx == 0 && no captures),
                                // return whole match; otherwise error.
                                if cap_idx == 0 && ms.captures.is_empty() {
                                    result.extend_from_slice(&src[match_start..match_end]);
                                } else {
                                    return Err(LuaError::Runtime(RuntimeError {
                                        message: format!("invalid capture index %{n}"),
                                        level: 0,
                                        traceback: vec![],
                                    }));
                                }
                            } else {
                                let cap = ms.captures[cap_idx];
                                match cap.len {
                                    CaptureLen::Finished(len) => {
                                        result.extend_from_slice(&src[cap.start..cap.start + len]);
                                    }
                                    CaptureLen::Position => {
                                        let pos_str = format!("{}", cap.start + 1);
                                        result.extend_from_slice(pos_str.as_bytes());
                                    }
                                    CaptureLen::Unfinished => {
                                        return Err(LuaError::Runtime(RuntimeError {
                                            message: "unfinished capture".into(),
                                            level: 0,
                                            traceback: vec![],
                                        }));
                                    }
                                }
                            }
                        }
                        i += 2;
                        continue;
                    } else if next == b'%' {
                        result.push(b'%');
                        i += 2;
                        continue;
                    }
                }
                result.push(repl_bytes[i]);
                i += 1;
            }
            Ok(result)
        }
        Val::Table(_) => {
            // Table replacement: look up first capture (or whole match) as key.
            // Uses gettable for __index metamethod support.
            let key = if ms.captures.is_empty() {
                let r = state.gc.intern_string(&src[match_start..match_end]);
                Val::Str(r)
            } else {
                get_capture_val(state, &ms.captures[0], src)?
            };
            let val = state.gettable(repl, key)?;
            Ok(val_to_replacement(state, val, src, match_start, match_end))
        }
        Val::Function(_) => {
            // Function replacement: call with captures (or whole match).
            let call_base = state.top;
            state.ensure_stack(call_base + LUA_MAXCAPTURES + 2);
            state.stack_set(call_base, repl);

            if ms.captures.is_empty() {
                let r = state.gc.intern_string(&src[match_start..match_end]);
                state.stack_set(call_base + 1, Val::Str(r));
                state.top = call_base + 2;
            } else {
                let mut n = 0;
                for cap in &ms.captures {
                    let val = get_capture_val(state, cap, src)?;
                    state.stack_set(call_base + 1 + n, val);
                    n += 1;
                }
                state.top = call_base + 1 + n;
            }

            state.call_function(call_base, 1)?;

            let result_val = state.stack_get(call_base);
            state.top = call_base;

            Ok(val_to_replacement(
                state,
                result_val,
                src,
                match_start,
                match_end,
            ))
        }
        _ => {
            // Non-string, non-table, non-function: use as-is.
            Ok(src[match_start..match_end].to_vec())
        }
    }
}

/// Convert a capture to a Val.
fn get_capture_val(state: &mut LuaState, cap: &Capture, src: &[u8]) -> LuaResult<Val> {
    match cap.len {
        CaptureLen::Position =>
        {
            #[allow(clippy::cast_precision_loss)]
            Ok(Val::Num((cap.start + 1) as f64))
        }
        CaptureLen::Finished(len) => {
            let r = state.gc.intern_string(&src[cap.start..cap.start + len]);
            Ok(Val::Str(r))
        }
        CaptureLen::Unfinished => Err(LuaError::Runtime(RuntimeError {
            message: "unfinished capture".into(),
            level: 0,
            traceback: vec![],
        })),
    }
}

/// Convert a replacement value to bytes. If nil/false, use the original match.
fn val_to_replacement(
    state: &LuaState,
    val: Val,
    src: &[u8],
    match_start: usize,
    match_end: usize,
) -> Vec<u8> {
    if val.is_nil() || val == Val::Bool(false) {
        return src[match_start..match_end].to_vec();
    }
    match val {
        Val::Str(r) => state
            .gc
            .string_arena
            .get(r)
            .map(|s| s.data().to_vec())
            .unwrap_or_default(),
        _ => format!("{val}").into_bytes(),
    }
}

// ---------------------------------------------------------------------------
// string.dump (stub)
// ---------------------------------------------------------------------------

/// `string.dump(function)` -- Serializes a function to a binary string.
///
/// Stub: not yet implemented (requires bytecode serialization).
pub fn str_dump(state: &mut LuaState) -> LuaResult<u32> {
    let func_val = arg(state, 0);
    let Val::Function(closure_ref) = func_val else {
        return Err(state.type_error(1, "function"));
    };

    // Get the Proto from the closure (must be a Lua closure).
    let proto = {
        let Some(closure) = state.gc.closures.get(closure_ref) else {
            return Err(LuaError::Runtime(RuntimeError {
                message: "unable to dump given function".into(),
                level: 0,
                traceback: vec![],
            }));
        };
        match closure {
            crate::vm::closure::Closure::Lua(lua_cl) => {
                crate::vm::proto::ProtoRef::clone(&lua_cl.proto)
            }
            crate::vm::closure::Closure::Rust(_) => {
                return Err(LuaError::Runtime(RuntimeError {
                    message: "unable to dump given function".into(),
                    level: 0,
                    traceback: vec![],
                }));
            }
        }
    };

    // Dump with the string arena (patched Proto from a live closure).
    let bytes = crate::vm::dump::dump(&proto, Some(&state.gc.string_arena), false);

    // Intern the result bytes as a Lua string.
    let str_ref = state.gc.intern_string(&bytes);
    state.push(Val::Str(str_ref));
    Ok(1)
}
