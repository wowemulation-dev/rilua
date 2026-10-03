# AGENTS.md

Guide for coding agents working in the rilua codebase. Follows the
[AGENTS.md](https://agents.md/) convention: a single, agent-agnostic
file that any AI coding tool can consume.

## Project Overview

rilua is a from-scratch implementation of **Lua 5.1.1** in Rust, targeting behavioral equivalence with the PUC-Rio reference interpreter. Part of the WoW Emulation project for World of Warcraft addon support.

Key constraints:
- **Zero external dependencies** - only Rust standard library
- **Lua 5.1.1 semantics** - not 5.2+, not Luau (though Luau is an architecture reference)
- **Behavioral equivalence** - output must match PUC-Rio Lua 5.1.1 exactly

## Essential Commands

```bash
# Build
cargo build

# Run interpreter (always specify --bin, crate has two binaries)
cargo run --bin rilua                      # Start the REPL
cargo run --bin rilua -- <file.lua>        # Execute a Lua file
cargo run --bin rilua -- -e 'print("hi")'  # Execute a string

# Run bytecode compiler
cargo run --bin riluac -- <file.lua>       # Compile to bytecode
cargo run --bin riluac -- -l <file.lua>    # List bytecode

# Run all tests
cargo nextest run

# Run specific test
cargo nextest run <test_name>
cargo nextest run --lib                           # Unit tests only
cargo nextest run <test_name>  # Specific integration test

# Format check
cargo fmt -- --check

# Strict lint
cargo clippy --all-targets

# Dependency checks (cargo-deny)
cargo deny check advisories    # Check for security advisories
cargo deny check bans           # Check for banned/undesirable dependencies
cargo deny check licenses        # Verify license compliance
cargo deny check sources         # Verify crate source restrictions
cargo deny check                # Run all checks (advisories, bans, licenses, sources)

# Build documentation
cargo doc --no-deps

# Quality gate (run before commits)
cargo fmt -- --check && cargo clippy --all-targets && cargo nextest run && cargo deny check && cargo doc --no-deps
```

### Nextest and Flamegraph

```bash
# Nextest - faster test runner
cargo nextest run                    # Run all tests with nextest
cargo nextest run <test_name>        # Run specific test
cargo nextest run --profile ci       # Run tests with CI profile

# Flamegraph - generate CPU profile
cargo flamegraph --bin rilua -- test.lua  # Generate flamegraph for interpreter
cargo flamegraph --bin riluac <file.lua>  # Generate flamegraph for compiler
```

Note: Flamegraph requires Linux and perf (install via `sudo apt install linux-perf` on Ubuntu/Debian).

**Note**: The quality gate now includes `cargo deny check` to enforce the zero-dependency policy.

### Zero-Dependency Policy

No production dependencies. Dev-dependencies require justification
(currently only `flamegraph`). `cargo-deny` enforces this via
`deny.toml` (banned crates, license allowlist, single-version policy).
See `deny.toml` for details.

**Important**: Always use `--bin rilua` or `--bin riluac` with `cargo run`.
The crate has two binaries and no `default-run`, so `cargo run` without
`--bin` will fail.

### Setup Lua 5.1.1 Reference

Set up the reference Lua 5.1.1 implementation for comparison and testing:

**Full Reproducible Setup Script**:
```bash
#!/bin/bash
# Script to download, verify, unpack, and compile Lua 5.1.1
# Will skip download if tarball already exists with correct checksum
# Will skip compilation if binaries already exist

set -e

LUA_VERSION="5.1.1"
LUA_TARBALL="lua-${LUA_VERSION}.tar.gz"
LUA_DIR="lua-${LUA_VERSION}"
EXPECTED_SHA256="c5daeed0a75d8e4dd2328b7c7a69888247868154acbda69110e97d4a6e17d1f0"
LUA_URL="https://lua.org/ftp/${LUA_TARBALL}"

# Step 1: Download if tarball doesn't exist or has wrong checksum
if [ -f "$LUA_TARBALL" ]; then
    ACTUAL_SHA256=$(sha256sum "$LUA_TARBALL" | cut -d' ' -f1)
    if [ "$ACTUAL_SHA256" = "$EXPECTED_SHA256" ]; then
        echo "Using existing $LUA_TARBALL (checksum verified)"
    else
        echo "Checksum mismatch, re-downloading..."
        rm -f "$LUA_TARBALL"
    fi
fi

if [ ! -f "$LUA_TARBALL" ]; then
    echo "Downloading $LUA_TARBALL..."
    curl -fsSL "$LUA_URL" -o "$LUA_TARBALL" || \
        wget -q "$LUA_URL" -O "$LUA_TARBALL"

    # Verify checksum
    ACTUAL_SHA256=$(sha256sum "$LUA_TARBALL" | cut -d' ' -f1)
    if [ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]; then
        echo "Checksum verification failed!"
        echo "  Expected: $EXPECTED_SHA256"
        echo "  Actual:   $ACTUAL_SHA256"
        exit 1
    fi
    echo "Checksum verified"
fi

# Step 2: Unpack if directory doesn't exist
if [ -d "$LUA_DIR" ]; then
    echo "Using existing $LUA_DIR"
else
    echo "Unpacking $LUA_TARBALL..."
    tar -xzf "$LUA_TARBALL"
    echo "Unpacked to $LUA_DIR"
fi

# Step 3: Compile if binaries don't exist
if [ -x "$LUA_DIR/src/lua" ] && [ -x "$LUA_DIR/src/luac" ]; then
    echo "Lua binaries already compiled"
else
    echo "Compiling Lua $LUA_VERSION..."
    cd "$LUA_DIR"
    make clean > /dev/null 2>&1 || true
    make linux
    echo "Compilation complete"
    cd ..
fi

# Step 4: Verify binaries work
echo "Verifying binaries..."
"$LUA_DIR/src/lua" -e "print('Lua $LUA_VERSION working correctly')"
"$LUA_DIR/src/luac" -v 2>&1 | head -1

echo ""
echo "=========================================="
echo "Lua $LUA_VERSION is ready to use!"
echo "=========================================="
echo ""
echo "Available binaries:"
echo "  Interpreter: $LUA_DIR/src/lua"
echo "  Compiler:   $LUA_DIR/src/luac"
echo ""
echo "Usage examples:"
echo "  # Run a Lua script"
echo "  $LUA_DIR/src/lua script.lua"
echo ""
echo "  # Run inline Lua code"
echo "  $LUA_DIR/src/lua -e \"print('hello')\""
echo ""
echo "  # Compile to bytecode"
echo "  $LUA_DIR/src/luac script.lua -o script.luac"
echo ""
echo "  # Compare with rilua output"
echo "  $LUA_DIR/src/lua -e 'test code'  # Standard Lua"
echo "  cargo run --bin rilua -- -e 'test code'  # rilua"
```

**Quick One-Liner Setup**:
```bash
# Download, verify, unpack, and compile in one command
curl -fsSL https://lua.org/ftp/lua-5.1.1.tar.gz -o lua-5.1.1.tar.gz && \
echo "c5daeed0a75d8e4dd2328b7c7a69888247868154acbda69110e97d4a6e17d1f0  lua-5.1.1.tar.gz" | sha256sum -c && \
tar -xzf lua-5.1.1.tar.gz && \
cd lua-5.1.1 && make linux && \
cd .. && echo "Lua 5.1.1 ready at ./lua-5.1.1/src/lua"
```

### Debug Builds

Set environment variables **before compiling** (compile-time flags via `option_env!`):

```bash
LUA_DEBUG_LEXER=1 cargo build    # Print tokens during lexing
LUA_DEBUG_PARSER=1 cargo build   # Print AST after parsing
LUA_DEBUG_COMPILER=1 cargo build # Print bytecode after compilation
LUA_DEBUG_VM=1 cargo build       # Print each VM instruction
LUA_DEBUG_GC=1 cargo build       # Print GC statistics
```

Example: `LUA_DEBUG_VM=1 cargo run --bin rilua -- test.lua`

## Code Organization

**Pipeline**: Source -> Lexer -> Parser -> AST -> Compiler -> Proto -> VM

Key directories: `src/compiler/` (lexer, parser, codegen), `src/vm/`
(state, execute, GC, tables, strings, closures), `src/stdlib/` (one
file per library), `src/bin/` (rilua, riluac).

See `docs/src/architecture.md` for the full module tree and design
documentation.

### Key Design Decisions

| Decision     | Choice                                     | Doc                                              |
| ------------ | ------------------------------------------ | ------------------------------------------------ |
| Architecture | Design principles and module structure     | [docs/src/architecture.md](docs/src/architecture.md)     |
| API          | Trait-based, Rust-idiomatic                | [docs/src/api.md](docs/src/api.md)                       |
| Stdlib       | Modular, per-library files                 | [docs/src/stdlib.md](docs/src/stdlib.md)                 |
| Features     | Lua 5.1.1 compatibility coverage           | [docs/src/features.md](docs/src/features.md)             |
| Testing      | Spec-driven, multi-layer                   | [docs/src/testing.md](docs/src/testing.md)               |
| References   | Studied implementations                    | [docs/src/references.md](docs/src/references.md)         |
| Use cases    | WoW ecosystem and general embedding        | [docs/src/use-cases.md](docs/src/use-cases.md)           |

### Naming Conventions

- **Modules**: lowercase with underscores (`call_info`, not `callInfo`)
- **Types**: PascalCase (`LuaState`, `CallInfo`, `Proto`)
- **Functions/Methods**: snake_case (`do_call`, `push_value`)
- **Constants**: SCREAMING_SNAKE_CASE (`LUA_VERSION`, `MAX_STACK`)
- **Enum variants**: PascalCase (standard Rust)

### Value Types

The main value enum is `Val` in `src/vm/value.rs`. Variants include:
- `Nil`, `Bool(bool)`, `Number(f64)` - primitives
- `String(GcRef<String>)` - interned strings
- `Table(GcRef<Table>)` - tables
- `Function(Function)` - functions (closure or C function)
- `Thread(GcRef<Thread>)` - coroutines

## Lint Configuration

**Very strict** - defined in `Cargo.toml`:

- Clippy groups: `all`, `pedantic`, `nursery`, `cargo` (all at warn level)
- Safety lints (high priority): `unwrap_used`, `panic`, `todo`, `unimplemented`, `expect_used`
- Targeted allows for VM/compiler-specific patterns (casts, enum glob use)

**Implications**:
- Must handle errors explicitly with `Result` - no `unwrap()` or `expect()` in non-test code
- Use `?` operator for error propagation
- Test code can use `#[allow(clippy::unwrap_used)]` for assertions

## Testing Approach

### Unit Tests

Located in each module via `#[cfg(test)] mod tests` blocks. Test internal functionality.

### Integration Tests

Located in `tests/` directory. Run Lua files through the full pipeline and use `assert()` to validate behavior.

### Oracle Comparison Tests

Framework in `tests/helpers/oracle.rs`. Runs the same Lua code in both rilua and PUC-Rio Lua 5.1.1, comparing stdout/stderr/exit code.

Requires reference Lua binary:
- Environment variable: `LUA_REFERENCE_BIN`
- Default path: `./lua-5.1.1/src/lua`

### Bytecode Comparison Tests

Compile Lua snippets with both rilua and `luac -l`, comparing instruction output.

### PUC-Rio Test Suite

Official Lua 5.1.1 test files in `./lua-5.1-tests/` (from official tarball).
All 23/23 PUC-Rio tests pass. See `docs/src/testing.md` for details.

## CLI Specification

The `rilua` binary reproduces the PUC-Rio `lua.c` command-line interface.
See `README.md` for usage examples. Additional REPL behaviors:

- `LUA_INIT` environment variable (execute string, or `@filename`)
- `arg` table with script and interpreter arguments
- `_PROMPT`/`_PROMPT2` globals, multiline input detection, `=expr` shorthand
- SIGINT handling (interrupt flag checked per instruction; Unix raw FFI signal(), Windows SetConsoleCtrlHandler)
- Version string: `"Lua 5.1.1  Copyright (C) 1994-2006 Lua.org, PUC-Rio"`
- Exit codes: 0 on success, 1 (EXIT_FAILURE) on error

See `lua.c` in the PUC-Rio source for reference.

## Reference Repositories

See `docs/src/references.md` for full list. Key references:

| Repository                                    | Purpose                                      |
| --------------------------------------------- | -------------------------------------------- |
| `./lua-5.1.1/` (from official tarball)        | PUC-Rio source - authoritative semantics     |
| `./lua-5.1-tests/` (from official tests)      | Official test suite - compatibility target   |
| `~/Repos/github.com/luau-lang/luau`           | Architecture reference (AST-based pipeline)  |
| `~/Repos/github.com/mlua-rs/mlua`             | API design reference (Rust-idiomatic)        |
| `~/Repos/github.com/CppCXY/lua-rs`            | Benchmark comparison (Rust, Lua 5.5 port)    |
| `~/Repos/github.com/cogwheel/lua-wow`         | WoW-compatible Lua configuration             |
| `~/Repos/github.com/Meorawr/elune`            | Lua 5.1 with WoW taint model                 |
| `~/Repos/sourceforge.net/projects/wowbench`   | WoW addon environment test harness           |

### Reference Distributions

The PUC-Rio source distribution contains files not in git (notably `luac.c` for the bytecode compiler/lister).

| Archive   | URL                                    | SHA256                                                             |
| --------- | -------------------------------------- | ------------------------------------------------------------------ |
| Lua 5.1.1 | <https://lua.org/ftp/lua-5.1.1.tar.gz> | `c5daeed0a75d8e4dd2328b7c7a69888247868154acbda69110e97d4a6e17d1f0` |
| Lua 5.1.5 | <https://lua.org/ftp/lua-5.1.5.tar.gz> | `2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333` |

### Reference Binaries for Testing

After running the setup script above, PUC-Rio binaries are at
`./lua-5.1.1/src/lua` and `./lua-5.1.1/src/luac`. Oracle comparison
tests use `LUA_REFERENCE_BIN` (see [Oracle Comparison Tests](#oracle-comparison-tests)).

**PUC-Rio luac usage** (not riluac):
- `luac -l script.lua` - list bytecode instructions
- `luac -l -l script.lua` - list with constants and locals
- `luac -o out.luac script.lua` - compile to bytecode

### Official Test Suite

The official test suite is available for comparison:
- **Download URL**: https://www.lua.org/tests/lua5.1-tests.tar.gz
- **SHA256**: `49e4ca6561f82ea605908c5041ab5fad66ed9930fa0686675bd51b02767f18ad`
- **Location**: `./lua-5.1-tests/` (git-ignored)
- **Documentation**: https://lua.org/tests/

```bash
# Download test suite
curl -fsSL https://www.lua.org/tests/lua5.1-tests.tar.gz -o lua-5.1-tests.tar.gz
echo "49e4ca6561f82ea605908c5041ab5fad66ed9930fa0686675bd51b02767f18ad  lua-5.1-tests.tar.gz" | sha256sum -c
tar -xzf lua-5.1-tests.tar.gz
mv lua5.1-tests lua-5.1-tests   # tarball extracts to lua5.1-tests/
```

#### Running PUC-Rio Tests with rilua

```bash
# Tests MUST be run from the lua-5.1-tests/ directory
cd lua-5.1-tests
mkdir -p libs

# Run the full suite via all.lua
RILUA_TEST_LIB=1 ../target/release/rilua all.lua

# Run a single test
RILUA_TEST_LIB=1 LUA_PATH="?;./?.lua" ../target/release/rilua <test>.lua

# Compare all tests between PUC-Rio and rilua
scripts/compare.sh ./lua-5.1.1/src/lua ./target/release/rilua
```

`RILUA_TEST_LIB=1` activates the T module (PUC-Rio `ltests.c`
equivalent). Build in release mode — some tests are slow in debug
builds.

See `docs/src/testing.md` for running modes, `all.lua` behavior, and
T module details.

The [Lua 5.1 Reference Manual](https://lua.org/manual/5.1/) is authoritative
for language semantics.

## Performance Gate

Every commit must produce the same performance or better against the
PUC-Rio test suite. The current baseline is stored in `.perf-baseline`
(median milliseconds over 5 runs of the full test suite in release mode).

### Measuring performance

```bash
./scripts/bench-puc-rio.sh [binary] [runs]
```

Runs the PUC-Rio test suite multiple times and reports min/median/max
wall-clock time in milliseconds. Defaults to `target/release/rilua` and
5 runs.

### Checking for regression

```bash
./scripts/perf-gate.sh [baseline_ms] [threshold_pct]
```

Builds release, runs 5 iterations, and fails if the median exceeds the
baseline by more than the threshold (default 5%). Reads baseline from
`.perf-baseline` if no argument provided.

### Updating the baseline

After a confirmed performance improvement, update the baseline:

```bash
./scripts/bench-puc-rio.sh target/release/rilua 5 > .perf-baseline
```

### Rules

- Performance must not regress by more than 5% vs baseline.
- After confirmed improvements, update `.perf-baseline`.
- Profile with `cargo flamegraph` before and after optimization work.
- Flamegraphs go in `flamegraphs/`, profiling notes in `docs/src/performance.md`.

## Development Workflow

1. **Before implementing**: Read relevant sections of Lua 5.1 Reference Manual and PUC-Rio C source
2. **Write code**: Follow existing patterns, handle errors explicitly
3. **Test**: Run `cargo nextest run` for affected areas
4. **Verify**: Run quality gate before committing

### Documentation (mdbook)

The `docs/` directory is an [mdbook](https://rust-lang.github.io/mdBook/)
project with [mdbook-mermaid](https://github.com/badboy/mdbook-mermaid)
support for diagrams.

**Structure:**

```text
docs/
  book.toml           mdbook configuration
  src/
    SUMMARY.md        Table of contents (required by mdbook)
    README.md          Introduction page
    architecture.md    Design and module structure
    api.md             Public Rust API
    ...
```

**Build and verify locally:**

```bash
# Install tools (or use mise install)
cargo install mdbook mdbook-mermaid

# Build the book
mdbook build docs

# Serve locally with live reload
mdbook serve docs --open
```

The generated output goes to `docs/book/` (gitignored). Read the Docs
builds the book automatically on push via `.readthedocs.yaml`.

**When modifying documentation:**

1. Edit files in `docs/src/`
2. If adding a new page, add it to `docs/src/SUMMARY.md`
3. Run `mdbook build docs` to verify no errors
4. Cross-references between docs use relative paths (e.g.,
   `[testing](testing.md)`) since all files are in the same directory

### GitHub Pages (WASM Demo)

The `gh-pages` branch hosts the WASM demo at
https://wowemulation-dev.github.io/rilua/. It is an orphan branch
containing only the built demo files (`index.html` + `pkg/`).

**Updating the demo after changes to rilua or the demo source:**

```bash
# 1. Build the WASM demo from main branch
cd examples/wasm-demo
./build.sh

# 2. Switch to gh-pages branch
git stash --include-untracked
git checkout gh-pages

# 3. Copy updated files
cp examples/wasm-demo/index.html index.html
cp examples/wasm-demo/pkg/rilua_wasm_demo_bg.wasm pkg/
cp examples/wasm-demo/pkg/rilua_wasm_demo.js pkg/
cp examples/wasm-demo/pkg/rilua_wasm_demo.d.ts pkg/
cp examples/wasm-demo/pkg/rilua_wasm_demo_bg.wasm.d.ts pkg/
cp examples/wasm-demo/pkg/package.json pkg/

# 4. Commit and push
git add index.html pkg/
git commit -m "deploy: update WASM demo for vX.Y.Z"
git push origin gh-pages

# 5. Return to main
git checkout main
git stash pop
```

The `pkg/` directory in the source tree is gitignored. The `gh-pages`
branch tracks these files directly. The demo should be rebuilt and
deployed after releases that change runtime behavior.

### When in Doubt

1. Check PUC-Rio Lua 5.1.1 source for reference behavior
2. Run oracle comparison tests to verify equivalence
3. Consult `docs/src/` directory for architecture documentation

## Writing Guidelines Addendum

### KISS and DRY

These are mandatory for all documentation, code comments, and commit
messages.

**KISS** (Keep It Simple):

- State each fact once, in the simplest form that conveys the meaning.
- Do not restate the same concept with different words across sections
  or files.
- Do not add explanatory prose when a cross-reference suffices.

**DRY** (Don't Repeat Yourself):

- Every fact has exactly one canonical location. All other mentions
  must cross-reference it, not restate it.
- If a concept is covered in a dedicated page (e.g., `wasm.md` for
  WASM details), other pages link to it instead of re-explaining.
- Code comments do not duplicate information from doc comments or
  documentation files.
- Constants, type definitions, and behavioral contracts have a single
  source of truth.

**Before writing documentation or comments**, check whether the
information already exists elsewhere. If it does, link to it.

### Technical Terms

Rust technical terms that overlap with words restricted in the global
writing guidelines are permitted when used in their precise technical
sense. For example, "safe"/"unsafe" referring to Rust's safety model
(`unsafe` blocks, safe API boundaries) and "modern" in "modern C++" as
a language standard designation are acceptable.

## Must Follow Rules

1. **Run `cargo fmt` before committing**
2. **Fix all clippy warnings** -- lint configuration is strict
3. **All tests must pass** -- both unit and integration
4. **Handle errors explicitly** -- return `Result` from library code, avoid
   `unwrap()` in non-test code
5. **Study the reference implementation before writing code** -- read the
   relevant sections of the
   [Lua 5.1 Reference Manual](https://lua.org/manual/5.1/) and the
   corresponding C source in `./lua-5.1.1/src/`. The goal is behavioral
   equivalence with PUC-Rio.
6. **Validate against Lua 5.1.1 semantics** -- use the reference C source
   as the authority for language semantics, error messages, and standard
   library behavior.
7. **Zero external dependencies** -- only Rust's standard library. Do not
   add crates to `Cargo.toml`.
8. **Do not commit agent-local configuration** -- tool-specific
   directories such as `.claude/`, `.serena/`, `.cursor/`, `.aider*`,
   and similar local agent state are not part of the project. `AGENTS.md`
   itself **is** committed (it is the shared guide for all agents).
9. **Apply the karpathy-guidelines skill on every change** -- before
   writing or modifying code, invoke the `karpathy-guidelines` skill and
   follow its rules: state assumptions, keep changes minimal and
   surgical, and define verifiable success criteria. This is mandatory,
   not advisory.

## Task Completion

No task is considered complete until the full quality gate passes:

```bash
cargo fmt -- --check && cargo clippy --all-targets && cargo nextest run && cargo deny check && cargo doc --no-deps
```

For changes that affect runtime performance (VM, GC, compiler, stdlib),
also verify no regression:

```bash
./scripts/perf-gate.sh
```

## MediaWiki (WoW Wiki)

A MediaWiki instance is available at https://wowwiki.k8s.kogito.corp/
for documenting Blizzard NGDP, World of Warcraft Classic internals, and
related technical research.

Interact with the wiki using the Playwright MCP or the `playwright-cli`
skill.

### Accounts

| Account              | Password               | Purpose                                      |
| -------------------- | ---------------------- | -------------------------------------------- |
| `Danielsreichenbach` | `3wLLUMzResUWE8HWBDcZ` | Content editing (Daniel S. Reichenbach)       |
| `admin`              | `Yna8JQnPNvLc`         | MediaWiki administration (skins, extensions)  |

**Rules:**

- Use the `Danielsreichenbach` account for all content editing.
- Use the `admin` account only for MediaWiki configuration (skins,
  extensions, namespaces, permissions).

## Toolchain

- Rust edition 2024, minimum 1.92.0 (managed via `.mise.toml`)
- No `rustfmt.toml` or `clippy.toml` -- uses defaults with lint overrides in `Cargo.toml`
- CI: GitHub Actions (`.github/workflows/ci.yml`) with nextest, clippy, docs
