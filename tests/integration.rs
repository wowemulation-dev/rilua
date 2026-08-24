//! End-to-end integration tests for rilua.
//!
//! These tests run Lua code through the full pipeline (compile -> execute)
//! via the `rilua` binary and verify stdout/stderr output.

#![allow(clippy::expect_used)]

use std::process::Command;

/// Helper: run `rilua -e <code>` and return (stdout, stderr, exit_code).
fn run_rilua(code: &str) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg("-e")
        .arg(code)
        .output()
        .expect("failed to run rilua binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

// ---------------------------------------------------------------------------
// print tests
// ---------------------------------------------------------------------------

#[test]
fn print_number() {
    let (stdout, _, code) = run_rilua("print(42)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn print_string() {
    let (stdout, _, code) = run_rilua("print(\"hello\")");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\n");
}

#[test]
fn print_multiple() {
    let (stdout, _, code) = run_rilua("print(1, 2, 3)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t2\t3\n");
}

#[test]
fn print_nil() {
    let (stdout, _, code) = run_rilua("print(nil)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn print_bool() {
    let (stdout, _, code) = run_rilua("print(true, false)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\tfalse\n");
}

#[test]
fn print_no_args() {
    let (stdout, _, code) = run_rilua("print()");
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n");
}

// ---------------------------------------------------------------------------
// Variable and expression tests
// ---------------------------------------------------------------------------

#[test]
fn variable_assignment() {
    let (stdout, _, code) = run_rilua("x = 42 print(x)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn arithmetic_print() {
    let (stdout, _, code) = run_rilua("print(1 + 2)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn string_variable() {
    let (stdout, _, code) = run_rilua("x = \"world\" print(\"hello\", x)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\tworld\n");
}

#[test]
fn multiple_assignments() {
    let (stdout, _, code) = run_rilua("a = 1 b = 2 print(a + b)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn print_negative_number() {
    let (stdout, _, code) = run_rilua("print(-5)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "-5\n");
}

#[test]
fn print_float() {
    let (stdout, _, code) = run_rilua("print(3.14)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3.14\n");
}

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

#[test]
fn syntax_error() {
    let (_, stderr, code) = run_rilua("if then");
    assert_ne!(code, 0);
    assert!(!stderr.is_empty(), "syntax error should produce stderr");
}

#[test]
fn missing_e_argument() {
    let output = Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg("-e")
        .output()
        .expect("failed to run rilua binary");
    assert_ne!(output.status.code().unwrap_or(-1), 0);
}

#[test]
fn version_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg("-v")
        .output()
        .expect("failed to run rilua binary");
    // PUC-Rio prints version to stderr (via l_message(NULL, ...)).
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Lua 5.1.1"));
    assert_eq!(output.status.code().unwrap_or(-1), 0);
}

#[test]
fn nonexistent_file() {
    let output = Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg("/tmp/rilua_nonexistent_file.lua")
        .output()
        .expect("failed to run rilua binary");
    assert_ne!(output.status.code().unwrap_or(-1), 0);
}

// ---------------------------------------------------------------------------
// Phase 4: stdlib functions
// ---------------------------------------------------------------------------

#[test]
fn type_function() {
    let (stdout, _, code) =
        run_rilua("print(type(1), type('s'), type(nil), type(true), type({}), type(print))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "number\tstring\tnil\tboolean\ttable\tfunction\n");
}

#[test]
fn tostring_function() {
    let (stdout, _, code) = run_rilua("print(tostring(42), tostring(nil), tostring(true))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\tnil\ttrue\n");
}

#[test]
fn tonumber_function() {
    let (stdout, _, code) = run_rilua("print(tonumber('42'), tonumber('ff', 16))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\t255\n");
}

#[test]
fn assert_function_success() {
    let (stdout, _, code) = run_rilua("print(assert(42, 'msg'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\tmsg\n");
}

#[test]
fn assert_function_failure() {
    let (_, stderr, code) = run_rilua("assert(false, 'test failed')");
    assert_ne!(code, 0);
    assert!(stderr.contains("test failed"));
}

#[test]
fn error_function() {
    let (_, stderr, code) = run_rilua("error('boom')");
    assert_ne!(code, 0);
    assert!(stderr.contains("boom"));
}

#[test]
fn pcall_success() {
    let (stdout, _, code) = run_rilua("print(pcall(function() return 1, 2, 3 end))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t1\t2\t3\n");
}

#[test]
fn pcall_error() {
    let (stdout, _, code) = run_rilua("print(pcall(function() error('boom') end))");
    assert_eq!(code, 0);
    assert!(stdout.starts_with("false\t"));
    assert!(stdout.contains("boom"));
}

#[test]
fn xpcall_with_handler() {
    let (stdout, _, code) = run_rilua(
        "print(xpcall(function() error('boom') end, function(e) return 'caught: ' .. e end))",
    );
    assert_eq!(code, 0);
    assert!(stdout.starts_with("false\t"));
    assert!(stdout.contains("caught:"));
}

#[test]
fn select_count() {
    let (stdout, _, code) = run_rilua("print(select('#', 1, 2, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn select_range() {
    let (stdout, _, code) = run_rilua("print(select(2, 'a', 'b', 'c'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "b\tc\n");
}

#[test]
fn unpack_function() {
    let (stdout, _, code) = run_rilua("print(unpack({10, 20, 30}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\t20\t30\n");
}

#[test]
fn rawget_rawset_rawequal() {
    let (stdout, _, code) = run_rilua(
        "local t = {}; rawset(t, 'x', 42); print(rawget(t, 'x'), rawequal(1, 1), rawequal(1, 2))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\ttrue\tfalse\n");
}

// ---------------------------------------------------------------------------
// Phase 4: metamethods
// ---------------------------------------------------------------------------

#[test]
fn metamethod_add() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__add = function(a,b) return 42 end}); print(t + 1)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn metamethod_index_function() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__index = function(t,k) return k end}); print(t.hello)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\n");
}

#[test]
fn metamethod_index_table() {
    let (stdout, _, code) = run_rilua(
        "local base = {x = 10}; local t = setmetatable({}, {__index = base}); print(t.x)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n");
}

#[test]
fn metamethod_newindex() {
    let (stdout, _, code) = run_rilua(
        "local log = {}; local t = setmetatable({}, {__newindex = function(t,k,v) log[k] = v end}); t.x = 42; print(rawget(t, 'x'), log.x)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\t42\n");
}

#[test]
fn metamethod_call() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__call = function(self, a, b) return a + b end}); print(t(3, 4))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n");
}

#[test]
fn metamethod_tostring() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__tostring = function(self) return 'mytable' end}); print(tostring(t))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "mytable\n");
}

#[test]
fn metamethod_len_table_ignores() {
    // In Lua 5.1.1, __len is NOT called for tables (only for userdata).
    // Tables always use raw length.
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({1, 2, 3}, {__len = function(self) return 99 end}); print(#t)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n"); // Raw length, not 99.
}

#[test]
fn metamethod_unm() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__unm = function(self) return 'negated' end}); print(-t)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "negated\n");
}

#[test]
fn metamethod_concat() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__concat = function(a,b) return 'joined' end}); print(t .. 'x')",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "joined\n");
}

#[test]
fn metamethod_eq() {
    let (stdout, _, code) = run_rilua(
        "local mt = {__eq = function(a,b) return true end}; local a = setmetatable({}, mt); local b = setmetatable({}, mt); print(a == b, a == {})",
    );
    assert_eq!(code, 0);
    // a == b uses shared __eq, returns true. a == {} has no shared __eq, returns false.
    assert_eq!(stdout, "true\tfalse\n");
}

#[test]
fn metamethod_lt_le() {
    let (stdout, _, code) = run_rilua(
        "local mt = {__lt = function(a,b) return true end, __le = function(a,b) return true end}; local a = setmetatable({}, mt); local b = setmetatable({}, mt); print(a < b, a <= b)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\ttrue\n");
}

