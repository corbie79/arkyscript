use tynex_compiler::{Mode, compile, prepare, source_map::Position};

#[test]
fn optional_suffixes_keep_comments_and_original_lf_or_crlf_lines() {
    let source = "local receiver: any = {}\nlocal result = receiver -- receiver\n ?.nested -- field\n ?[ -- key\n  'key'\n ]\n ?.callback\n ?( -- argument\n  42\n )\nprint(result)\n";
    for source in [source.to_owned(), source.replace('\n', "\r\n")] {
        let output = compile(&source, Mode::Strict, true).unwrap();
        assert_eq!(source.lines().count(), output.lua.lines().count());
        assert_eq!(
            source.bytes().filter(|b| *b == b'\r').count(),
            output.lua.bytes().filter(|b| *b == b'\r').count()
        );
        for (line, text) in output.lua.lines().enumerate() {
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
        for marker in ["-- receiver", "-- field", "-- key", "-- argument", "'key'"] {
            assert!(output.lua.contains(marker));
        }
        assert!(output.lua.contains("== nil then return nil end"));
    }
}

#[test]
fn function_type_parentheses_disambiguate_optional_callback_from_optional_result() {
    let source = "local function invoke(callback: (function(number): number)?, value: number): number? return callback?(value) end print(invoke(nil, 42))";
    assert!(compile(source, Mode::Strict, true).is_ok());
    assert!(prepare("local n: (number)? = nil", true).is_ok());
    assert!(
        prepare(
            "local callback: ((function(): (number, string)))? = nil",
            true
        )
        .is_ok()
    );
}

#[test]
fn single_grouped_return_types_accept_optional_suffixes() {
    for source in [
        "local function factory(): (function(number): number)? return nil end local callback: (function(number): number)? = factory()",
        "local function factory(): (function(number): number)? return function(n: number): number return n end end local n: number? = factory()?(42)",
        "local function number_or_nil(): (number)? return nil end local n: number? = number_or_nil()",
        "local function factory(): function(): number? return function(): number? return nil end end local callback = factory() local n: number? = callback()",
    ] {
        let output = compile(source, Mode::Strict, true).unwrap();
        assert!(!output.lua.contains('?'), "{source}");
    }
    let errors = compile(
        "local function factory(): function(): number? return nil end",
        Mode::Strict,
        true,
    )
    .unwrap_err();
    assert!(errors.iter().any(|error| error.code == "T001"));
}

#[test]
fn optional_suffix_on_a_return_pack_is_rejected_clearly() {
    for returns in [
        "()?",
        "(number, string)?",
        "(...number)?",
        "(number, ...string)?",
    ] {
        let source = format!("local function invalid(): {returns} end");
        let errors = compile(&source, Mode::Gradual, true).unwrap_err();
        assert_eq!(errors[0].code, "P001");
        assert!(errors[0].message.contains("single grouped return type"));
    }
}

#[test]
fn nested_optional_closures_close_before_enclosing_class_default_wrapper() {
    let source = "local outer: any = function(value: any): any return value end local inner: any = function(): any return {} end class Holder value: any = outer?(inner?()) end print(Holder {}.value)";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert!(output.lua.contains("end)() end;"));
    assert_eq!(output.lua.matches("local __tynex_value =").count(), 2);
}

#[test]
fn parser_accepts_optional_and_regular_suffix_composition_and_method_shorthand() {
    for source in [
        "local value = object?.field.method(argument)",
        "local value = object?[key]?.field?:method(argument)",
        "object?:method 'argument'",
        "object?:method {argument}",
        "local value = callback?()().field?[key]",
        "local value = (object?.field).method(argument)",
        "local function f(...) return callback?(other?(...), ...) end",
        "try callback?() except err then print(err) end",
    ] {
        assert!(prepare(source, true).is_ok(), "{source}");
    }
}

#[test]
fn optional_suffixes_reject_assignments_non_calls_and_separated_operators() {
    for source in [
        "object?.field = 1",
        "object?[key] = 1",
        "object?.field.other = 1",
        "object?:method() = 1",
        "object?.field",
        "callback?{}",
        "callback?'argument'",
        "callback? (1)",
        "object? .field",
        "object??()",
        "object?.",
    ] {
        assert!(prepare(source, true).is_err(), "{source}");
    }
    for source in ["callback?()", "local value = object?.field"] {
        assert!(prepare(source, false).is_err(), "{source}");
    }
}
