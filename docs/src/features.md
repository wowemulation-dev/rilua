# Lua Feature Matrix (5.1 -- 5.5)

Cross-version feature reference for the Lua programming language. The
table lists the version that adds each feature. It also lists the
version that deprecates or removes each feature.

rilua targets **Lua 5.1.1**. The World of Warcraft game client uses Lua
5.1.1. We document features from later versions for reference.

## Legend

Version columns:

- ✓ -- Available (standard, documented in reference manual)
- D -- Deprecated (per reference manual or available only via compatibility flags)
- (empty) -- Not present (not yet introduced, or removed)

rilua column:

- ✓ -- Fully implemented
- ~ -- Partially implemented (known limitations)
- ✗ -- Not implemented

---

## 1. Types and Values

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| nil | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| boolean | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Only `false` and `nil` are falsy |
| number (float only) | ✓ | ✓ | | | | ✓ | All numbers are f64 in 5.1--5.2 |
| number (integer subtype) | | | ✓ | ✓ | ✓ | ✗ | 64-bit integers + 64-bit floats |
| string | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Immutable, 8-bit clean byte sequences |
| function | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | First-class values |
| table | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Associative arrays |
| userdata (full) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Arbitrary host data with metatables |
| userdata (light) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Raw pointer, no metatable |
| thread | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Coroutine execution context |
| Reference semantics | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Tables, functions, threads, userdata |
| Automatic string-number coercion | ✓ | ✓ | ✓ | D | D | ✓ | Restricted in 5.4+ (moved to string metamethods) |

---

