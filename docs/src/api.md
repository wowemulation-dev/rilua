# Public API

## Decision

**Rust-idiomatic, trait-based API inspired by mlua's design. Not a
1:1 mirror of the Lua C API.**

## Overview

PUC-Rio Lua exposes a stack-based C API where values are pushed and
popped from a virtual stack. This works well in C but is unergonomic
in Rust -- it lacks type safety, requires manual stack management,
and does not leverage Rust's trait system. rilua uses traits for type
conversion, methods for common operations, and the type system for
safety instead.

See [future-api.md](future-api.md) for planned ergonomic improvements.

## Core Type: Lua

```text
/// A Lua interpreter instance.
///
/// Owns the full VM state including value stack, call stack, GC heap,
/// and global table. All interaction with Lua values goes through this
/// struct.
pub struct Lua {
    // Internal state (not public)
}

impl Lua {
    /// Create a new Lua state with all standard libraries loaded.
    pub fn new() -> LuaResult<Self> { ... }

    /// Create a new Lua state without any libraries.
    pub fn new_empty() -> Self { ... }

    /// Create a new Lua state with selected standard libraries.
    ///
    /// ```ignore
    /// let lua = Lua::new_with(StdLib::BASE | StdLib::STRING)?;
    /// ```
    pub fn new_with(libs: StdLib) -> LuaResult<Self> { ... }

    // -- Execution --

    /// Compile and execute a Lua source string.
    /// Chunk name is set to `"=(string)"`.
    pub fn exec(&mut self, source: &str) -> LuaResult<()> { ... }

    /// Compile and execute Lua source bytes with a given chunk name.
    /// Source is `&[u8]` because Lua files may contain arbitrary bytes.
    pub fn exec_bytes(
        &mut self, source: &[u8], name: &str,
    ) -> LuaResult<()> { ... }

    /// Read a file and execute its contents as a Lua chunk.
    /// Chunk name is set to `@<path>`.
    pub fn exec_file(&mut self, path: &str) -> LuaResult<()> { ... }

    /// Compile a Lua source string and return a function handle.
    /// Chunk name is set to `"=(string)"`.
    pub fn load(&mut self, source: &str) -> LuaResult<Function> { ... }

    /// Compile Lua source bytes (or load a binary chunk) and return
    /// a function handle.
    pub fn load_bytes(
        &mut self, source: &[u8], name: &str,
    ) -> LuaResult<Function> { ... }

    /// Read a file (or stdin if `None`) and compile it, returning a
    /// function handle. Handles shebang lines in executable scripts.
    pub fn load_file(
        &mut self, path: Option<&str>,
    ) -> LuaResult<Function> { ... }

    // -- Globals --

    /// Get a global variable, converting via FromLua.
    /// Takes `&mut self` because looking up a string key may intern
    /// the name.
    pub fn global<V: FromLua>(
        &mut self, name: &str,
    ) -> LuaResult<V> { ... }

    /// Set a global variable from a Rust value, converting via IntoLua.
    pub fn set_global<V: IntoLua>(
        &mut self, name: &str, value: V,
    ) -> LuaResult<()> { ... }

    // -- Object creation --

    /// Allocate a new empty table and return a handle.
    pub fn create_table(&mut self) -> Table { ... }

    /// Intern a byte string via the GC string table.
    pub fn create_string(&mut self, s: &[u8]) -> Val { ... }

    /// Register a Rust function as a global Lua function.
    /// See [Implementing Native Functions](#implementing-native-functions)
    /// for the `RustFn` signature and usage.
    pub fn register_function(
        &mut self, name: &str, func: RustFn,
    ) -> LuaResult<()> { ... }

    // -- Userdata creation --

    /// Create a new userdata containing `data` with no metatable.
    pub fn create_userdata<T: Any>(&mut self, data: T) -> AnyUserData { ... }

    /// Create a new userdata with a named, registry-cached metatable.
    /// The metatable is stored in the registry under `type_name` and
    /// reused for subsequent calls with the same name.
    pub fn create_typed_userdata<T: Any>(
        &mut self, data: T, type_name: &str,
    ) -> LuaResult<AnyUserData> { ... }

    /// Create or retrieve a named metatable for a userdata type.
    pub fn create_userdata_metatable(
        &mut self, type_name: &str,
    ) -> LuaResult<Table> { ... }

    // -- Calling functions --

    /// Call a loaded Function handle with arguments and collect results.
    ///
    /// Arguments and results are raw `Val` values. For type-safe
    /// conversions, use `IntoLua`/`FromLua` on individual values.
    pub fn call_function(
        &mut self, func: &Function, args: &[Val],
    ) -> LuaResult<Vec<Val>> { ... }

    /// Call a loaded Function handle, appending a stack traceback
    /// on error. Used by the CLI to match PUC-Rio's `docall` pattern.
    pub fn call_function_traced(
        &mut self, func: &Function, args: &[Val],
    ) -> LuaResult<Vec<Val>> { ... }

    // -- Table operations --

    /// Raw set on a table handle (no metamethod dispatch).
    pub fn table_raw_set(
        &mut self, table: &Table, key: Val, value: Val,
    ) -> LuaResult<()> { ... }

    // -- GC control --

    /// Run a full garbage collection cycle.
    pub fn gc_collect(&mut self) -> LuaResult<()> { ... }

    /// Get current memory usage in bytes.
    pub fn gc_count(&self) -> usize { ... }

    /// Stop automatic garbage collection.
    pub fn gc_stop(&mut self) { ... }

    /// Restart automatic garbage collection.
    pub fn gc_restart(&mut self) { ... }

    /// Perform an incremental GC step. Returns true if a cycle completed.
    pub fn gc_step(&mut self, step_size: i64) -> LuaResult<bool> { ... }

    /// Set GC pause parameter (percentage). Returns the previous value.
    pub fn gc_set_pause(&mut self, pause: u32) -> u32 { ... }

    /// Set GC step multiplier. Returns the previous value.
    pub fn gc_set_step_multiplier(&mut self, stepmul: u32) -> u32 { ... }
}
```

## Selective Library Loading

```text
bitflags! {
    pub struct StdLib: u16 {
        const BASE      = 0x0001;
        const PACKAGE   = 0x0002;
        const TABLE     = 0x0004;
        const IO        = 0x0008;
        const OS        = 0x0010;
        const STRING    = 0x0020;
        const MATH      = 0x0040;
        const DEBUG     = 0x0080;
        const COROUTINE = 0x0100;
        const ALL       = 0x01FF;
    }
}
```

Use `Lua::new_with(StdLib::BASE | StdLib::STRING)` to load only
selected libraries. This enables sandboxing by excluding dangerous
libraries (IO, OS, debug).

## Conversion Traits

### IntoLua / FromLua

```text
/// Convert a Rust value into a Lua value.
pub trait IntoLua {
    fn into_lua(self, lua: &mut Lua) -> LuaResult<Val>;
}

