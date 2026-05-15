//! Mirrors the "Embedding Example" code block in `docs/src/api.md`.
//!
//! Keep this file in sync with the snippet: changes here must be
//! reflected in the docs and vice versa.
//!
//! Usage:
//!     cargo run --example embedding

#![allow(clippy::float_cmp)]

use rilua::{Lua, LuaApiMut, StdLib, Val};

fn main() -> rilua::LuaResult<()> {
    let mut lua = Lua::new_with(StdLib::ALL)?;

    // Execute Lua code
    lua.exec(
        r#"
        x = 1 + 2
        msg = string.format("x = %d", x)
    "#,
    )?;

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
