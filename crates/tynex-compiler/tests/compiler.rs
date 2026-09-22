use tynex_compiler::{Mode, compile};

fn accepts(source: &str, mode: Mode) {
    compile(source, mode, true).unwrap();
}
fn rejects(source: &str, mode: Mode, code: &str) {
    assert!(
        compile(source, mode, true)
            .unwrap_err()
            .iter()
            .any(|e| e.code == code),
        "{source}"
    );
}

#[test]
fn inferred_locals_and_returns_do_not_require_annotations_in_strict() {
    accepts(
        "local function answer() return 42 end local value = answer() print(value)",
        Mode::Strict,
    );
}
#[test]
fn known_type_errors_are_rejected_in_both_modes() {
    for mode in [Mode::Gradual, Mode::Strict] {
        rejects("local score: number = 'bad'", mode, "T001");
        rejects("local score = 1 score = false", mode, "T001");
        rejects(
            "local function f(n: number): string return n end",
            mode,
            "T001",
        );
        rejects(
            "local function f(n: number): number return n end f('bad')",
            mode,
            "T001",
        );
    }
}
#[test]
fn gradual_parameters_work_and_strict_requires_their_types() {
    let source = "local function double(x) return x * 2 end print(double(21))";
    accepts(source, Mode::Gradual);
    rejects(source, Mode::Strict, "T002");
}
#[test]
fn raw_lua_dialect_never_accepts_annotations() {
    assert!(compile("local x: number = 1", Mode::Strict, false).is_err());
    assert!(compile("local x = 1 print(x)", Mode::Strict, false).is_ok());
}
#[test]
fn optional_types_are_explicit_and_missing_returns_are_checked() {
    accepts("local x: number? = nil x = 1", Mode::Strict);
    rejects("local x: number = nil", Mode::Gradual, "T001");
    rejects(
        "local function f(): number print(1) end",
        Mode::Strict,
        "T001",
    );
}
#[test]
fn strings_comments_and_positions_survive_generation() {
    let source = "-- 한국어 : number\nlocal text: string = [=[\n: number class await\n]=]\nlocal n: -- annotation\nnumber = 1\nprint(text, n)";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert_eq!(source.len(), output.lua.len());
    assert!(output.lua.contains("[=[\n: number class await\n]=]"));
    assert_eq!(
        source
            .match_indices('\n')
            .map(|(i, _)| i)
            .collect::<Vec<_>>(),
        output
            .lua
            .match_indices('\n')
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    );
    assert!(!output.lua.contains("annotation"));
}
#[test]
fn errors_use_original_unicode_source_location() {
    let source = "-- 한국어\nlocal n: number = '문자열'";
    let errors = compile(source, Mode::Strict, true).unwrap_err();
    assert_eq!(errors[0].location(source), (2, 7));
}
#[test]
fn function_scope_does_not_leak() {
    rejects(
        "local function f() local secret = 1 return secret end print(secret)",
        Mode::Strict,
        "T003",
    );
}
#[test]
fn external_dynamic_values_need_an_explicit_strict_boundary() {
    accepts(
        "local lib: any = require('legacy') print(lib.work(1))",
        Mode::Strict,
    );
    accepts(
        "local lib = require('legacy') print(lib.work(1))",
        Mode::Gradual,
    );
    rejects("local lib = require('legacy')", Mode::Strict, "T002");
}
#[test]
fn function_arity_is_checked() {
    rejects(
        "local function f(n: number) return n end f()",
        Mode::Strict,
        "T006",
    );
    rejects(
        "local function f() return 1 end f(2)",
        Mode::Gradual,
        "T006",
    );
}
#[test]
fn malformed_or_unimplemented_syntax_is_never_silently_accepted() {
    for source in [
        "local a, = 1, 2",
        "local function f(..., x) return x end",
        "class User name: end",
        "async function f() end",
        "local a=1 (a)=2",
        "local a = 1e+",
        "local a=1..2",
        "local a='\\xGG'",
        "local a='\\999'",
        "local a='\\q'",
        "return 1 print(2)",
        "--[=[ unterminated",
    ] {
        assert!(compile(source, Mode::Gradual, true).is_err(), "{source}");
    }
}
#[test]
fn precedence_recursion_and_parenthesized_calls_work() {
    accepts(
        "local function f(n: number): number return f(n) end local n = -2^2 + 3*4 print((f)(n))",
        Mode::Strict,
    );
}
#[test]
fn escaped_and_long_strings_and_numeric_literals_work() {
    accepts(
        "local s='hi\\x41\\255\\z   there' local t=[==[quote ' --]==] local x=0x1.fp2 + .5 + 1e-2 print(s,t,x)",
        Mode::Strict,
    );
}
#[test]
fn nesting_limit_is_a_diagnostic() {
    let source = format!("local x = {}1{}", "(".repeat(150), ")".repeat(150));
    assert!(compile(&source, Mode::Strict, true).is_err());
    let source = format!("local x = {}1", "1+".repeat(500));
    rejects(&source, Mode::Strict, "T008");
}
