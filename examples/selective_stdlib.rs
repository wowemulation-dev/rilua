//! Mirrors the "Loading" code block in `docs/src/stdlib.md`.
//!
//! Shows how to load only a subset of the standard libraries, useful for
//! sandboxing untrusted scripts by excluding `io`, `os`, `debug`, and
//! `package`.
//!
//! Usage:
//!     cargo run --example selective_stdlib

use rilua::{Lua, StdLib};

fn main() -> rilua::LuaResult<()> {
    let mut lua = Lua::new_with(StdLib::BASE | StdLib::STRING | StdLib::TABLE | StdLib::MATH)?;
    // io, os, debug, package omitted (sandboxed)
    lua.exec(r#"print(string.upper("ok"))"#)?;
    Ok(())
}
