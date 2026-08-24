//! Centralized platform abstraction layer.
//!
//! Libc FFI bindings that cannot be replaced live here so that consumer
//! modules (`io.rs`, `os.rs`, `execute.rs`) stay platform-agnostic.
//! Platform differences are hidden behind safe wrapper functions with
//! `#[cfg(target_os)]` dispatch.
//!
//! Where possible, C functions are replaced with Rust std equivalents:
//! - `time(NULL)` -> `current_time()` via `SystemTime`
//! - `errno`/`strerror` -> `std::io::Error::last_os_error()` (in consumers)
//! - `mkstemp`/`tmpnam` -> `c_tmpname()` via `File::create_new()`
//! - `isatty` -> `std::io::IsTerminal` (in `rilua.rs` binary)
//!
//! On WASM (`target_arch = "wasm32"`), all C FFI is replaced with
//! pure-Rust stubs. Core VM functions (`strtod`, `localeconv`, `strcoll`)
//! use ASCII-only equivalents. Stdlib I/O and OS functions return errors.
//!
//! Remaining FFI on native targets (locale, broken-down time, FILE*
//! streams) has no Rust std equivalent and must stay as C bindings.
//!
//! Reference: PUC-Rio's `luaconf.h` for the equivalent C approach.

// ---------------------------------------------------------------------------
// Opaque C FILE type
// ---------------------------------------------------------------------------

/// Opaque C `FILE` type -- never instantiated, only used as `*mut`.
pub(crate) enum LibcFile {}

// ---------------------------------------------------------------------------
// Portable C functions (identical on all native platforms)
// ---------------------------------------------------------------------------

// On MSVC, `fprintf`/`fscanf` are inline functions in headers since VS2015.
// The non-inline definitions live in `legacy_stdio_definitions.lib`.
// All other C runtime functions are in `ucrt.lib`.
#[cfg(not(target_arch = "wasm32"))]
#[allow(unsafe_code)]
#[cfg_attr(target_env = "msvc", link(name = "ucrt"))]
#[cfg_attr(target_env = "msvc", link(name = "legacy_stdio_definitions"))]
#[allow(unknown_lints)]
#[allow(suspicious_runtime_symbol_definitions)]
unsafe extern "C" {
    pub(crate) fn fopen(filename: *const u8, mode: *const u8) -> *mut LibcFile;
    pub(crate) fn fclose(file: *mut LibcFile) -> i32;
    pub(crate) fn fflush(file: *mut LibcFile) -> i32;
    pub(crate) fn fread(ptr: *mut u8, size: usize, nmemb: usize, stream: *mut LibcFile) -> usize;
    pub(crate) fn fwrite(ptr: *const u8, size: usize, nmemb: usize, stream: *mut LibcFile)
    -> usize;
    pub(crate) fn fgets(s: *mut u8, n: i32, stream: *mut LibcFile) -> *mut u8;
    pub(crate) fn fseek(stream: *mut LibcFile, offset: i64, whence: i32) -> i32;
    pub(crate) fn ftell(stream: *mut LibcFile) -> i64;
    pub(crate) fn ferror(stream: *mut LibcFile) -> i32;
    pub(crate) fn clearerr(stream: *mut LibcFile);
    #[allow(dead_code)]
    pub(crate) fn feof(stream: *mut LibcFile) -> i32;
    pub(crate) fn getc(stream: *mut LibcFile) -> i32;
    pub(crate) fn ungetc(c: i32, stream: *mut LibcFile) -> i32;
    pub(crate) fn setvbuf(stream: *mut LibcFile, buf: *mut u8, mode: i32, size: usize) -> i32;
    pub(crate) fn tmpfile() -> *mut LibcFile;
    pub(crate) fn fscanf(stream: *mut LibcFile, format: *const u8, ...) -> i32;
    pub(crate) fn fprintf(stream: *mut LibcFile, format: *const u8, ...) -> i32;

    pub(crate) fn strlen(s: *const u8) -> usize;

    // Number parsing / locale
    pub(crate) fn strtod(nptr: *const u8, endptr: *mut *mut u8) -> f64;
    pub(crate) fn localeconv() -> *const LConv;
    pub(crate) fn strcoll(s1: *const u8, s2: *const u8) -> i32;

    // Time functions (clock, mktime, strftime, setlocale still need FFI;
    // time(NULL) replaced by current_time() using SystemTime)
    pub(crate) fn clock() -> ClockT;
    pub(crate) fn mktime(tm: *mut Tm) -> TimeT;
    pub(crate) fn strftime(s: *mut u8, max: usize, format: *const u8, tm: *const Tm) -> usize;
    pub(crate) fn setlocale(category: i32, locale: *const u8) -> *const u8;

    // Character classification and case conversion (ctype.h).
    // Used by string.lower, string.upper, and pattern matching.
    pub(crate) fn isalpha(c: i32) -> i32;
    pub(crate) fn iscntrl(c: i32) -> i32;
    pub(crate) fn isdigit(c: i32) -> i32;
    pub(crate) fn islower(c: i32) -> i32;
    pub(crate) fn ispunct(c: i32) -> i32;
    pub(crate) fn isspace(c: i32) -> i32;
    pub(crate) fn isupper(c: i32) -> i32;
    pub(crate) fn isalnum(c: i32) -> i32;
    pub(crate) fn isxdigit(c: i32) -> i32;
    pub(crate) fn tolower(c: i32) -> i32;
    pub(crate) fn toupper(c: i32) -> i32;
}