/// Convert a Lua value into a Rust value.
pub trait FromLua: Sized {
    fn from_lua(val: Val, lua: &Lua) -> LuaResult<Self>;
}
```

Implemented for:

| Rust Type | Lua Type | Direction |
|-----------|----------|-----------|
| `()` | nil | both |
| `bool` | boolean | both |
| `f64`, `f32` | number | both |
| `i8`..`i64`, `u8`..`u64` | number (with range check) | both |
| `String` | string | both |
| `&str` | string | IntoLua only |
| `&[u8]` | string | IntoLua only |
| `Vec<u8>` | string (raw bytes) | FromLua only |
| `Val` | (any) | both (passthrough) |
| `Table` | table | both |
| `Function` | function | both |
| `Thread` | thread | both |
| `AnyUserData` | userdata | both |

### IntoLuaMulti / FromLuaMulti

For functions with multiple arguments or return values:

```text
pub trait IntoLuaMulti {
    fn into_lua_multi(self, lua: &mut Lua) -> LuaResult<Vec<Val>>;
}

pub trait FromLuaMulti: Sized {
    fn from_lua_multi(values: &[Val], lua: &Lua) -> LuaResult<Self>;
}
```

Implemented for `Vec<Val>` (variable-length) and `()` (zero values).

## Handle Types

### Table

```text
pub struct Table(/* GcRef<table::Table> */);

impl Table {
    /// Get a value by key (raw, no metamethods).
    pub fn raw_get(
        &self, state: &LuaState, key: Val,
    ) -> LuaResult<Val> { ... }

    /// Set a value by key (raw, no metamethods).
    pub fn raw_set(
        &self, state: &mut LuaState, key: Val, value: Val,
    ) -> LuaResult<()> { ... }

    /// Raw length (no `__len` metamethod).
    pub fn raw_len(&self, state: &LuaState) -> i64 { ... }

    /// Set or clear the metatable.
    pub fn set_metatable(
        &self, state: &mut LuaState, mt: Option<Table>,
    ) -> LuaResult<()> { ... }

