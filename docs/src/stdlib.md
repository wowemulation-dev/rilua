# Standard Library

## Decision

**Modular implementation, one file per library, matching PUC-Rio Lua
5.1.1 standard library behavior.**

## Overview

Lua 5.1.1 ships with 9 standard libraries (base, coroutine, string,
table, math, io, os, debug, package). The coroutine library is
registered as part of `luaopen_base()` but occupies its own namespace. Each library is a
collection of functions registered in a table (or as globals for the
base library). Libraries can be loaded selectively — an embedded Lua
environment may omit `io` and `os` for sandboxing.

## Libraries

### Base Library (`stdlib/base.rs`)

Global functions not in any table.

| Function | Status | Notes |
|----------|--------|-------|
| `assert` | Required | Error with optional message |
| `collectgarbage` | Required | 7 options |
| `dofile` | Required | Load and execute file |
| `error` | Required | Throw error object at level |
| `getfenv` | Required | Get function environment |
| `getmetatable` | Required | Get metatable (respects `__metatable`) |
| `ipairs` | Required | Integer key iterator |
| `load` | Required | Load chunk from function |
| `loadfile` | Required | Load chunk from file |
| `loadstring` | Required | Load chunk from string |
| `gcinfo` | Required | Deprecated GC info (returns KB used) |
| `next` | Required | Table traversal |
| `pairs` | Required | Generic table iterator |
| `pcall` | Required | Protected call |
| `print` | Required | Print to stdout (uses `tostring`) |
| `rawequal` | Required | Equality without metamethods |
| `rawget` | Required | Table access without metamethods |
| `rawset` | Required | Table assignment without metamethods |
| `_G` | Required | Global table reference |
| `select` | Required | `select(n, ...)` or `select('#', ...)` |
| `setfenv` | Required | Set function environment |
| `setmetatable` | Required | Set metatable (respects `__metatable`) |
| `tonumber` | Required | Convert to number (with base) |
| `tostring` | Required | Convert to string (uses `__tostring`) |
| `type` | Required | Type name as string |
| `unpack` | Required | Table to multiple values |
| `xpcall` | Required | Protected call with error handler |
| `_VERSION` | Required | `"Lua 5.1"` |
| `newproxy` | Optional | Undocumented, creates proxy userdata |

#### Base Library Behavioral Notes

**`assert(v [, message])`** — if `v` is nil or false, calls
`error(message)`. Default message is `"assertion failed!"`. Returns
all arguments on success.

**`error(message [, level])`** — level 0 means no position prefix.
Level 1 (default) prefixes with the current function's location.
Level 2 uses the caller's location, etc. If message is not a string,
no position prefix is added.

**`getfenv(f)`** — `f` can be a function or a number (stack level).
Level 0 returns the thread environment. Level 1 (default) returns
the current function's environment. `getfenv(0)` differs from
`getfenv()` (the latter defaults to level 1).

**`getmetatable(object)`** — if the metatable has a `__metatable`
field, returns that field's value instead of the actual metatable.
This protects metatables from user inspection.

**`ipairs(t)`** — returns an iterator function, the table, and 0.
The iterator returns `index, value` pairs starting at 1 until
`t[index]` is nil. Uses raw access (no metamethods).

**`pcall(f, ...)`** — calls `f(...)` in protected mode. Returns
`true, results...` on success or `false, error` on failure. The
error object can be any type, not just strings.

**`select(index, ...)`** — if index is `"#"`, returns the count of
remaining arguments. If index is negative, counts from the end.
Error: `"index out of range"` if the resulting position is < 1.

**`setfenv(f, table)`** — level 0 changes the thread environment
(returns nothing). Cannot change environments of C functions (error:
`"'setfenv' cannot change environment of given object"`).

**`setmetatable(table, metatable)`** — first arg must be a table
(not userdata). If the existing metatable has a `__metatable` field,
error: `"cannot change a protected metatable"`.

**`tonumber(e [, base])`** — base 10 uses `lua_isnumber` (handles
strings, hex `0xff`, whitespace). Other bases (2-36) use unsigned
integer conversion only. Returns nil on failure.

