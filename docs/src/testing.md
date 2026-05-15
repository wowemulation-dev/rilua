# Testing Strategy

## Decision

**Spec-driven, multi-layer testing. Unit tests for internals,
oracle comparison for behavioral equivalence, integration tests
for language semantics, PUC-Rio official test suite as the
compatibility target.**

Current: 1325 tests (609 unit, 431 integration, 277 oracle, 5 proptest, 3 lua51).
With `dynmod` feature: 1333 tests (611 unit, 6 dynmod, 431 integration,
277 oracle, 5 proptest, 3 lua51). All 5 layers are active.

## Test Layers

### Layer 1: Unit Tests

In-module `#[cfg(test)]` blocks testing internal components in
isolation. Every implementation chunk includes unit tests.

**Lexer tests** (`src/compiler/lexer.rs`):

- Token type recognition for all keywords and symbols
- Number literal parsing (decimal, hex, float, exponent)
- String literal parsing (escapes, long brackets)
- Comment handling (single-line, long comments)
- Error cases (unterminated strings, invalid escapes)
- Source position tracking

**Parser tests** (`src/compiler/parser.rs`):

- Expression parsing with operator precedence
- Statement parsing for each statement type
- Block and scope handling
- Error recovery and error messages
- AST structure verification

**Compiler tests** (`src/compiler/codegen.rs`):

- Instruction emission for each AST node type
- Register allocation
- Constant pool management
- Upvalue resolution
- Jump backpatching

**VM tests** (`src/vm/`):

- Instruction dispatch correctness
- Stack manipulation
- GC arena allocation and collection
- Table operations (get, set, resize)
- String interning
- Closure creation and upvalue management

### Layer 2: Oracle Comparison Tests

Oracle comparison tests run the same Lua code in both rilua and
PUC-Rio Lua 5.1.1, comparing output to verify behavioral equivalence.
This catches divergences that unit tests and integration tests might
miss.

#### Reference Binaries

- **lua** (interpreter): `./lua-5.1.1/src/lua`
- **luac** (compiler/lister): `./lua-5.1.1/src/luac`

Both are built from the official PUC-Rio Lua 5.1.1 tarball. See
`AGENTS.md` for download, verification, and build instructions.

The `lua` binary path is configured via the `LUA_REFERENCE_BIN`
environment variable (defaults to `./lua-5.1.1/src/lua`). Tests
that require the reference binary skip gracefully if it is not
available.

#### Oracle Test Framework

Test helpers in `tests/helpers/`:

```text
// tests/helpers/oracle.rs

/// Run code in PUC-Rio Lua 5.1.1 and return (stdout, stderr, exit_code).
fn run_reference(code: &str) -> (String, String, i32);

/// Run code in rilua and return (stdout, stderr).
fn run_rilua(code: &str) -> (String, String);

/// Assert rilua produces the expected stdout for the given code.
fn assert_output(code: &str, expected: &str);

/// Run in both interpreters, assert stdout matches.
fn assert_matches_reference(code: &str);
```

Usage in tests:

```text
#[test]
fn arithmetic_matches_reference() {
    assert_matches_reference("print(1 + 2)");
    assert_matches_reference("print(2 ^ 10)");
    assert_matches_reference("print(10 % 3)");
    assert_matches_reference("print(-7 % 3)");  // floor modulo
}
```

#### Bytecode Comparison

After the compiler is implemented, bytecode comparison tests compile
Lua snippets with both rilua and `luac -l`, then compare instruction
output. This verifies the compiler produces correct bytecode before
the VM exists.

```text
/// Compile code with rilua and return a formatted instruction listing.
fn compile_rilua(code: &str) -> String;

/// Compile code with PUC-Rio luac -l and return the listing.
fn compile_reference(code: &str) -> String;

/// Assert both compilers produce equivalent bytecode.
fn assert_bytecode_matches(code: &str);
```

Bytecode comparison checks instruction opcodes and operands. It does
not compare constant pool ordering or debug info formatting, since
these may differ between implementations without affecting semantics.

#### When Each Test Category Activates