## 2. Lexical Elements

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| 21 reserved keywords | ✓ | | | | | ✓ | `and break do else elseif end false for function if in local nil not or repeat return then true until while` |
| 22 reserved keywords | | ✓ | ✓ | ✓ | | ✗ | 5.2+ adds `goto` to the 5.1 set |
| 23 reserved keywords | | | | | ✓ | ✗ | 5.5 adds `global`; effectively 22 when `LUA_COMPAT_GLOBAL` is on (default) |
| `global` keyword | | | | | ✓ | ✗ | Conditionally reserved; unreserved by default via `LUA_COMPAT_GLOBAL` |
| Short strings (`"..."` / `'...'`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Long strings (`[[...]]`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | With levels: `[=[...]=]` |
| Escape: `\a \b \f \n \r \t \v \\ \" \'` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Escape: `\ddd` (decimal byte) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Escape: `\xHH` (hex byte) | | ✓ | ✓ | ✓ | ✓ | ✗ | 5.2 feature |
| Escape: `\z` (skip whitespace) | | ✓ | ✓ | ✓ | ✓ | ✗ | 5.2 feature |
| Escape: `\u{XXXX}` (Unicode) | | | ✓ | ✓ | ✓ | ✗ | UTF-8 encoding of codepoint |
| Decimal numeric literals | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `42`, `3.14`, `1e10` |
| Hexadecimal integer literals | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `0xff` |
| Hexadecimal float literals | | ✓ | ✓ | ✓ | ✓ | ✗ | `0x1.Bp10` |
| Line comments (`--`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Block comments (`--[[...]]`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Shebang line (`#!`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | First line only |
| Empty statement (`;`) | | ✓ | ✓ | ✓ | ✓ | ✓ | 5.1 allows `;` as separator only, not as statement |

---

## 3. Variables and Scope

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| Global variables | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Stored in environment table |
| Local variables | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Lexically scoped |
| Upvalues (closures) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Inner functions capture outer locals |
| Function environments (`setfenv`/`getfenv`) | ✓ | | | | | ✓ | Replaced by `_ENV` in 5.2 |
| `_ENV` variable | | ✓ | ✓ | ✓ | ✓ | ✗ | Free names translated to `_ENV.var` |
| `_G` global table | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `local x <const>` | | | | ✓ | ✓ | ✗ | Compile-time constant local |
| `local x <close>` | | | | ✓ | ✓ | ✗ | Calls `__close` on scope exit |
| `global` declarations | | | | | ✓ | ✗ | `global x`, `global *`, `global x <const>` |
| Named vararg tables | | | | | ✓ | ✗ | Named access to varargs |

---

## 4. Statements and Control Flow

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `do ... end` blocks | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Explicit scope |
| Assignment (single and multiple) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `x, y = y, x` |
| `if ... elseif ... else ... end` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `while ... do ... end` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Pre-test loop |
| `repeat ... until` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Post-test loop; body visible to condition |
| Numeric `for` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `for i = start, limit, step do` |
| Generic `for` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `for k, v in iterator do` |
| Generic `for` closing value | | | | ✓ | ✓ | ✗ | 4th variable: to-be-closed |
| Read-only `for` loop variables | | | | | ✓ | ✗ | Control variable in generic `for` is read-only |
| `break` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Must be last statement in 5.1 |
| `break` anywhere in block | | ✓ | ✓ | ✓ | ✓ | ✗ | 5.1 requires `do break end` workaround |
| `goto` and `::label::` | | ✓ | ✓ | ✓ | ✓ | ✗ | Cannot jump into or across local scope |
| `return` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | With optional expression list |
| Function calls as statements | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |

---

## 5. Expressions and Operators

### 5.1 Arithmetic

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `+` `-` `*` `/` `%` `^` (unary `-`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Floor division `//` | | | ✓ | ✓ | ✓ | ✗ | Rounds quotient toward negative infinity |

### 5.2 Bitwise

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `&` (AND) | | | ✓ | ✓ | ✓ | ✗ | Language-level operator (not bit32 library) |
| `\|` (OR) | | | ✓ | ✓ | ✓ | ✗ | |
| `~` (XOR / unary NOT) | | | ✓ | ✓ | ✓ | ✗ | Binary XOR and unary NOT |
| `<<` `>>` (shifts) | | | ✓ | ✓ | ✓ | ✗ | |

### 5.3 Relational

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `==` `~=` `<` `>` `<=` `>=` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |

### 5.4 Logical

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `and` `or` `not` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Short-circuit evaluation |

### 5.5 Other

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `..` (concatenation) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `#` (length) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Table constructors `{}` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Array, record, mixed |
| Parenthesized expressions | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Truncate multi-return to single value |

---

## 6. Functions

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| Named definition | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `function f() end` |
| Local definition | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `local function f() end` |
| Anonymous (lambda) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `function() end` |
| Method definition (`:`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Implicit `self` parameter |
| Dotted names | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `function a.b.c() end` |
| No-parenthesis call | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Single string or table arg |
| Method call (`:`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `obj:method(args)` |
| Multiple return values | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Varargs (`...`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Proper tail calls | ✓ | ✓ | ✓ | ✓ | ✓ | ~ | Known bug with return-from-C |
| Closures (upvalues) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| C/Rust functions | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Native host functions |
| Light C functions | | ✓ | ✓ | ✓ | ✓ | ✗ | No allocation overhead |

---

## 7. Metatables and Metamethods

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `getmetatable` / `setmetatable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `__index` (read) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Table or function |
| `__newindex` (write) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Table or function |
| `__call` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Call non-function values |
| `__add` `__sub` `__mul` `__div` `__mod` `__pow` `__unm` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Arithmetic metamethods |
| `__idiv` | | | ✓ | ✓ | ✓ | ✗ | Floor division metamethod |
| `__band` `__bor` `__bxor` `__bnot` `__shl` `__shr` | | | ✓ | ✓ | ✓ | ✗ | Bitwise metamethods |
| `__eq` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.1-5.2: same type and same metamethod required; 5.3+: checks each operand's metatable independently |
| `__lt` `__le` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `__concat` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `__len` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ honors `__len` for tables |
| `__tostring` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `__metatable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Protect metatable from access |
| `__mode` (weak tables) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `"k"`, `"v"`, or `"kv"` |
| `__gc` (userdata finalizer) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `__gc` (table finalizer) | | ✓ | ✓ | ✓ | ✓ | ✗ | Extended to tables in 5.2 |
| `__pairs` | | ✓ | ✓ | ✓ | ✓ | ✗ | Added in 5.2; present in source and manual index through 5.5 |
| `__ipairs` | | ✓ | D | | | ✗ | Added in 5.2; deprecated in 5.3 (`LUA_COMPAT_IPAIRS`), removed in 5.4 |
| `__close` | | | | ✓ | ✓ | ✗ | For to-be-closed variables |

---

## 8. Garbage Collection

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| Incremental mark-sweep | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Tri-color with write barriers |
| Generational mode (experimental) | | ✓ | | | | ✗ | Removed in 5.3 as experimental |
| Generational mode (stable) | | | | ✓ | ✓ | ✗ | Re-introduced in 5.4 |
| Incremental major collections | | | | | ✓ | ✗ | Major GC done incrementally in 5.5 |
| `collectgarbage("collect")` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `collectgarbage("stop"/"restart")` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `collectgarbage("count")` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2 returns 2 values (kbytes, remainder); 5.1 and 5.3+ return single value |
| `collectgarbage("step")` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `collectgarbage("setpause"/"setstepmul")` | ✓ | ✓ | ✓ | ✓ | | ✓ | Replaced by `collectgarbage("param")` in 5.5 |
| `collectgarbage("param")` | | | | | ✓ | ✗ | Unified parameter access: `("param", name [, value])` |
| `collectgarbage("isrunning")` | | ✓ | ✓ | ✓ | ✓ | ✗ | |
| `collectgarbage("incremental")` | | ✓ | | ✓ | ✓ | ✗ | Switch to incremental mode; 5.4+ accepts parameters |
| `collectgarbage("generational")` | | ✓ | | ✓ | ✓ | ✗ | Switch to generational mode; 5.4+ accepts parameters |
| Weak tables (`__mode`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Ephemeron tables | | ✓ | ✓ | ✓ | ✓ | ✗ | Weak keys with value dependency |
| Emergency GC | | ✓ | ✓ | ✓ | ✓ | ✗ | Runs GC on allocation failure |
| Userdata finalizers | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Via `__gc` metamethod |
| Table finalizers | | ✓ | ✓ | ✓ | ✓ | ✗ | Extended `__gc` to tables |
| `gcinfo()` | D | | | | | ✓ | Deprecated in 5.1; removed in 5.2 |

---

## 9. Coroutines

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `coroutine.create` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `coroutine.resume` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `coroutine.yield` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `coroutine.wrap` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Returns iterator function |
| `coroutine.status` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `coroutine.running` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ returns 2 values (thread, is-main) |
| `coroutine.isyieldable` | | | ✓ | ✓ | ✓ | ✗ | |
| `coroutine.close` | | | | ✓ | ✓ | ✗ | Close coroutine and its to-be-closed vars |
| Yieldable `pcall`/`xpcall` | | ✓ | ✓ | ✓ | ✓ | ✗ | Coroutines can yield across protected calls |
| Yieldable metamethods | | ✓ | ✓ | ✓ | ✓ | ✗ | |

---

## 10. Error Handling

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `error(msg [, level])` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `pcall(f, ...)` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `xpcall(f, msgh)` | ✓ | | | | | ✓ | 5.1 signature: 2 args only |
| `xpcall(f, msgh, ...)` | | ✓ | ✓ | ✓ | ✓ | ✗ | 5.2+ passes extra args to `f` |
| Error objects (any value) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `warn(msg)` | | | | ✓ | ✓ | ✗ | Non-fatal warning system |

---

## 11. Standard Library: Base Functions

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `assert` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `collectgarbage` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Options vary by version |
| `dofile` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `error` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `getmetatable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `ipairs` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.3+ uses `lua_geti` (respects `__index`); 5.2 `__ipairs` metamethod separate |
| `load` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ accepts string and mode/env args |
| `loadfile` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ accepts mode/env args |
| `next` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `pairs` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `pcall` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `print` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.4+ calls `__tostring` directly |
| `rawequal` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `rawget` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `rawset` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `rawlen` | | ✓ | ✓ | ✓ | ✓ | ✗ | Length without `__len` |
| `select` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `setmetatable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `tonumber` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `tostring` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `type` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `xpcall` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Signature changed in 5.2 (see sec. 10) |
| `_G` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `_VERSION` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `"Lua 5.x"` |
| `warn` | | | | ✓ | ✓ | ✗ | Non-fatal warnings |
| `unpack` (global) | ✓ | D | D | | | ✓ | Moved to `table.unpack` in 5.2; 5.3 via `LUA_COMPAT_UNPACK` |
| `loadstring` | ✓ | D | D | | | ✓ | Use `load` in 5.2+; 5.3 via `LUA_COMPAT_LOADSTRING` |
| `getfenv` | ✓ | | | | | ✓ | Removed in 5.2 (replaced by `_ENV`) |
| `setfenv` | ✓ | | | | | ✓ | Removed in 5.2 (replaced by `_ENV`) |
| `module` | ✓ | D | D | | | ✓ | Deprecated in 5.2; 5.3 via `LUA_COMPAT_MODULE` |
| `newproxy` | ✓ | | | | | ✓ | Undocumented in 5.1; removed in 5.2 |
| `gcinfo` | D | | | | | ✓ | Deprecated since 5.0; removed in 5.2 |

---

## 12. Standard Library: String

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `string.byte` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.char` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.dump` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.3+ adds `strip` parameter |
| `string.find` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.format` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+: `%s` calls `__tostring` via `luaL_tolstring`; 5.4+: `%p` specifier |
| `string.gmatch` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.4+ adds optional `init` parameter |
| `string.gsub` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.len` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.lower` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.match` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.rep` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ adds optional `sep` parameter |
| `string.reverse` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.sub` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.upper` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `string.pack` | | | ✓ | ✓ | ✓ | ✗ | Binary data packing |
| `string.unpack` | | | ✓ | ✓ | ✓ | ✗ | Binary data unpacking |
| `string.packsize` | | | ✓ | ✓ | ✓ | ✗ | Packed size calculation |
| `string.gfind` | D | | | | | ✓ | Deprecated alias for `gmatch`; removed in 5.2 |
| String metatable (`__index`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Method syntax: `s:upper()` |
| Frontier pattern (`%f`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Implemented in 5.1 (undocumented); documented in 5.2 |
| Pattern class `%g` | | ✓ | ✓ | ✓ | ✓ | ✗ | Printable characters (except space) |
| `\0` in patterns | | ✓ | ✓ | ✓ | ✓ | ✗ | Null bytes in patterns |

---

## 13. Standard Library: Table

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `table.concat` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `table.insert` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ stricter argument checking |
| `table.remove` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ stricter argument checking |
| `table.sort` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `table.unpack` | | ✓ | ✓ | ✓ | ✓ | ✗ | Replaces global `unpack` from 5.1 |
| `table.pack` | | ✓ | ✓ | ✓ | ✓ | ✗ | Returns table with `.n` field |
| `table.move` | | | ✓ | ✓ | ✓ | ✗ | Move elements between positions/tables |
| `table.create` | | | | | ✓ | ✗ | Pre-allocate table with size hints |
| `table.maxn` | ✓ | D | D | | | ✓ | Largest positive numeric key; 5.3 via `LUA_COMPAT_MAXN` |
| `table.foreach` | D | | | | | ✓ | Deprecated since 5.0 |
| `table.foreachi` | D | | | | | ✓ | Deprecated since 5.0 |
| `table.getn` | D | | | | | ✓ | Use `#` operator instead |
| `table.setn` | D | | | | | ✓ | No replacement; removed in 5.2 |

---

## 14. Standard Library: Math

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `math.abs` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.acos` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.asin` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.atan` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.3+ accepts 2 args (replaces `atan2`) |
| `math.ceil` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.cos` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.deg` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.exp` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.floor` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.fmod` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.log` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ adds optional `base` parameter |
| `math.max` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.min` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.modf` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.rad` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.random` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.4 switches to xoshiro256** RNG |
| `math.randomseed` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.sin` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.sqrt` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.tan` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.huge` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Positive infinity |
| `math.pi` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `math.maxinteger` | | | ✓ | ✓ | ✓ | ✗ | Largest integer value |
| `math.mininteger` | | | ✓ | ✓ | ✓ | ✗ | Smallest integer value |
| `math.tointeger` | | | ✓ | ✓ | ✓ | ✗ | Convert float to integer if fits |
| `math.type` | | | ✓ | ✓ | ✓ | ✗ | `"integer"`, `"float"`, or `false` |
| `math.ult` | | | ✓ | ✓ | ✓ | ✗ | Unsigned integer less-than comparison |
| `math.atan2` | ✓ | ✓ | D | D | D | ✓ | Use `math.atan(y, x)` in 5.3+; 5.3+ via `LUA_COMPAT_MATHLIB` |
| `math.cosh` | ✓ | ✓ | D | D | D | ✓ | 5.3+ via `LUA_COMPAT_MATHLIB` |
| `math.sinh` | ✓ | ✓ | D | D | D | ✓ | 5.3+ via `LUA_COMPAT_MATHLIB` |
| `math.tanh` | ✓ | ✓ | D | D | D | ✓ | 5.3+ via `LUA_COMPAT_MATHLIB` |
| `math.pow` | ✓ | ✓ | D | D | D | ✓ | Use `x ^ y` operator; 5.3+ via `LUA_COMPAT_MATHLIB` |
| `math.frexp` | ✓ | ✓ | D | D | ✓ | ✓ | Restored as standard in 5.5; 5.3-5.4 via `LUA_COMPAT_MATHLIB` |
| `math.ldexp` | ✓ | ✓ | D | D | ✓ | ✓ | Restored as standard in 5.5; 5.3-5.4 via `LUA_COMPAT_MATHLIB` |
| `math.log10` | ✓ | D | D | D | D | ✓ | Use `math.log(x, 10)`; 5.3+ via `LUA_COMPAT_MATHLIB` |
| `math.mod` | D | | | | | ✓ | Alias for `fmod`; deprecated since 5.0 |

---

## 15. Standard Library: I/O

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `io.close` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.flush` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.input` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.lines` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ accepts read format options |
| `io.open` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.output` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.popen` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.read` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ adds `"*L"` format (line with newline) |
| `io.tmpfile` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.type` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.write` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `io.stdin` / `io.stdout` / `io.stderr` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `file:close` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+: pipe close returns exit status |
| `file:flush` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `file:lines` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ accepts format options |
| `file:read` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `file:seek` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `file:setvbuf` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `file:write` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+: returns file handle (for chaining) |

---

## 16. Standard Library: OS

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `os.clock` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.date` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.difftime` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.execute` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ returns `true`/`nil`, reason, code |
| `os.exit` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ adds optional `close` parameter |
| `os.getenv` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.remove` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.rename` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.setlocale` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.time` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `os.tmpname` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |

---

## 17. Standard Library: Debug

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `debug.debug` | ✓ | ✓ | ✓ | ✓ | ✓ | ~ | Stub |
| `debug.gethook` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Returns hook function, mask string, count |
| `debug.getinfo` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ adds `nparams`, `isvararg`, `istailcall` |
| `debug.getlocal` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | 5.2+ accesses vararg info |
| `debug.getmetatable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.getregistry` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.getupvalue` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.sethook` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Line, call, return, and count hooks |
| `debug.setlocal` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.setmetatable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.setupvalue` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.traceback` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `debug.upvalueid` | | ✓ | ✓ | ✓ | ✓ | ✗ | Unique ID for upvalue |
| `debug.upvaluejoin` | | ✓ | ✓ | ✓ | ✓ | ✗ | Make upvalues share |
| `debug.getuservalue` | | ✓ | ✓ | ✓ | ✓ | ✗ | 5.4+ supports multiple user values |
| `debug.setuservalue` | | ✓ | ✓ | ✓ | ✓ | ✗ | 5.4+ supports multiple user values |
| `debug.setcstacklimit` | | | | ✓ | | ✗ | Added in 5.4; removed in 5.5 |
| `debug.getfenv` | ✓ | | | | | ✓ | Removed with environment model in 5.2 |
| `debug.setfenv` | ✓ | | | | | ✓ | Removed with environment model in 5.2 |

---

## 18. Standard Library: Package

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `require` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `package.config` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `package.cpath` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `package.loaded` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `package.loadlib` | ✓ | ✓ | ✓ | ✓ | ✓ | ~ | Loads rilua-native modules (not PUC-Rio C modules); requires `dynmod` feature |
| `package.path` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `package.preload` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| `package.searchpath` | | ✓ | ✓ | ✓ | ✓ | ✗ | |
| `package.searchers` | | ✓ | ✓ | ✓ | ✓ | ✗ | Replaces `package.loaders` |
| `package.loaders` | ✓ | D | D | | | ✓ | Renamed to `package.searchers` in 5.2; 5.3 via `LUA_COMPAT_LOADERS` |
| `package.seeall` | ✓ | D | D | | | ✓ | Deprecated in 5.2; 5.3 via `LUA_COMPAT_MODULE` |
| `module` | ✓ | D | D | | | ✓ | Deprecated in 5.2; 5.3 via `LUA_COMPAT_MODULE` |

---

## 19. Standard Library: bit32

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `bit32.arshift` | | ✓ | D | | | ✗ | Arithmetic right shift |
| `bit32.band` | | ✓ | D | | | ✗ | Bitwise AND |
| `bit32.bnot` | | ✓ | D | | | ✗ | Bitwise NOT |
| `bit32.bor` | | ✓ | D | | | ✗ | Bitwise OR |
| `bit32.btest` | | ✓ | D | | | ✗ | Test bits |
| `bit32.bxor` | | ✓ | D | | | ✗ | Bitwise XOR |
| `bit32.extract` | | ✓ | D | | | ✗ | Extract bits |
| `bit32.replace` | | ✓ | D | | | ✗ | Replace bits |
| `bit32.lrotate` | | ✓ | D | | | ✗ | Left rotate |
| `bit32.lshift` | | ✓ | D | | | ✗ | Left shift |
| `bit32.rrotate` | | ✓ | D | | | ✗ | Right rotate |
| `bit32.rshift` | | ✓ | D | | | ✗ | Right shift |

---

## 20. Standard Library: UTF-8

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `utf8.char` | | | ✓ | ✓ | ✓ | ✗ | Create UTF-8 string from codepoints |
| `utf8.charpattern` | | | ✓ | ✓ | ✓ | ✗ | Pattern matching one UTF-8 character |
| `utf8.codes` | | | ✓ | ✓ | ✓ | ✗ | Iterator over codepoints |
| `utf8.codepoint` | | | ✓ | ✓ | ✓ | ✗ | Get codepoints from string |
| `utf8.len` | | | ✓ | ✓ | ✓ | ✗ | Count UTF-8 characters |
| `utf8.offset` | | | ✓ | ✓ | ✓ | ✗ | 5.5: also returns final position |

---

## 21. rilua: Interpreter CLI

Reproduces the PUC-Rio Lua 5.1.1 standalone interpreter (`lua.c`).

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `rilua [options] [script [args]]` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Option `-e stat` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Execute string |
| Option `-i` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Interactive mode after script |
| Option `-l name` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Require library |
| Option `-v` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Version info |
| Option `--` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Stop option handling |
| Option `-` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Execute stdin |
| Option `-E` | | ✓ | ✓ | ✓ | ✓ | ✗ | Ignore environment variables |
| Option `-W` | | | | ✓ | ✓ | ✗ | Turn on warnings |
| `LUA_INIT` env var | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Execute string or `@filename` |
| `arg` table | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `arg[0]` is script name |
| REPL / interactive mode | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | `_PROMPT` / `_PROMPT2` globals |
| REPL `=expr` shorthand | ✓ | ✓ | ✓ | ✓ | | ✓ | Calculator mode added in 5.3 alongside; `=expr` removed in 5.5 |

---

## 22. rilua: Bytecode Compiler CLI

Reproduces the PUC-Rio `luac` bytecode compiler/lister.

| Feature | 5.1 | 5.2 | 5.3 | 5.4 | 5.5 | rilua | Notes |
|---------|:---:|:---:|:---:|:---:|:---:|:-----:|-------|
| `riluac [options] [files]` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | |
| Option `-l` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | List bytecode |
| Option `-l -l` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | List with constants and locals |
| Option `-p` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Parse only (syntax check) |
| Option `-o file` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Write binary output |
| Option `-s` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Strip debug info |
| Option `-v` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | Version info |

---

## 23. rilua: Rust Embedding API

These features are rilua-specific (not part of the Lua language standard)
but correspond to the C API provided by PUC-Rio Lua.

| Feature | rilua | Notes |
|---------|:-----:|-------|
| `Lua` struct (state ownership) | ✓ | Main VM state |
| `IntoLua` / `FromLua` traits | ✓ | Type-safe value conversion |
| `Table` handle | ✓ | Read/write table fields |
| `Function` handle | ✓ | Call Lua functions from Rust |
| `Thread` handle | ✓ | Coroutine manipulation |
| `AnyUserData` handle | ✓ | Typed Rust data in Lua |
| `StdLib` bitflags | ✓ | Selective library loading |
| `LuaResult<T>` error handling | ✓ | Result-based (no longjmp) |
| `Lua::load()` / `Lua::load_bytes()` | ✓ | Load and compile chunks |
| `Lua::gc_collect()` etc. | ✓ | GC control from Rust |
| Registry table | ✓ | Global storage for host |

---

## Version Evolution Summary

### Lua 5.1 (2006) -- rilua target

The baseline. All numbers are f64. Function environments via
`setfenv`/`getfenv`. Module system via `module()` and `package.loaders`.
Pattern matching (not regex). Incremental GC. Coroutines. 21 keywords.

### Lua 5.2 (2011)

Replaced function environments with `_ENV` lexical scoping. Added
`goto`/labels, `bit32` library, `table.pack`/`table.unpack`, `rawlen`,
yieldable `pcall`/`xpcall`, ephemeron tables, table finalizers (`__gc`),
hex string escapes, hexadecimal floats, `package.searchpath`. Documented
frontier patterns (`%f`, already implemented in 5.1). Removed
`setfenv`/`getfenv`, `newproxy`. Deprecated `module`, `loadstring`,
global `unpack`, `table.maxn`, `math.log10`.

### Lua 5.3 (2015)

Added 64-bit integer subtype alongside floats. Introduced native bitwise
operators (`&`, `|`, `~`, `<<`, `>>`), floor division (`//`), `utf8`
library, `string.pack`/`unpack`/`packsize`, `table.move`,
`math.tointeger`/`math.type`/`math.maxinteger`/`math.mininteger`,
`coroutine.isyieldable`, `math.ult`, `\u{XXXX}` string escapes,
`string.dump` strip parameter. Changed `ipairs` to use `lua_geti`
(respects `__index` metamethods). Deprecated `bit32` library,
`math.atan2`, `math.cosh`/`sinh`/`tanh`, `math.pow`,
`math.frexp`/`ldexp`, `math.log10`. Deprecated `__ipairs` metamethod.
Removed experimental generational GC.

### Lua 5.4 (2020)

Added to-be-closed variables (`<close>`), const locals (`<const>`),
generic `for` closing value (4th variable), stable generational GC,
`warn()` function and `-W` CLI flag, `coroutine.close`, `__close`
metamethod, `string.gmatch` init parameter, `string.format` `%p`,
xoshiro256** RNG, `debug.setcstacklimit`, multiple user values for
userdata. Relaxed `__eq` to check each operand's metatable independently
(no longer requires same metamethod). Removed `bit32` library and
`__ipairs` metamethod. Deprecated math functions (`atan2`, `cosh`, `sinh`,
`tanh`, `pow`, `frexp`, `ldexp`, `log10`) remain behind
`LUA_COMPAT_MATHLIB` (under `LUA_COMPAT_5_3`). Restricted implicit
string-to-number coercion (moved to string metatable metamethods).

### Lua 5.5 (2025)

Added `global` keyword for explicit global declarations, named vararg
tables, read-only for-loop variables, `table.create`, incremental major
GC collections, compact arrays (60% less memory for large arrays),
decimal float printing, `collectgarbage("param")` unified parameter API.
Enhanced `utf8.offset` to return final position. Restored `math.frexp`
and `math.ldexp` as standard functions (no longer behind compat flag).
Removed `debug.setcstacklimit`, `=expr` REPL shorthand,
`collectgarbage("setpause"/"setstepmul")`.

---

## Corrections from Previous Versions

This matrix corrects version attributions found during verification
against the Lua reference manuals and PUC-Rio source code.

### Round 1 corrections (version attribution errors)

- **Bitwise operators** (`&`, `|`, `~`, `<<`, `>>`) are language-level
  features added in **5.3**, not 5.2. The `bit32` library (a function
  library, not operators) was added in 5.2.
- **`rawlen`** was added in **5.2**, not 5.3.
- **`table.move`** was added in **5.3**, not 5.2.
- **`coroutine.isyieldable`** was added in **5.3**, not 5.2.
- **`\xHH`** and **`\z`** string escapes were added in **5.2**, not 5.1.
- **Empty statement** (`;` as a statement) was added in **5.2**. In 5.1,
  `;` is only a statement separator.
- **`math.log10`** was deprecated in **5.2** (with `math.log(x, 10)` as
  replacement), not 5.3.

### Round 2 corrections (manual/source verification)

- **Frontier pattern `%f`**: Was already **implemented in 5.1.1** source
  code (`lstrlib.c` lines 383--393) but undocumented. 5.2 officially
  documented it. The Lua 5.2 readme lists "frontier patterns" but this
  refers to documentation, not new code. rilua implements `%f`.
- **`__pairs` metamethod**: Was **not removed in 5.3**. It remained
  functional through 5.3 (previously corrected from "removed in 5.3" to
  "removed in 5.4"; see Round 3 for further correction).
- **`__ipairs` metamethod**: Was **deprecated** (not removed) in 5.3.
  The 5.3 manual says "its `__ipairs` metamethod has been deprecated."
  It was removed in **5.4**.
- **`=expr` REPL shorthand**: Was **not removed in 5.3**. Calculator
  mode (`addreturn`) was added in 5.3, but `=expr` coexisted through
  5.4 with a "for compatibility with 5.2" comment. `=expr` was finally
  removed in **5.5**.

### Round 3 corrections (source code and rilua verification)

- **`\xHH` and `\z` escape sequences**: rilua column was ✓ but should be
  **✗**. The rilua lexer (`scan_short_string`) only handles 5.1 escapes
  (`\a \b \f \n \r \t \v \\ \" \' \<newline> \ddd`). Previous note
  "rilua includes as extension" was incorrect.
- **`table.unpack`**: rilua column was ✓ but should be **✗**. rilua only
  registers the global `unpack` (5.1 standard). `table.unpack` is a 5.2+
  feature not present in the table library.
- **`__pairs` metamethod**: Was **never removed** from PUC-Rio source or
  manual index. `lbaselib.c:luaB_pairs` unconditionally checks `__pairs`
  in 5.2, 5.3, 5.4, and 5.5. Corrected from "removed in 5.4" to ✓ in
  all versions 5.2 through 5.5.
- **`string.format` `%s` and `__tostring`**: Started in **5.2** (when
  `luaL_tolstring` was introduced), not 5.3. The 5.1 `lstrlib.c` uses
  `luaL_checklstring` (no metamethod); 5.2+ uses `luaL_tolstring`.
- **Reserved keyword count**: 5.1 has 21 keywords. 5.2+ has 22 (adds
  `goto`). Split into separate rows to avoid implying 5.2+ also has 21.
- **Math compat functions** (`atan2`, `cosh`, `sinh`, `tanh`, `pow`,
  `frexp`, `ldexp`, `log10`): Available in 5.4 behind `LUA_COMPAT_MATHLIB`
  (via `LUA_COMPAT_5_3`). Marked D instead of empty.
- **`math.frexp` and `math.ldexp`**: Restored as standard functions in
  **5.5** (always available, not behind any compat flag). Marked ✓ in 5.5.
- **`global` keyword**: Conditionally reserved in 5.5. Unreserved by
  default when `LUA_COMPAT_GLOBAL` is defined (which it is by default).
- **`__eq` note**: Removed "5.2+ requires same type and same metamethod"
  -- this restriction applies to all versions including 5.1. The 5.1
  source (`get_compTM`) already requires both operands to share the same
  metamethod function.
- **`collectgarbage("count")` note**: Clarified that 5.2 is the outlier
  returning 2 values (kbytes + remainder). Both 5.1 and 5.3+ return a
  single combined value.
- **`table.insert`/`table.remove` stricter checking**: Changed "5.3+" to
  "5.2+". Position bounds validation via `luaL_argcheck` was introduced
  in 5.2, not 5.3.

### Round 4 Corrections

- **`debug.setcstacklimit`**: Changed 5.5 from ✓ to empty. This function
  was added in 5.4 but removed in 5.5 (not present in 5.5 `dblib[]`).
- **`package.loaders`**: Changed 5.3 from empty to D. Available in 5.3
  via `LUA_COMPAT_LOADERS` (under `LUA_COMPAT_5_1`).
- **`module` and `package.seeall`**: Changed 5.3 from empty to D. Available
  in 5.3 via `LUA_COMPAT_MODULE` (under `LUA_COMPAT_5_1`).
- **Version Evolution Summary (5.4)**: Changed "Removed `__pairs`/`__ipairs`
  metamethods" to "Removed `__ipairs` metamethod". The `__pairs` metamethod
  was never removed (present unconditionally in `lbaselib.c` through 5.5).
- **Version Evolution Summary (5.4)**: Changed "Removed ... deprecated math
  functions" to "Moved ... behind `LUA_COMPAT_MATHLIB`". The table data
  correctly shows D (not empty) for these functions in 5.4, so "Removed"
  was contradictory.

### Round 5 Corrections

- **`xpcall(f, msgh, ...)`**: Changed rilua column from ✓ to ✗. The rilua
  implementation matches the 5.1 signature (2 args only); extra arguments
  are not forwarded to `f` (`state.top = func_pos + 1` in base.rs).
- **Legend**: Refined D definition from "available via compatibility flags,
  not recommended" to "per reference manual or available only via
  compatibility flags". In 5.1, functions like `gcinfo`, `table.foreach`,
  `table.foreachi`, `table.getn` are deprecated per the reference manual
  but unconditionally registered (no compat flag). The original wording
  only described the later-version compat-flag pattern.

### Round 6 Corrections

- **`collectgarbage("incremental")`**: Added ✓ for 5.2. Present in `opts[]`
  as `LUA_GCINC` (value 11) for switching from generational to incremental
  mode. Removed in 5.3 (with experimental generational GC), re-added in 5.4.
- **`collectgarbage("generational")`**: Added ✓ for 5.2. Present in `opts[]`
  as `LUA_GCGEN` (value 10). Same lifecycle as `"incremental"`.
- **`module` (section 11)**: Changed 5.3 from empty to D for consistency
  with section 18 (which was already corrected in Round 4). Available in
  5.3 via `LUA_COMPAT_MODULE` under `LUA_COMPAT_5_1`.

### Round 7 Corrections (Lua 5.3 focused)

- **`unpack` (global)**: Changed 5.3 from empty to D. Available in 5.3
  via `LUA_COMPAT_UNPACK` (under `LUA_COMPAT_5_1`).
- **`loadstring`**: Changed 5.3 from empty to D. Available in 5.3
  via `LUA_COMPAT_LOADSTRING` (under `LUA_COMPAT_5_1`).
- **`table.maxn`**: Changed 5.3 from empty to D. Available in 5.3
  via `LUA_COMPAT_MAXN` (under `LUA_COMPAT_5_2`).
- **Math compat function notes**: Changed "5.4-5.5 via `LUA_COMPAT_MATHLIB`"
  to "5.3+ via `LUA_COMPAT_MATHLIB`" for `math.atan2`, `math.cosh`,
  `math.sinh`, `math.tanh`, `math.pow`, `math.log10`. The flag exists in
  5.3 under `LUA_COMPAT_5_2`, same mechanism as 5.4's `LUA_COMPAT_5_3`.
- **`math.frexp`/`math.ldexp` notes**: Changed "5.4 via `LUA_COMPAT_MATHLIB`"
  to "5.3-5.4 via `LUA_COMPAT_MATHLIB`". Same compat mechanism applies in 5.3.
- **`ipairs` note**: Changed "5.3+ respects metamethods" to clarify it uses
  `lua_geti` (which respects `__index`), distinct from the 5.2 `__ipairs`
  metamethod.
- **Added `math.ult` row**: New in 5.3 (unsigned integer less-than). Present
  unconditionally in `mathlib[]` in 5.3+.
- **5.3 version summary**: Added `math.ult`, `string.dump` strip parameter,
  `ipairs` metamethod behavioral change, and `math.log10` deprecation.

### Round 8 Corrections (Lua 5.4 focused)

- **`__eq` note**: Changed "Same type and same metamethod required (all
  versions)" to distinguish 5.1-5.2 behavior (requires same metamethod via
  `get_compTM`) from 5.3+ behavior (checks each operand's metatable
  independently via `luaT_gettmbyobj`).
- **5.4 version summary**: Added `math.log10` to the list of deprecated math
  functions behind `LUA_COMPAT_MATHLIB` (was omitted while the other 7
  functions were listed). Added `debug.setcstacklimit`, multiple user values
  for userdata, generic `for` closing value, `-W` CLI flag, and `__eq`
  relaxation. Reworded "Moved ... behind `LUA_COMPAT_MATHLIB`" to "remain
  behind `LUA_COMPAT_MATHLIB` (under `LUA_COMPAT_5_3`)" since these were
  already behind the flag in 5.3.

### Round 9 Corrections (Lua 5.5 focused)

- **`collectgarbage("setpause"/"setstepmul")`**: Changed 5.5 from ✓ to
  empty. These options were removed in 5.5, replaced by the unified
  `collectgarbage("param", name [, value])` API (`LUA_GCPARAM`). The
  `opts[]` array in 5.5's `lbaselib.c` no longer contains `"setpause"`
  or `"setstepmul"`.
- **Added `collectgarbage("param")` row**: New in 5.5 only. Provides
  unified get/set for GC parameters: `"pause"`, `"stepmul"`, `"stepsize"`,
  `"minormul"`, `"majorminor"`, `"minormajor"`.
- **Reserved keyword count for 5.5**: Changed "22 reserved keywords" from
  ✓ to empty for 5.5. Added new "23 reserved keywords" row for 5.5.
  The 5.5 `llex.h` enum lists 23 reserved words (including `global`);
  `LUA_COMPAT_GLOBAL` (on by default) unreserves `global`, making the
  effective count 22, but the manual lists 23.
- **5.5 version summary**: Added restorations (`math.frexp`/`math.ldexp`),
  removals (`debug.setcstacklimit`, `=expr` REPL shorthand,
  `collectgarbage("setpause"/"setstepmul")`), and the new
  `collectgarbage("param")` API.
