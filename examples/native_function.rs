//! Mirrors the "Implementing Native Functions" code block in
//! `docs/src/api.md`.
//!
//! Keep this file in sync with the snippet: changes here must be
//! reflected in the docs and vice versa.
//!
//! Usage:
//!     cargo run --example native_function

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
