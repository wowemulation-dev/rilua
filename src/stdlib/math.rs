//! Math library: mathematical functions wrapping Rust's f64 methods.
//!
//! Reference: `lmathlib.c` in PUC-Rio Lua 5.1.1.

use crate::error::{LuaError, LuaResult, RuntimeError};
use crate::vm::state::LuaState;
use crate::vm::value::Val;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PI: f64 = std::f64::consts::PI;
const RADIANS_PER_DEGREE: f64 = PI / 180.0;

/// RAND_MAX matching common C implementations (2^31 - 1).
const RAND_MAX: u64 = 0x7FFF_FFFF;

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

#[inline]
fn nargs(state: &LuaState) -> usize {
    state.top.saturating_sub(state.base)
}

// ---------------------------------------------------------------------------
// Return helpers
// ---------------------------------------------------------------------------

#[inline]
#[allow(clippy::unnecessary_wraps)]
fn push_num(state: &mut LuaState, n: f64) -> LuaResult<u32> {
    state.stack_set(state.base, Val::Num(n));
    state.top = state.base + 1;
    Ok(1)
}

// ---------------------------------------------------------------------------
// Single-argument math functions
// ---------------------------------------------------------------------------

pub fn math_abs(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.abs())
}

pub fn math_sin(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.sin())
}

pub fn math_cos(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.cos())
}

pub fn math_tan(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.tan())
}

pub fn math_sinh(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.sinh())
}

pub fn math_cosh(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.cosh())
}

pub fn math_tanh(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.tanh())
}

pub fn math_asin(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.asin())
}

pub fn math_acos(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.acos())
}

pub fn math_atan(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.atan())
}

pub fn math_ceil(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.ceil())
}

pub fn math_floor(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.floor())
}

pub fn math_sqrt(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.sqrt())
}

pub fn math_exp(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.exp())
}

pub fn math_log(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.ln())
}

pub fn math_log10(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x.log10())
}

// ---------------------------------------------------------------------------
// Two-argument math functions
// ---------------------------------------------------------------------------

pub fn math_atan2(state: &mut LuaState) -> LuaResult<u32> {
    let y = state.check_number(1)?;
    let x = state.check_number(2)?;
    push_num(state, y.atan2(x))
}

pub fn math_fmod(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    let y = state.check_number(2)?;
    // Rust's % operator for f64 is IEEE 754 remainder (same as C fmod).
    push_num(state, x % y)
}

pub fn math_pow(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    let y = state.check_number(2)?;
    push_num(state, x.powf(y))
}

pub fn math_ldexp(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    #[allow(clippy::cast_possible_truncation)]
    let e = state.check_integer(2)? as i32;
    push_num(state, ldexp(x, e))
}

// ---------------------------------------------------------------------------
// Angle conversion
// ---------------------------------------------------------------------------

pub fn math_deg(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x / RADIANS_PER_DEGREE)
}

pub fn math_rad(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    push_num(state, x * RADIANS_PER_DEGREE)
}

// ---------------------------------------------------------------------------
// Multi-return functions
// ---------------------------------------------------------------------------

/// `math.modf(x)` -- returns integer part and fractional part.
pub fn math_modf(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    let ip = x.trunc();
    let fp = x - ip;
    state.ensure_stack(state.base + 2);
    state.stack_set(state.base, Val::Num(ip));
    state.stack_set(state.base + 1, Val::Num(fp));
    state.top = state.base + 2;
    Ok(2)
}

/// `math.frexp(x)` -- returns mantissa and exponent such that x = m * 2^e.
pub fn math_frexp(state: &mut LuaState) -> LuaResult<u32> {
    let x = state.check_number(1)?;
    let (m, e) = frexp(x);
    state.ensure_stack(state.base + 2);
    state.stack_set(state.base, Val::Num(m));
    state.stack_set(state.base + 1, Val::Num(f64::from(e)));
    state.top = state.base + 2;
    Ok(2)
}

// ---------------------------------------------------------------------------
// Variadic functions
// ---------------------------------------------------------------------------

/// `math.min(x, ...)` -- returns the minimum of its arguments.
pub fn math_min(state: &mut LuaState) -> LuaResult<u32> {
    let n = nargs(state);
    if n == 0 {
        return Err(state.type_error(1, "number"));
    }
    let mut dmin = state.check_number(1)?;
    for i in 2..=n {
        let d = state.check_number(i)?;
        if d < dmin {
            dmin = d;
        }
    }
    push_num(state, dmin)
}