// ---------------------------------------------------------------------------
// WASM stubs -- pure-Rust replacements for all C FFI
// ---------------------------------------------------------------------------

/// WASM stubs for C functions. FILE* operations return error values.
/// Core VM functions (`strtod`, `localeconv`, `strcoll`) use pure-Rust
/// implementations.
///
/// All stubs are marked `unsafe` to match the `extern "C"` signatures
/// they replace, so call sites don't need `#[cfg]`-gated `unsafe` blocks.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
pub(crate) mod wasm_stubs {
    use super::*;

    // -- FILE* stream operations (all return error / null / 0) --

    pub(crate) unsafe fn fopen(_filename: *const u8, _mode: *const u8) -> *mut LibcFile {
        std::ptr::null_mut()
    }

    pub(crate) unsafe fn fclose(_file: *mut LibcFile) -> i32 {
        -1
    }

    pub(crate) unsafe fn fflush(_file: *mut LibcFile) -> i32 {
        -1
    }

    pub(crate) unsafe fn fread(
        _ptr: *mut u8,
        _size: usize,
        _nmemb: usize,
        _stream: *mut LibcFile,
    ) -> usize {
        0
    }

    pub(crate) unsafe fn fwrite(
        _ptr: *const u8,
        _size: usize,
        _nmemb: usize,
        _stream: *mut LibcFile,
    ) -> usize {
        0
    }

    pub(crate) unsafe fn fgets(_s: *mut u8, _n: i32, _stream: *mut LibcFile) -> *mut u8 {
        std::ptr::null_mut()
    }

    pub(crate) unsafe fn fseek(_stream: *mut LibcFile, _offset: i64, _whence: i32) -> i32 {
        -1
    }

    pub(crate) unsafe fn ftell(_stream: *mut LibcFile) -> i64 {
        -1
    }

    pub(crate) unsafe fn ferror(_stream: *mut LibcFile) -> i32 {
        1
    }

    pub(crate) unsafe fn clearerr(_stream: *mut LibcFile) {}

    #[allow(dead_code)]
    pub(crate) unsafe fn feof(_stream: *mut LibcFile) -> i32 {
        1
    }

    pub(crate) unsafe fn getc(_stream: *mut LibcFile) -> i32 {
        -1 // EOF
    }

    pub(crate) unsafe fn ungetc(_c: i32, _stream: *mut LibcFile) -> i32 {
        -1 // EOF
    }

    pub(crate) unsafe fn setvbuf(
        _stream: *mut LibcFile,
        _buf: *mut u8,
        _mode: i32,
        _size: usize,
    ) -> i32 {
        -1
    }

    pub(crate) unsafe fn tmpfile() -> *mut LibcFile {
        std::ptr::null_mut()
    }

    pub(crate) unsafe fn strlen(s: *const u8) -> usize {
        if s.is_null() {
            return 0;
        }
        let mut len = 0;
        // SAFETY: walks until NUL byte, caller must provide valid C string.
        unsafe {
            while *s.add(len) != 0 {
                len += 1;
            }
        }
        len
    }

    // -- Core VM: number parsing / locale --

