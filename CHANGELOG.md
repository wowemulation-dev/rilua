# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

## [Unreleased]

## [0.1.24](https://github.com/wowemulation-dev/rilua/compare/v0.1.23...v0.1.24) - 2026-08-24

### Testing

- add regression test for issue #46 nil field call misattribution

## [0.1.22](https://github.com/wowemulation-dev/rilua/compare/v0.1.21...v0.1.22) - 2026-08-24

### Added

- *(api)* expose check_/push_/error helpers on LuaState

### Documentation

- update unsafe code descriptions across project docs

### Fixed

- *(ci)* silence unknown lint on MSRV toolchain

### Changelog

- add unreleased entries for compiler, docs, tooling, and fixes

### Compiler

- fix debug line numbers and PUC-Rio ordering

### Examples

- fix import ordering and formatting

### Tooling

- switch from markdownlint-cli2 to markdownlint-cli

### Vm

- fix line continuation in arg_error

### Compiler

- fix debug line numbers to use self.current_line for accurate source positions
- reorder remove_locals before emit_abc(Return) to match PUC-Rio close_func ordering
- remove unused (for step) local variable from numeric for loop
- use self.current_span() in lexer for accurate span info in string tokens

### Documentation

- clarify unsafe code boundaries (confined to libc FFI and dynmod)
- update comparison table with actual unsafe code descriptions
- remove misleading binary compatibility non-goal

### Tooling

- switch from markdownlint-cli2 to markdownlint-cli

### Fixes

- fix import ordering in examples
- fix line continuation in vm/state.rs
## [0.1.21](https://github.com/wowemulation-dev/rilua/compare/v0.1.20...v0.1.21) - 2026-02-27

### Added

- add multi-implementation benchmark script

### Documentation

- add lua-rs (CppCXY) to implementation comparison
- update comparison with measured benchmark data
- add "Why rilua" section to README
- add comparison with PUC-Rio, mlua, and Luau

### Performance

- add #[inline] to table lookup hot paths

## [0.1.20](https://github.com/wowemulation-dev/rilua/compare/v0.1.19...v0.1.20) - 2026-02-25

### Documentation

- add CppCXY/lua-rs reference, replace local paths with GitHub URLs

## [0.1.19](https://github.com/wowemulation-dev/rilua/compare/v0.1.18...v0.1.19) - 2026-02-24

### Documentation

- add WASM demo link to README

## [0.1.18](https://github.com/wowemulation-dev/rilua/compare/v0.1.17...v0.1.18) - 2026-02-24

### Documentation

- update test counts and lockfile to match v0.1.16

## [0.1.17](https://github.com/wowemulation-dev/rilua/compare/v0.1.16...v0.1.17) - 2026-02-23

### Added

- add per-test benchmark script for PUC-Rio vs rilua comparison

### Documentation

- update benchmarks with bench-all.lua results after dead-key fix
- update CHANGELOG with dead-key fix and benchmark additions
- add per-test benchmark results and optimization priorities

### Fixed

- reuse dead hash slots in new_key to prevent chain corruption

### Fixed

- Fix hash chain corruption when inserting keys into tables with GC-swept
  nil-valued entries. After GC sweeps key strings, stale GcRefs caused
  `main_position` to return incorrect buckets, corrupting hash chains via
  Brent's Case A relocation. Detected as globals (e.g. `assert`) becoming
  nil when running multiple test files sequentially.

### Added

- Per-test benchmark script (`scripts/benchmark-tests.sh`) for comparing
  PUC-Rio and rilua performance on each test file individually

### Documentation

- Update per-test benchmark results with bench-all.lua: rilua is 1.75x
  slower individual (1.93x combined runner) vs PUC-Rio
- Add optimization priority analysis and profiling workflow

## [0.1.15](https://github.com/wowemulation-dev/rilua/compare/v0.1.14...v0.1.15) - 2026-02-22

### Documentation

- remove duplicate Trace doc comment in LuaString
- apply KISS and DRY principles across documentation
- restructure docs/ as mdbook with mermaid support
- add WASM documentation and sync with recent changes

### Fixed

- add mdbook-mermaid to PATH in RTD build command
- remove quotes from RTD yaml commands
- use explicit install path for mdbook on Read the Docs

### Changed

- Simplify `.mise.toml`: use `"latest"` versions for all dev tools, remove
  inline task definitions, add profiling tools (flamegraph, inferno) and
  coverage tool (cargo-llvm-cov), organize sections with comments

### Documentation

- Add `docs/wasm.md`: WebAssembly target documentation covering library
  availability, platform stubs, building for browser, and limitations