**`tostring(e)`** — checks `__tostring` metamethod first. Without
metamethod: numbers use `"%.14g"` format, booleans produce
`"true"`/`"false"`, nil produces `"nil"`, other types produce
`"typename: pointer"`.

**`unpack(list [, i [, j]])`** — `i` defaults to 1, `j` defaults to
`#list`. Returns `list[i]` through `list[j]` using raw access. Error:
`"table too big to unpack"` if the range exceeds stack space.

**`xpcall(f, err)`** — calls `f()` with zero arguments (extra args
are discarded). The error handler receives the original error object
and its return value becomes the error returned by `xpcall`.

### Coroutine Library (`stdlib/base.rs`)

| Function | Notes |
|----------|-------|
| `coroutine.create` | Create coroutine from function |
| `coroutine.resume` | Resume suspended coroutine |
| `coroutine.running` | Return running coroutine (returns nothing if main thread) |
| `coroutine.status` | Return status string (running/suspended/normal/dead) |
| `coroutine.wrap` | Create coroutine as iterator function |
| `coroutine.yield` | Suspend execution, return values to resume |

### String Library (`stdlib/string.rs`)

Registered as the `string` table and as the string metatable's
`__index`.

| Function | Notes |
|----------|-------|
| `string.byte` | Character codes |
| `string.char` | Characters from codes |
| `string.dump` | Dump function bytecode |
| `string.find` | Pattern matching search |
| `string.format` | Formatted string output |
| `string.gmatch` | Global pattern match iterator |
| `string.gsub` | Global pattern substitution |
| `string.len` | String length |
| `string.lower` | Lowercase conversion |
| `string.match` | Pattern match extraction |
| `string.rep` | String repetition |
| `string.reverse` | String reversal |
| `string.sub` | Substring extraction |
| `string.upper` | Uppercase conversion |
| `string.gfind` | Deprecated alias for gmatch (works by default; raises error only if `LUA_COMPAT_GFIND` is undefined) |

Lua 5.1 patterns are NOT regular expressions. They support character
classes (`%a`, `%d`, `%w`, etc.), anchors (`^`, `$`), quantifiers
(`*`, `+`, `-`, `?`), captures, and backreferences (`%1` through
`%9` to match a previous capture). They do not support alternation.

#### String Library Behavioral Notes

**`string.byte(s [, i [, j]])`** — `i` defaults to 1, `j` defaults
to `i`. Negative positions count from end. Returns one integer per
byte (0-255). Stack check: `"string slice too long"`.

**`string.char(...)`** — each argument must be 0-255 (error:
`"invalid value"`). No arguments returns empty string.

**`string.dump(function)`** — function must be a Lua function, not a
C function (error: `"unable to dump given function"`).

**`string.find(s, pattern [, init [, plain]])`** — `init` defaults
to 1 (negative counts from end). Plain mode does literal substring
search. Returns `start, end, captures...` on match, nil on failure.
Positions are 1-based.