    /// Pure-Rust `strtod` replacement.
    ///
    /// Parses a NUL-terminated byte string as a float. Handles decimal
    /// and leading whitespace. Returns the parsed value and sets `*endptr`
    /// to point past the consumed bytes.
    pub(crate) unsafe fn strtod(nptr: *const u8, endptr: *mut *mut u8) -> f64 {
        // SAFETY: All pointer operations below require that `nptr` points to
        // a valid NUL-terminated byte string. The caller (luaO_str2d) ensures
        // this by passing a NUL-terminated buffer.
        unsafe {
            if nptr.is_null() {
                if !endptr.is_null() {
                    *endptr = nptr.cast_mut();
                }
                return 0.0;
            }

            // Read bytes until NUL.
            let mut len = 0;
            while *nptr.add(len) != 0 {
                len += 1;
            }
            let bytes = std::slice::from_raw_parts(nptr, len);

            // Skip leading whitespace.
            let start = bytes
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(len);

            if start >= len {
                if !endptr.is_null() {
                    *endptr = nptr.cast_mut();
                }
                return 0.0;
            }

            let rest = &bytes[start..];

            // Try parsing as a Rust float (handles decimal, inf, nan).
            let Ok(s) = std::str::from_utf8(rest) else {
                if !endptr.is_null() {
                    *endptr = nptr.cast_mut();
                }
                return 0.0;
            };

            // Try progressively shorter prefixes to find longest valid parse.
            let mut best_len = 0;
            let mut best_val = 0.0;
            for end in (1..=s.len()).rev() {
                if let Ok(val) = s[..end].parse::<f64>() {
                    best_len = end;
                    best_val = val;
                    break;
                }
            }

            if !endptr.is_null() {
                *endptr = nptr.add(start + best_len).cast_mut();
            }
            best_val
        }
    }

    /// Returns a static `LConv` with decimal_point = '.'.
    static WASM_LCONV: LConv = LConv {
        decimal_point: c".".as_ptr().cast(),
    };

    pub(crate) unsafe fn localeconv() -> *const LConv {
        &raw const WASM_LCONV
    }

    /// Byte-wise string comparison (no locale awareness).
    pub(crate) unsafe fn strcoll(s1: *const u8, s2: *const u8) -> i32 {
        // SAFETY: Caller must provide valid NUL-terminated strings.
        unsafe {
            let len1 = strlen(s1);
            let len2 = strlen(s2);
            let a = std::slice::from_raw_parts(s1, len1);
            let b = std::slice::from_raw_parts(s2, len2);
            match a.cmp(b) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }
        }
    }

    // -- Time / locale (return errors or zero) --

    pub(crate) unsafe fn clock() -> ClockT {
        0
    }

    pub(crate) unsafe fn mktime(_tm: *mut Tm) -> TimeT {
        -1
    }

    pub(crate) unsafe fn strftime(
        _s: *mut u8,
        _max: usize,
        _format: *const u8,
        _tm: *const Tm,
    ) -> usize {
        0
    }

    pub(crate) unsafe fn setlocale(_category: i32, _locale: *const u8) -> *const u8 {
        std::ptr::null()
    }

    // -- Character classification (ASCII-only on WASM) --
    // The C ctype functions take an `int` (unsigned char promoted).
    // We truncate to u8 and use Rust's ASCII methods.

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn isalpha(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_alphabetic())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn iscntrl(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_control())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn isdigit(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_digit())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn islower(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_lowercase())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn ispunct(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_punctuation())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn isspace(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_whitespace())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn isupper(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_uppercase())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn isalnum(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_alphanumeric())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn isxdigit(c: i32) -> i32 {
        i32::from((c as u8).is_ascii_hexdigit())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn tolower(c: i32) -> i32 {
        i32::from((c as u8).to_ascii_lowercase())
    }

    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    pub(crate) unsafe fn toupper(c: i32) -> i32 {
        i32::from((c as u8).to_ascii_uppercase())
    }
}

// Re-export WASM stubs as if they were the extern functions.
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_stubs::{
    clearerr, clock, fclose, ferror, fflush, fgets, fopen, fread, fseek, ftell, fwrite, getc,
    isalnum, isalpha, iscntrl, isdigit, islower, ispunct, isspace, isupper, isxdigit, localeconv,
    mktime, setlocale, setvbuf, strcoll, strftime, strlen, strtod, tmpfile, tolower, toupper,
    ungetc,
};

// ---------------------------------------------------------------------------
// fprintf/fscanf wrappers for number I/O
// ---------------------------------------------------------------------------
//
// fprintf and fscanf are C variadic functions. Rust stable cannot declare
// variadic fn signatures outside extern "C" blocks. Instead of exposing
// them directly, we provide purpose-specific wrappers that io.rs calls.