| Category | Activates after | Mechanism |
|----------|----------------|-----------|
| Unit tests | Phase 0 (skeleton) | `cargo test --lib` |
| Bytecode comparison | Phase 2 (compiler) | Compare rilua compiler output with `luac -l` |
| Oracle comparison | Phase 3 + `print` | Compare rilua output with `lua -e` |
| Integration `.lua` tests | Phase 3 + `assert` | `cargo test --test integration` |
| PUC-Rio test suite | Phase 5a (base lib) | `cargo test --test lua51` |

### Layer 3: Integration Tests

Lua scripts in `tests/` that exercise language features through the
full pipeline. Each test uses `assert()` to validate behavior.

Organization mirrors the Lua 5.1 Reference Manual. Language tests
cover Chapter 2 ("The Language"), standard library tests cover
Chapter 5 ("Standard Libraries").

**Language tests** (Chapter 2):

| File | Section | Description |
|------|---------|-------------|
| `lexical.lua` | 2.1 | Lexical conventions (keywords, names, strings, numbers, comments) |
| `types.lua` | 2.2 | Values and types, coercion (2.2.1) |
| `variables.lua` | 2.3 | Global, local, and table field variables |
| `statements.lua` | 2.4 | Chunks, blocks, assignment, control structures, for loops, local declarations |
| `expressions.lua` | 2.5 | Arithmetic, relational, logical operators, concatenation, length, precedence, table constructors, function calls, function definitions |
| `visibility.lua` | 2.6 | Lexical scoping, upvalues, closures |
| `errors.lua` | 2.7 | error(), pcall, xpcall, error objects, stack traces |
| `metatables.lua` | 2.8 | Metamethods for arithmetic, comparison, indexing, call, concatenation, length |
| `environments.lua` | 2.9 | Function environments, setfenv, getfenv |
| `gc.lua` | 2.10 | Garbage collection, finalizers (2.10.1), weak tables (2.10.2) |
| `coroutines.lua` | 2.11 | create, resume, yield, wrap, status, error propagation |

**Standard library tests** (Chapter 5):

| File | Section | Description |
|------|---------|-------------|
| `stdlib-base.lua` | 5.1 | Base library (assert, type, tonumber, tostring, select, unpack, etc.) |
| `stdlib-package.lua` | 5.3 | Package/module library (require, module, loaders, etc.) |
| `stdlib-string.lua` | 5.4 | String library (find, format, gmatch, gsub, etc.) |
| `stdlib-table.lua` | 5.5 | Table library (concat, insert, remove, sort, maxn) |
| `stdlib-math.lua` | 5.6 | Math library (abs, floor, ceil, random, sin, cos, etc.) |
| `stdlib-io.lua` | 5.7 | I/O library (open, read, write, lines, etc.) |
| `stdlib-os.lua` | 5.8 | OS library (clock, date, time, execute, etc.) |
| `stdlib-debug.lua` | 5.9 | Debug library (getinfo, getlocal, sethook, traceback, etc.) |

Test infrastructure files: `tests/helpers/mod.rs` (shared utilities),
`tests/helpers/oracle.rs` (PUC-Rio comparison), `tests/integration.rs`
(runner), `tests/lua51.rs` (PUC-Rio suite runner).

### Layer 4: PUC-Rio Official Test Suite

The PUC-Rio Lua 5.1.1 test suite (`./lua-5.1-tests/`) is the
compatibility target. These are verbatim test files from the
official Lua test tarball. See `AGENTS.md` for download instructions.

#### Official Running Modes

