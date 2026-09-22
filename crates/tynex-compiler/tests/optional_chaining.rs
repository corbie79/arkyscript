use std::collections::BTreeMap;
use tynex_compiler::{Mode, compile, declaration, prepare};

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
fn optional_record_access_preserves_required_and_nullable_types() {
    accepts("local function read(value: {name: string}?): string? return value?.name end");
    accepts("local function read(value: {name: string}): string return value?.name end");
    accepts("local value = {n = 1} local n: number = value?.n");
    rejects(
        "local function read(value: {name: string}?): string return value?.name end",
        "T001",
    );
    accepts("local nothing = nil local value: nil = nothing?.unknown.other");
}
#[test]
fn contiguous_chains_only_skip_when_an_explicit_guard_sees_nil() {
    accepts("local function read(value: {child: {n: number}}?): number? return value?.child.n end");
    accepts(
        "local function read(value: {child: {n: number}?}?): number? return value?.child?.n end",
    );
    rejects(
        "local function read(value: {child: {n: number}?}?): number? return value?.child.n end",
        "T005",
    );
    rejects(
        "local function read(value: {child: {n: number}}?): number? return (value?.child).n end",
        "T005",
    );
}
#[test]
fn optional_indexing_uses_normal_key_contracts() {
    accepts("local function read(value: {string}?): string? return value?[1] end");
    accepts(
        "local function read(value: {[string]: {n: number}}?, key: string): number? return value?[key]?.n end",
    );
    rejects(
        "local function read(value: {string}?): string? return value?['wrong'] end",
        "T001",
    );
    rejects(
        "local function read(value: {[string]: {n: number}}?, key: string): number? return value?[key].n end",
        "T005",
    );
}
#[test]
fn false_and_other_nonnullable_primitive_values_do_not_short_circuit() {
    rejects("local flag = false local x = flag?.name", "T005");
    rejects("local flag = false flag?()", "T007");
    rejects(
        "local function read(value: boolean?): number? return value?.n end",
        "T005",
    );
    accepts("local function read(value: {flag: boolean}?): boolean? return value?.flag end");
}
#[test]
fn optional_calls_validate_arguments_and_preserve_nonnullable_results() {
    accepts(
        "local function invoke(f: (function(number): number)?, n: number): number? return f?(n) end",
    );
    accepts(
        "local function invoke(f: function(number): number, n: number): number return f?(n) end",
    );
    rejects(
        "local function invoke(f: (function(number): number)?): number? return f?('bad') end",
        "T001",
    );
    rejects(
        "local function invoke(f: (function(number): number)?): number? return f?() end",
        "T006",
    );
    accepts("local f: (function(number): number)? = nil local answer: nil = f?(1)");
    rejects(
        "local f: (function(number): number)? = nil f?('bad')",
        "T001",
    );
}
#[test]
fn nested_callable_members_need_their_own_guard() {
    accepts("local function invoke(v: {f: (function(): number)?}?): number? return v?.f?() end");
    rejects(
        "local function invoke(v: {f: (function(): number)?}?): number? return v?.f() end",
        "T007",
    );
    accepts("local function invoke(v: {f: function(): {n: number}}?): number? return v?.f().n end");
}
#[test]
fn nil_branches_still_check_arguments_with_known_contracts() {
    rejects(
        "local f: (function(number): number)? = nil local function wrong(n: number): number return n end f?(wrong('x'))",
        "T001",
    );
    rejects(
        "local obj: {f: function(number): number}? = nil obj?.f('x')",
        "T001",
    );
    rejects(
        "local function number(n: number): number return n end local obj = nil local value = obj?[number('x')]",
        "T001",
    );
    rejects(
        "local function number(n: number): number return n end local obj = nil obj?.f(number('x'))",
        "T001",
    );
}
#[test]
fn optional_methods_support_classes_traits_and_strings() {
    accepts(
        "class C n: number = 1 function add(n: number): number return self.n + n end end local function invoke(c: C?): number? return c?:add(2) end",
    );
    accepts(
        "trait T read: function(Self): number end local function invoke(t: T?): number? return t?:read() end",
    );
    accepts("local function upper(s: string?): string? return s?:upper() end");
    rejects(
        "class C function add(n: number): number return n end end local function invoke(c: C?): number? return c?:add('x') end",
        "T001",
    );
    rejects(
        "trait T same: function(Self, Self): boolean end local function invoke(a: T?, b: T): boolean? return a?:same(b) end",
        "T015",
    );
    rejects(
        "class C end local function invoke(c: C?): number? return c?:missing() end",
        "T014",
    );
}
#[test]
fn optional_chain_call_results_expand_with_lua_rules() {
    accepts(
        "local function invoke(f: (function(): (number, string))?): (number?, string?) return f?() end",
    );
    rejects(
        "local function invoke(f: (function(): (number, string))?): number? return f?() end",
        "T006",
    );
    accepts("local function invoke(f: (function(): (number, string))?): number? return (f?()) end");
    accepts(
        "local function pair(): (number, string) return 1, 'ok' end local n: number, s: string = pair?()",
    );
    accepts(
        "local function invoke(f: (function(): (number, string))?): () local n: number?, s: string? = f?() end",
    );
    rejects(
        "local function invoke(f: (function(): (number, string))?): () local n: number, s: string = f?() end",
        "T001",
    );
}
#[test]
fn skipped_call_produces_one_nil_even_when_success_has_no_results() {
    accepts("local function none(): () end local function invoke(): () return none?() end");
    rejects(
        "local function invoke(f: (function(): ())?): () return f?() end",
        "T006",
    );
    accepts("local function invoke(f: (function(): ())?): nil return f?() end");
    accepts("local f: (function(): ())? = nil local function invoke(): nil return f?() end");
}
#[test]
fn optional_calls_expand_in_arguments_and_table_list_tails() {
    accepts(
        "local function pair(): (number, string) return 1, 'ok' end local function use(n: number, s: string): () end use(pair?())",
    );
    rejects(
        "local function use(n: number): () end local function invoke(f: (function(): (number, string))?): () use(f?()) end",
        "T006",
    );
    accepts(
        "local function pair(): (number, number) return 1, 2 end local values: {number} = {pair?()}",
    );
    rejects(
        "local function pair(): (number, string) return 1, 'x' end local values: {number} = {pair?()}",
        "T001",
    );
    accepts(
        "local function invoke(f: (function(): (number, number))?): {number?} return {f?()} end",
    );
}
#[test]
fn potentially_executed_calls_invalidate_captured_refinements() {
    rejects(
        "local n: number? = 1 local function invoke(f: (function(): ())?): () if n ~= nil then f?() local value: number = n end end",
        "T001",
    );
    rejects(
        "local n: number? = 1 local function invoke(v: {[number]: number}?, key: function(): number): () if n ~= nil then local value = v?[key()] local result: number = n end end",
        "T001",
    );
    accepts(
        "local n: number? = 1 local function invoke(v: {x: number}?): () if n ~= nil then local value = v?.x local result: number = n end end",
    );
}
#[test]
fn dynamic_optional_chains_keep_explicit_strict_boundaries() {
    accepts(
        "local dynamic: any = require('legacy') local result: any = dynamic?.field?[1]?:method()?()",
    );
    compile(
        "local dynamic = require('legacy') local result = dynamic?.field?()",
        Mode::Gradual,
        true,
    )
    .unwrap();
    assert!(
        compile(
            "local dynamic = require('legacy') local result = dynamic?.field?()",
            Mode::Strict,
            true
        )
        .is_err()
    );
    rejects(
        "local function invoke(f: (function(number): number)?): number? return f?(false) end",
        "T001",
    );
}
#[test]
fn static_dependency_discovery_survives_optional_require_and_skipped_arguments() {
    let source =
        "local one: any = require?('one') local nil_value = nil nil_value?.call(require('two'))";
    let prepared = prepare(source, true).unwrap();
    let imports = prepared.imports(Mode::Strict);
    assert_eq!(
        imports
            .iter()
            .filter_map(|i| i.name.as_deref())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
    let modules = BTreeMap::from([("one".into(), declaration("export: {n: number}").unwrap())]);
    prepare("local one = require?('one') local n: number = one.n", true)
        .unwrap()
        .compile(Mode::Strict, &modules)
        .unwrap();
}
#[test]
fn optional_constructor_calls_keep_nominal_types_and_argument_counts() {
    accepts("class C n: number end local c: C = C?({n = 1})");
    rejects("class C n: number end local c = C?({})", "T001");
    rejects(
        "class C end local function pair(): ({}, number) return {}, 1 end local c = C?(pair())",
        "T006",
    );
}

#[test]
fn optional_calls_forward_variadic_arguments_and_results() {
    accepts(
        "local function forward(f: (function(...number): (...number))?, ...: number): (...number?) return f?(...) end",
    );
    rejects(
        "local function forward(f: (function(...number): (...number))?, ...: string): (...number?) return f?(...) end",
        "T001",
    );
    rejects(
        "local function forward(f: (function(...number): (...number))?, ...: number): (...number) return f?(...) end",
        "T001",
    );
    accepts(
        "local function forward(f: function(...number): (...number), ...: number): (...number) return f?(...) end",
    );
}
#[test]
fn known_nil_narrowing_does_not_erase_method_contracts() {
    rejects(
        "class C function f(n: number): number return n end end local c: C? = nil c?:f('bad')",
        "T001",
    );
    accepts(
        "class C function f(n: number): number return n end end local c: C? = nil local result: nil = c?:f(1)",
    );
    rejects(
        "local function test(v: {f: function(number): number}?): () if v == nil then v?.f('bad') end end",
        "T001",
    );
}
#[test]
fn receiver_snapshot_does_not_hide_post_call_invalidations() {
    rejects(
        "local v: {n: number}? = {n = 1} local function key(): string v = nil return 'n' end if v ~= nil then local result = v?[key()] local value: {n: number} = v end",
        "T001",
    );
    accepts("local function fail(): number error('never') end local n: number = fail?()");
}

#[test]
fn audit_nil_and_never_paths_preserve_reachability() {
    accepts("local function stop(): number error?('stop') end");
    rejects(
        "local function stop(): number local nothing = nil nothing?.call() end",
        "T001",
    );
    let prefix = "local function choose(ok: boolean) if ok then return error end end local maybe = choose(false) ";
    accepts(&format!("{prefix} local n: nil = maybe?('stop')"));
    rejects(
        &format!("{prefix} local n: number = maybe?('stop')"),
        "T001",
    );
    accepts(&format!("{prefix} local n: nil = maybe?('stop')?.field?()"));
    rejects("local f: number? = nil f?()", "T007");
    accepts("local n: nil = (nil)?.field?(error('never evaluated'))");
}
#[test]
fn audit_optional_constructor_factories_keep_nominal_precision() {
    let prefix = "class C n: number end local function choose(ok: boolean) if ok then return C end end local constructor = choose(false) ";
    accepts(&format!(
        "{prefix} local value: C? = constructor?({{n = 1}})"
    ));
    rejects(
        &format!("{prefix} local value: C = constructor?({{n = 1}})"),
        "T001",
    );
    rejects(
        &format!("{prefix} local value = constructor?({{n = 'bad'}})"),
        "T001",
    );
}
#[test]
fn audit_contextual_table_arguments_still_validate_nested_members() {
    accepts(
        "local function use(f: (function({item: number?}): number)?): number? return f?({item = nil}) end",
    );
    rejects(
        "local function use(f: (function({item: number}): number)?): number? return f?({item = 'bad'}) end",
        "T001",
    );
    rejects(
        "local f: (function({item: number}): number)? = nil f?({item = 'bad'})",
        "T001",
    );
    accepts(
        "local function use(f: (function({number}): {number})?): {number}? return f?({1, 2, 3}) end",
    );
}
