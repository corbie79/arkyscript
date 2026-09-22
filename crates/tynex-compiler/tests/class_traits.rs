use std::collections::BTreeMap;
use tynex_compiler::{Mode, compile, prepare};

fn accepts(source: &str) {
    for mode in [Mode::Gradual, Mode::Strict] {
        compile(source, mode, true).unwrap_or_else(|errors| panic!("{mode}: {errors:?}\n{source}"));
    }
}
fn rejects(source: &str, code: &str) {
    for mode in [Mode::Gradual, Mode::Strict] {
        let errors = compile(source, mode, true).unwrap_err();
        assert!(
            errors.iter().any(|e| e.code == code),
            "{mode}: {errors:?}\n{source}"
        );
    }
}

#[test]
fn class_fields_methods_and_inferred_defaults() {
    accepts(
        r#"
class Player
 name: string
 score = 0
 tags: {string} = {}
 function add(n: number): () self.score = self.score + n end
 function label(): string return self.name end
end
local p: Player = Player {name = "Mina"}
p:add(42)
p.tags[1] = p:label()
local same: Player = p
same.score = 10
"#,
    );
}
#[test]
fn constructor_contracts_are_checked_in_both_modes() {
    for initializer in [
        "{}",
        "{name = 1}",
        "{name = 'x', extra = 2}",
        "{name = 'x', [1] = 2}",
    ] {
        assert!(
            compile(
                &format!("class P name: string end local p = P {initializer}"),
                Mode::Gradual,
                true
            )
            .is_err()
        );
        assert!(
            compile(
                &format!("class P name: string end local p = P {initializer}"),
                Mode::Strict,
                true
            )
            .is_err()
        );
    }
    rejects("class P end local p = P()", "T006");
    rejects("class P end local p = P(1)", "T014");
    rejects("class P end local p = P({}, {})", "T006");
    accepts("class P n: number = 1 maybe: string? value: any end local p = P {n = nil}");
}
#[test]
fn initializers_from_variables_do_not_bypass_contracts() {
    accepts(
        "class P name: string count: number = 0 end local input = {name = 'Mina'} local p = P(input)",
    );
    rejects(
        "class P name: string end local input = {} local p = P(input)",
        "T014",
    );
    rejects(
        "class P name: string end local input = {name = 'Mina', extra = true} local p = P(input)",
        "T014",
    );
    rejects(
        "class P name: string end local input = {name = 42} local p = P(input)",
        "T001",
    );
}
#[test]
fn class_identity_is_nominal_including_shadowed_names() {
    rejects(
        "class A n: number end class B n: number end local a: A = B {n = 1}",
        "T001",
    );
    rejects(
        "class A n: number end local a = A {n = 1} do class A n: number end local b: A = a end",
        "T001",
    );
    rejects("class A n: number end local a: A = {n = 1}", "T001");
}
#[test]
fn class_data_fields_have_closed_known_types() {
    rejects(
        "class A n: number end local a = A {n = 1} a.n = 'x'",
        "T001",
    );
    rejects(
        "class A n: number end local a = A {n = 1} a.missing = true",
        "T014",
    );
    rejects("class A end local a = A {} print(a.missing)", "T014");
    rejects(
        "class A end local a = A {} local k = 'anything' a[k] = 1",
        "T014",
    );
}
#[test]
fn recursive_self_types_and_later_method_signatures() {
    accepts(
        r#"
class Node
 next: Self?
 value: number = 0
 function walk(): number
  if self.next ~= nil then return self:read() end
  return 0
 end
 function read(): number return self.value end
 function identity(): Self return self end
end
local a = Node {}
local b = Node {next = a}
local c: Node = b:identity()
"#,
    );
}
#[test]
fn nominal_type_names_resolve_inside_nested_contracts() {
    accepts(
        "class A end local a = A {} local list: {A} = {a} local m: {item: A} = {item = a} local function f(v: A): A return v end local g: function(A): A = f print(g(m.item))",
    );
    rejects("local a: Missing = nil", "T013");
    rejects("local function f(a: Self): () end", "T013");
    rejects("do class A end end local a: A = nil", "T013");
}
#[test]
fn definitions_and_methods_are_immutable() {
    rejects("class A end A = A", "T012");
    rejects("class A end local A = 1", "T012");
    rejects("trait T end T = T", "T012");
    rejects(
        "class A function f(): number return 1 end end A.f = function(): number return 2 end",
        "T014",
    );
    rejects(
        "class A function f(): number return 1 end end local a = A {} a.f = function(): number return 2 end",
        "T014",
    );
}
#[test]
fn traits_require_explicit_nominal_implementation() {
    accepts(
        r#"
trait Named
 label: function(Self): string
 function describe(): string return "name: " .. self:label() end
end
class Player is Named
 name: string
 function label(): string return self.name end
end
class Device is Named
 function label(): string return "device" end
end
local function show(value: Named): string return value:describe() end
local a: string = show(Player {name = "Mina"})
local b: string = show(Device {})
"#,
    );
    rejects(
        "trait T f: function(Self): number end class A function f(): number return 1 end end local value: T = A {}",
        "T001",
    );
    rejects("trait T end local value: T = {}", "T001");
}
#[test]
fn trait_requirements_and_default_bodies_are_checked() {
    rejects(
        "trait T f: function(Self): number end class A is T end",
        "T014",
    );
    rejects(
        "trait T f: function(Self, number): number end class A is T function f(x: string): number return 1 end end",
        "T014",
    );
    rejects(
        "trait T f: function(Self): number end class A is T function f(): string return 'bad' end end",
        "T014",
    );
    rejects("trait T f: function(number): number end", "T014");
    rejects("trait T function f(): number return 'bad' end end", "T001");
    rejects(
        "trait T function f(): number return self.secret end end",
        "T014",
    );
    rejects("trait T end local t = T {}", "T014");
}
#[test]
fn multiple_trait_defaults_require_an_explicit_override() {
    let prefix = "trait A function f(): number return 1 end end trait B function f(): number return 2 end end ";
    rejects(&format!("{prefix} class C is A, B end"), "T014");
    accepts(&format!(
        "{prefix} class C is A, B function f(): number return 3 end end local c = C {{}} local n: number = c:f()"
    ));
    rejects(
        "trait A function f(): number return 1 end end class C is A f: number = 2 end",
        "T014",
    );
}
#[test]
fn self_parameters_require_the_same_concrete_implementor() {
    accepts(
        "trait Equal equal: function(Self, Self): boolean end class A is Equal function equal(other: Self): boolean return self == other end end local a = A {} local b = A {} local yes: boolean = a:equal(b)",
    );
    rejects(
        "trait Equal equal: function(Self, Self): boolean end local function unsafe(a: Equal, b: Equal): boolean return a:equal(b) end",
        "T015",
    );
    accepts(
        "trait Clone clone: function(Self): Self end class A is Clone function clone(): Self return self end end local function copy(v: Clone): Clone return v:clone() end local c: Clone = copy(A {})",
    );
}
#[test]
fn trait_defaults_keep_rigid_self_in_their_bodies() {
    accepts(
        "trait T equal: function(Self, Self): boolean function same(): boolean return self:equal(self) end end class A is T function equal(other: Self): boolean return self == other end end local yes: boolean = A {}:same()",
    );
}
#[test]
fn duplicate_members_and_nontraits_are_rejected() {
    rejects("class A n: number n: string end", "T014");
    rejects(
        "class A n: number function n(): number return 1 end end",
        "T014",
    );
    rejects(
        "trait T f: function(Self): number f: function(Self): number end",
        "T014",
    );
    rejects("class A end class B is A end", "T014");
    rejects("trait T end class A is T, T end", "T014");
}
#[test]
fn gradual_allows_unannotated_members_but_strict_needs_inference() {
    let source = "class A value function f(arg) return arg end end local a = A {} a.value = 1 print(a:f('ok'))";
    compile(source, Mode::Gradual, true).unwrap();
    let errors = compile(source, Mode::Strict, true).unwrap_err();
    assert!(errors.iter().any(|e| e.code == "T002"));
}
#[test]
fn imported_class_and_trait_metadata_preserves_identity() {
    let module = compile("trait T f: function(Self): number end class A is T n: number = 1 function f(): number return self.n end end return {A = A, T = T}", Mode::Strict, true).unwrap().interface;
    let modules = BTreeMap::from([("model".into(), module)]);
    let source = "import m from 'model' class B is m.T function f(): number return 2 end end local a: m.A = m.A {} local t: m.T = B {} local function read(v: m.T): number return v:f() end print(read(a),read(t))";
    prepare(source, true)
        .unwrap()
        .compile(Mode::Strict, &modules)
        .unwrap();
    let wrong = prepare("import m from 'model' class A n: number = 1 function f(): number return self.n end end local a: m.A = A {}", true).unwrap().compile(Mode::Strict, &modules).unwrap_err();
    assert!(wrong.iter().any(|e| e.code == "T001"));
}
#[test]
fn same_named_classes_in_distinct_modules_are_distinct() {
    let a = compile("class A end return A", Mode::Strict, true)
        .unwrap()
        .interface;
    let b = compile("class A end return A", Mode::Strict, true)
        .unwrap()
        .interface;
    let modules = BTreeMap::from([("a".into(), a), ("b".into(), b)]);
    let errors = prepare(
        "import A from 'a' import B from 'b' local value: A = B {}",
        true,
    )
    .unwrap()
    .compile(Mode::Strict, &modules)
    .unwrap_err();
    assert!(errors.iter().any(|e| e.code == "T001"));
}

