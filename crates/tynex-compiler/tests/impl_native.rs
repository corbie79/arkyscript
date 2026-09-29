use std::collections::BTreeMap;
use tynex_compiler::{Interface, Mode, compile, declaration, prepare};

fn checked(source: &str, modules: &BTreeMap<String, Interface>) -> Interface {
    prepare(source, true)
        .unwrap()
        .compile(Mode::Strict, modules)
        .unwrap()
        .interface
}
fn bad(source: &str, modules: &BTreeMap<String, Interface>) {
    let result = prepare(source, true).and_then(|p| p.compile(Mode::Strict, modules));
    assert!(result.is_err(), "unexpected success: {source}");
}
const NATIVE: &str = "declare class Node x: number move: function(Self, number): () end declare class Sprite extends Node constructor: function(): Self texture: number end declare submit: function(Node): ()";

#[test]
fn detached_impl_preserves_self_defaults_and_async_contracts() {
    checked(
        include_str!("../../../examples/impl.arky"),
        &BTreeMap::new(),
    );
    checked(
        "class C end trait T async function value(): number return 42 end end impl T for C end local t: Task<number> = C {}:value()",
        &BTreeMap::new(),
    );
    checked(
        "class C end trait T f: function(Self, number): number end impl T for C function f(n: number): number if n == 0 then return 0 end return self:f(n-1) end end",
        &BTreeMap::new(),
    );
}
#[test]
fn impl_rejects_missing_duplicate_overriding_and_non_top_level_bodies() {
    for source in [
        "class C end trait T f: function(Self): number end impl T for C end",
        "class C end trait T end impl T for C end impl T for C end",
        "class C function f(): number return 1 end end trait T f: function(Self): number end impl T for C function f(): number return 2 end end",
        "class C end trait T end if true then impl T for C end end",
        "class C end trait T end local function f(): () impl T for C end end",
        "class C end trait T end impl T for C function extra(): () end end",
        "class C end trait T f: function(Self): number end impl T for C function f(): string return 'bad' end end",
    ] {
        bad(source, &BTreeMap::new());
    }
}
#[test]
fn native_declarations_validate_inheritance_constructors_and_calls() {
    let modules = BTreeMap::from([("engine".into(), declaration(NATIVE).unwrap())]);
    checked(
        include_str!("../../../examples/native_traits/main.arky"),
        &modules,
    );
    checked(
        "import e from 'engine' local s = e.Sprite() local n: e.Node = s n:move(1) e.submit(s)",
        &modules,
    );
    for code in [
        "e.Node()",
        "e.Sprite(1)",
        "e.submit(1)",
        "e.Sprite().texture = 'bad'",
        "local function f(n: e.Node): e.Sprite return n end",
    ] {
        bad(&format!("import e from 'engine' {code}"), &modules);
    }
    for source in [
        "declare class A extends Missing end",
        "declare class A constructor: function(): number end",
        "declare class A x: number end declare class B extends A x: string end",
        "declare class A x: Missing end",
        "declare class A end declare class A end",
        "declare class A x: number = 1 end",
        "declare class A function f(): () end end",
    ] {
        assert!(declaration(source).is_err(), "{source}");
    }
}
#[test]
fn imports_preserve_impl_contracts_and_require_initialization_order() {
    let model = compile("class C end return {C=C}", Mode::Strict, true)
        .unwrap()
        .interface;
    let modules = BTreeMap::from([("model".into(), model.clone())]);
    let extension = checked(
        "import m from 'model' trait T function f(): number return 42 end end impl T for m.C end return {T=T}",
        &modules,
    );
    let modules = BTreeMap::from([("model".into(), model), ("extension".into(), extension)]);
    checked(
        "import m from 'model' import ext from 'extension' local v: ext.T = m.C {} print(v:f())",
        &modules,
    );
    bad(
        "import m from 'model' print(m.C {}:f()) import ext from 'extension'",
        &modules,
    );
    bad("if true then import ext from 'extension' end", &modules);
    bad(
        "import m from 'model' import ext from 'extension' impl ext.T for m.C end",
        &modules,
    );
}
#[test]
fn diamond_imports_merge_without_losing_or_duplicating_impls() {
    let model = checked("class C end return {C=C}", &BTreeMap::new());
    let modules = BTreeMap::from([("model".into(), model)]);
    let extension = checked(
        "import m from 'model' trait T function f(): number return 1 end end impl T for m.C end return {C=m.C,T=T}",
        &modules,
    );
    let imports = BTreeMap::from([("ext".into(), extension)]);
    let a = checked("import e from 'ext' return e", &imports);
    let b = checked("import e from 'ext' return e", &imports);
    checked(
        "import a from 'a' import b from 'b' local t: b.T = a.C {} print(t:f())",
        &BTreeMap::from([("a".into(), a), ("b".into(), b)]),
    );
}
#[test]
fn sibling_modules_cannot_add_conflicting_methods() {
    let model = checked("class C end return {C=C}", &BTreeMap::new());
    let modules = BTreeMap::from([("model".into(), model)]);
    let src = "import m from 'model' trait T function f(): number return 1 end end impl T for m.C end return T";
    let a = checked(src, &modules);
    let b = checked(src, &modules);
    bad(
        "import a from 'a' import b from 'b'",
        &BTreeMap::from([("a".into(), a), ("b".into(), b)]),
    );
}
#[test]
fn native_parent_extensions_are_visible_and_cannot_clash_with_children() {
    let modules = BTreeMap::from([("e".into(), declaration(NATIVE).unwrap())]);
    checked(
        "import e from 'e' trait T function label(): string return 'node' end end impl T for e.Node end local t: T = e.Sprite() print(t:label())",
        &modules,
    );
    bad(
        "import e from 'e' trait T function texture(): string return 'bad' end end impl T for e.Node end",
        &modules,
    );
}

#[test]
fn native_readonly_and_inline_default_conflicts_are_rejected() {
    let modules = BTreeMap::from([(
        "e".into(),
        declaration("declare class N readonly id: number constructor: function(): Self end")
            .unwrap(),
    )]);
    checked("import e from 'e' print(e.N().id)", &modules);
    bad("import e from 'e' local n = e.N() n.id = 3", &modules);
    bad(
        "trait A function f(): number return 1 end end class C is A end trait B function f(): number return 2 end end impl B for C end",
        &BTreeMap::new(),
    );
}