/// Writes a float to `stream` using C's `fprintf(stream, "%.14g", d)`.
///
/// Returns the number of bytes written, or a negative value on error.
/// On WASM, always returns -1 (no I/O).
#[allow(unsafe_code)]
pub(crate) fn c_fprintf_number(stream: *mut LibcFile, d: f64) -> i32 {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (stream, d);
        -1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let fmt = b"%.14g\0";
        // SAFETY: fprintf is a C function, stream must be a valid FILE*.
        unsafe { fprintf(stream, fmt.as_ptr(), d) }
    }
}

/// Reads a float from `stream` using C's `fscanf(stream, "%lf", &d)`.
///
/// Returns `Some(value)` on success, `None` if no number could be read.
/// On WASM, always returns `None` (no I/O).
#[allow(unsafe_code)]
pub(crate) fn c_fscanf_number(stream: *mut LibcFile) -> Option<f64> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = stream;
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut d: f64 = 0.0;
        let fmt = b"%lf\0";
        // SAFETY: fscanf is a C function, stream must be a valid FILE*.
        let result = unsafe { fscanf(stream, fmt.as_ptr(), &raw mut d) };
        if result == 1 { Some(d) } else { None }
    }
}

// ---------------------------------------------------------------------------
// Minimal struct lconv -- only decimal_point is needed
// ---------------------------------------------------------------------------

/// Minimal `struct lconv` -- we only need the `decimal_point` field.
#[repr(C)]
pub(crate) struct LConv {
    pub(crate) decimal_point: *const u8,
    // remaining fields omitted
}

// SAFETY: LConv only contains a raw pointer to a static byte string.
// The WASM static instance is immutable and lives for the program lifetime.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
unsafe impl Sync for LConv {}

// ---------------------------------------------------------------------------
// Time types
// ---------------------------------------------------------------------------

/// C `time_t` -- signed 64-bit on modern Linux/macOS, also on 64-bit Windows.
pub(crate) type TimeT = i64;

/// C `clock_t` type.
#[cfg(not(target_os = "windows"))]
pub(crate) type ClockT = i64;

#[cfg(target_os = "windows")]
pub(crate) type ClockT = i32;

/// `CLOCKS_PER_SEC`: 1_000_000 on POSIX, 1_000 on Windows.
#[cfg(not(target_os = "windows"))]
pub(crate) const CLOCKS_PER_SEC: f64 = 1_000_000.0;

#[cfg(target_os = "windows")]
pub(crate) const CLOCKS_PER_SEC: f64 = 1_000.0;

// ---------------------------------------------------------------------------
// struct tm
// ---------------------------------------------------------------------------

/// C `struct tm` for broken-down time.
///
/// Field names mirror the C `struct tm` convention (`tm_sec`, `tm_min`, etc.)
/// to maintain clarity about the FFI mapping.
#[repr(C)]
#[allow(clippy::struct_field_names)]
pub(crate) struct Tm {
    pub(crate) tm_sec: i32,
    pub(crate) tm_min: i32,
    pub(crate) tm_hour: i32,
    pub(crate) tm_mday: i32,
    pub(crate) tm_mon: i32,
    pub(crate) tm_year: i32,
    pub(crate) tm_wday: i32,
    pub(crate) tm_yday: i32,
    pub(crate) tm_isdst: i32,
    // glibc/BSD extensions (must be present for correct struct size on
    // Linux and macOS):
    #[cfg(not(any(target_os = "windows", target_arch = "wasm32")))]
    tm_gmtoff: i64,
    #[cfg(not(any(target_os = "windows", target_arch = "wasm32")))]
    tm_zone: *const i8,
}

// On Windows (no extra fields) clippy says this is derivable, but on
// Linux/macOS the *const i8 field prevents #[derive(Default)].
#[allow(clippy::derivable_impls)]
impl Default for Tm {
    fn default() -> Self {
        Self {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            #[cfg(not(any(target_os = "windows", target_arch = "wasm32")))]
            tm_gmtoff: 0,
            #[cfg(not(any(target_os = "windows", target_arch = "wasm32")))]
            tm_zone: std::ptr::null(),
        }
    }
}

// ---------------------------------------------------------------------------
// Current time (pure Rust replacement for time(NULL))
// ---------------------------------------------------------------------------