**`string.format(formatstring, ...)`** — specifiers: `c d i o u x X
e E f g G q s` and `%%`. Flags: `- + (space) # 0`. Width/precision
max 2 digits each. `%q` produces a Lua-readable quoted string
(escapes `"`, `\`, newlines, `\r`, `\0`). `%s` with no precision and
string >= 100 chars pushes directly (no truncation).

**`string.gmatch(s, pattern)`** — returns an iterator. Each call
returns the next match's captures. Empty matches advance by 1
character to prevent infinite loops.

**`string.gsub(s, pattern, repl [, n])`** — replacement can be
string (`%0`=match, `%1`-`%9`=captures, `%%`=literal %), function
(called with captures; falsy return keeps original), or table
(first capture as key; falsy result keeps original). Returns the
result string and substitution count.

**`string.sub(s, i [, j])`** — `j` defaults to -1 (end of string).
Negative positions count from end. Returns empty string if
`start > end`.

#### Pattern Language Specification

**Character classes** (from `match_class` in `lstrlib.c`):

| Class | Matches | Negation |
|-------|---------|----------|
| `%a` | letters (`isalpha`) | `%A` |
| `%c` | control characters (`iscntrl`) | `%C` |
| `%d` | digits (`isdigit`) | `%D` |
| `%l` | lowercase letters (`islower`) | `%L` |
| `%p` | punctuation (`ispunct`) | `%P` |
| `%s` | whitespace (`isspace`) | `%S` |
| `%u` | uppercase letters (`isupper`) | `%U` |
| `%w` | alphanumeric (`isalnum`) | `%W` |
| `%x` | hex digits (`isxdigit`) | `%X` |
| `%z` | the null byte (`\0`) | `%Z` |
| `%.` | literal `.` (any `%` + non-letter = literal) | — |

**Bracket classes**: `[abc]` matches any of a, b, c. `[^abc]`
negated. `[a-z]` ranges. `%` classes work inside brackets.

**Single character matchers**: `.` matches any character. `%x`
matches a class. `[...]` matches a bracket class. Anything else
matches literally.

**Quantifiers**:

| Quantifier | Meaning | Strategy |
|------------|---------|----------|
| `*` | 0 or more | Greedy (max first, backtrack) |
| `+` | 1 or more | Greedy |
| `-` | 0 or more | Lazy (min first, extend) |
| `?` | 0 or 1 | Greedy |

**Anchors**: `^` at pattern start anchors to beginning. `$` at
pattern end anchors to end. Elsewhere they are literal.

**Captures**: `(...)` captures matched text. `()` captures the
position (1-based integer) instead of text. Maximum 32 captures
(`LUA_MAXCAPTURES`). Backreferences: `%1` through `%9` match the
same text as the corresponding capture.

**Special patterns**: `%bxy` matches balanced delimiters (e.g.,
`%b()` matches balanced parentheses). `%f[set]` is a frontier
pattern — matches a position where the previous character does not
match `[set]` and the current character does. At string start, the
"previous character" is `\0`.

**Error conditions**: `"malformed pattern (ends with '%%')"`,
`"malformed pattern (missing ']')"`, `"invalid capture index"`,
`"unfinished capture"`, `"invalid pattern capture"`,
`"too many captures"`, `"missing '[' after '%%f' in pattern"`.

### Table Library (`stdlib/table.rs`)

| Function | Notes |
|----------|-------|
| `table.concat` | Concatenate array elements |
| `table.insert` | Insert element at position |
| `table.maxn` | Maximum positive numeric key |
| `table.remove` | Remove element at position |
| `table.sort` | In-place sort |
| `table.foreach` | Deprecated: iterate table (use pairs) |
| `table.foreachi` | Deprecated: iterate array (use ipairs) |
| `table.getn` | Deprecated: table length (use # operator) |
| `table.setn` | Deprecated: raises error in 5.1.1 |

#### Table Library Behavioral Notes

**`table.concat(list [, sep [, i [, j]]])`** — sep defaults to `""`,
i defaults to 1, j defaults to `#list`. Each element must be a
string or number (error: `"table contains non-strings"`). Uses raw
access. Returns empty string if `i > j`.

**`table.insert(list, [pos,] value)`** — 2 args appends at end. 3
args inserts at pos, shifting elements up. Error: `"wrong number of
arguments to 'insert'"` for other counts.

**`table.maxn(list)`** — scans ALL keys (both parts) via `next()`.
Returns the largest positive numeric key (including non-integer keys
like 1.5). Returns 0 if no positive numeric keys exist.

**`table.remove(list [, pos])`** — pos defaults to `#list` (remove
from end). Shifts elements down, sets last element to nil. Returns
the removed element, or nothing if the table was empty.

**`table.sort(list [, comp])`** — Quicksort with median-of-three
pivot. Tail recursion on the larger partition. Default comparison uses
`<` (invokes `__lt` metamethods). Error: `"invalid order function for
sorting"` if the comparison is inconsistent (e.g., NaN values break
strict weak ordering). Not stable.

**`table.setn(table, n)`** — error: `"'setn' is obsolete"`.

### Math Library (`stdlib/math.rs`)

| Function | Notes |
|----------|-------|
| `math.abs` | Absolute value |
| `math.acos` | Arc cosine |
| `math.asin` | Arc sine |
| `math.atan` | Arc tangent |
| `math.atan2` | Two-argument arc tangent |
| `math.ceil` | Ceiling |
| `math.cos` | Cosine |
| `math.cosh` | Hyperbolic cosine |
| `math.deg` | Radians to degrees |
| `math.exp` | Exponential |
| `math.floor` | Floor |
| `math.fmod` | Float modulo |
| `math.frexp` | Decompose float |
| `math.huge` | Infinity constant |
| `math.ldexp` | Scale by power of 2 |
| `math.log` | Natural logarithm |
| `math.log10` | Base-10 logarithm |
| `math.max` | Maximum |
| `math.min` | Minimum |
| `math.mod` | Deprecated alias for fmod (enabled by default via `LUA_COMPAT_MOD`) |
| `math.modf` | Integer and fractional parts |
| `math.pi` | Pi constant |
| `math.pow` | Power |
| `math.rad` | Degrees to radians |
| `math.random` | Random number |
| `math.randomseed` | Set random seed |
| `math.sin` | Sine |
| `math.sinh` | Hyperbolic sine |
| `math.sqrt` | Square root |
| `math.tan` | Tangent |
| `math.tanh` | Hyperbolic tangent |

#### Math Library Behavioral Notes

All single-argument functions use `f64` methods from Rust's standard
library. Each takes one number argument and returns one number.

**`math.random([m [, n]])`** — 0 args: float in `[0, 1)`. 1 arg:
integer in `[1, m]`. 2 args: integer in `[m, n]`. Error: `"interval
is empty"` if the range is invalid. Uses C `rand()` equivalent
(deterministic for a given seed).

**`math.randomseed(x)`** — seeds the random generator. Argument
must be convertible to integer.

**`math.min(...)` / `math.max(...)`** — requires at least 1 argument.
NaN asymmetry: if NaN is the first argument, it is returned. If NaN
appears later, it is skipped (since `NaN < x` and `NaN > x` are
both false).

**`math.frexp(x)`** — returns 2 values: mantissa `m` and integer
exponent `e` where `x = m * 2^e` and `0.5 <= |m| < 1`.

**`math.modf(x)`** — returns 2 values: integer part and fractional
part.

**`math.log(x)`** — natural logarithm only (no base parameter in
5.1). Base-10 is `math.log10`.

**Lua `%` vs `math.fmod`**: the Lua `%` operator uses
`a - floor(a/b)*b` (result has same sign as `b`), while `math.fmod`
uses C `fmod` (result has same sign as `a`). Example: `-1 % 5` is
`4` in Lua but `math.fmod(-1, 5)` is `-1`.

### I/O Library (`stdlib/io.rs`)

| Function | Notes |
|----------|-------|
| `io.close` | Close file |
| `io.flush` | Flush output |
| `io.input` | Set/get default input |
| `io.lines` | Line iterator |
| `io.open` | Open file |
| `io.output` | Set/get default output |
| `io.popen` | Open process (platform-dependent) |
| `io.read` | Read from default input |
| `io.tmpfile` | Create temporary file |
| `io.type` | Check file handle type |
| `io.write` | Write to default output |
| File methods | `:close`, `:flush`, `:lines`, `:read`, `:seek`, `:setvbuf`, `:write` |
| `io.stdin` | Standard input file handle |
| `io.stdout` | Standard output file handle |
| `io.stderr` | Standard error file handle |

### OS Library (`stdlib/os.rs`)

| Function | Notes |
|----------|-------|
| `os.clock` | CPU time |
| `os.date` | Date formatting |
| `os.difftime` | Time difference |
| `os.execute` | Run shell command |
| `os.exit` | Exit process |
| `os.getenv` | Environment variable |
| `os.remove` | Delete file |
| `os.rename` | Rename file |
| `os.setlocale` | Set locale |
| `os.time` | Current time |
| `os.tmpname` | Temporary file name |

### Debug Library (`stdlib/debug.rs`)

| Function | Notes |
|----------|-------|
| `debug.debug` | Interactive debug prompt |
| `debug.getfenv` | Get environment |
| `debug.gethook` | Get hook function |
| `debug.getinfo` | Function information |
| `debug.getlocal` | Local variable value |
| `debug.getmetatable` | Raw metatable |
| `debug.getregistry` | Registry table |
| `debug.getupvalue` | Upvalue value |
| `debug.setfenv` | Set environment |
| `debug.sethook` | Set hook function |
| `debug.setlocal` | Set local variable |
| `debug.setmetatable` | Set metatable |
| `debug.setupvalue` | Set upvalue |
| `debug.traceback` | Stack traceback |

### Package Library (`stdlib/package.rs`)

| Function/Field | Notes |
|----------------|-------|
| `require` | Module loader (registered as global) |
| `module` | Create module (registered as global) |
| `package.config` | Directory/path separator configuration string |
| `package.cpath` | C module search path |
| `package.loaded` | Cache of loaded modules |
| `package.loaders` | Ordered list of module searchers |
| `package.loadlib` | Load native module (see [Native Module Loading](#native-module-loading)) |
| `package.path` | Lua module search path |
| `package.preload` | Pre-registered module loaders |
| `package.seeall` | Set module environment to globals |

#### Native Module Loading

PUC-Rio Lua's `package.loadlib` loads C modules via `lua_CFunction`
(`int (*)(lua_State *)`). rilua cannot load PUC-Rio C modules because:

- Rust has no stable ABI. The internal types (`LuaState`, `Val`) change
  layout between compiler versions.
- rilua's function signature (`fn(&mut LuaState) -> LuaResult<u32>`)
  differs from PUC-Rio's `extern "C" fn(*mut lua_State) -> c_int`.
- Building a C API compatibility shim (`lua.h`-compatible) would require
  reimplementing the entire PUC-Rio stack API (~120 functions) with
  `extern "C"` wrappers, plus maintaining ABI stability guarantees that
  Rust does not provide.

Instead, rilua defines its own native module ABI. Modules are Rust
`cdylib` crates compiled against the same rilua version and `rustc`
version as the host. This is gated behind the `dynmod` Cargo feature
(default off). Without the feature, `package.loadlib` returns
`(nil, msg, "absent")`.

When `dynmod` is enabled:

- `package.loadlib(path, funcname)` loads a shared library, validates
  a `RILUA_MODULE_INFO` descriptor (magic bytes, version, struct sizes),
  and looks up the named entry point.
- The C module loaders (`package.loaders[3]` and `[4]`) search
  `package.cpath` for modules named `rilua_open_<modname>`.
- Library handles are stored as userdata with a `__gc` metamethod that
  calls `dlclose`/`FreeLibrary` on collection.

See `src/dynmod.rs` for the ABI contract and `examples/native_module/`
for a working example.

## Loading

Libraries are loaded via `Lua::new()` (all standard libraries) or
selectively via `Lua::new_with(StdLib)`.

Mirrored at [`examples/selective_stdlib.rs`](../../examples/selective_stdlib.rs).

```rust
use rilua::{Lua, StdLib};

fn main() -> rilua::LuaResult<()> {
    let mut lua = Lua::new_with(
        StdLib::BASE | StdLib::STRING | StdLib::TABLE | StdLib::MATH,
    )?;
    // io, os, debug, package omitted (sandboxed)
    lua.exec(r#"print(string.upper("ok"))"#)?;
    Ok(())
}
```

## Error Message Formats

All `luaL_error` messages are prefixed by source location
(`"source:line: "` or empty string).

**Argument errors**: `"bad argument #N to 'funcname' (message)"`.
For methods, narg is decremented by 1 (implicit self). If narg
becomes 0: `"calling 'name' on bad self (message)"`.

**Type errors**: `"bad argument #N to 'funcname' (expected expected,
got actual)"`.

**Key format constants**:

| Constant | Value |
|----------|-------|
| `LUA_MAXCAPTURES` | 32 |
| `LUA_NUMBER_FMT` | `"%.14g"` |

## Implementation Priority

1. **Base library** (with coroutine) — required for any Lua program
2. **String library** — heavily used, pattern matching is complex
3. **Table library** — common operations
4. **Math library** — straightforward wrappers around `f64` methods
5. **I/O library** — file operations
6. **OS library** — system operations
7. **Package library** — module system
8. **Debug library** — introspection, lowest priority