Per [lua.org/tests/](https://lua.org/tests/), the test suite has
three running modes:

1. **Portable mode**: `lua -e"_U=true" all.lua` -- skips
   system-dependent tests and memory-intensive operations.
2. **Full mode**: `lua all.lua` -- tests every corner of the
   language. Requires compiled C libraries in `libs/` subdirectory.
3. **Internal mode**: Recompile Lua with `ltests.c`/`ltests.h` to
   enable the T (testC) library for internal VM tests.

#### What `all.lua` Does

The `all.lua` runner is not a simple file list. It modifies the
runtime environment before running each test:

- Sets GC parameters: `collectgarbage("setstepmul", 180)` and
  `setpause(190)`.
- Redefines `dofile` to round-trip every test file through
  `string.dump` + `loadstring`, implicitly testing binary chunk
  serialization and deserialization.
- Wraps `big.lua` in `coroutine.wrap` (the file yields values).
- `calls.lua` expects a `deep` variable set by `main.lua`.
- Sets a `debug.sethook` call/return hook during cleanup (line 118).

#### How rilua Runs These Tests

rilua does **not** run `all.lua` directly. Instead, each test file
is executed individually using two approaches:

**Important**: Tests must be run from the `lua-5.1-tests/` directory.
Several tests depend on relative paths: `attrib.lua` creates files
in `libs/`, `math.lua` and `verybig.lua` require `checktable.lua`
via `LUA_PATH`, and file tests reference paths relative to the test
directory. Running from the project root will cause false failures.

**Individual file execution** (primary):
```bash
# Run from the test directory (required)
cd lua-5.1-tests
mkdir -p libs

# Run a single test
RILUA_TEST_LIB=1 LUA_PATH="?;./?.lua" ../target/release/rilua <test>.lua

# Run all tests
for f in *.lua; do
  [ "$f" = "all.lua" ] && continue
  echo -n "$(basename $f .lua): "
  timeout 30 env RILUA_TEST_LIB=1 LUA_PATH="?;./?.lua" \
    ../target/release/rilua "$f" >/dev/null 2>&1 && echo "PASS" || echo "FAIL"
done
```

**Comparison script** (`scripts/compare.sh`):
```bash
# Compare all test files between PUC-Rio and rilua
scripts/compare.sh ./lua-5.1.1/src/lua ./target/release/rilua
```

The comparison script runs each `.lua` file individually (except
`all.lua`) with a 10-second timeout, reporting PASS/FAIL/TIMEOUT
for both interpreters. This differs from `all.lua` in that:

- No `string.dump`/`loadstring` round-trip (tests run directly).
- No GC parameter tuning.
- No inter-test state (`big.lua` runs standalone, not in a
  coroutine; `calls.lua` runs without `deep` from `main.lua`).
- Each test gets a fresh interpreter state.

rilua also passes `all.lua` directly (see
[What `all.lua` Does](#what-alllua-does) for its additional
requirements).

#### T Module

rilua implements PUC-Rio's internal test library (`T` global),
the Rust equivalent of `ltests.c`. Activate it with the
`RILUA_TEST_LIB=1` environment variable. 25 functions are
registered:

| Function | Description |
|----------|-------------|
| `T.querytab` | Returns (array size, hash size) for a table |
| `T.hash` | Returns hash-part index for a key in a table |
| `T.int2fb` | Converts integer to float-byte encoding |
| `T.log2` | Returns floor(log2(x)) |
| `T.listcode` | Returns list of opcodes for a function |
| `T.setyhook` | Sets yield-on-hook for a coroutine thread |
| `T.resume` | Resumes a coroutine (no arguments) |
| `T.d2s` | Converts f64 to 8-byte native-endian string |
| `T.s2d` | Converts 8-byte native-endian string to f64 |
| `T.testC` | C API mini-interpreter (28 commands) |
| `T.newuserdata` | Create userdata with given byte size |
| `T.udataval` | Return unique integer ID for userdata |
| `T.pushuserdata` | Find/create userdata by its ID |
| `T.ref` | Store object in registry, return integer key |
| `T.unref` | Remove registry entry |
| `T.getref` | Get value from registry by key |
| `T.upvalue` | Get/set upvalue n of closure f |
| `T.checkmemory` | No-op stub (GC consistency check) |
| `T.gsub` | String substitution |
| `T.doonnewstack` | Run code in a new coroutine |
| `T.newstate` | Create independent Lua state |
| `T.closestate` | Close a state created by newstate |
| `T.doremote` | Execute code string in remote state |
| `T.loadlib` | Load standard libraries into remote state |
| `T.totalmem` | Get/set memory limit (OOM simulation) |

Four tests (`api.lua`, `checktable.lua`, `closure.lua`, `code.lua`)
use T extensively. When `T` is nil, guarded sections (`if T then ...
end`) are skipped. All four pass with `RILUA_TEST_LIB=1`.

#### Test Files

| Test File | Area |
|-----------|------|
| `all.lua` | Test runner (chains all tests with dump/undump) |
| `api.lua` | C API interactions (requires T.testC) |
| `attrib.lua` | require/package system, assignments, operators |
| `big.lua` | String overflow, large line counts, table constructs |
| `calls.lua` | Function calls and returns |
| `checktable.lua` | Table invariant checker (utility functions only) |
| `closure.lua` | Closures, upvalues, and coroutines |
| `code.lua` | Code generation, optimizations (uses T.listcode) |
| `constructs.lua` | Syntax, operator priority, language constructs |
| `db.lua` | Debug library |
| `errors.lua` | Error handling |
| `events.lua` | Metatables and metamethods |
| `files.lua` | I/O library |
| `gc.lua` | Garbage collection |
| `literals.lua` | Scanner/lexer and literal parsing |
| `locals.lua` | Local variables |
| `main.lua` | Standalone interpreter (lua.c) options |
| `math.lua` | Math library |
| `nextvar.lua` | Tables, next(), size operator, for loops |
| `pm.lua` | Pattern matching |
| `sort.lua` | table.sort |
| `strings.lua` | String library |
| `vararg.lua` | Vararg functions |
| `verybig.lua` | Very large programs |

#### Current Status

All 23 files pass: api, attrib, big, calls, checktable, closure, code,
constructs, db, errors, events, files, gc, literals, locals, main,
math, nextvar, pm, sort, strings, vararg, verybig.

The `all.lua` runner also passes (see
[What `all.lua` Does](#what-alllua-does) for its additional
requirements beyond individual file execution).

**Compatibility flags**: The PUC-Rio test suite was written with
default compat options enabled (e.g., `LUA_COMPAT_VARARG` enables
the `arg` table in vararg functions). WoW's Lua disables some of
these. Tests that depend on compat options may need conditional
handling.

### Layer 5: Behavioral Equivalence Tests

Tests that specifically verify behavioral equivalence with PUC-Rio
Lua 5.1.1. These test edge cases where implementations commonly
diverge:

- Numeric formatting (`tostring(0.1)`, `string.format("%g", 0.1)`)
- Error message wording (programs may match on error strings)
- GC behavior (`collectgarbage` return values)
- Weak table clearing timing
- Finalizer execution order
- String-to-number coercion edge cases
- Integer overflow behavior (all numbers are f64)
- Modulo with negative operands (floor division)
- Concatenation type coercion

These tests use the oracle comparison framework to run each case in
both rilua and PUC-Rio, comparing exact output. They are the last
line of defense before the PUC-Rio test suite.

## Test Workflow

### Test-Driven Development

New features are implemented test-first where possible:

1. Write a Lua test script that exercises the feature.
2. Run the test against PUC-Rio Lua 5.1.1 to verify expected
   behavior.
3. Implement the feature in rilua.
4. Run the test and fix until it passes.
5. Run the oracle comparison to verify matching output.
6. Run the full test suite to check for regressions.

This ensures every feature is validated against the reference
implementation.

### Quality Gate

Every commit must pass:

```bash
cargo fmt -- --check && \
cargo clippy --all-targets && \
cargo test && \
cargo doc --no-deps
```

This ensures:

1. Consistent formatting
2. No lint warnings
3. All tests pass (unit, integration, oracle, PUC-Rio suite)
4. Documentation builds without errors

### Coverage Tracking

Test coverage is measured by:

1. **Feature coverage** -- which Lua 5.1.1 features are implemented
   and tested (tracked in CHANGELOG.md).
2. **PUC-Rio test suite progress** -- 23 of 23 official test files
   passing (tracked in CI).
3. **Oracle comparison count** -- number of Lua snippets verified
   against PUC-Rio output.
4. **Code coverage** -- `cargo-tarpaulin` or `llvm-cov` for line
   coverage metrics (informational, not a gate).