    /// Get the underlying GcRef.
    pub fn gc_ref(self) -> GcRef<table::Table> { ... }
}
```

Note: Table handle methods take `&LuaState` (the internal VM state),
not `&Lua`. This is because handles are used both from the public API
and from stdlib internals. Use `lua.state()` / `lua.state_mut()` (which
are `pub(crate)`) to get the state reference, or use
`Lua::table_raw_set()` for the public-facing convenience method.

### Function

```text
pub struct Function(/* GcRef<Closure> */);

impl Function {
    /// Get the underlying GcRef.
    pub fn gc_ref(self) -> GcRef<Closure> { ... }
}
```

Call a Function via `Lua::call_function()` or `Lua::call_function_traced()`.

### Thread

```text
pub struct Thread(/* GcRef<LuaThread> */);

impl Thread {
    /// Get the status of this coroutine thread.
    pub fn status(&self, state: &LuaState) -> ThreadStatus { ... }

    /// Get the underlying GcRef.
    pub fn gc_ref(self) -> GcRef<LuaThread> { ... }
}

pub enum ThreadStatus {
    Initial,    // loaded, not yet started
    Running,    // currently executing
    Suspended,  // yielded, waiting to be resumed
    Normal,     // resumed another coroutine, waiting
    Dead,       // finished or errored
}
```

### AnyUserData

```text
pub struct AnyUserData(/* GcRef<Userdata> */);

impl AnyUserData {
    /// Borrow the inner data as `&T`.
    /// Returns None if the type doesn't match or the userdata was collected.
    pub fn borrow<'a, T: Any>(
        &self, state: &'a LuaState,
    ) -> Option<&'a T> { ... }

    /// Borrow the inner data as `&mut T`.
    pub fn borrow_mut<'a, T: Any>(
        &self, state: &'a mut LuaState,
    ) -> Option<&'a mut T> { ... }

    /// Set or clear the metatable.
    pub fn set_metatable(
        &self, state: &mut LuaState, mt: Option<Table>,
    ) -> LuaResult<()> { ... }

    /// Get the metatable, if set.
    pub fn metatable(&self, state: &LuaState) -> Option<Table> { ... }

    /// Get the underlying GcRef.
    pub fn gc_ref(self) -> GcRef<Userdata> { ... }
}
```

## Embedding Example

Mirrored at [`examples/embedding.rs`](../../examples/embedding.rs).

```rust
use rilua::{Lua, LuaApiMut, StdLib, Val};