/// `math.max(x, ...)` -- returns the maximum of its arguments.
pub fn math_max(state: &mut LuaState) -> LuaResult<u32> {
    let n = nargs(state);
    if n == 0 {
        return Err(state.type_error(1, "number"));
    }
    let mut dmax = state.check_number(1)?;
    for i in 2..=n {
        let d = state.check_number(i)?;
        if d > dmax {
            dmax = d;
        }
    }
    push_num(state, dmax)
}

// ---------------------------------------------------------------------------
// Random number generation
// ---------------------------------------------------------------------------

/// Linear congruential generator step.
///
/// Uses glibc-compatible constants. Returns value in `[0, 2^31 - 1]`.
fn rng_next(state: &mut LuaState) -> u64 {
    // glibc LCG: next = (state * 1103515245 + 12345) mod 2^31
    state.rng_state = (state
        .rng_state
        .wrapping_mul(1_103_515_245)
        .wrapping_add(12345))
        & RAND_MAX;
    state.rng_state
}

/// `math.random([m [, n]])` -- pseudo-random number generator.
///
/// - No arguments: returns a uniform random float in `[0, 1)`.
/// - One argument `u`: returns a random integer in `[1, u]`.
/// - Two arguments `l, u`: returns a random integer in `[l, u]`.
pub fn math_random(state: &mut LuaState) -> LuaResult<u32> {
    // Generate base random value r in [0, 1).
    // The `%` avoids the (rare) case of r==1, matching PUC-Rio.
    let raw = rng_next(state);
    #[allow(clippy::cast_precision_loss)]
    let r: f64 = (raw % RAND_MAX) as f64 / RAND_MAX as f64;

    let n = nargs(state);
    match n {
        0 => {
            // No arguments: float in [0, 1).
            push_num(state, r)
        }
        1 => {
            // One argument: integer in [1, u].
            let u = state.check_integer(1)?;
            if u < 1 {
                return Err(state.arg_error(1, "interval is empty"));
            }
            #[allow(clippy::cast_precision_loss)]
            let result = (r * u as f64).floor() + 1.0;
            push_num(state, result)
        }
        2 => {
            // Two arguments: integer in [l, u].
            let l = state.check_integer(1)?;
            let u = state.check_integer(2)?;
            if l > u {
                return Err(state.arg_error(2, "interval is empty"));
            }
            #[allow(clippy::cast_precision_loss)]
            let result = (r * (u - l + 1) as f64).floor() + l as f64;
            push_num(state, result)
        }
        _ => Err(LuaError::Runtime(RuntimeError {
            message: "wrong number of arguments".into(),
            level: 0,
            traceback: vec![],
        })),
    }
}

/// `math.randomseed(x)` -- sets the seed for the pseudo-random generator.
pub fn math_randomseed(state: &mut LuaState) -> LuaResult<u32> {
    let seed = state.check_integer(1)?;
    #[allow(clippy::cast_sign_loss)]
    {
        state.rng_state = seed as u64;
    }
    state.top = state.base;
    Ok(0)
}

// ---------------------------------------------------------------------------
// frexp / ldexp -- IEEE 754 bit manipulation
// ---------------------------------------------------------------------------

/// Decomposes `x` into mantissa `m` and exponent `e` such that
/// `x = m * 2^e`, where `0.5 <= |m| < 1` (or `m = 0` for zero input).
///
/// Equivalent to C's `frexp()`. Implemented via IEEE 754 bit manipulation
/// since Rust's standard library does not provide `frexp`.
fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return (x, 0);
    }

    let bits = x.to_bits();
    let sign = bits & (1_u64 << 63);
    let exponent = ((bits >> 52) & 0x7FF) as i32;

    if exponent == 0 {
        // Subnormal: normalize by scaling up.
        let (m, e) = frexp(x * f64::from_bits(0x4330_0000_0000_0000)); // x * 2^52
        return (m, e - 52);
    }

    // Normal number: set biased exponent to 1022 -> value in [0.5, 1.0).
    // Biased exponent 1022 represents 2^(1022 - 1023) = 2^(-1).
    let mantissa_bits = sign | (0x3FE_u64 << 52) | (bits & 0x000F_FFFF_FFFF_FFFF);
    let mantissa = f64::from_bits(mantissa_bits);
    let exp = exponent - 1022;

    (mantissa, exp)
}