- Add platform support section to `docs/architecture.md` with target
  matrix and platform abstraction design
- Add platform support section to `README.md`
- Update docs index (`docs/README.md`) with WASM documentation link
- Fix Read the Docs mdbook build (use `--root` for predictable
  install path with asdf-managed Rust)

### Other

- Track `examples/wasm-demo/Cargo.lock` for reproducible builds

## [0.1.14](https://github.com/wowemulation-dev/rilua/compare/v0.1.13...v0.1.14) - 2026-02-21

### Added

- WASM support: compile rilua to `wasm32-unknown-unknown` with pure-Rust
  replacements for all C FFI (strtod, localeconv, strcoll, ctype functions).
  Stdlib I/O and OS functions return errors since there is no filesystem
  in WASM. Loads base, string, table, math, and coroutine libraries.
- Browser-based WASM demo (`examples/wasm-demo/`): single-page HTML UI
  that runs Lua code in the browser via wasm-pack + wasm-bindgen.
  Custom print captures output to a thread-local buffer.

## [0.1.13](https://github.com/wowemulation-dev/rilua/compare/v0.1.12...v0.1.13) - 2026-02-20

### Documentation

- update sponsor link and tailor .editorconfig
- standardize README badges
- add performance documentation and gitignore flamegraphs
- fix stale test counts, loadlib description, and contradictions

## [0.1.12](https://github.com/wowemulation-dev/rilua/compare/v0.1.11...v0.1.12) - 2026-02-20

### Added

- add property-based fuzzing for lexer, parser, compiler, and VM

## [0.1.11](https://github.com/wowemulation-dev/rilua/compare/v0.1.10...v0.1.11) - 2026-02-20

### Fixed