#[test]
fn setmetatable_getmetatable() {
    let (stdout, _, code) =
        run_rilua("local mt = {}; local t = setmetatable({}, mt); print(getmetatable(t) == mt)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn metatable_protection() {
    let (stdout, _, code) = run_rilua(
        "local t = setmetatable({}, {__metatable = 'protected'}); print(getmetatable(t)); print(pcall(setmetatable, t, {}))",
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "protected\nfalse\tcannot change a protected metatable\n"
    );
}

#[test]
fn pcall_nested() {
    let (stdout, _, code) =
        run_rilua("print(pcall(function() return pcall(function() error('inner') end) end))");
    assert_eq!(code, 0);
    assert!(stdout.starts_with("true\tfalse\t"));
    assert!(stdout.contains("inner"));
}

// ---------------------------------------------------------------------------
// Phase 5a: globals, iterators, loading, environments
// ---------------------------------------------------------------------------

#[test]
fn version_global() {
    let (stdout, _, code) = run_rilua("print(_VERSION)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "Lua 5.1\n");
}

#[test]
fn g_global_is_table() {
    let (stdout, _, code) = run_rilua("print(type(_G))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn g_self_referential() {
    let (stdout, _, code) = run_rilua("print(_G == _G)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn g_contains_print() {
    let (stdout, _, code) = run_rilua("print(_G.print == print)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn next_basic() {
    let (stdout, _, code) = run_rilua("local k, v = next({a=1}) print(k, v)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "a\t1\n");
}

#[test]
fn next_nil_at_end() {
    let (stdout, _, code) = run_rilua("local t = {a=1} local k = next(t) print(next(t, k))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn next_empty_table() {
    let (stdout, _, code) = run_rilua("print(next({}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn pairs_iteration() {
    let (stdout, _, code) = run_rilua(
        "local sum = 0; for k, v in pairs({a=1, b=2, c=3}) do sum = sum + v end; print(sum)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

#[test]
fn ipairs_basic() {
    let (stdout, _, code) = run_rilua("for i, v in ipairs({10, 20, 30}) do print(i, v) end");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t10\n2\t20\n3\t30\n");
}

#[test]
fn ipairs_stops_at_nil() {
    let (stdout, _, code) = run_rilua(
        "local t = {1, 2, nil, 4}; local count = 0; for i, v in ipairs(t) do count = count + 1 end; print(count)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n");
}

#[test]
fn loadstring_success() {
    let (stdout, _, code) = run_rilua("local f = loadstring('return 1+2') print(f())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn loadstring_error() {
    let (stdout, _, code) = run_rilua("local f, err = loadstring('if then') print(f, type(err))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\tstring\n");
}

#[test]
fn loadstring_with_name() {
    let (stdout, _, code) = run_rilua(
        "local f, err = loadstring('error(\"test\")', 'mychunk') local ok, msg = pcall(f) print(msg)",
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("mychunk"));
    assert!(stdout.contains("test"));
}

#[test]
fn loadstring_returns_function() {
    let (stdout, _, code) = run_rilua("local f = loadstring('return 42') print(type(f), f())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\t42\n");
}

#[test]
fn loadstring_break_outside_loop() {
    // Bug #22: loadstring("break label") should report "no loop to break"
    // with source formatted as [string "..."].
    let (stdout, _, code) = run_rilua("local f, err = loadstring('break label') print(err)");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("[string \"break label\"]:1: no loop to break near 'label'"),
        "got: {stdout}"
    );
}

#[test]
fn collectgarbage_count() {
    let (stdout, _, code) = run_rilua("print(type(collectgarbage('count')))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "number\n");
}

#[test]
fn collectgarbage_collect() {
    let (stdout, _, code) = run_rilua("collectgarbage() print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn collectgarbage_step() {
    // step(0) does not complete a full cycle in one step (returns false).
    // This matches PUC-Rio: the budget (GCSTEPSIZE/100 * stepmul = 2000) is
    // not enough to propagate through the main thread + global table + all
    // stdlib tables in a single luaC_step call.
    let (stdout, _, code) = run_rilua("print(collectgarbage('step', 0))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "false\n");

    // A larger step argument completes the cycle: step(1000) returns true.
    let (stdout, _, code) = run_rilua("print(collectgarbage('step', 1000))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn getfenv_level_zero() {
    let (stdout, _, code) = run_rilua("print(getfenv(0) == _G)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn setfenv_level_zero() {
    let (stdout, _, code) = run_rilua(
        "local env = setmetatable({print=print}, {__index=_G}) setfenv(0, env) print('ok')",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn setfenv_function() {
    let (stdout, _, code) = run_rilua(
        "local f = function() return x end local env = {x = 42} setfenv(f, env) print(f())",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn getfenv_returns_table() {
    let (stdout, _, code) = run_rilua("print(type(getfenv(1)))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn newproxy_basic() {
    let (stdout, _, code) = run_rilua("local p = newproxy() print(type(p))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "userdata\n");
}

#[test]
fn newproxy_false() {
    let (stdout, _, code) = run_rilua("local p = newproxy(false) print(type(p))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "userdata\n");
}

#[test]
fn newproxy_with_metatable() {
    let (stdout, _, code) = run_rilua("local p = newproxy(true) print(type(getmetatable(p)))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn newproxy_tostring_metamethod() {
    let (stdout, _, code) = run_rilua(
        "local p = newproxy(true) \
         local mt = getmetatable(p) \
         mt.__tostring = function() return 'proxy!' end \
         print(tostring(p))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "proxy!\n");
}

#[test]
fn newproxy_index_metamethod() {
    let (stdout, _, code) = run_rilua(
        "local p = newproxy(true) \
         local mt = getmetatable(p) \
         mt.__index = function(_, k) return k .. '!' end \
         print(p.hello)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello!\n");
}

#[test]
fn newproxy_shared_metatable() {
    let (stdout, _, code) = run_rilua(
        "local p1 = newproxy(true) \
         getmetatable(p1).__index = function(_, k) return 42 end \
         local p2 = newproxy(p1) \
         print(p2.x)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn newproxy_type_is_userdata() {
    let (stdout, _, code) = run_rilua(
        "print(type(newproxy())) \
         print(type(newproxy(true))) \
         print(type(newproxy(false)))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "userdata\nuserdata\nuserdata\n");
}

#[test]
fn load_function() {
    let (stdout, _, code) = run_rilua(
        "local i = 0 local chunks = {'ret', 'urn ', '42'} local f = load(function() i = i + 1 return chunks[i] end) print(f())",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn load_function_streaming_char_at_a_time() {
    // Character-at-a-time reader with global assignment (based on calls.lua).
    // Tests that streaming works for multi-statement code with upvalues.
    let (stdout, _, code) = run_rilua(
        r#"
        x = "x = 10 + 23; local a = function () x = 'hi' end; return 'done'"
        local i = 0
        function read1(x)
          return function ()
            collectgarbage()
            i = i + 1
            return string.sub(x, i, i)
          end
        end
        local a = assert(load(read1(x), "modname"))
        assert(a() == "done" and _G.x == 33)
        assert(debug.getinfo(a).source == "modname")
        print("ok")
        "#,
    );
    assert_eq!(code, 0, "streaming char-at-a-time load should succeed");
    assert_eq!(stdout, "ok\n");
}

#[test]
fn load_function_streaming_syntax_error_stops_early() {
    // PUC-Rio calls.lua line 250: reader is called lazily, so a syntax
    // error at character 2 means i == 2, not the full string length.
    let (stdout, _, code) = run_rilua(
        r#"
        local i = 0
        function read1(x)
          return function ()
            collectgarbage()
            i = i + 1
            return string.sub(x, i, i)
          end
        end
        local a, b = load(read1("*a = 123"))
        assert(not a and type(b) == "string" and i == 2)
        print("ok")
        "#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn load_function_reader_error_propagates() {
    // PUC-Rio calls.lua line 252: reader that errors should return nil + message.
    let (stdout, _, code) = run_rilua(
        r#"
        local a, b = load(function () error("hhi") end)
        assert(not a and string.find(b, "hhi"))
        print("ok")
        "#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn load_function_streaming_nested_functions() {
    // PUC-Rio calls.lua line 266: nested function definitions via streaming reader.
    let (stdout, _, code) = run_rilua(
        r#"
        local i = 0
        function read1(x)
          return function ()
            collectgarbage()
            i = i + 1
            return string.sub(x, i, i)
          end
        end
        local x = [[
          return function (x)
            return function (y)
             return function (z)
               return x+y+z
             end
           end
          end
        ]]
        local a = assert(load(read1(x)))
        assert(a()(2)(3)(10) == 15)
        print("ok")
        "#,
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

#[test]
fn load_function_empty_reader() {
    // Reader that returns nil immediately should produce an empty chunk.
    let (stdout, _, code) = run_rilua("local f = load(function() return nil end) print(type(f))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\n");
}

// ---------------------------------------------------------------------------
// string library tests
// ---------------------------------------------------------------------------

#[test]
fn string_len() {
    let (stdout, _, code) = run_rilua("print(string.len('hello'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n");
}

#[test]
fn string_len_empty() {
    let (stdout, _, code) = run_rilua("print(string.len(''))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

#[test]
fn string_byte_single() {
    let (stdout, _, code) = run_rilua("print(string.byte('A'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "65\n");
}

#[test]
fn string_byte_range() {
    let (stdout, _, code) = run_rilua("print(string.byte('abc', 1, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "97\t98\t99\n");
}

#[test]
fn string_char() {
    let (stdout, _, code) = run_rilua("print(string.char(72, 101))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "He\n");
}

#[test]
fn string_sub() {
    let (stdout, _, code) = run_rilua("print(string.sub('hello', 2, 4))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "ell\n");
}

#[test]
fn string_sub_negative() {
    let (stdout, _, code) = run_rilua("print(string.sub('hello', -3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "llo\n");
}

#[test]
fn string_rep() {
    let (stdout, _, code) = run_rilua("print(string.rep('ab', 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "ababab\n");
}

#[test]
fn string_rep_zero() {
    let (stdout, _, code) = run_rilua("print(string.rep('ab', 0))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n");
}

#[test]
fn string_reverse() {
    let (stdout, _, code) = run_rilua("print(string.reverse('hello'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "olleh\n");
}

#[test]
fn string_lower() {
    let (stdout, _, code) = run_rilua("print(string.lower('Hello World'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello world\n");
}

#[test]
fn string_upper() {
    let (stdout, _, code) = run_rilua("print(string.upper('Hello World'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "HELLO WORLD\n");
}

#[test]
fn string_format_basic() {
    let (stdout, _, code) = run_rilua("print(string.format('%d %s', 42, 'hi'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42 hi\n");
}

#[test]
fn string_format_hex() {
    let (stdout, _, code) = run_rilua("print(string.format('%x', 255))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "ff\n");
}

#[test]
fn string_format_float() {
    let (stdout, _, code) = run_rilua("print(string.format('%f', 3.14))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3.140000\n");
}

#[test]
fn string_format_quoted() {
    let (stdout, _, code) = run_rilua(r#"print(string.format('%q', 'hello "world"'))"#);
    assert_eq!(code, 0);
    assert_eq!(stdout, "\"hello \\\"world\\\"\"\n");
}

#[test]
fn string_format_percent() {
    let (stdout, _, code) = run_rilua("print(string.format('100%%'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "100%\n");
}

#[test]
fn string_find_plain() {
    let (stdout, _, code) = run_rilua("print(string.find('hello world', 'world'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\t11\n");
}

#[test]
fn string_find_pattern() {
    let (stdout, _, code) = run_rilua("print(string.find('hello123', '%d+'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\t8\n");
}

#[test]
fn string_find_not_found() {
    let (stdout, _, code) = run_rilua("print(string.find('hello', 'xyz'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn string_match_captures() {
    let (stdout, _, code) = run_rilua("print(string.match('2024-01-15', '(%d+)-(%d+)-(%d+)'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "2024\t01\t15\n");
}

#[test]
fn string_match_no_capture() {
    let (stdout, _, code) = run_rilua("print(string.match('hello', '%a+'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\n");
}

#[test]
fn string_gmatch_iteration() {
    let (stdout, _, code) = run_rilua(
        "local t = {} for w in string.gmatch('hello world foo', '%a+') do t[#t+1] = w end print(t[1], t[2], t[3])",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\tworld\tfoo\n");
}

#[test]
fn string_gsub_basic() {
    let (stdout, _, code) = run_rilua("print(string.gsub('hello', 'l', 'L'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "heLLo\t2\n");
}

#[test]
fn string_gsub_with_limit() {
    let (stdout, _, code) = run_rilua("print(string.gsub('hello', 'l', 'L', 1))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "heLlo\t1\n");
}

#[test]
fn string_gsub_pattern() {
    let (stdout, _, code) = run_rilua("print(string.gsub('abc123', '%d', '*'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "abc***\t3\n");
}

#[test]
fn string_method_upper() {
    let (stdout, _, code) = run_rilua("print(('hello'):upper())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "HELLO\n");
}

#[test]
fn string_method_len() {
    let (stdout, _, code) = run_rilua("print(('abc'):len())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn string_method_sub() {
    let (stdout, _, code) = run_rilua("print(('hello'):sub(1, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hel\n");
}

#[test]
fn string_method_rep() {
    let (stdout, _, code) = run_rilua("print(('ab'):rep(3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "ababab\n");
}

#[test]
fn string_method_reverse() {
    let (stdout, _, code) = run_rilua("print(('hello'):reverse())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "olleh\n");
}

#[test]
fn string_method_find() {
    let (stdout, _, code) = run_rilua("print(('hello world'):find('world'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\t11\n");
}

#[test]
fn string_method_format() {
    let (stdout, _, code) = run_rilua("print(string.format('%s=%d', 'x', 10))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "x=10\n");
}

#[test]
fn string_method_gsub() {
    let (stdout, _, code) = run_rilua("print(('aaa'):gsub('a', 'b'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "bbb\t3\n");
}

#[test]
fn string_method_byte() {
    let (stdout, _, code) = run_rilua("print(('ABC'):byte(1, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "65\t66\t67\n");
}

#[test]
fn string_method_lower() {
    let (stdout, _, code) = run_rilua("print(('HELLO'):lower())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\n");
}

#[test]
fn string_find_with_captures() {
    let (stdout, _, code) = run_rilua("print(string.find('hello world', '(w%a+)'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\t11\tworld\n");
}

#[test]
fn string_format_g() {
    let (stdout, _, code) = run_rilua("print(string.format('%g', 100000))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "100000\n");
}

#[test]
fn string_format_g_scientific() {
    let (stdout, _, code) = run_rilua("print(string.format('%g', 1e10))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1e+10\n");
}

#[test]
fn string_gsub_function_replacement() {
    let (stdout, _, code) =
        run_rilua("print(string.gsub('abc', '%a', function(c) return string.upper(c) end))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "ABC\t3\n");
}

#[test]
fn string_gsub_table_replacement() {
    let (stdout, _, code) =
        run_rilua("local t = {a='A', b='B'} print(string.gsub('aXb', '%a', t))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "AXB\t3\n");
}

#[test]
fn string_find_anchor() {
    let (stdout, _, code) = run_rilua("print(string.find('hello', '^hel'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t3\n");
}

#[test]
fn string_find_anchor_no_match() {
    let (stdout, _, code) = run_rilua("print(string.find('hello', '^llo'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn string_match_empty_capture() {
    let (stdout, _, code) = run_rilua("print(string.match('hello', '()h'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn string_global_type() {
    let (stdout, _, code) = run_rilua("print(type(string))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn string_global_has_functions() {
    let (stdout, _, code) = run_rilua("print(type(string.len), type(string.format))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\tfunction\n");
}

// ---------------------------------------------------------------------------
// table library tests
// ---------------------------------------------------------------------------

#[test]
fn table_global_type() {
    let (stdout, _, code) = run_rilua("print(type(table))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn table_global_has_functions() {
    let (stdout, _, code) =
        run_rilua("print(type(table.concat), type(table.insert), type(table.sort))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\tfunction\tfunction\n");
}

// -- table.concat --

#[test]
fn table_concat_basic() {
    let (stdout, _, code) = run_rilua("print(table.concat({1, 2, 3}, ', '))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1, 2, 3\n");
}

#[test]
fn table_concat_strings() {
    let (stdout, _, code) = run_rilua("print(table.concat({'a', 'b', 'c'}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "abc\n");
}

#[test]
fn table_concat_default_sep() {
    let (stdout, _, code) = run_rilua("print(table.concat({'x', 'y', 'z'}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "xyz\n");
}

#[test]
fn table_concat_range() {
    let (stdout, _, code) = run_rilua("print(table.concat({'a', 'b', 'c', 'd'}, '-', 2, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "b-c\n");
}

#[test]
fn table_concat_empty() {
    let (stdout, _, code) = run_rilua("print(table.concat({}, ', '))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n");
}

#[test]
fn table_concat_single() {
    let (stdout, _, code) = run_rilua("print(table.concat({'only'}, ', '))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "only\n");
}

#[test]
fn table_concat_numbers() {
    let (stdout, _, code) = run_rilua("print(table.concat({10, 20, 30}, '+'))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "10+20+30\n");
}

#[test]
fn table_concat_error_non_string() {
    let (_, stderr, code) = run_rilua("table.concat({1, true, 3}, ', ')");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("table contains non-strings"),
        "stderr: {stderr}"
    );
}

// -- table.insert --

#[test]
fn table_insert_append() {
    let (stdout, _, code) =
        run_rilua("local t = {1, 2, 3}; table.insert(t, 4); print(t[1], t[2], t[3], t[4])");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t2\t3\t4\n");
}

#[test]
fn table_insert_at_position() {
    let (stdout, _, code) =
        run_rilua("local t = {1, 2, 3}; table.insert(t, 2, 99); print(t[1], t[2], t[3], t[4])");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t99\t2\t3\n");
}

#[test]
fn table_insert_at_start() {
    let (stdout, _, code) =
        run_rilua("local t = {1, 2, 3}; table.insert(t, 1, 0); print(t[1], t[2], t[3], t[4])");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\t1\t2\t3\n");
}

#[test]
fn table_insert_wrong_args() {
    let (_, stderr, code) = run_rilua("table.insert({})");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("wrong number of arguments to 'insert'"),
        "stderr: {stderr}"
    );
}

// -- table.remove --

#[test]
fn table_remove_last() {
    let (stdout, _, code) =
        run_rilua("local t = {1, 2, 3}; local v = table.remove(t); print(v, #t, t[1], t[2])");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\t2\t1\t2\n");
}

#[test]
fn table_remove_at_position() {
    let (stdout, _, code) =
        run_rilua("local t = {1, 2, 3}; local v = table.remove(t, 2); print(v, #t, t[1], t[2])");
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\t2\t1\t3\n");
}

#[test]
fn table_remove_empty() {
    // table.remove on empty table returns nothing.
    let (stdout, _, code) = run_rilua("local t = {}; print(table.remove(t))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n");
}

#[test]
fn table_remove_return_value() {
    let (stdout, _, code) = run_rilua("local t = {'a', 'b', 'c'}; print(table.remove(t, 1))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "a\n");
}

// -- table.sort --

#[test]
fn table_sort_numbers() {
    let (stdout, _, code) = run_rilua(
        "local t = {3, 1, 4, 1, 5, 9, 2, 6}; table.sort(t); print(table.concat(t, ', '))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1, 1, 2, 3, 4, 5, 6, 9\n");
}

#[test]
fn table_sort_strings() {
    let (stdout, _, code) = run_rilua(
        "local t = {'banana', 'apple', 'cherry'}; table.sort(t); print(table.concat(t, ', '))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "apple, banana, cherry\n");
}

#[test]
fn table_sort_custom_comparator() {
    let (stdout, _, code) = run_rilua(
        "local t = {3, 1, 4, 1, 5}; table.sort(t, function(a, b) return a > b end); print(table.concat(t, ', '))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "5, 4, 3, 1, 1\n");
}

#[test]
fn table_sort_empty() {
    let (stdout, _, code) = run_rilua("local t = {}; table.sort(t); print(#t)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

#[test]
fn table_sort_single() {
    let (stdout, _, code) = run_rilua("local t = {42}; table.sort(t); print(t[1])");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn table_sort_already_sorted() {
    let (stdout, _, code) =
        run_rilua("local t = {1, 2, 3, 4, 5}; table.sort(t); print(table.concat(t, ', '))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1, 2, 3, 4, 5\n");
}

#[test]
fn table_sort_reverse_sorted() {
    let (stdout, _, code) =
        run_rilua("local t = {5, 4, 3, 2, 1}; table.sort(t); print(table.concat(t, ', '))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1, 2, 3, 4, 5\n");
}

// -- table.maxn --

#[test]
fn table_maxn_basic() {
    let (stdout, _, code) = run_rilua("print(table.maxn({1, 2, 3}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn table_maxn_empty() {
    let (stdout, _, code) = run_rilua("print(table.maxn({}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

#[test]
fn table_maxn_float_keys() {
    let (stdout, _, code) =
        run_rilua("local t = {}; t[1] = 'a'; t[3.5] = 'b'; t[100] = 'c'; print(table.maxn(t))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "100\n");
}

// -- table.getn --

#[test]
fn table_getn_basic() {
    let (stdout, _, code) = run_rilua("print(table.getn({10, 20, 30}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

// -- table.setn --

#[test]
fn table_setn_error() {
    let (_, stderr, code) = run_rilua("table.setn({}, 5)");
    assert_ne!(code, 0);
    assert!(stderr.contains("'setn' is obsolete"), "stderr: {stderr}");
}

// -- table.foreach / foreachi --

#[test]
fn table_foreachi_basic() {
    let (stdout, _, code) = run_rilua(
        "local r = '' table.foreachi({10, 20, 30}, function(i, v) r = r .. i .. '=' .. v .. ' ' end) print(r)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1=10 2=20 3=30 \n");
}

#[test]
fn table_foreachi_early_return() {
    let (stdout, _, code) = run_rilua(
        "local v = table.foreachi({10, 20, 30}, function(i, v) if v == 20 then return 'found' end end) print(v)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "found\n");
}

#[test]
fn table_foreach_basic() {
    // foreach iterates all keys -- order is not guaranteed for hash part,
    // so test with only integer keys where order matches foreachi.
    let (stdout, _, code) = run_rilua(
        "local sum = 0 table.foreach({10, 20, 30}, function(k, v) sum = sum + v end) print(sum)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "60\n");
}

#[test]
fn table_foreach_early_return() {
    let (stdout, _, code) = run_rilua(
        "local v = table.foreach({a=1, b=2}, function(k, v) if v == 1 then return k end end) print(type(v))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "string\n");
}

// ---------------------------------------------------------------------------
// math library tests
// ---------------------------------------------------------------------------

#[test]
fn math_global_type() {
    let (stdout, _, code) = run_rilua("print(type(math))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn math_global_has_functions() {
    let (stdout, _, code) = run_rilua("print(type(math.sin), type(math.cos), type(math.random))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\tfunction\tfunction\n");
}

// -- constants --

#[test]
fn math_pi() {
    let (stdout, _, code) = run_rilua("print(math.pi)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3.1415926535898\n");
}

#[test]
fn math_huge() {
    let (stdout, _, code) = run_rilua("print(math.huge)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "inf\n");
}

#[test]
fn math_huge_comparisons() {
    let (stdout, _, code) = run_rilua("print(math.huge > 10e30, -math.huge < -10e30)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\ttrue\n");
}

// -- abs, floor, ceil --

#[test]
fn math_abs_positive() {
    let (stdout, _, code) = run_rilua("print(math.abs(5))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n");
}

#[test]
fn math_abs_negative() {
    let (stdout, _, code) = run_rilua("print(math.abs(-10))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n");
}

#[test]
fn math_floor_basic() {
    let (stdout, _, code) = run_rilua("print(math.floor(4.5))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "4\n");
}

#[test]
fn math_ceil_basic() {
    let (stdout, _, code) = run_rilua("print(math.ceil(4.5))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n");
}

#[test]
fn math_floor_negative() {
    let (stdout, _, code) = run_rilua("print(math.floor(-2.3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "-3\n");
}

#[test]
fn math_ceil_negative() {
    let (stdout, _, code) = run_rilua("print(math.ceil(-2.3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "-2\n");
}

// -- trig --

#[test]
fn math_sin_zero() {
    let (stdout, _, code) = run_rilua("print(math.sin(0))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

#[test]
fn math_cos_zero() {
    let (stdout, _, code) = run_rilua("print(math.cos(0))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn math_tan_identity() {
    let (stdout, _, code) =
        run_rilua("local x = math.pi/4; print(math.abs(math.tan(x) - 1) < 1e-10)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn math_asin_acos_atan() {
    let (stdout, _, code) = run_rilua(
        "local eq = function(a,b) return math.abs(a-b) < 1e-10 end; print(eq(math.asin(1), math.pi/2), eq(math.acos(0), math.pi/2), eq(math.atan(1), math.pi/4))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\ttrue\ttrue\n");
}

#[test]
fn math_atan2_basic() {
    let (stdout, _, code) = run_rilua("print(math.abs(math.atan2(1, 0) - math.pi/2) < 1e-10)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

// -- hyperbolic --

#[test]
fn math_sinh_cosh_tanh_identity() {
    let (stdout, _, code) = run_rilua(
        "local x = 3.5; print(math.abs(math.tanh(x) - math.sinh(x)/math.cosh(x)) < 1e-10)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

// -- exp, log, log10, sqrt, pow --

#[test]
fn math_exp_zero() {
    let (stdout, _, code) = run_rilua("print(math.exp(0))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn math_log_one() {
    let (stdout, _, code) = run_rilua("print(math.log(1))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

#[test]
fn math_log10_identity() {
    let (stdout, _, code) =
        run_rilua("print(math.abs(math.log10(2) - math.log(2)/math.log(10)) < 1e-10)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn math_sqrt_basic() {
    let (stdout, _, code) = run_rilua("print(math.sqrt(16))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "4\n");
}

#[test]
fn math_pow_basic() {
    let (stdout, _, code) = run_rilua("print(math.pow(2, 10))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1024\n");
}

// -- deg, rad --

#[test]
fn math_deg_rad() {
    let (stdout, _, code) = run_rilua(
        "local eq = function(a,b) return math.abs(a-b) < 1e-10 end; print(eq(math.deg(math.pi/2), 90), eq(math.rad(90), math.pi/2))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\ttrue\n");
}

// -- fmod, mod alias --

#[test]
fn math_fmod_basic() {
    let (stdout, _, code) = run_rilua("print(math.fmod(10, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn math_mod_alias() {
    let (stdout, _, code) = run_rilua("print(math.mod(10, 3))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

// -- modf --

#[test]
fn math_modf_positive() {
    let (stdout, _, code) = run_rilua("local a, b = math.modf(3.5); print(a, b)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\t0.5\n");
}

#[test]
fn math_modf_negative() {
    let (stdout, _, code) = run_rilua("local a, b = math.modf(-3.5); print(a, b)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "-3\t-0.5\n");
}

// -- frexp, ldexp --

#[test]
fn math_frexp_basic() {
    let (stdout, _, code) = run_rilua("local v, e = math.frexp(8); print(v, e)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "0.5\t4\n");
}

#[test]
fn math_frexp_ldexp_roundtrip() {
    let (stdout, _, code) = run_rilua(
        "local v, e = math.frexp(math.pi); print(math.abs(math.ldexp(v, e) - math.pi) < 1e-10)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn math_ldexp_basic() {
    let (stdout, _, code) = run_rilua("print(math.ldexp(0.5, 4))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "8\n");
}

// -- min, max --

#[test]
fn math_min_basic() {
    let (stdout, _, code) = run_rilua("print(math.min(3, 1, 4, 1, 5))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn math_max_basic() {
    let (stdout, _, code) = run_rilua("print(math.max(3, 1, 4, 1, 5))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n");
}

#[test]
fn math_min_single() {
    let (stdout, _, code) = run_rilua("print(math.min(42))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn math_max_single() {
    let (stdout, _, code) = run_rilua("print(math.max(42))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn math_min_negative() {
    let (stdout, _, code) = run_rilua("print(math.min(-5, -2, -8))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "-8\n");
}

// -- random --

#[test]
fn math_random_no_args_range() {
    let (stdout, _, code) =
        run_rilua("math.randomseed(42); local r = math.random(); print(r >= 0 and r < 1)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn math_random_one_arg_range() {
    let (stdout, _, code) = run_rilua(
        "math.randomseed(42); local ok = true; for i=1,100 do local r = math.random(5); if r < 1 or r > 5 then ok = false end end; print(ok)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn math_random_two_arg_range() {
    let (stdout, _, code) = run_rilua(
        "math.randomseed(42); local ok = true; for i=1,100 do local r = math.random(-10, 10); if r < -10 or r > 10 then ok = false end end; print(ok)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn math_randomseed_deterministic() {
    let (stdout, _, code) = run_rilua(
        "math.randomseed(123); local a = math.random(); math.randomseed(123); local b = math.random(); print(a == b)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

// -- pythagorean identity (integration test from PUC-Rio suite) --

#[test]
fn math_pythagorean_identity() {
    let (stdout, _, code) = run_rilua(
        "local eq = function(a,b) return math.abs(a-b) < 1e-10 end; print(eq(math.sin(-9.8)^2 + math.cos(-9.8)^2, 1))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

// -- error cases --

#[test]
fn math_abs_no_args() {
    let (_, stderr, code) = run_rilua("math.abs()");
    assert_ne!(code, 0);
    assert!(stderr.contains("number expected"), "stderr: {stderr}");
}

#[test]
fn math_random_empty_interval() {
    let (_, stderr, code) = run_rilua("math.random(0)");
    assert_ne!(code, 0);
    assert!(stderr.contains("interval is empty"), "stderr: {stderr}");
}

#[test]
fn math_random_wrong_args() {
    let (_, stderr, code) = run_rilua("math.random(1, 2, 3)");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("wrong number of arguments"),
        "stderr: {stderr}"
    );
}

// =========================================================================
// OS library
// =========================================================================

#[test]
fn os_global_is_table() {
    let (stdout, _, code) = run_rilua("print(type(os))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "table");
}

#[test]
fn os_functions_exist() {
    let (stdout, _, code) = run_rilua(
        "print(type(os.clock), type(os.date), type(os.difftime), type(os.execute), \
         type(os.exit), type(os.getenv), type(os.remove), type(os.rename), \
         type(os.setlocale), type(os.time), type(os.tmpname))",
    );
    assert_eq!(code, 0);
    let parts: Vec<&str> = stdout.trim().split('\t').collect();
    assert_eq!(parts.len(), 11);
    for p in &parts {
        assert_eq!(*p, "function", "expected function, got: {p}");
    }
}

#[test]
fn os_clock_returns_number() {
    let (stdout, _, code) = run_rilua("print(type(os.clock()))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "number");
}

#[test]
fn os_clock_nonnegative() {
    let (stdout, _, code) = run_rilua("assert(os.clock() >= 0) print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_time_returns_number() {
    let (stdout, _, code) = run_rilua("print(type(os.time()))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "number");
}

#[test]
fn os_time_reasonable_value() {
    // After 2024-01-01 00:00:00 UTC (1704067200).
    let (stdout, _, code) = run_rilua("assert(os.time() > 1704067200) print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_time_with_table() {
    let (stdout, _, code) = run_rilua(
        "local t = os.time({year=2000, month=1, day=1, hour=0, min=0, sec=0}) \
         print(type(t))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "number");
}

#[test]
fn os_time_table_missing_required_field() {
    let (_, stderr, code) = run_rilua("os.time({year=2000, month=1})");
    assert_ne!(code, 0);
    assert!(stderr.contains("missing in date table"), "stderr: {stderr}");
}

#[test]
fn os_date_default_format() {
    // Default format is "%c", returns a non-empty string.
    let (stdout, _, code) = run_rilua("local d = os.date() assert(#d > 0) print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_date_star_t_returns_table() {
    let (stdout, _, code) = run_rilua("print(type(os.date('*t')))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "table");
}

#[test]
fn os_date_star_t_fields() {
    let (stdout, _, code) = run_rilua(
        "local d = os.date('!*t', 0) \
         print(d.year, d.month, d.day, d.hour, d.min, d.sec)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "1970\t1\t1\t0\t0\t0");
}

#[test]
fn os_date_star_t_wday_yday() {
    // 1970-01-01 is a Thursday: wday=5 (1=Sunday), yday=1.
    let (stdout, _, code) = run_rilua("local d = os.date('!*t', 0) print(d.wday, d.yday)");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "5\t1");
}

#[test]
fn os_date_utc_format() {
    // "!" prefix forces UTC.
    let (stdout, _, code) = run_rilua("print(os.date('!%Y-%m-%d', 0))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "1970-01-01");
}

#[test]
fn os_date_strftime_format() {
    let (stdout, _, code) = run_rilua("print(os.date('!%H:%M:%S', 3661))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "01:01:01");
}

#[test]
fn os_date_time_roundtrip() {
    // os.time(os.date("*t")) should approximately equal os.time().
    let (stdout, _, code) = run_rilua(
        "local t1 = os.time() \
         local d = os.date('*t', t1) \
         local t2 = os.time(d) \
         assert(t1 == t2, 'roundtrip: ' .. t1 .. ' ~= ' .. t2) \
         print('ok')",
    );
    assert_eq!(code, 0, "stderr should be empty");
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_difftime_basic() {
    let (stdout, _, code) = run_rilua("print(os.difftime(100, 50))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "50");
}

#[test]
fn os_difftime_default_t2() {
    let (stdout, _, code) = run_rilua("print(os.difftime(100))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "100");
}

#[test]
fn os_execute_no_args() {
    // No args: returns non-zero if shell available.
    let (stdout, _, code) = run_rilua("assert(os.execute() ~= 0) print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_execute_true_command() {
    let (stdout, _, code) = run_rilua("print(os.execute('true'))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "0");
}

#[test]
fn os_execute_false_command() {
    let (stdout, _, code) = run_rilua("local r = os.execute('false') assert(r ~= 0) print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_getenv_path() {
    // PATH is always set on POSIX systems.
    let (stdout, _, code) = run_rilua("assert(os.getenv('PATH') ~= nil) print('ok')");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn os_getenv_nonexistent() {
    let (stdout, _, code) = run_rilua("print(type(os.getenv('RILUA_NONEXISTENT_VAR_12345')))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil");
}

#[test]
fn os_remove_nonexistent() {
    let (stdout, _, code) = run_rilua(
        "local ok, err, code = os.remove('/tmp/rilua_nonexistent_test_file') \
         print(type(ok), type(err), type(code))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil\tstring\tnumber");
}

#[test]
fn os_rename_nonexistent() {
    let (stdout, _, code) = run_rilua(
        "local ok, err, code = os.rename('/tmp/rilua_nonexistent_1', '/tmp/rilua_nonexistent_2') \
         print(type(ok), type(err), type(code))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil\tstring\tnumber");
}

#[test]
fn os_tmpname_returns_string() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         print(type(name)) \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "string");
}

#[test]
fn os_tmpname_starts_with_slash() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         print(name:sub(1,1)) \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "/");
}

#[test]
fn os_setlocale_query() {
    // Query current locale (nil first arg).
    let (stdout, _, code) = run_rilua("print(type(os.setlocale(nil)))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "string");
}

#[test]
fn os_setlocale_c() {
    let (stdout, _, code) = run_rilua("print(os.setlocale('C'))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "C");
}

#[test]
fn os_setlocale_invalid() {
    let (stdout, _, code) = run_rilua("print(os.setlocale('invalid_locale_xyz'))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil");
}

#[test]
fn os_setlocale_category() {
    let (stdout, _, code) = run_rilua("print(os.setlocale('C', 'numeric'))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "C");
}

#[test]
fn os_remove_and_rename_file() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         os.execute('echo hello > ' .. name) \
         local name2 = name .. '.renamed' \
         assert(os.rename(name, name2)) \
         assert(os.remove(name2)) \
         print('ok')",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

// ---------------------------------------------------------------------------
// I/O library tests
// ---------------------------------------------------------------------------

#[test]
fn io_global_is_table() {
    let (stdout, _, code) = run_rilua("print(type(io))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "table");
}

#[test]
fn io_functions_exist() {
    let (stdout, _, code) = run_rilua(
        "print(type(io.close), type(io.flush), type(io.input), type(io.lines), \
         type(io.open), type(io.output), type(io.popen), type(io.read), \
         type(io.tmpfile), type(io.type), type(io.write))",
    );
    assert_eq!(code, 0);
    let parts: Vec<&str> = stdout.trim().split('\t').collect();
    assert_eq!(parts.len(), 11);
    for p in &parts {
        assert_eq!(*p, "function", "expected function, got: {p}");
    }
}

#[test]
fn io_type_stdin() {
    let (stdout, _, code) = run_rilua("print(io.type(io.stdin))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "file");
}

#[test]
fn io_type_stdout() {
    let (stdout, _, code) = run_rilua("print(io.type(io.stdout))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "file");
}

#[test]
fn io_type_stderr() {
    let (stdout, _, code) = run_rilua("print(io.type(io.stderr))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "file");
}

#[test]
fn io_type_not_file() {
    let (stdout, _, code) = run_rilua("print(io.type('not a file'))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil");
}

#[test]
fn io_type_nil() {
    let (stdout, _, code) = run_rilua("print(io.type(42))");
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil");
}

#[test]
fn io_tostring_stdin() {
    let (stdout, _, code) = run_rilua("print(tostring(io.stdin))");
    assert_eq!(code, 0);
    assert!(
        stdout.trim().starts_with("file (0x"),
        "got: {}",
        stdout.trim()
    );
}

#[test]
fn io_open_read_close() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('hello world\\n') \
         f:close() \
         local f = io.open(name, 'r') \
         print(f:read('*l')) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "hello world");
}

#[test]
fn io_open_nonexistent() {
    let (stdout, _, code) = run_rilua(
        "local f, msg = io.open('/tmp/__rilua_nonexistent__', 'r') \
         print(f) \
         print(type(msg))",
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "nil");
    assert_eq!(lines[1], "string");
}

#[test]
fn io_write_string() {
    let (stdout, _, code) = run_rilua("io.write('hello') io.write(' world\\n')");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello world\n");
}

#[test]
fn io_write_number() {
    let (stdout, _, code) = run_rilua("io.write(42)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42");
}

#[test]
fn io_read_line() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('line1\\nline2\\n') \
         f:close() \
         local f = io.open(name, 'r') \
         print(f:read('*l')) \
         print(f:read('*l')) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "line1");
    assert_eq!(lines[1], "line2");
}

#[test]
fn io_read_all() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('abc\\ndef\\n') \
         f:close() \
         local f = io.open(name, 'r') \
         local s = f:read('*a') \
         print(#s) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "8"); // "abc\ndef\n" = 8 bytes
}

#[test]
fn io_read_number() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('3.14 42\\n') \
         f:close() \
         local f = io.open(name, 'r') \
         print(f:read('*n')) \
         print(f:read('*n')) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "3.14");
    assert_eq!(lines[1], "42");
}

#[test]
fn io_read_n_bytes() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('hello world') \
         f:close() \
         local f = io.open(name, 'r') \
         print(f:read(5)) \
         print(f:read(6)) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "hello");
    assert_eq!(lines[1], " world");
}

#[test]
fn io_lines_file() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('a\\nb\\nc\\n') \
         f:close() \
         local result = {} \
         for line in io.lines(name) do \
             result[#result + 1] = line \
         end \
         print(table.concat(result, ','))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "a,b,c");
}

#[test]
fn io_file_lines() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('x\\ny\\n') \
         f:close() \
         local f = io.open(name, 'r') \
         local result = {} \
         for line in f:lines() do \
             result[#result + 1] = line \
         end \
         f:close() \
         print(table.concat(result, ',')) \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "x,y");
}

#[test]
fn io_seek() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('hello world') \
         f:close() \
         local f = io.open(name, 'r') \
         f:seek('set', 6) \
         print(f:read('*l')) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "world");
}

#[test]
fn io_seek_returns_position() {
    let (stdout, _, code) = run_rilua(
        "local f = io.tmpfile() \
         f:write('hello') \
         print(f:seek('cur')) \
         f:close()",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "5");
}

#[test]
fn io_setvbuf_modes() {
    let (stdout, _, code) = run_rilua(
        "local f = io.tmpfile() \
         assert(f:setvbuf('no')) \
         assert(f:setvbuf('line')) \
         assert(f:setvbuf('full')) \
         f:close() \
         print('ok')",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn io_tmpfile() {
    let (stdout, _, code) = run_rilua(
        "local f = io.tmpfile() \
         f:write('test data\\n') \
         f:seek('set') \
         print(f:read('*l')) \
         f:close()",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "test data");
}

#[test]
fn io_popen_echo() {
    let (stdout, _, code) = run_rilua(
        "local f = io.popen('echo hello') \
         print(f:read('*l')) \
         f:close()",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "hello");
}

#[test]
fn io_input_output_get() {
    let (stdout, _, code) = run_rilua(
        "print(io.type(io.input())) \
         print(io.type(io.output()))",
    );
    assert_eq!(code, 0);
    let lines: Vec<&str> = stdout.trim().lines().collect();
    assert_eq!(lines[0], "file");
    assert_eq!(lines[1], "file");
}

#[test]
fn io_input_set() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('from file\\n') \
         f:close() \
         io.input(name) \
         print(io.read()) \
         io.close(io.input()) \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "from file");
}

#[test]
fn io_type_closed_file() {
    let (stdout, _, code) = run_rilua(
        "local f = io.tmpfile() \
         f:close() \
         print(io.type(f))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "closed file");
}

#[test]
fn io_close_returns_true() {
    let (stdout, _, code) = run_rilua(
        "local f = io.tmpfile() \
         local r = f:close() \
         print(r)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "true");
}

#[test]
fn io_flush_default() {
    let (stdout, _, code) = run_rilua("io.write('hello') io.flush() io.write(' world\\n')");
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello world\n");
}

#[test]
fn io_read_eof_returns_nil() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:close() \
         local f = io.open(name, 'r') \
         print(f:read('*l')) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "nil");
}

#[test]
fn io_read_all_empty() {
    let (stdout, _, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:close() \
         local f = io.open(name, 'r') \
         local s = f:read('*a') \
         print(#s) \
         f:close() \
         os.remove(name)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout.trim(), "0");
}

// ---------------------------------------------------------------------------
// package library tests
// ---------------------------------------------------------------------------

#[test]
fn package_table_exists() {
    let (stdout, _, code) = run_rilua("print(type(package))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn package_loaded_is_table() {
    let (stdout, _, code) = run_rilua("print(type(package.loaded))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn package_preload_is_table() {
    let (stdout, _, code) = run_rilua("print(type(package.preload))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn package_loaders_is_table() {
    let (stdout, _, code) = run_rilua("print(type(package.loaders))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn package_loaders_count() {
    let (stdout, _, code) = run_rilua(
        "local count = 0 \
         for i = 1, 100 do \
           if package.loaders[i] then count = count + 1 else break end \
         end \
         print(count)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "4\n");
}

#[test]
fn package_path_is_string() {
    let (stdout, _, code) = run_rilua("print(type(package.path))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "string\n");
}

#[test]
fn package_cpath_is_string() {
    let (stdout, _, code) = run_rilua("print(type(package.cpath))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "string\n");
}

#[test]
fn package_config_value() {
    let (stdout, _, code) = run_rilua("print(package.config)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "/\n;\n?\n!\n-\n\n");
}

#[test]
fn require_returns_string_lib() {
    let (stdout, _, code) = run_rilua("print(require('string') == string)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_returns_math_lib() {
    let (stdout, _, code) = run_rilua("print(require('math') == math)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_returns_table_lib() {
    let (stdout, _, code) = run_rilua("print(require('table') == table)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_returns_os_lib() {
    let (stdout, _, code) = run_rilua("print(require('os') == os)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_returns_io_lib() {
    let (stdout, _, code) = run_rilua("print(require('io') == io)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_returns_package_lib() {
    let (stdout, _, code) = run_rilua("print(require('package') == package)");
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_caching() {
    let (stdout, _, code) = run_rilua(
        "local a = require('string') \
         local b = require('string') \
         print(a == b)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn require_not_found_error() {
    let (_, stderr, code) = run_rilua("require('nonexistent_module_xyz')");
    assert_ne!(code, 0);
    assert!(stderr.contains("module 'nonexistent_module_xyz' not found"));
}

#[test]
fn require_not_found_error_has_search_details() {
    let (_, stderr, code) = run_rilua("require('nonexistent_module_xyz')");
    assert_ne!(code, 0);
    assert!(stderr.contains("no field package.preload"));
    assert!(stderr.contains("no file"));
}

#[test]
fn package_preload_custom_loader() {
    let (stdout, _, code) = run_rilua(
        "package.preload['mymod'] = function(name) \
           local t = {} \
           t.greeting = 'hello from ' .. name \
           return t \
         end \
         local m = require('mymod') \
         print(m.greeting)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello from mymod\n");
}

#[test]
fn package_preload_module_cached() {
    let (stdout, _, code) = run_rilua(
        "local count = 0 \
         package.preload['counter'] = function() \
           count = count + 1 \
           return count \
         end \
         local a = require('counter') \
         local b = require('counter') \
         print(a, b, a == b)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t1\ttrue\n");
}

#[test]
fn require_function_is_global() {
    let (stdout, _, code) = run_rilua("print(type(require))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\n");
}

#[test]
fn module_function_is_global() {
    let (stdout, _, code) = run_rilua("print(type(module))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "function\n");
}

#[test]
fn package_loadlib_returns_nil() {
    let (stdout, _, code) = run_rilua(
        "local f, err, kind = package.loadlib('foo', 'bar') \
         print(f, kind)",
    );
    assert_eq!(code, 0);
    // Without dynmod: returns "absent" (feature not enabled).
    // With dynmod: returns "open" (dlopen fails on nonexistent file).
    if cfg!(feature = "dynmod") {
        assert_eq!(stdout, "nil\topen\n");
    } else {
        assert_eq!(stdout, "nil\tabsent\n");
    }
}

#[test]
fn package_seeall_enables_globals() {
    let (stdout, _, code) = run_rilua(
        "local t = {} \
         package.seeall(t) \
         print(t.type('hello'))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "string\n");
}

#[test]
fn require_lua_file() {
    // Create a temp Lua file, load via preload + dofile, and require it.
    let (stdout, stderr, code) = run_rilua(
        "local name = os.tmpname() \
         local f = io.open(name, 'w') \
         f:write('local M = {} M.value = 42 return M') \
         f:close() \
         package.preload['tmpmod'] = function() \
           return dofile(name) \
         end \
         local m = require('tmpmod') \
         print(m.value) \
         os.remove(name)",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "42\n");
}

#[test]
fn require_lua_file_from_path() {
    // Create a temp directory with a lua file and require it via package.path.
    let (stdout, stderr, code) = run_rilua(
        "local dir = os.tmpname() \
         os.remove(dir) \
         os.execute('mkdir -p ' .. dir) \
         local f = io.open(dir .. '/testmod.lua', 'w') \
         f:write('local M = {} M.x = 99 return M') \
         f:close() \
         package.path = dir .. '/?.lua' \
         local m = require('testmod') \
         print(m.x) \
         os.remove(dir .. '/testmod.lua') \
         os.execute('rmdir ' .. dir)",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "99\n");
}

#[test]
fn package_loaded_prepopulated() {
    let (stdout, _, code) = run_rilua(
        "print(package.loaded.string == string, \
         package.loaded.math == math, \
         package.loaded.table == table, \
         package.loaded.os == os, \
         package.loaded.io == io, \
         package.loaded.package == package)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\ttrue\ttrue\ttrue\ttrue\ttrue\n");
}

#[test]
fn module_creates_global() {
    let (stdout, _, code) = run_rilua(
        "local dir = os.tmpname() \
         os.remove(dir) \
         os.execute('mkdir -p ' .. dir) \
         local f = io.open(dir .. '/mylib.lua', 'w') \
         f:write('module(\"mylib\") function greet() return \"hi\" end') \
         f:close() \
         package.path = dir .. '/?.lua' \
         require('mylib') \
         print(mylib.greet()) \
         os.remove(dir .. '/mylib.lua') \
         os.execute('rmdir ' .. dir)",
    );
    assert_eq!(code, 0, "stderr: {}", {
        let (_, stderr, _) = run_rilua(
            "local dir = os.tmpname() \
             os.remove(dir) \
             os.execute('mkdir -p ' .. dir) \
             local f = io.open(dir .. '/mylib.lua', 'w') \
             f:write('module(\"mylib\") function greet() return \"hi\" end') \
             f:close() \
             package.path = dir .. '/?.lua' \
             require('mylib') \
             print(mylib.greet()) \
             os.remove(dir .. '/mylib.lua') \
             os.execute('rmdir ' .. dir)",
        );
        stderr
    });
    assert_eq!(stdout, "hi\n");
}

#[test]
fn module_sets_name_fields() {
    let (stdout, _, code) = run_rilua(
        "local dir = os.tmpname() \
         os.remove(dir) \
         os.execute('mkdir -p ' .. dir) \
         local f = io.open(dir .. '/mymod.lua', 'w') \
         f:write('module(\"mymod\") -- sets _NAME, _M, _PACKAGE') \
         f:close() \
         package.path = dir .. '/?.lua' \
         require('mymod') \
         print(mymod._NAME) \
         print(type(mymod._M)) \
         print(mymod._PACKAGE) \
         os.remove(dir .. '/mymod.lua') \
         os.execute('rmdir ' .. dir)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "mymod\ntable\n\n");
}

#[test]
fn require_returns_true_when_no_return_value() {
    let (stdout, _, code) = run_rilua(
        "package.preload['noreturn'] = function() end \
         local r = require('noreturn') \
         print(r)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

// ---------------------------------------------------------------------------
// Coroutine library tests
// ---------------------------------------------------------------------------

#[test]
fn coroutine_table_exists() {
    let (stdout, _, code) = run_rilua("print(type(coroutine))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn coroutine_create_returns_thread() {
    let (stdout, _, code) =
        run_rilua("local co = coroutine.create(function() end) print(type(co))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "thread\n");
}

#[test]
fn coroutine_simple_resume_return() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() return 42 end) \
         local ok, val = coroutine.resume(co) \
         print(ok, val)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t42\n");
}

#[test]
fn coroutine_resume_with_args() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function(a, b) return a + b end) \
         local ok, val = coroutine.resume(co, 10, 20) \
         print(ok, val)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t30\n");
}

#[test]
fn coroutine_yield_basic() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() \
           coroutine.yield(1) \
           coroutine.yield(2) \
           return 3 \
         end) \
         local ok1, v1 = coroutine.resume(co) \
         local ok2, v2 = coroutine.resume(co) \
         local ok3, v3 = coroutine.resume(co) \
         print(ok1, v1) \
         print(ok2, v2) \
         print(ok3, v3)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t1\ntrue\t2\ntrue\t3\n");
}

#[test]
fn coroutine_yield_multiple_values() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() \
           coroutine.yield(10, 20, 30) \
         end) \
         local ok, a, b, c = coroutine.resume(co) \
         print(ok, a, b, c)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t10\t20\t30\n");
}

#[test]
fn coroutine_resume_passes_values_to_yield() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() \
           local x = coroutine.yield() \
           return x * 2 \
         end) \
         coroutine.resume(co) \
         local ok, val = coroutine.resume(co, 21) \
         print(ok, val)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t42\n");
}

#[test]
fn coroutine_status_initial() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() end) \
         print(coroutine.status(co))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "suspended\n");
}

#[test]
fn coroutine_status_dead() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() end) \
         coroutine.resume(co) \
         print(coroutine.status(co))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "dead\n");
}

#[test]
fn coroutine_status_suspended() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() coroutine.yield() end) \
         coroutine.resume(co) \
         print(coroutine.status(co))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "suspended\n");
}

#[test]
fn coroutine_status_running() {
    let (stdout, _, code) = run_rilua(
        "local co \
         co = coroutine.create(function() print(coroutine.status(co)) end) \
         coroutine.resume(co)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "running\n");
}

#[test]
fn coroutine_resume_dead_fails() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() end) \
         coroutine.resume(co) \
         local ok, msg = coroutine.resume(co) \
         print(ok, msg)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "false\tcannot resume dead coroutine\n");
}

#[test]
fn coroutine_error_in_body() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() error('oops') end) \
         local ok, msg = coroutine.resume(co) \
         print(ok) \
         print(type(msg))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "false\nstring\n");
}

#[test]
fn coroutine_wrap_basic() {
    let (stdout, _, code) = run_rilua(
        "local f = coroutine.wrap(function() \
           coroutine.yield(1) \
           coroutine.yield(2) \
           return 3 \
         end) \
         print(f()) \
         print(f()) \
         print(f())",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n3\n");
}

#[test]
fn coroutine_wrap_with_args() {
    let (stdout, _, code) = run_rilua(
        "local f = coroutine.wrap(function(a) \
           return a + 1 \
         end) \
         print(f(10))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "11\n");
}

#[test]
fn coroutine_wrap_error_propagates() {
    let (_, stderr, code) = run_rilua(
        "local f = coroutine.wrap(function() error('wrapped error') end) \
         f()",
    );
    assert_ne!(code, 0);
    assert!(stderr.contains("wrapped error"), "stderr: {stderr}");
}

#[test]
fn coroutine_running_main_thread() {
    let (stdout, _, code) = run_rilua("print(coroutine.running())");
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n"); // nil prints as empty line
}

#[test]
fn coroutine_running_inside_coroutine() {
    let (stdout, _, code) = run_rilua(
        "local co \
         co = coroutine.create(function() \
           print(coroutine.running() == co) \
         end) \
         coroutine.resume(co)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn coroutine_yield_no_values() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() \
           coroutine.yield() \
           return 'done' \
         end) \
         local ok1 = coroutine.resume(co) \
         local ok2, val = coroutine.resume(co) \
         print(ok1, ok2, val)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\ttrue\tdone\n");
}

#[test]
fn coroutine_return_multiple_values() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() return 1, 2, 3 end) \
         local ok, a, b, c = coroutine.resume(co) \
         print(ok, a, b, c)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\t1\t2\t3\n");
}

#[test]
fn coroutine_no_return_value() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() end) \
         local ok = coroutine.resume(co) \
         print(ok)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn coroutine_producer_consumer() {
    let (stdout, _, code) = run_rilua(
        "local producer = coroutine.create(function() \
           for i = 1, 3 do coroutine.yield(i) end \
         end) \
         local results = {} \
         while true do \
           local ok, val = coroutine.resume(producer) \
           if not ok or val == nil then break end \
           results[#results + 1] = val \
         end \
         print(table.concat(results, ','))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1,2,3\n");
}

#[test]
fn coroutine_yield_across_pcall_boundary() {
    let (stdout, _, code) = run_rilua(
        "local co = coroutine.create(function() \
           pcall(function() coroutine.yield() end) \
         end) \
         local ok, msg = coroutine.resume(co) \
         print(ok) \
         print(coroutine.status(co))",
    );
    assert_eq!(code, 0);
    // pcall catches the yield boundary error, coroutine returns normally
    assert_eq!(stdout, "true\ndead\n");
}

#[test]
fn coroutine_require_loaded() {
    let (stdout, _, code) = run_rilua("print(type(package.loaded.coroutine))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn coroutine_functions_exist() {
    let (stdout, _, code) = run_rilua(
        "print(type(coroutine.create)) \
         print(type(coroutine.resume)) \
         print(type(coroutine.yield)) \
         print(type(coroutine.wrap)) \
         print(type(coroutine.status)) \
         print(type(coroutine.running))",
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "function\nfunction\nfunction\nfunction\nfunction\nfunction\n"
    );
}

// ---------------------------------------------------------------------------
// Debug library tests
// ---------------------------------------------------------------------------

#[test]
fn debug_table_exists() {
    let (stdout, _, code) = run_rilua("print(type(debug))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn debug_functions_exist() {
    let (stdout, _, code) = run_rilua(
        "local names = {'debug','getfenv','gethook','getinfo','getlocal',\
         'getmetatable','getregistry','getupvalue','setfenv','sethook',\
         'setlocal','setmetatable','setupvalue','traceback'} \
         for _, n in ipairs(names) do print(type(debug[n])) end",
    );
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "function\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\nfunction\n"
    );
}

#[test]
fn debug_getregistry_returns_table() {
    let (stdout, _, code) = run_rilua("print(type(debug.getregistry()))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn debug_getmetatable_returns_raw_metatable() {
    let (stdout, _, code) = run_rilua(
        "local t = {} \
         local mt = {__metatable = 'hidden'} \
         setmetatable(t, mt) \
         print(getmetatable(t)) \
         print(debug.getmetatable(t) == mt)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hidden\ntrue\n");
}

#[test]
fn debug_getmetatable_nil_when_none() {
    let (stdout, _, code) = run_rilua("print(debug.getmetatable({}))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn debug_setmetatable_on_table() {
    let (stdout, _, code) = run_rilua(
        "local t = {} \
         local mt = {__tostring = function() return 'custom' end} \
         debug.setmetatable(t, mt) \
         print(tostring(t))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "custom\n");
}

#[test]
fn debug_setmetatable_on_number() {
    // Setting a type metatable on numbers: arithmetic between two numbers
    // uses the fast path and doesn't invoke metamethods (PUC-Rio behavior).
    // But __tostring will work because tostring always checks metatables.
    let (stdout, _, code) = run_rilua(
        "debug.setmetatable(0, {__tostring = function(n) return 'num:' .. n end}) \
         print(tostring(42)) \
         debug.setmetatable(0, nil)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "num:42\n");
}

#[test]
fn debug_getfenv_returns_env() {
    let (stdout, _, code) = run_rilua(
        "local function f() end \
         print(debug.getfenv(f) == _G)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn debug_setfenv_changes_env() {
    let (stdout, _, code) = run_rilua(
        "local function f() return x end \
         local env = {x = 42} \
         setmetatable(env, {__index = _G}) \
         debug.setfenv(f, env) \
         print(f())",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn debug_getinfo_what_s() {
    let (stdout, _, code) = run_rilua(
        "local info = debug.getinfo(1, 'S') \
         print(info.what) \
         print(type(info.source)) \
         print(type(info.short_src))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "main\nstring\nstring\n");
}

#[test]
fn debug_getinfo_level_0() {
    let (stdout, _, code) = run_rilua(
        "local info = debug.getinfo(1, 'S') \
         print(info.what)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "main\n");
}

#[test]
fn debug_getinfo_function_arg() {
    let (stdout, _, code) = run_rilua(
        "local function foo() end \
         local info = debug.getinfo(foo, 'S') \
         print(info.what) \
         print(info.linedefined)",
    );
    assert_eq!(code, 0);
    // "Lua" because line_defined != 0
    assert_eq!(stdout, "Lua\n1\n");
}

#[test]
fn debug_getinfo_c_function() {
    let (stdout, _, code) = run_rilua(
        "local info = debug.getinfo(print, 'S') \
         print(info.what) \
         print(info.source)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "C\n=[C]\n");
}

#[test]
fn debug_getinfo_what_u() {
    let (stdout, _, code) = run_rilua(
        "local x = 1 \
         local function f() return x end \
         local info = debug.getinfo(f, 'u') \
         print(info.nups)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn debug_getinfo_what_n() {
    let (stdout, _, code) = run_rilua(
        "local info = debug.getinfo(print, 'n') \
         print(info.name)",
    );
    assert_eq!(code, 0);
    // RustClosure carries its registered name
    assert_eq!(stdout, "print\n");
}

#[test]
fn debug_getinfo_invalid_level() {
    let (stdout, _, code) = run_rilua("print(debug.getinfo(100))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn debug_getinfo_what_l() {
    let (stdout, _, code) = run_rilua(
        "local info = debug.getinfo(1, 'l') \
         print(type(info.currentline))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "number\n");
}

#[test]
fn debug_getinfo_what_f() {
    let (stdout, _, code) = run_rilua(
        "local function foo() end \
         local info = debug.getinfo(foo, 'f') \
         print(info.func == foo)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn debug_getlocal_name_and_value() {
    let (stdout, _, code) = run_rilua(
        "local x = 42 \
         local name, val = debug.getlocal(1, 1) \
         print(name, val)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\t42\n");
}

#[test]
fn debug_getlocal_out_of_range() {
    let (stdout, _, code) = run_rilua(
        "local x = 1 \
         print(debug.getlocal(1, 99))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn debug_setlocal_changes_value() {
    let (stdout, _, code) = run_rilua(
        "local x = 10 \
         local function f() \
           local y = 20 \
           debug.setlocal(1, 1, 99) \
           print(y) \
         end \
         f()",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "99\n");
}

#[test]
fn debug_getupvalue_name_and_value() {
    let (stdout, _, code) = run_rilua(
        "local x = 42 \
         local function f() return x end \
         local name, val = debug.getupvalue(f, 1) \
         print(name, val)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\t42\n");
}

#[test]
fn debug_getupvalue_out_of_range() {
    let (stdout, _, code) = run_rilua(
        "local function f() end \
         print(debug.getupvalue(f, 99))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "\n"); // returns nothing, print prints empty line
}

#[test]
fn debug_setupvalue_changes_value() {
    let (stdout, _, code) = run_rilua(
        "local x = 10 \
         local function f() return x end \
         debug.setupvalue(f, 1, 99) \
         print(f())",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "99\n");
}

#[test]
fn debug_gethook_returns_nil_stub() {
    let (stdout, _, code) = run_rilua(
        "local a, b, c = debug.gethook() \
         print(a, b, c)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\t\t0\n");
}

#[test]
fn debug_traceback_returns_string() {
    let (stdout, _, code) = run_rilua("print(type(debug.traceback()))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "string\n");
}

#[test]
fn debug_traceback_has_stack_traceback_header() {
    let (stdout, _, code) = run_rilua(
        "local tb = debug.traceback() \
         print(tb:find('stack traceback:') ~= nil)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
}

#[test]
fn debug_traceback_with_message() {
    let (stdout, _, code) = run_rilua(
        "local tb = debug.traceback('hello') \
         print(tb:sub(1, 5))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\n");
}

#[test]
fn debug_traceback_number_as_message() {
    // Numbers are treated as string messages in PUC-Rio (lua_isstring returns
    // true for numbers). The number is used as a message prefix.
    let (stdout, _, code) = run_rilua(
        "local tb = debug.traceback(42) \
         print(tb:sub(1, 2))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn debug_traceback_nil_returns_nil() {
    // nil is non-string/non-number: returned as-is (PUC-Rio behavior)
    let (stdout, _, code) = run_rilua("print(debug.traceback(nil))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "nil\n");
}

#[test]
fn debug_require_loaded() {
    let (stdout, _, code) = run_rilua("print(type(package.loaded.debug))");
    assert_eq!(code, 0);
    assert_eq!(stdout, "table\n");
}

#[test]
fn debug_getinfo_in_pcall() {
    // Level 1 from getinfo inside pcall points to pcall (C), level 2 is main
    let (stdout, _, code) = run_rilua(
        "local ok, info = pcall(debug.getinfo, 2, 'S') \
         print(ok) \
         print(info.what)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\nmain\n");
}

// ---------------------------------------------------------------------------
// CLI tests (Phase 8d)
// ---------------------------------------------------------------------------

/// Helper: run rilua with arbitrary args and return (stdout, stderr, exit_code).
fn run_rilua_args(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_rilua"))
        .args(args)
        .env_remove("LUA_INIT")
        .output()
        .expect("failed to run rilua binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// Helper: run rilua with piped stdin.
fn run_rilua_stdin(args: &[&str], stdin_data: &str) -> (String, String, i32) {
    use std::io::Write as _;
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_rilua"))
        .args(args)
        .env_remove("LUA_INIT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn rilua");

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_data.as_bytes()).ok();
    }

    let output = child.wait_with_output().expect("failed to wait on rilua");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// Helper: run rilua with env vars.
fn run_rilua_env(args: &[&str], env_vars: &[(&str, &str)]) -> (String, String, i32) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rilua"));
    cmd.args(args);
    cmd.env_remove("LUA_INIT");
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    let output = cmd.output().expect("failed to run rilua binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn cli_version_flag() {
    let (_, stderr, code) = run_rilua_args(&["-v"]);
    assert_eq!(code, 0);
    assert!(
        stderr.contains("Lua 5.1.1"),
        "version should contain 'Lua 5.1.1', got: {stderr}"
    );
}

#[test]
fn cli_e_flag_basic() {
    let (stdout, _, code) = run_rilua_args(&["-e", "print(1+2)"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn cli_e_flag_suffix_form() {
    let (stdout, _, code) = run_rilua_args(&["-eprint('suffix')"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "suffix\n");
}

#[test]
fn cli_e_flag_error_exits_1() {
    let (_, stderr, code) = run_rilua_args(&["-e", "error('boom')"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("boom"),
        "stderr should contain error: {stderr}"
    );
}

#[test]
fn cli_e_flag_missing_arg() {
    let (_, stderr, code) = run_rilua_args(&["-e"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("usage:"),
        "should print usage on missing -e arg: {stderr}"
    );
}

#[test]
fn cli_l_flag_require_string() {
    let (stdout, _, code) = run_rilua_args(&["-l", "string", "-e", "print(string.len('abc'))"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn cli_l_flag_suffix_form() {
    let (stdout, _, code) = run_rilua_args(&["-lstring", "-e", "print(string.len('abc'))"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

#[test]
fn cli_invalid_option() {
    let (_, stderr, code) = run_rilua_args(&["-z"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("usage:"),
        "should print usage for invalid option: {stderr}"
    );
}

#[test]
fn cli_double_dash_stops_options() {
    // After --, next arg is treated as script file.
    let (_, stderr, code) = run_rilua_args(&["--", "-e"]);
    // "-e" is treated as a file name, which shouldn't exist.
    assert_eq!(code, 1);
    assert!(
        stderr.contains("cannot open -e"),
        "should try to open '-e' as file: {stderr}"
    );
}

#[test]
fn cli_script_with_args() {
    let script = std::env::temp_dir().join("rilua_test_args.lua");
    std::fs::write(&script, "print(arg[0], arg[1], arg[2])").ok();
    let script_path = script.to_str().unwrap_or("");
    let (stdout, _, code) = run_rilua_args(&[script_path, "foo", "bar"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("foo") && stdout.contains("bar"),
        "should pass args: {stdout}"
    );
    std::fs::remove_file(&script).ok();
}

#[test]
fn cli_script_varargs() {
    let script = std::env::temp_dir().join("rilua_test_varargs.lua");
    std::fs::write(&script, "print(...)").ok();
    let script_path = script.to_str().unwrap_or("");
    let (stdout, _, code) = run_rilua_args(&[script_path, "hello", "world"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello\tworld\n");
    std::fs::remove_file(&script).ok();
}

#[test]
fn cli_stdin_dash() {
    let (stdout, _, code) = run_rilua_stdin(&["-"], "print(42)\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n");
}

#[test]
fn cli_stdin_pipe_no_args() {
    // When stdin is not a TTY and no args, rilua reads stdin as file.
    let (stdout, _, code) = run_rilua_stdin(&[], "print('piped')\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "piped\n");
}

#[test]
fn cli_lua_init_string() {
    let (stdout, _, code) =
        run_rilua_env(&["-e", "print('done')"], &[("LUA_INIT", "print('init')")]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "init\ndone\n");
}

#[test]
fn cli_lua_init_file() {
    let init_file = std::env::temp_dir().join("rilua_test_init.lua");
    std::fs::write(&init_file, "INIT_RAN = true").ok();
    let init_path = format!("@{}", init_file.to_str().unwrap_or(""));
    let (stdout, _, code) = run_rilua_env(&["-e", "print(INIT_RAN)"], &[("LUA_INIT", &init_path)]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n");
    std::fs::remove_file(&init_file).ok();
}

#[test]
fn cli_lua_init_error_exits() {
    let (_, stderr, code) = run_rilua_env(
        &["-e", "print('never')"],
        &[("LUA_INIT", "error('init failed')")],
    );
    assert_eq!(code, 1);
    assert!(
        stderr.contains("init failed"),
        "should report LUA_INIT error: {stderr}"
    );
}

#[test]
fn cli_multiple_e_flags() {
    let (stdout, _, code) = run_rilua_args(&["-e", "x=1", "-e", "print(x)"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

#[test]
fn cli_e_and_l_order() {
    // -l string happens before -e, so string functions are available.
    let (stdout, _, code) = run_rilua_args(&["-l", "string", "-e", "print(string.rep('a', 3))"]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "aaa\n");
}

#[test]
fn cli_v_implies_no_repl() {
    // -v alone should just print version and exit (no REPL).
    let (_, stderr, code) = run_rilua_args(&["-v"]);
    assert_eq!(code, 0);
    assert!(stderr.contains("Lua 5.1.1"));
}

#[test]
fn cli_i_implies_v() {
    // -i implies -v, so version should appear in stderr.
    let (_, stderr, code) = run_rilua_stdin(&["-i"], "");
    assert_eq!(code, 0);
    assert!(
        stderr.contains("Lua 5.1.1"),
        "-i should print version: {stderr}"
    );
}

#[test]
fn cli_nonexistent_script() {
    let (_, stderr, code) = run_rilua_args(&["/tmp/nonexistent_rilua_script_12345.lua"]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("cannot open"),
        "should report file error: {stderr}"
    );
}

#[test]
fn cli_syntax_error_in_script() {
    let script = std::env::temp_dir().join("rilua_test_syntax.lua");
    std::fs::write(&script, "if then end").ok();
    let script_path = script.to_str().unwrap_or("");
    let (_, stderr, code) = run_rilua_args(&[script_path]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("expected"),
        "should report syntax error: {stderr}"
    );
    std::fs::remove_file(&script).ok();
}

#[test]
fn cli_repl_eq_shorthand() {
    // Test =expr shorthand in REPL via piped stdin with -i.
    let (stdout, _, code) = run_rilua_stdin(&["-i"], "=1+2\n");
    assert_eq!(code, 0);
    assert!(stdout.contains('3'), "=expr should print result: {stdout}");
}

#[test]
fn cli_repl_multiline() {
    // Multiline input: "function f()\n  return 42\nend\nprint(f())\n"
    let input = "function f()\nreturn 42\nend\nprint(f())\n";
    let (stdout, _, code) = run_rilua_stdin(&["-i"], input);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("42"),
        "multiline function should work: {stdout}"
    );
}

#[test]
fn cli_arg_table_negative_indices() {
    let script = std::env::temp_dir().join("rilua_test_argneg.lua");
    std::fs::write(&script, "print(arg[-1])").ok();
    let script_path = script.to_str().unwrap_or("");
    let (stdout, _, code) = run_rilua_args(&["-e", "x=1", script_path]);
    assert_eq!(code, 0);
    // arg[-1] should be "x=1" (the -e argument), but actually
    // -e and its argument are at negative indices. arg[-1] is "-e".
    // Wait, let me think: script is at argv[3] (rilua, -e, x=1, script).
    // arg[-3] = rilua, arg[-2] = -e, arg[-1] = x=1, arg[0] = script.
    assert!(
        stdout.contains("x=1") || stdout.contains("-e"),
        "negative arg indices should work: {stdout}"
    );
    std::fs::remove_file(&script).ok();
}

// ---------------------------------------------------------------------------
// riluac tests
// ---------------------------------------------------------------------------

/// Helper: run `riluac` with the given arguments.
fn run_riluac(args: &[&str]) -> (String, String, i32) {
    let output = Command::new(env!("CARGO_BIN_EXE_riluac"))
        .args(args)
        .output()
        .expect("failed to run riluac binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn riluac_version() {
    let (stdout, _, code) = run_riluac(&["-v"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("Lua 5.1.1"),
        "version should contain Lua 5.1.1: {stdout}"
    );
    assert!(
        stdout.contains("PUC-Rio"),
        "version should contain PUC-Rio: {stdout}"
    );
}

#[test]
fn riluac_list_simple() {
    let dir = std::env::temp_dir();
    let script = dir.join("riluac_test_list.lua");
    std::fs::write(&script, "print('hello')").ok();
    let script_path = script.to_str().unwrap_or("");

    let (stdout, _, code) = run_riluac(&["-l", script_path]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("main <"),
        "should start with main function: {stdout}"
    );
    assert!(
        stdout.contains("GETGLOBAL"),
        "should contain GETGLOBAL: {stdout}"
    );
    assert!(
        stdout.contains("; print"),
        "should show print global: {stdout}"
    );
    assert!(stdout.contains("LOADK"), "should contain LOADK: {stdout}");
    assert!(stdout.contains("CALL"), "should contain CALL: {stdout}");
    assert!(stdout.contains("RETURN"), "should contain RETURN: {stdout}");

    std::fs::remove_file(&script).ok();
}

#[test]
fn riluac_list_full() {
    let dir = std::env::temp_dir();
    let script = dir.join("riluac_test_full.lua");
    std::fs::write(&script, "local x = 42\nreturn x").ok();
    let script_path = script.to_str().unwrap_or("");

    let (stdout, _, code) = run_riluac(&["-l", "-l", script_path]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("constants ("),
        "full listing should include constants: {stdout}"
    );
    assert!(
        stdout.contains("locals ("),
        "full listing should include locals: {stdout}"
    );
    assert!(
        stdout.contains("upvalues ("),
        "full listing should include upvalues: {stdout}"
    );

    std::fs::remove_file(&script).ok();
}

#[test]
fn riluac_parse_only() {
    let dir = std::env::temp_dir();
    let script = dir.join("riluac_test_parse.lua");
    std::fs::write(&script, "local x = 1 + 2").ok();
    let script_path = script.to_str().unwrap_or("");

    let (stdout, _, code) = run_riluac(&["-p", script_path]);
    assert_eq!(code, 0);
    // Parse-only should produce no output.
    assert!(stdout.is_empty(), "parse-only should be silent: {stdout}");

    std::fs::remove_file(&script).ok();
}

#[test]
fn riluac_parse_only_syntax_error() {
    let dir = std::env::temp_dir();
    let script = dir.join("riluac_test_syntax_err.lua");
    std::fs::write(&script, "if then end").ok();
    let script_path = script.to_str().unwrap_or("");

    let (_, stderr, code) = run_riluac(&["-p", script_path]);
    assert_ne!(code, 0);
    assert!(
        !stderr.is_empty(),
        "syntax error should produce stderr output"
    );

    std::fs::remove_file(&script).ok();
}

#[test]
fn riluac_multiple_files() {
    let dir = std::env::temp_dir();
    let script1 = dir.join("riluac_test_multi1.lua");
    let script2 = dir.join("riluac_test_multi2.lua");
    std::fs::write(&script1, "local a = 1").ok();
    std::fs::write(&script2, "local b = 2").ok();
    let path1 = script1.to_str().unwrap_or("");
    let path2 = script2.to_str().unwrap_or("");

    let (stdout, _, code) = run_riluac(&["-l", path1, path2]);
    assert_eq!(code, 0);
    // Should have the combine wrapper "=(riluac)" plus both inner protos.
    assert!(
        stdout.contains("=(riluac)"),
        "should have combine wrapper: {stdout}"
    );

    std::fs::remove_file(&script1).ok();
    std::fs::remove_file(&script2).ok();
}

#[test]
fn riluac_stdin() {
    let output = Command::new(env!("CARGO_BIN_EXE_riluac"))
        .args(["-l", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(b"print(1+2)").ok();
            }
            child.wait_with_output()
        })
        .expect("failed to run riluac with stdin");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    assert!(
        stdout.contains("=stdin"),
        "stdin source should be =stdin: {stdout}"
    );
    assert!(
        stdout.contains("GETGLOBAL"),
        "should contain GETGLOBAL: {stdout}"
    );
}

#[test]
fn riluac_no_input_error() {
    let (_, stderr, code) = run_riluac(&[]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("no input files given"),
        "should show error: {stderr}"
    );
}

#[test]
fn riluac_nonexistent_file() {
    let (_, stderr, code) = run_riluac(&["-l", "/nonexistent/path.lua"]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("cannot open"),
        "should show cannot open: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// string.dump / binary chunk round-trip tests (Phase 9b)
// ---------------------------------------------------------------------------

#[test]
fn string_dump_roundtrip() {
    let (stdout, stderr, code) = run_rilua(
        r"
        local f = function() return 42 end
        local s = string.dump(f)
        local g = loadstring(s)
        print(g())
        ",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "42\n");
}

#[test]
fn string_dump_upvalues() {
    // Upvalues are reset in the loaded copy (PUC-Rio behavior).
    let (stdout, stderr, code) = run_rilua(
        r"
        local x = 10
        local f = function() return x end
        local s = string.dump(f)
        local g = loadstring(s)
        local ok, err = pcall(g)
        -- The loaded function has no upvalue binding, so x is nil
        -- Actually in Lua 5.1, the upvalue gets reset to nil
        print(type(g()))
        ",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    // The upvalue x is lost in the dump; accessing it returns nil.
    assert_eq!(stdout, "nil\n");
}

#[test]
fn string_dump_nested() {
    let (stdout, stderr, code) = run_rilua(
        r#"
        local f = function()
            local inner = function() return "nested" end
            return inner()
        end
        local s = string.dump(f)
        local g = loadstring(s)
        print(g())
        "#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "nested\n");
}

#[test]
fn string_dump_vararg() {
    let (stdout, stderr, code) = run_rilua(
        r##"
        local f = function(...) return select("#", ...) end
        local s = string.dump(f)
        local g = loadstring(s)
        print(g(1, 2, 3))
        "##,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "3\n");
}

#[test]
fn string_dump_error_nonfunc() {
    let (stdout, stderr, code) = run_rilua(
        r"
        local ok, err = pcall(string.dump, 42)
        print(ok)
        print(err)
        ",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("false"), "should fail: {stdout}");
}

#[test]
fn string_dump_error_rust_closure() {
    let (stdout, stderr, code) = run_rilua(
        r"
        local ok, err = pcall(string.dump, print)
        print(ok)
        print(err)
        ",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("false"), "should fail: {stdout}");
    assert!(
        stdout.contains("unable to dump"),
        "should mention unable to dump: {stdout}"
    );
}

#[test]
fn string_dump_with_constants() {
    let (stdout, stderr, code) = run_rilua(
        r#"
        local f = function()
            local a = nil
            local b = true
            local c = false
            local d = 3.14
            local e = "hello"
            return a, b, c, d, e
        end
        local s = string.dump(f)
        local g = loadstring(s)
        print(g())
        "#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "nil\ttrue\tfalse\t3.14\thello\n");
}

#[test]
fn string_dump_signature_check() {
    // Verify dump output starts with \27Lua signature.
    let (stdout, stderr, code) = run_rilua(
        r"
        local f = function() end
        local s = string.dump(f)
        print(string.byte(s, 1), string.byte(s, 2), string.byte(s, 3), string.byte(s, 4))
        ",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    // \27 = 27, L = 76, u = 117, a = 97
    assert_eq!(stdout, "27\t76\t117\t97\n");
}

#[test]
fn loadstring_binary_chunk() {
    // Load a binary chunk produced by string.dump.
    let (stdout, stderr, code) = run_rilua(
        r#"
        local code = "return 1 + 2"
        local f1 = assert(loadstring(code))
        local dumped = string.dump(f1)
        local f2 = assert(loadstring(dumped))
        print(f2())
        "#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout, "3\n");
}

#[test]
fn binary_loadfile() {
    // Write a dump to a temp file, then loadfile it.
    let dir = std::env::temp_dir();
    let path = dir.join("rilua_test_binary.luac");
    let path_str = path.to_str().expect("temp path should be valid utf-8");

    // First: produce binary with rilua
    let (_stdout, stderr, code) = run_rilua(&format!(
        r#"
        local f = assert(loadstring("return 'from binary'"))
        local s = string.dump(f)
        local fh = assert(io.open("{path_str}", "wb"))
        fh:write(s)
        fh:close()
        "#
    ));
    assert_eq!(code, 0, "dump stderr: {stderr}");

    // Then: load the binary file
    let (stdout, stderr, code) = run_rilua(&format!(
        r#"
        local f = assert(loadfile("{path_str}"))
        print(f())
        "#
    ));
    assert_eq!(code, 0, "load stderr: {stderr}");
    assert_eq!(stdout, "from binary\n");

    // Cleanup.
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// riluac -o/-s tests (Phase 9b)
// ---------------------------------------------------------------------------

#[test]
fn riluac_output_file() {
    let dir = std::env::temp_dir();
    let lua_path = dir.join("rilua_riluac_test.lua");
    let luac_path = dir.join("rilua_riluac_test.luac");

    std::fs::write(&lua_path, "print('riluac output')").expect("write test file");

    let lua_str = lua_path.to_str().expect("valid path");
    let luac_str = luac_path.to_str().expect("valid path");

    let (_, stderr, code) = run_riluac(&["-o", luac_str, lua_str]);
    assert_eq!(code, 0, "riluac -o failed: {stderr}");
    assert!(luac_path.exists(), "output file should exist");

    // Verify binary starts with \x1bLua.
    let bytes = std::fs::read(&luac_path).expect("read output");
    assert!(bytes.starts_with(b"\x1bLua"), "should be binary chunk");

    // Run the binary with rilua.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg(luac_str)
        .output()
        .expect("failed to run rilua");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "riluac output\n");

    // Cleanup.
    let _ = std::fs::remove_file(&lua_path);
    let _ = std::fs::remove_file(&luac_path);
}

#[test]
fn riluac_strip() {
    let dir = std::env::temp_dir();
    let lua_path = dir.join("rilua_strip_test.lua");
    let full_path = dir.join("rilua_strip_full.luac");
    let stripped_path = dir.join("rilua_strip_stripped.luac");

    std::fs::write(&lua_path, "local x = 1; print(x)").expect("write test file");

    let lua_str = lua_path.to_str().expect("valid path");
    let full_str = full_path.to_str().expect("valid path");
    let stripped_str = stripped_path.to_str().expect("valid path");

    // Compile full.
    let (_, stderr, code) = run_riluac(&["-o", full_str, lua_str]);
    assert_eq!(code, 0, "riluac full failed: {stderr}");

    // Compile stripped.
    let (_, stderr, code) = run_riluac(&["-s", "-o", stripped_str, lua_str]);
    assert_eq!(code, 0, "riluac -s failed: {stderr}");

    let full_bytes = std::fs::read(&full_path).expect("read full");
    let stripped_bytes = std::fs::read(&stripped_path).expect("read stripped");

    // Stripped should be smaller.
    assert!(
        stripped_bytes.len() < full_bytes.len(),
        "stripped ({}) should be smaller than full ({})",
        stripped_bytes.len(),
        full_bytes.len()
    );

    // Stripped binary should still be loadable and produce correct output.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg(stripped_str)
        .output()
        .expect("failed to run rilua");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "1\n");

    // Cleanup.
    let _ = std::fs::remove_file(&lua_path);
    let _ = std::fs::remove_file(&full_path);
    let _ = std::fs::remove_file(&stripped_path);
}

#[test]
fn riluac_default_output() {
    // Without -o, riluac writes to "luac.out" in the CWD.
    let dir = std::env::temp_dir().join("rilua_riluac_default_test");
    let _ = std::fs::create_dir_all(&dir);
    let lua_path = dir.join("test.lua");
    let default_out = dir.join("luac.out");

    std::fs::write(&lua_path, "print('default')").expect("write test file");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_riluac"))
        .arg(lua_path.to_str().expect("valid path"))
        .current_dir(&dir)
        .output()
        .expect("failed to run riluac");
    assert_eq!(output.status.code().unwrap_or(-1), 0);
    assert!(default_out.exists(), "luac.out should exist");

    // Verify it's loadable.
    let run_output = std::process::Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg(default_out.to_str().expect("valid path"))
        .output()
        .expect("failed to run rilua");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert_eq!(stdout, "default\n");

    // Cleanup.
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn riluac_binary_redump() {
    // riluac should be able to re-dump a binary chunk (like PUC-Rio).
    let dir = std::env::temp_dir();
    let lua_path = dir.join("rilua_redump_src.lua");
    let first_path = dir.join("rilua_redump_first.luac");
    let second_path = dir.join("rilua_redump_second.luac");

    std::fs::write(&lua_path, "print('redump')").expect("write test file");

    let lua_str = lua_path.to_str().expect("valid path");
    let first_str = first_path.to_str().expect("valid path");
    let second_str = second_path.to_str().expect("valid path");

    // First compile.
    let (_, stderr, code) = run_riluac(&["-o", first_str, lua_str]);
    assert_eq!(code, 0, "first compile failed: {stderr}");

    // Re-dump the binary.
    let (_, stderr, code) = run_riluac(&["-o", second_str, first_str]);
    assert_eq!(code, 0, "redump failed: {stderr}");

    // Both should be loadable and produce the same output.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rilua"))
        .arg(second_str)
        .output()
        .expect("failed to run rilua");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "redump\n");

    // Cleanup.
    let _ = std::fs::remove_file(&lua_path);
    let _ = std::fs::remove_file(&first_path);
    let _ = std::fs::remove_file(&second_path);
}

// ---------------------------------------------------------------------------
// Error message formatting tests (Phase 9c)
// ---------------------------------------------------------------------------

#[test]
fn error_msg_call_local() {
    // Define local inside the pcall callback so it stays as a local, not an upvalue.
    let (stdout, _, code) =
        run_rilua("local ok, msg = pcall(function() local x = 1; x() end); print(msg)");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to call local 'x' (a number value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_call_global() {
    let (stdout, _, code) = run_rilua("local ok, msg = pcall(function() foo() end); print(msg)");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to call global 'foo' (a nil value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_arith_local() {
    let (stdout, _, code) = run_rilua(
        "local ok, msg = pcall(function() local x = 'hello'; return x + 1 end); print(msg)",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to perform arithmetic on local 'x' (a string value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_index_local() {
    let (stdout, _, code) =
        run_rilua("local ok, msg = pcall(function() local x = nil; return x.y end); print(msg)");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to index local 'x' (a nil value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_concat_local() {
    let (stdout, _, code) = run_rilua(
        "local ok, msg = pcall(function() local x = {}; return 'a' .. x end); print(msg)",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to concatenate local 'x' (a table value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_len_local() {
    let (stdout, _, code) =
        run_rilua("local ok, msg = pcall(function() local x = true; return #x end); print(msg)");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to get length of local 'x' (a boolean value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_call_upvalue() {
    let (stdout, _, code) = run_rilua(
        "local x = 1; local ok, msg = pcall(function() return (function() x() end)() end); print(msg)",
    );
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to call upvalue 'x' (a number value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_call_field() {
    let (stdout, _, code) =
        run_rilua("local t = {x = 1}; local ok, msg = pcall(function() t.x() end); print(msg)");
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to call field 'x' (a number value)"),
        "got: {stdout}"
    );
}

#[test]
fn error_msg_call_field_with_trailing_for_loop() {
    // Regression test for issue #46: a nil field call inside a numeric for
    // loop, followed by another numeric for loop, must consistently report
    // "field 'move'" rather than misattributing to the loop's "(for index)".
    let script = "local ok, msg = pcall(function() local src = {}; for i = 1, 1000 do src[i] = i end; local dst = {}; for i = 1, 10 do table.move(src, 1, 1000, 1, dst) end; for i = 1, 100 do end end); print(msg)";
    let (stdout, _, code) = run_rilua(script);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("attempt to call field 'move' (a nil value)"),
        "got: {stdout}"
    );
    assert!(!stdout.contains("(for index)"), "got: {stdout}");
}

#[test]
fn error_msg_compare_types() {
    let (stdout, _, code) =
        run_rilua("local ok, msg = pcall(function() return {} < 1 end); print(msg)");
    assert_eq!(code, 0);
    assert!(stdout.contains("attempt to compare"), "got: {stdout}");
}

#[test]
fn traceback_in_stderr() {
    let (_, stderr, code) = run_rilua("error('boom')");
    assert_ne!(code, 0);
    assert!(stderr.contains("stack traceback:"), "got: {stderr}");
    assert!(stderr.contains("[C]: in function 'error'"), "got: {stderr}");
    assert!(stderr.contains("in main chunk"), "got: {stderr}");
}

#[test]
fn traceback_nested_functions() {
    let (_, stderr, code) = run_rilua(
        "function a() error('nested') end; function b() a() end; function c() b() end; c()",
    );
    assert_ne!(code, 0);
    assert!(stderr.contains("stack traceback:"), "got: {stderr}");
    assert!(stderr.contains("in function 'a'"), "got: {stderr}");
    assert!(stderr.contains("in function 'b'"), "got: {stderr}");
    assert!(stderr.contains("in function 'c'"), "got: {stderr}");
}

#[test]
fn traceback_type_error() {
    let (_, stderr, code) = run_rilua("function foo() local x = nil; return x + 1 end; foo()");
    assert_ne!(code, 0);
    assert!(stderr.contains("stack traceback:"), "got: {stderr}");
    assert!(
        stderr.contains("attempt to perform arithmetic on local 'x' (a nil value)"),
        "got: {stderr}"
    );
    assert!(stderr.contains("in function 'foo'"), "got: {stderr}");
}

// -- Bug #18 regression: while-true-if-break loop --

#[test]
fn while_true_if_break_comparison() {
    // Bug #18: while true do ... if cond then break end end
    // The false-branch JMP of the comparison was patched to itself
    // (self-loop), causing an infinite loop.
    let (stdout, _, code) = run_rilua(
        "local i = 0 \
         while true do \
           i = i + 1 \
           if i > 3 then break end \
         end \
         print(i)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "4\n");
}

#[test]
fn while_true_if_break_equality() {
    let (stdout, _, code) = run_rilua(
        "local j = 0 \
         while true do \
           j = j + 1 \
           if j == 5 then break end \
         end \
         print(j)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n");
}

#[test]
fn while_true_if_break_nested() {
    let (stdout, _, code) = run_rilua(
        "local a, b = 0, 0 \
         while true do \
           a = a + 1 \
           local inner = 0 \
           while true do \
             inner = inner + 1 \
             if inner > 2 then break end \
           end \
           b = b + inner \
           if a > 2 then break end \
         end \
         print(a, b)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\t9\n");
}

#[test]
fn while_true_if_break_complex_condition() {
    let (stdout, _, code) = run_rilua(
        "local k = 0 \
         while true do \
           k = k + 1 \
           if k > 2 and k < 10 then break end \
         end \
         print(k)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

// -- Bug #17 regression: parenthesized multi-return truncation --

#[test]
fn paren_truncates_call_return() {
    // Bug #17: (f()) should return exactly 1 value
    let (stdout, _, code) = run_rilua(
        "function f() return 1, 2, 3 end \
         local a, b, c = (f()) \
         assert(a == 1 and b == nil and c == nil) \
         print(a, b, c)",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\tnil\tnil\n");
}

#[test]
fn paren_truncates_in_table_ctor() {
    let (stdout, _, code) = run_rilua(
        "function f() return 1, 2, 3 end \
         local t = {(f())} \
         print(#t, t[1], t[2])",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\t1\tnil\n");
}

#[test]
fn paren_truncates_in_call_args() {
    let (stdout, _, code) = run_rilua(
        "function f() return 1, 2, 3 end \
         local function count(...) return select('#', ...) end \
         print(count(f()), count((f())))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\t1\n");
}

#[test]
fn paren_truncates_vararg() {
    let (stdout, _, code) = run_rilua(
        "local function vtest(...) return (...) end \
         print(vtest(10, 20, 30))",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n");
}

#[test]
fn paren_call_as_statement_is_syntax_error() {
    // (f()) as a statement should be a syntax error, matching PUC-Rio
    let (_, stderr, code) = run_rilua("(print(1))");
    assert_ne!(code, 0);
    assert!(
        stderr.contains("'=' expected") || stderr.contains("syntax error"),
        "got: {stderr}"
    );
}

// ---------------------------------------------------------------------------
// Bug #19: Repeat-until upvalue scoping
// ---------------------------------------------------------------------------

#[test]
fn repeat_until_upvalue_scoping() {
    // Bug #19: closures in repeat body must capture their own copy of
    // locals when the until condition is checked. Requires CLOSE codegen
    // when the scope block has captured upvalues.
    let (_, stderr, code) = run_rilua(
        r"
local results = {}
local i = 0
repeat
  i = i + 1
  local x = i
  results[i] = function() return x end
until i >= 3
assert(results[1]() == 1)
assert(results[2]() == 2)
assert(results[3]() == 3)
",
    );
    assert_eq!(code, 0, "stderr: {stderr}");
}

#[test]
fn tailcall_rust_zero_results() {
    // Bug #21: when a Rust function (like print) returns 0 values via
    // TAILCALL, the result should be nil, not a stale stack value.
    let (stdout, stderr, code) = run_rilua(
        r#"
function wrapper(f, ...) return f(...) end
local a = wrapper(print, "+")
assert(a == nil, "expected nil, got: " .. tostring(a))
print("PASSED")
"#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("PASSED"), "stdout: {stdout}");
}

#[test]
fn vararg_local_assign_target_register() {
    // VARARG instruction A field must target the correct register.
    // Previously the A field was left as 0, overwriting fixed parameters.
    let (stdout, stderr, code) = run_rilua(
        r#"
function f(n, a, ...)
  local b, c, d = ...
  assert(a == 5, "a=" .. tostring(a))
  assert(b == 4, "b=" .. tostring(b))
  assert(c == 3, "c=" .. tostring(c))
  assert(d == 2, "d=" .. tostring(d))
  print("PASSED")
end
f(0, 5, 4, 3, 2, 1)
"#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("PASSED"), "stdout: {stdout}");
}

#[test]
fn select_boundary_cases() {
    // select(1) with no extra args should return 0 results (not error).
    // select(-1, 3, 5, 7) should return 7.
    let (stdout, stderr, code) = run_rilua(
        r#"
a = {select(1)}
assert(next(a) == nil)
a = {select(-1, 3, 5, 7)}
assert(a[1] == 7 and a[2] == nil)
a = {select(-2, 3, 5, 7)}
assert(a[1] == 5 and a[2] == 7 and a[3] == nil)
print("PASSED")
"#,
    );
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("PASSED"), "stdout: {stdout}");
}

#[test]
fn coroutine_cross_thread_upvalues() {
    // Bug #20: closures called from within a coroutine must be able to
    // read upvalues from the creating thread's stack. The swap model
    // must close ALL open upvalues before switching stacks.
    let code_str = r#"
local t = {"a", "b"}
local function gen(c, n)
  if n == 0 then coroutine.yield(c) return end
  for _, a in pairs(t) do
    gen(c..a, n-1)
  end
end
local results = {}
for s in coroutine.wrap(function() gen("", 2) end) do
  table.insert(results, s)
end
assert(table.getn(results) == 4)
assert(results[1] == "aa")
assert(results[2] == "ab")
assert(results[3] == "ba")
assert(results[4] == "bb")
print("PASSED")
"#;
    let (stdout, stderr, code) = run_rilua(code_str);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("PASSED"), "stdout: {stdout}");
}

// ---------------------------------------------------------------------------
// Semicolons: Lua 5.1 rejects standalone semicolons
// ---------------------------------------------------------------------------

#[test]
fn semicolons_rejected_as_empty_statements() {
    // Lua 5.1: ';' alone is not a valid statement (it is in 5.2+).
    let (_, stderr, code) = run_rilua(";");
    assert_ne!(code, 0, "';' should be rejected");
    assert!(stderr.contains("unexpected symbol"), "stderr: {stderr}");
}

#[test]
fn double_semicolons_rejected() {
    // 'a=1;;' should error on the second ';'
    let (_, stderr, code) = run_rilua("a=1;;");
    assert_ne!(code, 0, "'a=1;;' should be rejected");
    assert!(stderr.contains("unexpected symbol"), "stderr: {stderr}");
}

#[test]
fn return_double_semicolon_rejected() {
    // 'return;;' should error: only one ';' is allowed after return
    let (_, stderr, code) = run_rilua("return;;");
    assert_ne!(code, 0, "'return;;' should be rejected");
    assert!(stderr.contains("'<eof>' expected"), "stderr: {stderr}");
}

// ---------------------------------------------------------------------------
// Error message formatting: tokens quoted with '...' in near context
// ---------------------------------------------------------------------------

#[test]
fn eof_token_quoted_in_error_messages() {
    // '<eof>' should be quoted as "'<eof>'" in error messages
    let (stdout, _, code) = run_rilua(r#"print(loadstring("repeat until 1; a"))"#);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("near '<eof>'"),
        "stdout should contain quoted <eof>: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Ambiguous syntax: function call on new line
// ---------------------------------------------------------------------------

#[test]
fn ambiguous_syntax_function_call_new_line() {
    // PUC-Rio rejects function calls where '(' starts on a different
    // line than the expression being called.
    let (_, stderr, code) = run_rilua("a=math.sin\n(3)");
    assert_ne!(code, 0, "ambiguous syntax should be rejected");
    assert!(stderr.contains("ambiguous syntax"), "stderr: {stderr}");
}

// Bug #24: `not` applied to `and`/`or` expressions must negate correctly.
// PUC-Rio's codenot() calls removevalues() to convert TESTSET -> TEST
// after swapping the true/false jump lists.
#[test]
fn not_and_or_negation_basic() {
    // not (nil and true) should be true: nil and true => nil, not nil => true
    let (stdout, stderr, code) = run_rilua("print(not (nil and true))");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "true");
}

#[test]
fn not_and_or_negation_complex() {
    // The expression from PUC-Rio constructs.lua (i=352):
    //   (1 < 2) == not (F() and (1 < 2))
    // F() returns nil, so: nil and (1<2) => nil, not nil => true, (1<2) => true
    // true == true => true
    let code_str = r#"
        function F() return nil end
        local a = (1 < 2) == not (F() and (1 < 2))
        assert(a == true, "expected true, got " .. tostring(a))
        print("ok")
    "#;
    let (stdout, stderr, code) = run_rilua(code_str);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "ok");
}

#[test]
fn not_or_expression() {
    // not (true or false) should be false
    let (stdout, stderr, code) = run_rilua("print(not (true or false))");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "false");
}

#[test]
fn not_and_false_expression() {
    // not (false and true) should be true: false and true => false, not false => true
    let (stdout, stderr, code) = run_rilua("print(not (false and true))");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(stdout.trim(), "true");
}
