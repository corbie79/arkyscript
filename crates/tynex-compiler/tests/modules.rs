use std::collections::BTreeMap;
use tynex_compiler::{Mode, compile, declaration, prepare};
#[test]
fn imports_follow_builtin_binding_and_skip_comments_strings_and_shadowing() {
    let source = "-- require('comment')\nlocal text=\"require('string')\" local load=require load('real') do local require=function(n:string):number return 1 end require('fake') end require(('other'))";
    let imports = prepare(source, true).unwrap().imports(Mode::Strict);
    assert_eq!(
        imports
            .iter()
            .filter_map(|i| i.name.as_deref())
            .collect::<Vec<_>>(),
        ["real", "other"]
    );
}
#[test]
fn modules_export_the_first_return_value_and_default_to_true() {
    for source in ["return nil", "return", "local x=1"] {
        let output = compile(source, Mode::Strict, true).unwrap();
        assert_eq!(output.interface.to_string(), "boolean");
    }
    assert_eq!(
        compile("return 1,'ignored'", Mode::Strict, true)
            .unwrap()
            .interface
            .to_string(),
        "number"
    );
}
#[test]
fn inferred_interfaces_check_consumer_calls_and_return_types() {
    let library = compile(
        "local m={} function m.add(a:number,b:number):number return a+b end return m",
        Mode::Strict,
        true,
    )
    .unwrap()
    .interface;
    let modules = BTreeMap::from([("maths".into(), library)]);
    let good = prepare("local m=require('maths') local n:number=m.add(1,2)", true).unwrap();
    assert!(good.compile(Mode::Strict, &modules).is_ok());
    let bad = prepare("local m=require('maths') local n:number=m.add(1,'x')", true).unwrap();
    assert!(
        bad.compile(Mode::Gradual, &modules)
            .unwrap_err()
            .iter()
            .any(|d| d.code == "T001")
    );
}
#[test]
fn declaration_interfaces_support_functions_records_and_variadics() {
    let interface = declaration(
        "export: {read: function(string): (string?, string?), log: function(...string): ()}",
    )
    .unwrap();
    let modules = BTreeMap::from([("legacy".into(), interface)]);
    let source =
        "local m=require('legacy') local data:string?,err:string?=m.read('x') m.log('one','two')";
    assert!(
        prepare(source, true)
            .unwrap()
            .compile(Mode::Strict, &modules)
            .is_ok()
    );
    assert!(declaration("export: number print('must not execute')").is_err());
    assert!(declaration("export: {f:function(number)}").is_err());
}
#[test]
fn function_annotations_are_erased_without_erasing_lua_functions() {
    let source =
        "local f: function(number): number = function(n:number):number return n end print(f(1))";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert_eq!(source.len(), output.lua.len());
    assert!(output.lua.contains("= function(n"));
    assert!(!output.lua.contains(": number"));
}
#[test]
fn dynamic_require_stays_an_explicit_strict_boundary() {
    let source = "local name='runtime' local m:any=require(name) print(m)";
    let output = compile(source, Mode::Strict, true).unwrap();
    assert_eq!(output.imports.len(), 1);
    assert!(output.imports[0].name.is_none());
    assert!(
        compile(
            "local name='runtime' local m=require(name)",
            Mode::Strict,
            true
        )
        .is_err()
    );
}
#[test]
fn resolving_an_exported_require_reveals_further_dependencies() {
    let p = prepare("local load=require('loader') local m=load('data')", true).unwrap();
    assert_eq!(p.imports(Mode::Gradual).len(), 1);
    let modules = BTreeMap::from([(
        "loader".into(),
        compile("return require", Mode::Strict, true)
            .unwrap()
            .interface,
    )]);
    let imports = p.imports_with(Mode::Gradual, &modules);
    assert_eq!(
        imports
            .iter()
            .filter_map(|i| i.name.as_deref())
            .collect::<Vec<_>>(),
        ["loader", "data"]
    );
}
