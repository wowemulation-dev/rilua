# Architecture Overview

rilua is a from-scratch implementation of Lua 5.1.1 in Rust. The goal is
behavioral equivalence with the PUC-Rio reference interpreter — executed
Lua code must produce identical results. Internal architecture is free to
diverge where Rust idioms offer better safety, clarity, or modularity.

## Design Principles

1. **Behavioral equivalence** — Lua code produces the same results as
   PUC-Rio Lua 5.1.1. Observable behavior (output, errors, GC API
   returns, weak table clearing, finalizer execution) must match.
2. **Idiomatic Rust** — Use Rust's type system, ownership, and error
   handling. Minimize unsafe code. Prefer enums over tagged unions,
   `Result` over longjmp, traits over function pointers.
3. **Zero external dependencies** — Only Rust's standard library. All
   data structures, algorithms, and patterns are self-contained.
4. **Modular design** — Clear module boundaries. The compiler does not
   depend on the VM. The GC does not depend on the compiler. Standard
   library functions are isolated from core VM logic.
5. **Spec-driven testing** — Test against the Lua 5.1.1 specification
   and PUC-Rio's official test suite. Unit tests for internals,
   integration tests for language semantics.

## Pipeline

```text
Source Code
    |
    v
 [Lexer]         src/compiler/lexer.rs
    |  tokens
    v
 [Parser]        src/compiler/parser.rs
    |  AST
    v
 [Compiler]      src/compiler/codegen.rs
    |  Proto (bytecode + constants + nested protos)
    v
 [VM]            src/vm/
    |  execution
    v
 Output / Side Effects
```

Unlike PUC-Rio's single-pass compiler that emits bytecode during
parsing, rilua uses an explicit AST intermediate representation. This
follows the approach used by Luau (Roblox's Lua 5.1-compatible scripting language).

Benefits of the AST phase:

- Separation between parsing and code generation
- Each phase is independently testable
- Future optimizations (constant folding, dead code elimination) can
  operate on the AST without modifying the parser
- Easier to understand and debug than interleaved parse-and-emit

## Module Structure

```text
src/
  lib.rs              Public API (Lua struct, traits, types)
  error.rs            Error types (syntax, runtime, argument)
  conversion.rs       IntoLua/FromLua trait implementations
  handles.rs          Table/Function/Thread/AnyUserData handle types
  platform.rs         Centralized FFI declarations (raw extern "C")
  bin/
    rilua.rs          Standalone interpreter (matches lua.c)
    riluac.rs         Bytecode compiler/lister (matches luac)
  compiler/
    mod.rs            Compiler module root
    lexer.rs          Tokenizer (source -> tokens)
    token.rs          Token types
    parser.rs         Parser (tokens -> AST)
    ast.rs            AST node types
    codegen.rs        Code generator (AST -> Proto)
  vm/
    mod.rs            VM module root
    state.rs          Lua state (the main VM struct)
    execute.rs        Instruction dispatch loop
    instructions.rs   Opcode definitions (Rust enums)
    proto.rs          Function prototype (bytecode container)
    value.rs          Value representation (Val enum)
    gc/
      mod.rs          GC module root
      arena.rs        Generational arena (typed Vec storage)
      collector.rs    Mark-sweep collector
      trace.rs        Trace trait for marking reachable objects
    table.rs          Table implementation (array + hash parts)
    string.rs         String interning
    closure.rs        Closures and upvalues
    callinfo.rs       Call stack (CallInfo chain)
    metatable.rs      Metamethod dispatch
    debug_info.rs     Debug info and variable name resolution
    dump.rs           Binary chunk serialization (string.dump)
    undump.rs         Binary chunk deserialization (loadstring)
    listing.rs        Bytecode listing (riluac -l output)
  stdlib/
    mod.rs            Standard library registration
    base.rs           Base library (print, assert, type, etc.)
    coroutine.rs      Coroutine library
    string.rs         String library
    table.rs          Table library
    math.rs           Math library
    io.rs             I/O library
    os.rs             OS library
    debug.rs          Debug library
    package.rs        Package/module library
    testlib.rs        T test module (PUC-Rio ltests.c equivalent)
```

## Key Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Compilation pipeline | Lexer -> Parser -> AST -> Compiler | Separation of concerns, testability |
| Instruction set | PUC-Rio's 38 opcodes as Rust enums | Behavioral equivalence, type safety |
| Value representation | Rust enum (Val) | Type safety, pattern matching |
| Garbage collection | Arena with generational indices | Zero unsafe, mark-sweep |
| Tables | Array + hash dual representation | Performance, PUC-Rio compatibility |
| Strings | Interned with cached hash | Pointer equality, O(1) comparison |
| Closures and upvalues | Open/closed upvalue model | PUC-Rio semantics |
| Error handling | Result-based | Idiomatic Rust, no longjmp |
| Public API | Trait-based, Rust-idiomatic | Ergonomic embedding ([api.md](api.md)) |
| Standard library | Modular, per-library files | Independent testing, optional loading ([stdlib.md](stdlib.md)) |
| Call stack | Dynamic CallInfo array | Separate from value stack, index-based |
| Metatables | PUC-Rio 5.1.1 dispatch semantics | 17 metamethods, type coercion rules |
| Coroutines | Threads with shared GC heap | Independent stacks, cooperative multithreading |
| Testing strategy | Spec-driven, multi-layer | Correctness assurance ([testing.md](testing.md)) |
| Platform abstraction | Centralized FFI with WASM stubs | Cross-platform without conditional code in consumers ([wasm.md](wasm.md)) |

## Platform Support

All C FFI declarations are centralized in `src/platform.rs`. FFI call
sites appear in the VM dispatch loop (`execute.rs`) and the
`io`/`os`/`string` libraries. See [wasm.md](wasm.md) for WASM-specific
stubs and library availability.

Supported targets:

| Target | Status | Notes |
|--------|--------|-------|
| `x86_64-unknown-linux-gnu` | Full | Primary development platform |
| `x86_64-apple-darwin` / `aarch64-apple-darwin` | Full | macOS (Intel + Apple Silicon) |
| `x86_64-pc-windows-msvc` | Full | MSVC toolchain, links ucrt |
| `wasm32-unknown-unknown` | Core | No I/O/OS; see [wasm.md](wasm.md) |

## Reference Implementations

See [references.md](references.md) for a classification of all studied
implementations and what we learned from each.