#[test]
fn deferred_defaults_discard_captured_flow_refinements() {
    rejects(
        "local n: number? = 1 if n ~= nil then class C x: number = n end n = nil local c = C {} end",
        "T001",
    );
    accepts(
        "local n: number? = 1 if n ~= nil then class C x: number? = n end local result: number = n end",
    );
}
#[test]
fn class_and_trait_definitions_expose_no_static_methods() {
    rejects(
        "class A function f(): number return 1 end end local n = A.f(A {})",
        "T014",
    );
    rejects("trait T f: function(Self): number end print(T.f)", "T014");
}

#[test]
fn abstract_traits_preserve_receiver_and_nested_self_safety() {
    rejects(
        "trait T f: function(Self): number end local function bad(a: T, b: T): number local f = a.f return f(b) end",
        "T015",
    );
    rejects(
        "trait T list: function(Self): {Self} end local function bad(a: T) return a:list() end",
        "T015",
    );
    rejects(
        "trait T callback: function(Self): function(Self): number end local function bad(a: T) return a:callback() end",
        "T015",
    );
    accepts(
        "class A value: number = 1 function read(): number return self.value end end local a = A {} local read = a.read local n: number = read(a)",
    );
}

#[test]
fn class_defaults_see_the_complete_recursive_constructor_contract() {
    accepts(
        "class A make: function(): Self = function(): Self return A {n = 1} end n: number end local a = A {n = 2} local b: A = a.make()",
    );
    rejects(
        "class A make: function(): Self = function(): Self return A {} end n: number end",
        "T001",
    );
}

