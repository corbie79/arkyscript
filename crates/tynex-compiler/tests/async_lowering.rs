use tynex_compiler::{Mode, compile, declaration, prepare, source_map::Position};

#[test]
fn async_functions_and_await_preserve_multiline_comments_and_source_maps() {
    let source = "trait Loadable\n load: async function(Self): number\n async -- async method\n function -- method name\n twice(): number\n  return await self:load()\n end\nend\nclass Counter is Loadable\n n: number = 42\n async function load(): number return self.n end\nend\nlocal async -- local function\n function entry(): number\n local value = Counter {}\n return await -- awaited call\n  value:twice()\nend\nlocal result: Task<number>=entry()\n";
    for source in [source.to_owned(), source.replace('\n', "\r\n")] {
        let output = compile(&source, Mode::Strict, true).unwrap();
        assert_eq!(source.lines().count(), output.lua.lines().count());
        assert_eq!(
            source.bytes().filter(|b| *b == b'\r').count(),
            output.lua.bytes().filter(|b| *b == b'\r').count()
        );
        assert!(output.source_map.mappings[0].original.is_none());
        for (line, text) in output.lua.lines().enumerate().skip(1) {
            assert_eq!(
                output
                    .source_map
                    .original_position(Position { line, column: 0 })
                    .unwrap()
                    .line,
                line,
                "{text}"
            );
        }
        for marker in [
            "-- async method",
            "-- method name",
            "-- local function",
            "-- awaited call",
        ] {
            assert!(output.lua.contains(marker));
        }
        assert!(output.lua.contains("__tynex_async_runtime.create(function"));
        assert!(output.lua.contains("__tynex_async_runtime.await("));
        assert!(
            output
                .lua
                .contains("entry; entry = __tynex_async_runtime.create(function")
        );
    }
}

#[test]
fn async_function_forms_and_task_pack_types_parse() {
    for source in [
        "local async function f() end",
        "async function f() end",
        "async function api.worker() end",
        "async function api:worker(...:any) return ... end",
        "local callback = async function(...:any) return ... end",
        "class Worker async function run() end end",
        "trait Runnable run:async function(Self):() async function twice() await self:run() end end",
        "local async function f(task:Task<number>):number return await task end",
        "local async function f(task:Task<>):() await task end",
        "local async function f(task:Task<...number>):(...number) return await task end",
        "local async function f(task:Task<Task<number>>):number return await await task end",
    ] {
        assert!(prepare(source, true).is_ok(), "{source}");
    }
    for source in [
        "export: Task<>",
        "export: Task<number, nil, string>",
        "export: Task<number, ...string>",
        "export: Task<Task<number>>",
        "export: async function(number): (number, nil)",
        "export: (async function(...any): (...any))?",
    ] {
        assert!(declaration(source).is_ok(), "{source}");
    }
}

#[test]
fn nested_wrappers_close_in_order_and_type_annotations_stop_before_assignment() {
    let source = "local async function pair(n:number):number return n end class Holder callback:async function():number = async function():number return await pair?(42) end end local pending:Task<number>=Holder{}.callback()";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert!(output.lua.contains("end) end;"));
    assert!(!output.lua.contains("Task<number>"));
    assert!(output.lua.contains("=Holder{}.callback()"));
    let source =
        "local async function f(task:Task<Task<number>>):number return await await task end";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert!(
        output
            .lua
            .contains("__tynex_async_values(__tynex_async_runtime.await( __tynex_async_values(__tynex_async_runtime.await( task))))")
    );
    let source = "local async function value():number return 42 end local async function wrap():Task<number> return value() end local nested:Task<Task<number>>=wrap()";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert!(output.lua.contains("=wrap()"));
    assert!(!output.lua.contains("Task<Task<number>>"));
}

#[test]
fn await_is_limited_to_its_source_async_function_body() {
    for source in [
        "await pending",
        "local function f() return await pending end",
        "local async function outer() local function sync() return await pending end end",
        "local async function outer() local sync = function() return await pending end end",
        "local async function outer() class C value = await pending end end",
        "local async function outer() class C function sync() return await pending end end end",
        "local async f() end",
        "async local function f() end",
        "local task:Task<number,> = nil",
        "local task:Task<...number, string> = nil",
        "local callback:async(number):number = nil",
    ] {
        assert!(prepare(source, true).is_err(), "{source}");
    }
    for source in [
        "local async function f() end",
        "async function f() end",
        "local f=async function() end",
    ] {
        assert!(prepare(source, false).is_err(), "{source}");
    }
    assert!(prepare("local async function outer() class C callback=async function() return await pending end end end", true).is_ok());
}

#[test]
fn async_declaration_terminates_before_a_parenthesized_call_statement() {
    for declaration in [
        "local async function pending():() end",
        "async function pending():() end",
    ] {
        let source = format!("local callback = function():() end\n{declaration}\n(callback)()");
        let output = compile(&source, Mode::Gradual, true).unwrap();
        assert!(output.lua.contains("end);\n(callback)()"));
    }
}

#[test]
fn nested_task_types_stop_at_the_existing_parser_depth_limit() {
    let source = format!(
        "local pending: {}number{}",
        "Task<".repeat(150),
        ">".repeat(150)
    );
    let errors = compile(&source, Mode::Strict, true).unwrap_err();
    assert_eq!(errors[0].code, "P001");
    assert!(
        errors[0]
            .message
            .contains("nesting exceeds the current limit (128)")
    );
}
