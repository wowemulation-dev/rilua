# Use Cases

## Overview

rilua targets two audiences: the World of Warcraft emulation community
and Rust developers who need an embeddable Lua 5.1.1 interpreter. The
WoW use cases drive the project's priorities. General embedding is a
secondary benefit of the architecture.

## Primary: World of Warcraft

### Addon development and testing

Run and test WoW addons outside the game client. Addon authors can
validate Lua logic, check for errors, and iterate without launching
WoW. This requires:

- Lua 5.1.1 behavioral equivalence (the client's Lua version)
- WoW-specific global aliases (`strtrim`, `strsplit`, `strjoin`,
  `wipe`, `tinsert`, `tremove`, degree-based trig functions)
- WoW-specific string.format positional arguments (`%2$d`)
- TOC file parsing for addon manifest loading
- Stub implementations of the WoW API (frame system, events, CVars)

The last two items are out of scope for rilua itself but enabled by
the embedding API. A separate WoW environment layer (analogous to
[wowbench](https://sourceforge.net/projects/wowbench/)) can be built
on top.

### Server-side scripting

Private server emulators (CMaNGOS, TrinityCore, AzerothCore) embed a
Lua interpreter for scripted content: boss encounters, quests, NPC
behavior, world events. rilua can serve as an alternative to the
bundled Lua or Eluna scripting engines. This requires:

- The embedding API (`Lua::new()`, `register_function`, `UserData`)
- Reliable error handling (scripts must not crash the server)
- Controlled GC (server processes are long-lived)

### Client Lua environment emulation

Reproduce the WoW client's Lua sandbox for compatibility testing and
emulation research. The WoW client runs a modified Lua 5.1.1 with:

- **Restricted stdlib**: no `io` library, no `os.execute`, no binary
  module loading, limited `debug` library
- **Taint system**: tracks whether code originated from Blizzard
  ("secure") or addons ("insecure"). Tainted code cannot perform
  protected actions (casting spells, using items). Taint propagates
  through reads, writes, and function calls.
- **Modified GC defaults**: `gcpause=110` (more aggressive than the
  standard 200), `maxcstack=4096` (doubled from standard 2048)
- **Removed Lua 5.0 compatibility**: no `LUA_COMPAT_VARARG`,
  `LUA_COMPAT_MOD`, `LUA_COMPAT_LSTR`, `LUA_COMPAT_GFIND`
- **Bitwise operations library**: `bit.band`, `bit.bor`, `bit.bxor`,
  `bit.bnot`, `bit.lshift`, `bit.rshift`, `bit.arshift`, `bit.mod`
  (32-bit integer operations)
- **UTF-8 BOM handling**: automatically strips byte order marks

The taint system is a significant extension. It requires per-object
and per-value taint tracking with propagation modes (read, write,
both, disabled). This is a post-1.0 feature, documented separately
in [elune](https://github.com/Meorawr/elune)'s implementation.

### Addon compatibility testing

Automated verification that addons behave correctly. A test harness
loads addons via their TOC manifests and runs them against a mock WoW
environment, checking for:

- Runtime errors
- Taint violations
- Correct event handling
- Saved variable serialization

This is the [wowbench](https://sourceforge.net/projects/wowbench/)
use case. rilua provides the interpreter; the test harness and WoW
API stubs are a separate layer.

## Secondary: General Lua 5.1.1

### Embedded scripting for Rust applications

The trait-based API (`IntoLua`/`FromLua`, `UserData`, `Function`)
makes rilua usable as a general-purpose embedded scripting language
for Rust programs. Use cases include:

- Game engines (configuration, modding, entity scripting)
- Command-line tools with user-configurable behavior
- Applications that need a sandboxed extension language
- No transitive dependencies (see [architecture.md](architecture.md))

### Lua 5.1.1 conformance reference

The PUC-Rio C source (`lua-5.1.1`) is the authoritative
implementation but difficult to read. rilua provides an alternative
reference implementation in Rust with:

- Named types and enums instead of tagged unions and macros
- Explicit control flow instead of longjmp-based error handling
- Pattern matching instead of switch-case fallthrough
- Module boundaries instead of translation-unit scoping

Useful for anyone studying Lua internals or building their own
implementation.

### Educational use

rilua demonstrates language implementation techniques in a real
project:

- Lexing and tokenization
- Recursive descent parsing with Pratt operator precedence
- AST-based compilation to register bytecode
- Register allocation and jump backpatching
- Mark-sweep garbage collection with weak references
- Closure capture with open/closed upvalue conversion
- Coroutines via separate thread stacks

The design documentation in `docs/` covers each subsystem with
algorithms sourced from PUC-Rio's C code and translated into Rust
patterns.

## Out of Scope

These are explicitly not goals:

- **General-purpose Lua replacement**: Luau, LuaJIT, and PUC-Rio 5.4
  serve this better
- **Performance-critical production scripting**: no JIT, no optimized
  GC; if throughput matters, use LuaJIT
- **Lua 5.2+ features**: `goto`, integer subtype, bitwise operators
  (native), generalized `for`, `_ENV` -- all out of scope
- **Binary compatibility with PUC-Rio**: the C ABI and in-memory
  `lua_State*` layout are intentionally different