#[test]
fn class_and_trait_names_do_not_shadow_builtin_type_spellings() {
    for name in ["number", "string", "boolean", "any", "Self"] {
        rejects(&format!("class {name} end"), "T014");
        rejects(&format!("trait {name} end"), "T014");
    }
    accepts("local string = 'ordinary Lua local' local number = 1 print(string, number)");
}
#[test]
fn function_signature_names_resolve_before_parameter_shadowing() {
    accepts(
        "class C function use(C: any, other: C): C return other end end local value = C {} local other: C = value:use(1, value)",
    );
    accepts(
        "class C end local function pass(C: any, value: C): C return value end local value: C = pass(1, C {})",
    );
    rejects(
        "class C end local function f(C: any): () local value: C = {} end",
        "T013",
    );
}

#[test]
fn constructors_check_expanded_argument_packs() {
    let prefix =
        "class C x: number end local function pair(): ({x: number}, number) return {x = 1}, 2 end ";
    rejects(&format!("{prefix} local c = C(pair())"), "T006");
    accepts(&format!("{prefix} local c = C((pair())) print(c.x)"));
    rejects(
        "class C end local function zero(): () end local c = C(zero())",
        "T006",
    );
    accepts(
        "class C x: number end local function one(): {x: number} return {x = 1} end local c = C(one())",
    );
    accepts("class C end local function zero(): () end local c = C({}, zero())");
    rejects(
        "class C end local function maybe(ok: boolean) if ok then return {} end end local c = C(maybe(true))",
        "T006",
    );
}
