# Examples

## run_file

Runs a Lua file passed as a command-line argument.

```bash
cargo run --example run_file -- examples/hello.lua
```

This shows the minimal embedding API:

1. `Lua::new()` creates a state with all standard libraries loaded
2. `Lua::exec_file(path)` reads, compiles, and executes a Lua file
3. Errors are printed to stderr with a non-zero exit code

## advanced_embedding

Shows advanced embedding patterns for Rust applications.

```bash
cargo run --example advanced_embedding
```

Covers:

- **Userdata with metatables**: A `Vec2` type with `__add`, `__mul`, `__len`, `__eq`, `__tostring` metamethods
- **Rust function registration**: Native functions exposed as Lua globals
- **Type conversions**: `IntoLua`/`FromLua` traits for i32, String, bool, f64, `Option<T>`
- **Calling Lua from Rust**: `call_function` to invoke Lua functions and read table results
- **Coroutine interaction**: Creating and resuming coroutines from Rust
- **Error handling**: Catching runtime errors, traced errors with stack traces

Uses `advanced.lua` as a companion script.

## hello.lua

Sample Lua script used by `run_file`. Prints a greeting and a factorial
table to exercise `print`, `string.format`, local variables, and
recursive function calls.
