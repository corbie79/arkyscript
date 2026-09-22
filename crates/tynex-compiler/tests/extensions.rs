use std::collections::BTreeMap;
use tynex_compiler::{Mode, compile, declaration, prepare, source_map::Position};

#[test]
fn static_import_discovery_is_independent_of_require_bindings() {
    let source = "local require = function() end import api from 'api'";
    let prepared = prepare(source, true).unwrap();
    let imports = prepared.imports(Mode::Gradual);
    assert_eq!(imports.len(), 1);
    assert!(imports[0].static_import);
    assert_eq!(imports[0].name.as_deref(), Some("api"));
    let modules = BTreeMap::from([("api".into(), declaration("declare n: number").unwrap())]);
    let result = prepared.compile(Mode::Strict, &modules).unwrap();
    assert!(result.lua.contains("local api = __tynex_import("));
}

#[test]
fn new_keywords_and_helper_namespace_are_only_reserved_in_typed_syntax() {
    for word in ["import", "from", "declare", "try", "except", "__tynex_pack"] {
        let source = format!("local {word}=1 print({word})");
        assert!(compile(&source, Mode::Gradual, true).is_err());
        assert!(compile(&source, Mode::Strict, false).is_ok());
    }
}

#[test]
fn invalid_exception_syntax_never_reaches_codegen() {
    for source in [
        "try print(1) end",
        "try print(1) else print(2) end",
        "try except err print(err) end",
        "try except err then else else end",
        "try break except err then end",
        "try except err then break end",
        "try ... except err then end",
    ] {
        assert!(compile(source, Mode::Gradual, true).is_err(), "{source}");
    }
}

#[test]
fn new_lowerings_keep_line_numbers_and_map_tokens_after_expansions() {
    let source = "import api from 'api'\ntry\n print(api.n)\nexcept err then\n print(err)\nelse\n print('ok')\nend\n";
    let output = compile(source, Mode::Gradual, true).unwrap();
    assert_eq!(source.lines().count(), output.lua.lines().count());
    assert!(output.source_map.mappings[0].original.is_none());
    for (line, text) in output.lua.lines().enumerate().skip(1) {
        let first = output
            .source_map
            .original_position(Position { line, column: 0 })
            .unwrap();
        assert_eq!(first.line, line, "{text}");
    }
    let prefix = output.lua.lines().next().unwrap();
    let column = prefix.find("'api'").unwrap();
    let position = output
        .source_map
        .original_position(Position { line: 0, column })
        .unwrap();
    assert_eq!(position.line, 0);
    let segment = output
        .source_map
        .mappings
        .iter()
        .rev()
        .find(|mapping| mapping.generated.line == 0 && mapping.generated.column <= column)
        .unwrap();
    assert_eq!(
        position.column + column - segment.generated.column,
        source.find("'api'").unwrap()
    );
}

#[test]
fn declaration_forms_are_exclusive_and_never_executable() {
    assert_eq!(
        declaration("declare n:number; declare label:string")
            .unwrap()
            .to_string(),
        "{label: string, n: number}"
    );
    assert!(declaration("export: function(number):number").is_ok());
    for text in [
        "",
        "declare n:number=1",
        "export:number declare n:number",
        "declare n:number export:number",
        "declare n:number declare n:string",
        "declare __tynex_import:any",
    ] {
        assert!(declaration(text).is_err(), "{text}");
    }
}

#[test]
fn importing_a_module_invalidates_narrowed_captured_variables() {
    let source = "local n:number?=1 import api from 'api' local value:number=n";
    let modules = BTreeMap::from([("api".into(), declaration("export: number").unwrap())]);
    let errors = prepare(source, true)
        .unwrap()
        .compile(Mode::Strict, &modules)
        .unwrap_err();
    assert!(errors.iter().any(|error| error.code == "T001"));
}
