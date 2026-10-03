# Documentation

Design and reference documentation for rilua, a Lua 5.1.1 interpreter
in Rust.

## Foundation

1. [architecture.md](architecture.md) -- Design principles, module
   structure, key decisions
2. [use-cases.md](use-cases.md) -- WoW ecosystem and general embedding
   use cases
3. [references.md](references.md) -- Implementations that we studied,
   and lessons from each

## API

1. [api.md](api.md) -- Public API: Lua struct, IntoLua/FromLua, handle
   types, embedding examples
2. [future-api.md](future-api.md) -- Planned API enhancements:
   closure-based functions, UserData trait, container conversions

## Implementation

1. [features.md](features.md) -- Feature coverage and compatibility
   notes
2. [stdlib.md](stdlib.md) -- All 9 standard libraries, function lists,
   implementation notes
3. [wasm.md](wasm.md) -- WebAssembly target: library availability,
   platform stubs, building for the browser

## Quality

1. [testing.md](testing.md) -- Unit tests, integration tests, PUC-Rio
   suite, behavioral equivalence
2. [performance.md](performance.md) -- Profiling, benchmarks, regression
   gate, optimization history