/// Returns the current time as seconds since the Unix epoch.
///
/// Equivalent to C's `time(NULL)`. Uses `SystemTime` to avoid FFI.
/// On WASM, `SystemTime::now()` may not be available, so returns 0.
pub(crate) fn current_time() -> TimeT {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        // SystemTime::now() can theoretically be before UNIX_EPOCH on misconfigured
        // systems; fall back to 0 (same as what time(NULL) would do on error: -1,
        // but 0 is safer for Lua's os.time()).
        #[allow(clippy::cast_possible_wrap)]
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => d.as_secs() as TimeT,
            Err(_) => 0,
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

// ---------------------------------------------------------------------------
// Standard streams (stdin, stdout, stderr)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
unsafe extern "C" {
    static stdin: *mut LibcFile;
    static stdout: *mut LibcFile;
    static stderr: *mut LibcFile;
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
unsafe extern "C" {
    #[link_name = "__stdinp"]
    static stdin: *mut LibcFile;
    #[link_name = "__stdoutp"]
    static stdout: *mut LibcFile;
    #[link_name = "__stderrp"]
    static stderr: *mut LibcFile;
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
#[link(name = "ucrt")]
unsafe extern "C" {
    fn __acrt_iob_func(index: u32) -> *mut LibcFile;
}

/// Returns a pointer to C `stdin`.
#[allow(unsafe_code)]
pub(crate) fn c_stdin() -> *mut LibcFile {
    #[cfg(target_arch = "wasm32")]
    {
        std::ptr::null_mut()
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        #[cfg(not(target_os = "windows"))]
        {
            stdin
        }
        #[cfg(target_os = "windows")]
        {
            __acrt_iob_func(0)
        }
    }
}

/// Returns a pointer to C `stdout`.
#[allow(unsafe_code)]
pub(crate) fn c_stdout() -> *mut LibcFile {
    #[cfg(target_arch = "wasm32")]
    {
        std::ptr::null_mut()
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        #[cfg(not(target_os = "windows"))]
        {
            stdout
        }
        #[cfg(target_os = "windows")]
        {
            __acrt_iob_func(1)
        }
    }
}

/// Returns a pointer to C `stderr`.
#[allow(unsafe_code)]
pub(crate) fn c_stderr() -> *mut LibcFile {
    #[cfg(target_arch = "wasm32")]
    {
        std::ptr::null_mut()
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        #[cfg(not(target_os = "windows"))]
        {
            stderr
        }
        #[cfg(target_os = "windows")]
        {
            __acrt_iob_func(2)
        }
    }
}

// ---------------------------------------------------------------------------
// popen / pclose
// ---------------------------------------------------------------------------

#[cfg(all(not(target_os = "windows"), not(target_arch = "wasm32")))]
#[allow(unsafe_code)]
unsafe extern "C" {
    fn popen(command: *const u8, r#type: *const u8) -> *mut LibcFile;
    fn pclose(stream: *mut LibcFile) -> i32;
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
#[link(name = "ucrt")]
unsafe extern "C" {
    #[link_name = "_popen"]
    fn popen(command: *const u8, r#type: *const u8) -> *mut LibcFile;
    #[link_name = "_pclose"]
    fn pclose(stream: *mut LibcFile) -> i32;
}

/// Opens a pipe to a command (POSIX `popen` / Windows `_popen`).
#[allow(unsafe_code)]
pub(crate) fn c_popen(command: *const u8, mode: *const u8) -> *mut LibcFile {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (command, mode);
        std::ptr::null_mut()
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        popen(command, mode)
    }
}

/// Closes a pipe opened with `c_popen`.
#[allow(unsafe_code)]
pub(crate) fn c_pclose(stream: *mut LibcFile) -> i32 {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = stream;
        -1
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        pclose(stream)
    }
}

// ---------------------------------------------------------------------------
// localtime / gmtime (thread-safe variants)
// ---------------------------------------------------------------------------

#[cfg(all(not(target_os = "windows"), not(target_arch = "wasm32")))]
#[allow(unsafe_code)]
unsafe extern "C" {
    fn localtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm;
    fn gmtime_r(timep: *const TimeT, result: *mut Tm) -> *mut Tm;
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
#[link(name = "ucrt")]
unsafe extern "C" {
    // Windows reverses parameter order and returns errno_t.
    // On 64-bit MSVC, `localtime_s`/`gmtime_s` are header-level wrappers
    // that resolve to `_localtime64_s`/`_gmtime64_s` in ucrtbase.dll.
    #[cfg_attr(target_env = "msvc", link_name = "_localtime64_s")]
    fn localtime_s(result: *mut Tm, timep: *const TimeT) -> i32;
    #[cfg_attr(target_env = "msvc", link_name = "_gmtime64_s")]
    fn gmtime_s(result: *mut Tm, timep: *const TimeT) -> i32;
}

/// Thread-safe `localtime_r` / `localtime_s`. Returns `true` on success.
#[allow(unsafe_code)]
pub(crate) fn c_localtime(timep: *const TimeT, result: *mut Tm) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (timep, result);
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        #[cfg(not(target_os = "windows"))]
        {
            !localtime_r(timep, result).is_null()
        }
        #[cfg(target_os = "windows")]
        {
            localtime_s(result, timep) == 0
        }
    }
}

/// Thread-safe `gmtime_r` / `gmtime_s`. Returns `true` on success.
#[allow(unsafe_code)]
pub(crate) fn c_gmtime(timep: *const TimeT, result: *mut Tm) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (timep, result);
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe {
        #[cfg(not(target_os = "windows"))]
        {
            !gmtime_r(timep, result).is_null()
        }
        #[cfg(target_os = "windows")]
        {
            gmtime_s(result, timep) == 0
        }
    }
}

// ---------------------------------------------------------------------------
// tmpname (pure Rust, no FFI)
// ---------------------------------------------------------------------------

/// Creates a temporary filename using Rust std.
///
/// Uses `std::env::temp_dir()` for the platform-appropriate temp directory
/// and `File::create_new()` for atomic creation. Retries with different
/// names on collision, similar to how mkstemp works.
///
/// Returns the filename bytes on success, or `None` on failure.
/// On WASM, always returns `None`.
pub(crate) fn c_tmpname() -> Option<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::fs::File;
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);

        let tmp_dir = std::env::temp_dir();
        let pid = std::process::id();

        // Try a few times in case of collision (unlikely but possible).
        for _ in 0..16 {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("lua_{pid}_{n}");
            let path = tmp_dir.join(&name);
            if File::create_new(&path).is_ok() {
                // File created and immediately dropped (closed).
                // Return the path as bytes.
                return path.to_str().map(|s| s.as_bytes().to_vec());
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Locale category constants
// ---------------------------------------------------------------------------

/// Locale category constants for `setlocale`.
pub(crate) mod locale {
    // Linux/macOS (glibc/BSD) values. Also used for WASM (values are
    // arbitrary since setlocale is a no-op there).
    #[cfg(not(target_os = "windows"))]
    pub(crate) const LC_ALL: i32 = 6;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const LC_COLLATE: i32 = 3;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const LC_CTYPE: i32 = 0;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const LC_MONETARY: i32 = 4;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const LC_NUMERIC: i32 = 1;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const LC_TIME: i32 = 2;

    // MSVCRT values.
    #[cfg(target_os = "windows")]
    pub(crate) const LC_ALL: i32 = 0;
    #[cfg(target_os = "windows")]
    pub(crate) const LC_COLLATE: i32 = 1;
    #[cfg(target_os = "windows")]
    pub(crate) const LC_CTYPE: i32 = 2;
    #[cfg(target_os = "windows")]
    pub(crate) const LC_MONETARY: i32 = 3;
    #[cfg(target_os = "windows")]
    pub(crate) const LC_NUMERIC: i32 = 4;
    #[cfg(target_os = "windows")]
    pub(crate) const LC_TIME: i32 = 5;
}

// ---------------------------------------------------------------------------
// setvbuf mode constants
// ---------------------------------------------------------------------------

/// Buffer mode constants for `setvbuf`.
pub(crate) mod bufmode {
    #[cfg(not(target_os = "windows"))]
    pub(crate) const IONBF: i32 = 2;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const IOFBF: i32 = 0;
    #[cfg(not(target_os = "windows"))]
    pub(crate) const IOLBF: i32 = 1;

    #[cfg(target_os = "windows")]
    pub(crate) const IONBF: i32 = 0x0004;
    #[cfg(target_os = "windows")]
    pub(crate) const IOFBF: i32 = 0x0000;
    #[cfg(target_os = "windows")]
    pub(crate) const IOLBF: i32 = 0x0040;
}

// ---------------------------------------------------------------------------
// Dynamic library loading (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "dynmod")]
pub(crate) mod dynlib {
    /// Handle to a dynamically loaded shared library.
    ///
    /// Wraps `dlopen`/`LoadLibraryA` and automatically calls
    /// `dlclose`/`FreeLibrary` on drop. Stores the path for diagnostics.
    pub(crate) struct DynLib {
        handle: *mut (),
        path: String,
    }

    // DynLib's handle is a dlopen/LoadLibrary pointer. It can be safely
    // transferred between threads; only concurrent use is unsafe (prevented
    // by &mut Lua).
    #[cfg(feature = "send")]
    #[allow(unsafe_code)]
    unsafe impl Send for DynLib {}

    impl DynLib {
        /// Opens a shared library at `path`.
        pub(crate) fn open(path: &str) -> Result<Self, String> {
            let handle = sys::open(path)?;
            Ok(Self {
                handle,
                path: path.to_string(),
            })
        }

        /// Looks up a symbol by name. Returns a raw pointer to the symbol.
        ///
        /// # Safety
        ///
        /// The caller must ensure the returned pointer is cast to the correct
        /// type before use.
        pub(crate) fn symbol(&self, name: &str) -> Result<*mut (), String> {
            sys::symbol(self.handle, name, &self.path)
        }

        /// Returns the path this library was loaded from.
        pub(crate) fn path(&self) -> &str {
            &self.path
        }
    }

    impl Drop for DynLib {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                sys::close(self.handle);
                self.handle = std::ptr::null_mut();
            }
        }
    }

    // -- Unix (Linux, macOS, BSDs) --

    #[cfg(unix)]
    mod sys {
        use std::ffi::CString;

        #[allow(unsafe_code)]
        unsafe extern "C" {
            fn dlopen(filename: *const i8, flags: i32) -> *mut ();
            fn dlsym(handle: *mut (), symbol: *const i8) -> *mut ();
            fn dlclose(handle: *mut ()) -> i32;
            fn dlerror() -> *const i8;
        }

        const RTLD_NOW: i32 = 2;

        fn last_error() -> String {
            #[allow(unsafe_code)]
            let msg = unsafe { dlerror() };
            if msg.is_null() {
                "unknown error".to_string()
            } else {
                #[allow(unsafe_code)]
                let cstr = unsafe { std::ffi::CStr::from_ptr(msg) };
                cstr.to_string_lossy().into_owned()
            }
        }

        pub(super) fn open(path: &str) -> Result<*mut (), String> {
            let cpath = CString::new(path).map_err(|_| format!("invalid path: {path}"))?;
            #[allow(unsafe_code)]
            let handle = unsafe { dlopen(cpath.as_ptr(), RTLD_NOW) };
            if handle.is_null() {
                Err(last_error())
            } else {
                Ok(handle)
            }
        }

        pub(super) fn symbol(
            handle: *mut (),
            name: &str,
            lib_path: &str,
        ) -> Result<*mut (), String> {
            let cname = CString::new(name).map_err(|_| format!("invalid symbol name: {name}"))?;
            // Clear any previous error.
            #[allow(unsafe_code)]
            unsafe {
                dlerror();
            }
            #[allow(unsafe_code)]
            let ptr = unsafe { dlsym(handle, cname.as_ptr()) };
            // dlsym can legitimately return null for a symbol whose value is 0.
            // Check dlerror to distinguish real errors.
            #[allow(unsafe_code)]
            let err = unsafe { dlerror() };
            if err.is_null() {
                Ok(ptr)
            } else {
                #[allow(unsafe_code)]
                let cstr = unsafe { std::ffi::CStr::from_ptr(err) };
                Err(format!(
                    "symbol '{}' not found in '{}': {}",
                    name,
                    lib_path,
                    cstr.to_string_lossy()
                ))
            }
        }

        pub(super) fn close(handle: *mut ()) {
            #[allow(unsafe_code)]
            unsafe {
                dlclose(handle);
            }
        }
    }

    // -- Windows --

    #[cfg(windows)]
    mod sys {
        use std::ffi::CString;

        #[allow(unsafe_code)]
        unsafe extern "stdcall" {
            fn LoadLibraryA(filename: *const i8) -> *mut ();
            fn GetProcAddress(module: *mut (), name: *const i8) -> *mut ();
            fn FreeLibrary(module: *mut ()) -> i32;
            fn GetLastError() -> u32;
        }

        pub(super) fn open(path: &str) -> Result<*mut (), String> {
            let cpath = CString::new(path).map_err(|_| format!("invalid path: {path}"))?;
            #[allow(unsafe_code)]
            let handle = unsafe { LoadLibraryA(cpath.as_ptr()) };
            if handle.is_null() {
                #[allow(unsafe_code)]
                let err = unsafe { GetLastError() };
                Err(format!("cannot load '{}': system error {}", path, err))
            } else {
                Ok(handle)
            }
        }

        pub(super) fn symbol(
            handle: *mut (),
            name: &str,
            lib_path: &str,
        ) -> Result<*mut (), String> {
            let cname = CString::new(name).map_err(|_| format!("invalid symbol name: {name}"))?;
            #[allow(unsafe_code)]
            let ptr = unsafe { GetProcAddress(handle, cname.as_ptr()) };
            if ptr.is_null() {
                #[allow(unsafe_code)]
                let err = unsafe { GetLastError() };
                Err(format!(
                    "symbol '{}' not found in '{}': system error {}",
                    name, lib_path, err
                ))
            } else {
                Ok(ptr)
            }
        }

        pub(super) fn close(handle: *mut ()) {
            #[allow(unsafe_code)]
            unsafe {
                FreeLibrary(handle);
            }
        }
    }

    // -- Unsupported platforms (WASM, etc.) --

    #[cfg(not(any(unix, windows)))]
    mod sys {
        pub(super) fn open(path: &str) -> Result<*mut (), String> {
            Err("dynamic libraries not supported on this platform".to_string())
        }

        pub(super) fn symbol(
            _handle: *mut (),
            _name: &str,
            _lib_path: &str,
        ) -> Result<*mut (), String> {
            Err("dynamic libraries not supported on this platform".to_string())
        }

        pub(super) fn close(_handle: *mut ()) {}
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, unsafe_code)]
mod tests {
    use super::*;

    #[test]
    fn current_time_reasonable() {
        let t = current_time();
        // Should be after 2024-01-01 (timestamp 1704067200).
        assert!(t > 1_704_067_200);
    }

    #[test]
    fn std_streams_not_null() {
        assert!(!c_stdin().is_null());
        assert!(!c_stdout().is_null());
        assert!(!c_stderr().is_null());
    }

    #[test]
    fn tmpname_succeeds() {
        let name = c_tmpname().expect("c_tmpname should succeed");
        assert!(!name.is_empty());
        // Clean up the temp file.
        let name_str = std::str::from_utf8(&name).expect("valid utf8");
        let _ = std::fs::remove_file(name_str);
    }

    #[test]
    fn tm_default_zeroed() {
        let tm = Tm::default();
        assert_eq!(tm.tm_sec, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_mday, 0);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_year, 0);
        assert_eq!(tm.tm_wday, 0);
        assert_eq!(tm.tm_yday, 0);
        assert_eq!(tm.tm_isdst, 0);
    }

    #[test]
    fn c_localtime_roundtrip() {
        let t: TimeT = 1_000_000_000; // 2001-09-09 01:46:40 UTC
        let mut tm = Tm::default();
        assert!(c_localtime(&raw const t, &raw mut tm));
        let t2 = unsafe { mktime(&raw mut tm) };
        assert_eq!(t, t2);
    }

    #[test]
    fn c_gmtime_epoch() {
        let t: TimeT = 0; // 1970-01-01 00:00:00 UTC
        let mut tm = Tm::default();
        assert!(c_gmtime(&raw const t, &raw mut tm));
        assert_eq!(tm.tm_year, 70);
        assert_eq!(tm.tm_mon, 0);
        assert_eq!(tm.tm_mday, 1);
        assert_eq!(tm.tm_hour, 0);
        assert_eq!(tm.tm_min, 0);
        assert_eq!(tm.tm_sec, 0);
    }

    #[test]
    fn locale_constants_valid() {
        // Verify constants are distinct (platform-specific values).
        let cats = [
            locale::LC_ALL,
            locale::LC_COLLATE,
            locale::LC_CTYPE,
            locale::LC_MONETARY,
            locale::LC_NUMERIC,
            locale::LC_TIME,
        ];
        // All must be non-negative.
        for &c in &cats {
            assert!(c >= 0);
        }
    }

    #[test]
    fn bufmode_constants_distinct() {
        assert_ne!(bufmode::IONBF, bufmode::IOFBF);
        assert_ne!(bufmode::IONBF, bufmode::IOLBF);
        assert_ne!(bufmode::IOFBF, bufmode::IOLBF);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn popen_echo() {
        let cmd = b"echo hello\0";
        let mode = b"r\0";
        let fp = c_popen(cmd.as_ptr(), mode.as_ptr());
        assert!(!fp.is_null());
        let mut buf = [0u8; 64];
        let n = unsafe { fread(buf.as_mut_ptr(), 1, buf.len(), fp) };
        assert!(n > 0);
        assert!(
            std::str::from_utf8(&buf[..n])
                .expect("valid utf8")
                .starts_with("hello")
        );
        assert_eq!(c_pclose(fp), 0);
    }
}