fn main() -> rilua::LuaResult<()> {
    let mut lua = Lua::new_with(StdLib::ALL)?;

    // Execute Lua code
    lua.exec(r#"
        x = 1 + 2
        msg = string.format("x = %d", x)
    "#)?;

    // Read Lua globals from Rust
    let x: f64 = lua.global("x")?;
    assert_eq!(x, 3.0);

    let msg: String = lua.global("msg")?;
    assert_eq!(msg, "x = 3");

    // Set Lua globals from Rust
    lua.set_global("greeting", "hello from Rust")?;
    lua.exec("print(greeting)")?;

    // Load and call a function
    let func = lua.load("return 1 + 2")?;
    let results = lua.call_function(&func, &[])?;
    assert_eq!(results, vec![Val::Num(3.0)]);

    Ok(())
}
```

## Implementing Native Functions

Native functions use the low-level stack-based API via `LuaState`.
This is the same API used by the standard library implementation.

Mirrored at [`examples/native_function.rs`](../../examples/native_function.rs).

```rust
use rilua::vm::state::LuaState;
use rilua::LuaResult;
use rilua::{Lua, LuaApiMut, RustFn};

/// A native function that adds two numbers.
/// Arguments are on the stack at indices base..top.
/// Returns the number of results pushed.
fn my_add(state: &mut LuaState) -> LuaResult<u32> {
    let a = state.check_number(1)?;
    let b = state.check_number(2)?;
    state.push_number(a + b);
    Ok(1)
}

fn main() -> rilua::LuaResult<()> {
    let mut lua = Lua::new()?;
    let f: RustFn = my_add;
    lua.register_function("my_add", f)?;
    lua.exec("print(my_add(10, 20))")?; // prints 30
    Ok(())
}
```

The `RustFn` type is `fn(&mut LuaState) -> LuaResult<u32>`. It takes
a function pointer, not a closure. For stateful native functions, use
RustClosure upvalues (see [future-api.md](future-api.md) for a
planned closure-based alternative).

## Internal Stack Model

Internally, rilua uses a virtual stack similar to PUC-Rio's C API
for stdlib function implementation. Understanding this model is
necessary for implementing stdlib functions and the debug library.

### Stack Index Addressing

Stack indices address values relative to the current call frame:

| Index type | Range | Resolution |
|-----------|-------|------------|
| Positive | 1, 2, 3, ... | Base-relative: `base + index - 1` |
| Negative | -1, -2, -3, ... | Top-relative: `top + index` |
| Pseudo-index | special constants | Registry, globals, environment, upvalues |

**Pseudo-indices** provide access to non-stack locations:

| Pseudo-index | Target |
|-------------|--------|
| `REGISTRY_INDEX` | Global registry table (shared across all threads) |
| `GLOBALS_INDEX` | Current thread's global table |
| `ENVIRON_INDEX` | Current function's environment table |
| Upvalue indices | C closure upvalue slots (one per captured value) |

### Push/Get Protocol

**Push operations** write to `stack[top]` then increment `top`.
Operations that allocate GC objects (strings, tables, closures)
trigger a GC check. Operations on immediate values (nil, boolean,
number) do not.

**Get operations** read from the addressed stack slot. Most are
non-mutating. Exception: number-to-string conversion mutates the
stack slot in place and triggers a GC check (string allocation).

**C closure upvalues** are stored as values directly inside the
closure struct (not as UpVal objects like Lua closures). When
creating a C closure with `n` upvalues, the top `n` stack values
are popped and copied into the closure's upvalue array.

### Table Operations

**With metamethods** (`gettable`/`settable`): invoke the full
`__index`/`__newindex` chain (up to 100 iterations). `gettable`
takes the key from the stack top, replaces it with the result.
`settable` takes key and value from the top, pops both.

**Without metamethods** (`rawget`/`rawset`): bypass the metamethod
chain. Call `luaH_get`/`luaH_set` directly. The raw variants require
the target to be a table (not just anything with a metatable).

**Integer shortcuts** (`rawgeti`/`rawseti`): take the integer key
as a parameter instead of from the stack.

### Call Protocol

1. Push the function onto the stack.
2. Push arguments in order (first argument pushed first).
3. Call with `(nargs, nresults)`.
4. The function and all arguments are removed.
5. `nresults` results are pushed (excess discarded, missing padded
   with nil). `MULTRET` (-1) means push all results.

**Protected calls** return a status code. On error, the function and
arguments are replaced by a single error message on the stack. An
optional error handler function is specified by stack index (converted
to a stack offset internally because the stack may be reallocated).

### Registry and Environments

**The registry** is a global table stored in the shared state. It is
accessible via the registry pseudo-index. C libraries use it to store
private data (metatables, module state, references) that should not
be accessible from Lua code.

**Function environments**: every closure (C or Lua) has an associated
environment table. For Lua closures, the environment is the table
used for global variable lookups. The environment is captured from
the creating function when a new closure is created.

**Thread environments**: each thread has its own global table. The
main thread's global table is the default environment for new
closures.

### Reference System

The reference system provides a way to store Lua values in a table
(typically the registry) and retrieve them later by integer handle.
It is a free-list allocator using integer keys:

- `ref(table)` pops a value from the stack, stores it at an integer
  key. If there is a free slot (from a previous `unref`), it reuses
  it. Otherwise appends at `#table + 1`.
- `unref(table, ref)` adds the slot back to the free list.
- `table[0]` stores the free list head. Each free slot stores the
  index of the next free slot as its value.
- Special constants: `REF_NIL` (-1) for nil values (never stored),
  `NO_REF` (-2) for invalid references.

### Library Registration Protocol

Each standard library is loaded by pushing its opener function,
pushing the library name as argument, then calling. The opener:

1. Creates a table for the library (or reuses an existing one from
   `_LOADED`).
2. Registers all functions via closure creation and field assignment.
3. Stores the table in both `_LOADED[name]` and as a global.

The base library uses an empty name and registers directly into the
global table.

### GC Handle Safety

Values on the Lua stack (between `stack[0]` and `stack[top-1]`) are
marked as reachable during GC traversal. This is the primary
mechanism for protecting values from collection.

**Stack traversal during GC**:

1. Mark the thread's global table.
2. Mark all values from `stack[0]` to `stack[top-1]`.
3. Nil out slots from `top` to the maximum `ci.top` across all call
   frames (clears stale references).

**GC checks** (`checkGC`) run after operations that allocate GC
objects. The check occurs before the allocation in the API function,
which is safe because the new object does not exist yet. After
allocation, the object is immediately placed on the stack (making it
reachable).

**Write barriers** maintain the tri-color invariant during
incremental GC:

- **Forward barrier**: when a black object (already traversed) gets
  a white value (not yet traversed) stored into it, the white value
  is immediately marked.
- **Back barrier** (for tables): the table is re-grayed so it will
  be re-traversed.

C closure upvalues and table entries both require write barriers when
mutated through the API.