/// Computes `x * 2^e`. Equivalent to C's `ldexp()`.
fn ldexp(x: f64, e: i32) -> f64 {
    // Use successive multiplications to handle large exponents that
    // would overflow a single 2^e construction.
    // This matches the behavior of C ldexp for edge cases.
    let mut result = x;
    let mut exp = e;

    // Process in chunks of 1023 (max normal exponent).
    while exp > 1023 {
        result *= f64::from_bits(0x7FE0_0000_0000_0000); // 2^1023
        exp -= 1023;
    }
    while exp < -1022 {
        result *= f64::from_bits(0x0010_0000_0000_0000); // 2^(-1022)
        exp += 1022;
    }

    // Final multiplication with remaining exponent.
    #[allow(clippy::cast_sign_loss)]
    let bias = ((1023 + exp) as u64) << 52;
    result * f64::from_bits(bias)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::suboptimal_flops)]
mod tests {
    use super::*;

    // --- frexp tests ---

    #[test]
    fn frexp_normal() {
        let (m, e) = frexp(8.0);
        assert!((0.5..1.0).contains(&m.abs()));
        assert!((m * 2.0_f64.powi(e) - 8.0).abs() < f64::EPSILON);
    }

    #[test]
    fn frexp_pi() {
        let (m, e) = frexp(PI);
        assert!((0.5..1.0).contains(&m.abs()));
        let reconstructed = m * 2.0_f64.powi(e);
        assert!(
            (reconstructed - PI).abs() < f64::EPSILON,
            "frexp(pi) roundtrip: {reconstructed} != {PI}"
        );
    }

    #[test]
    fn frexp_negative() {
        let (m, e) = frexp(-3.0);
        assert!(m < 0.0);
        assert!((0.5..1.0).contains(&m.abs()));
        assert!((m * 2.0_f64.powi(e) - (-3.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn frexp_zero() {
        let (m, e) = frexp(0.0);
        assert_eq!(m, 0.0);
        assert_eq!(e, 0);
    }

    #[test]
    fn frexp_one() {
        let (m, e) = frexp(1.0);
        assert!((m - 0.5).abs() < f64::EPSILON);
        assert_eq!(e, 1);
    }

    #[test]
    fn frexp_nan() {
        let (m, _e) = frexp(f64::NAN);
        assert!(m.is_nan());
    }

    #[test]
    fn frexp_infinity() {
        let (m, _e) = frexp(f64::INFINITY);
        assert!(m.is_infinite());
    }

    #[test]
    fn frexp_subnormal() {
        let x = 5e-324_f64; // smallest subnormal
        let (m, e) = frexp(x);
        assert!((0.5..1.0).contains(&m.abs()));
        let reconstructed = ldexp(m, e);
        assert_eq!(reconstructed, x);
    }

    // --- ldexp tests ---

    #[test]
    fn ldexp_basic() {
        assert!((ldexp(0.5, 1) - 1.0).abs() < f64::EPSILON);
        assert!((ldexp(0.5, 2) - 2.0).abs() < f64::EPSILON);
        assert!((ldexp(1.0, 10) - 1024.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ldexp_negative_exp() {
        assert!((ldexp(1.0, -1) - 0.5).abs() < f64::EPSILON);
        assert!((ldexp(1.0, -10) - (1.0 / 1024.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn ldexp_large_exp() {
        assert!(ldexp(1.0, 1024).is_infinite());
    }

    #[test]
    fn ldexp_zero() {
        assert_eq!(ldexp(0.0, 100), 0.0);
    }

    #[test]
    fn frexp_ldexp_roundtrip() {
        for &x in &[1.0, -1.0, PI, 0.001, 1e100, 1e-100, 0.5, 256.0] {
            let (m, e) = frexp(x);
            let reconstructed = ldexp(m, e);
            assert!(
                (reconstructed - x).abs() < x.abs() * f64::EPSILON * 2.0,
                "roundtrip failed for {x}: frexp -> ({m}, {e}) -> {reconstructed}"
            );
        }
    }
}
