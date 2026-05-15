# Future API Enhancements

Planned ergonomic improvements for the rilua embedding API. These are
not yet implemented. See `docs/api.md` for the current API.

## Closure-Based Function Creation

Currently, native functions must be declared as `fn` pointers
(`fn(&mut LuaState) -> LuaResult<u32>`). This prevents capturing
state. A closure-based API would allow registering Rust closures
directly:

```text
impl Lua {
    /// Create a Lua-callable function from a Rust closure.
    pub fn create_function<F>(&mut self, func: F) -> LuaResult<Function>
    where
        F: Fn(&mut Lua) -> LuaResult<u32> + 'static,
    { ... }
}
```

This requires storing a `Box<dyn Fn>` inside the closure rather than
a bare function pointer. The current `RustClosure` struct uses upvalue
slots for state, which works but is less ergonomic for embedders.

## Trait-Based Call and Resume

Function calling and coroutine resuming currently use raw `Val` slices.
Trait-based versions would automatically convert arguments and results:

```text
impl Lua {
    /// Call a Lua function with trait-based argument/result conversion.
    pub fn call<A, R>(&mut self, func: Function, args: A) -> LuaResult<R>
    where
        A: IntoLuaMulti,
        R: FromLuaMulti,
    { ... }

    /// Protected call -- catches Lua errors.
    pub fn pcall<A, R>(
        &mut self, func: Function, args: A,
    ) -> std::result::Result<R, LuaError>
    where
        A: IntoLuaMulti,
        R: FromLuaMulti,
    { ... }

    /// Create a new coroutine from a function.
    pub fn create_thread(
        &mut self, func: Function,
    ) -> LuaResult<Thread> { ... }
}

impl Thread {
    pub fn resume<A: IntoLuaMulti, R: FromLuaMulti>(
        &self, lua: &mut Lua, args: A,
    ) -> LuaResult<R> { ... }
}

impl Function {
    pub fn call<A: IntoLuaMulti, R: FromLuaMulti>(
        &self, lua: &mut Lua, args: A,
    ) -> LuaResult<R> { ... }
}
```

This depends on tuple impls for `IntoLuaMulti` / `FromLuaMulti`
(see below).

## Tuple Multi-Value Conversions

Currently `IntoLuaMulti` and `FromLuaMulti` are only implemented for
`Vec<Val>` and `()`. Tuple impls would enable ergonomic multi-argument
and multi-return patterns:

```text
// Implemented for tuples up to reasonable arity:
impl<A: IntoLua, B: IntoLua> IntoLuaMulti for (A, B) { ... }
impl<A: FromLua, B: FromLua> FromLuaMulti for (A, B) { ... }
// ... up to 8 or 12 elements
```

This enables patterns like:

```text
let (name, age): (String, f64) = lua.call(func, ("query", 42))?;
```

## Container Conversions

Additional `IntoLua` / `FromLua` implementations for standard Rust
container types:

| Rust Type | Lua Type | Notes |
|-----------|----------|-------|
| `Vec<T>` | table (sequence) | Keys are 1-indexed integers |
| `HashMap<K, V>` | table | Key and value types must implement traits |
| `Option<T>` | T or nil | `None` maps to nil, `Some(v)` maps to v |

```text
impl<T: IntoLua> IntoLua for Vec<T> { ... }
impl<T: FromLua> FromLua for Vec<T> { ... }

impl<K: IntoLua + Eq + Hash, V: IntoLua> IntoLua for HashMap<K, V> { ... }
impl<K: FromLua + Eq + Hash, V: FromLua> FromLua for HashMap<K, V> { ... }

impl<T: IntoLua> IntoLua for Option<T> { ... }
impl<T: FromLua> FromLua for Option<T> { ... }
```

## Trait-Based Table Access

Table handles currently only support raw access with `Val` keys.
Trait-based accessors would add type conversion and metamethod support:

```text
impl Table {
    /// Get with metamethods and type conversion.
    pub fn get<K: IntoLua, V: FromLua>(
        &self, lua: &mut Lua, key: K,
    ) -> LuaResult<V> { ... }

    /// Set with metamethods and type conversion.
    pub fn set<K: IntoLua, V: IntoLua>(
        &self, lua: &mut Lua, key: K, value: V,
    ) -> LuaResult<()> { ... }

    /// Length with `__len` metamethod.
    pub fn len(&self, lua: &mut Lua) -> LuaResult<i64> { ... }

    /// Iterate key-value pairs (wraps `next()`).
    pub fn pairs<K: FromLua, V: FromLua>(
        &self, lua: &mut Lua,
    ) -> TablePairs<K, V> { ... }

    /// Get the next key-value pair after `key`.
    pub fn next<K: IntoLua, NK: FromLua, NV: FromLua>(
        &self, lua: &mut Lua, key: K,
    ) -> LuaResult<Option<(NK, NV)>> { ... }
}
```

Note: the current raw methods take `&LuaState`. The trait-based
versions would take `&mut Lua` to support string interning and
metamethod dispatch.

## UserData Trait

The current userdata system uses `Box<dyn Any>` with manual metatable
construction. An mlua-style `UserData` trait would allow declarative
method and field registration:

```text
pub trait UserData {
    fn add_methods(methods: &mut UserDataMethods<Self>);
    fn add_fields(fields: &mut UserDataFields<Self>);
}

struct UserDataMethods<T> { ... }

impl<T> UserDataMethods<T> {
    pub fn add_method<M, A, R>(&mut self, name: &str, method: M)
    where
        M: Fn(&T, &mut Lua, A) -> LuaResult<R>,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    { ... }

    pub fn add_method_mut<M, A, R>(&mut self, name: &str, method: M)
    where
        M: FnMut(&mut T, &mut Lua, A) -> LuaResult<R>,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    { ... }
}
```

Usage:

```text
struct Player { name: String, health: f64 }

impl UserData for Player {
    fn add_methods(methods: &mut UserDataMethods<Self>) {
        methods.add_method("name", |p, _, ()| Ok(p.name.clone()));
        methods.add_method_mut("heal", |p, _, amount: f64| {
            p.health += amount;
            Ok(())
        });
    }
}
```

This depends on closure-based function creation and tuple multi-value
conversions.

## Native Function Helpers

Currently, native functions interact with the stack directly via
`LuaState` methods (`check_number`, `check_string`, etc.). Higher-level
helpers on `Lua` would provide trait-based argument extraction:

```text
impl Lua {
    /// Get argument at 1-based index, converting via FromLua.
    /// Raises an argument error if the value is missing or wrong type.
    pub fn check_arg<V: FromLua>(&self, index: u32) -> LuaResult<V> { ... }

    /// Push a return value onto the stack, converting via IntoLua.
    pub fn push<V: IntoLua>(&mut self, value: V) -> LuaResult<()> { ... }
}
```

This would enable:

```text
lua.register_function("greet", |lua| {
    let name: String = lua.check_arg(1)?;
    lua.push(format!("Hello, {name}!"))?;
    Ok(1)
});
```

Note: this also depends on closure-based function creation to accept
a closure that captures `&mut Lua` instead of `&mut LuaState`.

## Priority

These enhancements improve the embedding experience but are not
required for Lua 5.1.1 compatibility. They should be implemented
after the PUC-Rio test suite passes (Phase 9e).

Suggested implementation order:

1. Container conversions (`Vec<T>`, `HashMap`, `Option<T>`) --
   standalone, no dependencies
2. Tuple `IntoLuaMulti` / `FromLuaMulti` -- standalone, enables
   later items
3. Trait-based table access (`Table::get<K,V>`, `Table::set<K,V>`)
4. Closure-based function creation (`create_function`)
5. Trait-based call/resume (`Lua::call<A,R>`, `Thread::resume<A,R>`)
6. Native function helpers (`check_arg`, `push`)
7. UserData trait (depends on 2, 4, 5)