- Use correct MSVC symbol names and link libraries for CRT functions
- Link `legacy_stdio_definitions` for `fprintf`/`fscanf` on MSVC (inline since VS2015)
- Use `_gmtime64_s`/`_localtime64_s` symbol names on MSVC (header wrappers don't exist in ucrtbase.dll)

## [0.1.10](https://github.com/wowemulation-dev/rilua/compare/v0.1.9...v0.1.10) - 2026-02-20

### Fixed

- Link `ucrt` on Windows MSVC for libc FFI symbols (`gmtime_s`, `localtime_s`, `fprintf`, `fscanf`)

## [0.1.9](https://github.com/wowemulation-dev/rilua/compare/v0.1.8...v0.1.9) - 2026-02-20

### Added

- add advanced embedding example with API extensions

## [0.1.7](https://github.com/wowemulation-dev/rilua/compare/v0.1.6...v0.1.7) - 2026-02-20

### Added

- implement package.loadlib with rilua-native module ABI

## [0.1.6](https://github.com/wowemulation-dev/rilua/compare/v0.1.5...v0.1.6) - 2026-02-20

### Added

- `dynmod` feature: native module loading via `package.loadlib`. Modules
  are Rust `cdylib` crates compiled against the same rilua/rustc version.
  ABI validation (magic, version, struct sizes) before calling module code.
  Platform support: Unix (`dlopen`), Windows (`LoadLibraryA`), fallback stub.
  Disabled by default; without the feature, `package.loadlib` returns `"absent"`.
- Example native module (`examples/native_module/`) demonstrating the ABI
- Cross-platform SIGINT handling (Unix via raw `signal()` FFI, Windows
  via `SetConsoleCtrlHandler`). Second Ctrl+C terminates immediately.
  No-op on other platforms (e.g. WASM).
- `set_interrupted()` and `clear_interrupted()` public API for embedders
  to integrate custom interrupt sources
- `examples/run_file.rs` embedding example with `hello.lua` sample script

### Documentation

- add embedding example and examples README

### Changed

- Interrupt flag is unconditional (`AtomicBool` with `Relaxed` ordering,
  no `#[cfg]` gates). Compiles on all targets including wasm32.
- `INTERRUPTED` static is now private; `check_interrupted()` is `pub(crate)`

### Removed

- `libc` crate dependency (replaced with raw `extern "C"` FFI, restoring
  zero external runtime dependencies)

## [0.1.5](https://github.com/wowemulation-dev/rilua/compare/v0.1.4...v0.1.5) - 2026-02-19

### Documentation

- sync documentation with current code state

## [0.1.4](https://github.com/wowemulation-dev/rilua/compare/v0.1.3...v0.1.4) - 2026-02-16

### Performance

- arena sweep cache optimization (phase 4)

## [0.1.3](https://github.com/wowemulation-dev/rilua/compare/v0.1.2...v0.1.3) - 2026-02-16

### Performance

- GC and traceback optimizations (phase 3)
- hash-based constant pool deduplication in compiler

## [0.1.2](https://github.com/wowemulation-dev/rilua/compare/v0.1.1...v0.1.2) - 2026-02-15

### Performance

- phase 1 optimizations for compiler and GC hotspots

## [0.1.1](https://github.com/wowemulation-dev/rilua/compare/v0.1.0...v0.1.1) - 2026-02-15

### Changed

- centralize platform FFI and migrate safe operations to Rust std

### Added

- Cross-platform compilation support for Linux, macOS, and Windows
- Centralized platform abstraction layer (`src/platform.rs`) with
  `#[cfg(target_os)]` dispatch for all libc FFI declarations

### Changed

- Replaced `isatty` FFI with `std::io::IsTerminal` in CLI binary
- Replaced `time(NULL)` FFI with `SystemTime::now()` for current time
- Replaced `errno`/`strerror` FFI with `std::io::Error::last_os_error()`
  in I/O library error reporting (4 sites)
- Replaced `mkstemp`/`tmpnam` FFI with `File::create_new()` and
  `std::env::temp_dir()` for `os.tmpname`
- Moved scattered libc FFI declarations from `io.rs`, `os.rs`, and
  `execute.rs` into `platform.rs`

## [0.1.0] - 2026-02-15

### Added

- Full Lua 5.1.1 compilation pipeline: lexer, parser, AST, code generator
  producing PUC-Rio-compatible bytecode (38 register-based opcodes)
- Register-based VM with instruction dispatch, call stack (CallInfo),
  closures with open/closed upvalue model, tail call optimization
- Arena-based mark-sweep GC with generational indices, incremental
  collection (5-state machine), write barriers, `__gc` finalizers
- Value representation: 9-variant enum (Nil, Boolean, Number, String,
  Table, Function, UserData, Thread, LightUserData)
- String interning with PUC-Rio hash algorithm and cached hash values
- Table implementation: array + hash dual representation, Brent's
  collision resolution, 3-phase resize with optimal array/hash split
- Metatables and metamethods: arithmetic, comparison, concat, len,
  `__index`/`__newindex` (table and function), `__call`, `__eq`,
  `__lt`, `__le`, `__gc`, `__tostring`
- Protected calls: `pcall`, `xpcall` with Result-based error propagation
- Coroutines: create, resume, yield, wrap, status, running
- Standard library (all Lua 5.1.1 functions):
  - Base: 28 functions including print, assert, type, tostring, tonumber,
    pcall, xpcall, error, setmetatable, getmetatable, select, unpack,
    loadstring, loadfile, dofile, load, pairs, ipairs, next, rawget,
    rawset, rawequal, setfenv, getfenv, collectgarbage, newproxy
  - String: 14 functions (len, byte, char, sub, rep, reverse, lower,
    upper, format, find, match, gmatch, gsub, dump) with pattern
    matching engine
  - Table: 9 functions (concat, insert, remove, sort, maxn, getn, setn,
    foreach, foreachi) with PUC-Rio's median-of-three quicksort
  - Math: 28 functions (abs through tanh), math.pi, math.huge constants,
    glibc-compatible LCG random number generator
  - I/O: 11 library functions, 7 file methods, 3 standard handles,
    lines iterator with auto-close, libc FFI for C stdio
  - OS: 11 functions (clock, date, difftime, execute, exit, getenv,
    remove, rename, setlocale, time, tmpname) via libc FFI
  - Debug: 14 functions (getinfo, getlocal, setlocal, getupvalue,
    setupvalue, traceback, getregistry, getmetatable, setmetatable,
    getfenv, setfenv, gethook, sethook, debug)
  - Package: require, module, 4 loaders (preload, Lua file, 2 native module
    loaders via `dynmod` feature), path searching,
    package.loaded/preload/loaders/config/path/cpath
- Userdata infrastructure: typed arena, per-instance metatables,
  `__gc` support, registry metatable helpers
- Public Rust embedding API: `Lua` struct, `IntoLua`/`FromLua` traits,
  `Table`/`Function`/`Thread`/`AnyUserData` handle types, `StdLib`
  bitflags for selective library loading
- CLI interpreter (`rilua`): PUC-Rio `lua.c`-compatible command line
  interface with `-e`, `-l`, `-i`, `-v`, `--` flags, `LUA_INIT` env
  var, `arg` table, REPL with multiline detection
- Bytecode compiler/lister (`riluac`): `-l`/`-l -l`/`-p`/`-v`/`-o`/`-s`
  flags, binary chunk output, multiple file combining
- Bytecode serialization: `string.dump` and binary chunk loading,
  cross-compatible with PUC-Rio (byte-identical output)
- Byte-based source loading (`&[u8]` pipeline) for non-UTF-8 support
- Error message formatting with variable name resolution: `getobjname`,
  `symbexec`, `getfuncname` ported from PUC-Rio's `ldebug.c`. Messages
  include variable kind and name (local, global, field, upvalue, method)
- CLI stack tracebacks on unhandled errors via `call_function_traced`
- `LUA_COMPAT_VARARG`: implicit `arg` table for old-style vararg functions
- `LUA_COMPAT_GFIND`: `string.gfind` as alias for `string.gmatch`
- Architecture documentation in `docs/` (9 documents covering architecture,
  API, features, stdlib, testing, future API, references, use cases)
- T test module (`RILUA_TEST_LIB=1`): 25 functions for PUC-Rio test
  suite compatibility. Includes T.testC mini-interpreter (28 C API
  commands), remote state management (newstate/closestate/doremote),
  userdata reference tracking (ref/unref/getref), upvalue access,
  OOM memory limit simulation (totalmem), and string substitution (gsub)
- 1304 tests: 596 unit + 431 integration + 277 oracle comparison
- PUC-Rio test suite: 23/23 pass via `all.lua` runner

### Changed

- Complete rewrite from scratch. Previous implementation (based on
  lua-in-rust) replaced with new architecture:
  - AST-based pipeline (was: single-pass bytecode emission)
  - Register-based VM with PUC-Rio opcodes (was: stack-based custom opcodes)
  - Arena GC with generational indices (was: raw-pointer mark-sweep)
  - Trait-based API (was: stack-based C API mirror)

### Fixed

- Or-shortcircuit in EQ operand causing infinite loop
- Parser accepting bare names as statements
- Vararg table constructor capturing only first argument
- Mixed named params + varargs register misassignment
- Closure + for-loop break not closing upvalues
- Table constructor named field constant index overflow (9-bit truncation)
- `load(func_reader)` hang with infinite readers (added size limit)
- Non-ASCII bytes in pattern matching bracket classes
- `%z` character class not matching null bytes
- `gsub` `%n` replacement with position captures
- `%b` balanced match off-by-one
- Backref `%1` matching pointer offset
- `gsub` table replacement not invoking `__index` metamethods
- `debug.getinfo` activelines not populated
- `lastlinedefined` always 0 in function prototypes
- Shebang line handling in source files
- String coercion in stdlib functions
- While-true-if-break compiler bug: `compile_while` used `patch_jump`
  (single instruction) instead of `patch_list` (linked list walk) for
  the loop-back JMP, causing false-branch JMPs from conditions inside
  `while true` bodies to self-loop instead of jumping to the loop top.
  Unblocked math.lua and nextvar.lua in PUC-Rio test suite.
- Parenthesized call/vararg not truncating to single value: `(f())`
  returned all values instead of 1. Added `Expr::Paren` AST variant so
  codegen calls `discharge_vars`/`set_one_ret`, matching PUC-Rio's
  `prefixexp` -> `luaK_dischargevars` -> `luaK_setoneret` chain.
  Also fixes `(f())` as statement correctly being a syntax error.
- GC finalization order: userdata finalized in wrong order (oldest-first
  instead of newest-first). Added `alloc_seq` counter to track allocation
  order, sort `to_finalize` list to match PUC-Rio's LIFO semantics.
- GC traverse_table marking nil-valued hash keys: kept dead objects alive
  by marking keys of nil-valued entries. Now skips nil-valued entries,
  matching PUC-Rio's `removeentry` behavior.
- `lua_equal` returning true for out-of-range stack indices: PUC-Rio
  returns 0 for invalid indices (`luaO_nilobject`). Added validity checks.
- xpcall error handler collected by GC: handler closure stored only in
  Rust local was invisible to GC mark phase. Now stored on Lua stack.
- `call_gc_finalizer` not restoring state on `__gc` error: ci/base/top
  left in bad state after finalizer error. Added save/restore.
- `package.loaded` not set by library openers in remote states: caused
  `require("_G")` to fail. Added `set_package_loaded` helper.
- Table resize memory not tracked in `total_bytes`: OOM simulation
  (T.totalmem) couldn't detect table growth. Added memory delta tracking
  in `table_set` and `vm_settable`.
