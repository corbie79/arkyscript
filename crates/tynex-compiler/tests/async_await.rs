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
fn async_calls_produce_tasks_and_await_checks_body_results() {
    accepts(
        "local async function answer(): number return 42 end local t: Task<number> = answer() local n: number = scheduler.run(t)",
    );
    accepts(
        "local async function answer(): number return 42 end local async function main(): number return await answer() end local n: number = scheduler.run(main())",
    );
    rejects(
        "local async function answer(): number return 'wrong' end",
        "T001",
    );
    rejects(
        "local async function answer(): number return 42 end local n: number = answer()",
        "T001",
    );
    rejects("local async function answer(): number end", "T001");
}
#[test]
fn async_results_infer_and_missing_parameters_follow_mode() {
    accepts(
        "local async function answer() return 42 end local t: Task<number> = answer() local n: number = scheduler.run(t)",
    );
    compile("local async function echo(value) return value end local t = echo('ok') print(scheduler.run(t))", Mode::Gradual, true).unwrap();
    let errors = compile(
        "local async function echo(value) return value end",
        Mode::Strict,
        true,
    )
    .unwrap_err();
    assert!(errors.iter().any(|e| e.code == "T002"));
}
#[test]
fn async_anonymous_and_function_type_annotations_agree() {
    accepts(
        "local callback: async function(number): string = async function(n: number): string return tostring(n) end local t: Task<string> = callback(1)",
    );
    accepts(
        "local callback: function(number): Task<string> = async function(n: number): string return tostring(n) end local t = callback(1)",
    );
    rejects(
        "local callback: async function(number): string = function(n: number): string return tostring(n) end",
        "T001",
    );
    rejects(
        "local callback: async function(number): string = async function(n: number): number return n end",
        "T001",
    );
}
#[test]
fn await_rejects_non_tasks_and_nullable_tasks() {
    rejects(
        "local async function invalid(): number return await 1 end",
        "T016",
    );
    rejects(
        "local async function invalid(): number return await nil end",
        "T016",
    );
    rejects(
        "local async function invalid(value: Task<number>?): number return await value end",
        "T016",
    );
    accepts(
        "local async function valid(value: Task<number>?): number if value ~= nil then return await value end return 0 end",
    );
    rejects(
        "local async function invalid(value: {result: function(): number}): number return await value end",
        "T016",
    );
}
#[test]
fn await_expands_only_in_final_lua_value_positions() {
    accepts(
        "local async function pair(): (number, string) return 1, 'ok' end local async function main(): (number, string) return await pair() end local n: number, s: string = scheduler.run(main())",
    );
    accepts(
        "local async function main(t: Task<number, string>): () local n: number, s: string = await t end",
    );
    rejects(
        "local async function main(t: Task<number, string>): number return await t end",
        "T006",
    );
    accepts("local async function main(t: Task<number, string>): number return (await t) end");
    accepts(
        "local async function main(t: Task<number, string>): (number, boolean) return await t, true end",
    );
    accepts(
        "local function use(n: number, s: string): () end local async function main(t: Task<number, string>): () use(await t) end",
    );
    rejects(
        "local function use(n: number): () end local async function main(t: Task<number, string>): () use(await t) end",
        "T006",
    );
}
#[test]
fn empty_and_variadic_async_result_packs_remain_distinct() {
    accepts(
        "local async function empty(): () end local t: Task<> = empty() local async function main(): () return await t end",
    );
    rejects(
        "local async function empty(): () end local t: Task<number> = empty()",
        "T001",
    );
    rejects(
        "local async function value(): nil return nil end local t: Task<> = value()",
        "T001",
    );
    accepts(
        "local async function values(...: number): (...number) return ... end local t: Task<...number> = values(1, 2) local async function main(): (...number) return await t end",
    );
    rejects(
        "local async function values(...: string): (...string) return ... end local t: Task<...number> = values('wrong')",
        "T001",
    );
    accepts(
        "local async function main(t: Task<number, ...string>): (number, ...string) return await t end",
    );
}
#[test]
fn tasks_are_covariant_only_in_immutable_result_positions() {
    accepts("local async function value(): number return 1 end local t: Task<number?> = value()");
    rejects(
        "local async function value(): number? return nil end local t: Task<number> = value()",
        "T001",
    );
    rejects(
        "local async function values(): (number, string) return 1, 'x' end local t: Task<number> = values()",
        "T001",
    );
    rejects(
        "local async function value(): {n: number} return {n = 1} end local t: Task<{n: number?}> = value()",
        "T001",
    );
    accepts(
        "trait T f: function(Self): number end class C is T function f(): number return 1 end end local async function value(): C return C {} end local t: Task<T> = value()",
    );
}
#[test]
fn task_methods_expose_checked_results_and_immutable_protocol() {
    accepts(
        "local async function value(): (number, string) return 1, 'ok' end local t = value() local ready: boolean = t:ready() local status: string = t:status() local n: number, s: string = t:result() local cancelled: boolean = t:cancel()",
    );
    rejects(
        "local async function value(): number return 1 end local t = value() local n: string = t:result()",
        "T001",
    );
    rejects(
        "local async function value(): number return 1 end local t = value() t:ready(1)",
        "T006",
    );
    rejects(
        "local async function value(): number return 1 end local t = value() t.result = function(): number return 2 end",
        "T016",
    );
    rejects(
        "local async function value(): number return 1 end local t = value() t:unknown()",
        "T016",
    );
}
#[test]
fn scheduler_run_preserves_packs_and_follows_lexical_binding() {
    accepts(
        "local async function value(): (number, string) return 1, 'ok' end local run = scheduler.run local n: number, s: string = run(value())",
    );
    rejects(
        "local async function value(): number return 1 end local n: string = scheduler.run(value())",
        "T001",
    );
    rejects("scheduler.run()", "T006");
    rejects("scheduler.run(1)", "T016");
    rejects(
        "local async function value(): number return 1 end scheduler.run(value(), value())",
        "T006",
    );
    accepts(
        "local scheduler = {run = function(n: number): string return tostring(n) end} local s: string = scheduler.run(1)",
    );
}
#[test]
fn scheduler_step_pending_and_worker_adapter_have_contracts() {
    accepts(
        "local pending: number = scheduler.pending() local more: boolean = scheduler.step() local again: boolean = scheduler.step(10) local handle: any = require('legacy') local task: Task<any> = scheduler.from_worker(handle)",
    );
    rejects("scheduler.step('bad')", "T001");
    rejects("scheduler.pending(1)", "T006");
    rejects("scheduler.from_worker()", "T006");
    accepts(
        "local async function worker(handle: any): any return await scheduler.from_worker(handle) end",
    );
}
#[test]
fn await_invalidates_captured_flow_refinements_even_for_existing_tasks() {
    rejects(
        "local value: number? = 1 local async function use(t: Task<>): number if value ~= nil then await t return value end return 0 end",
        "T001",
    );
    accepts(
        "local value: number? = 1 local async function use(t: Task<>): number await t if value ~= nil then return value end return 0 end",
    );
}
#[test]
fn recursive_async_functions_and_forward_methods_use_task_signatures() {
    accepts(
        "local async function count(n: number): number if n == 0 then return 0 end return 1 + await count(n - 1) end local value: number = scheduler.run(count(3))",
    );
    accepts(
        "class C async function first(): number return await self:second() end async function second(): number return 2 end end local value: number = scheduler.run(C {}:first())",
    );
    rejects(
        "class C async function first(): number return self:second() end async function second(): number return 2 end end",
        "T001",
    );
}
#[test]
fn async_traits_and_self_substitution_preserve_concrete_and_abstract_types() {
    accepts(
        "trait T read: async function(Self): string async function label(): string return await self:read() end end class C is T async function read(): string return 'ok' end end local async function use(value: T): string return await value:label() end local result: string = scheduler.run(use(C {}))",
    );
    rejects(
        "trait T read: async function(Self): string end class C is T function read(): string return 'wrong kind' end end",
        "T014",
    );
    accepts(
        "trait Clone copy: async function(Self): Self end class C is Clone async function copy(): Self return self end end local async function duplicate(value: Clone): Clone return await value:copy() end",
    );
    accepts(
        "class Node next: Self? async function copy(): Self return self end end local t: Task<Node> = Node {}:copy()",
    );
}
#[test]
fn async_self_contracts_do_not_allow_unsafe_nested_mutability() {
    rejects(
        "trait T items: async function(Self): {Self} end local async function unsafe(value: T) return await value:items() end",
        "T015",
    );
    rejects(
        "trait T same: async function(Self, Self): boolean end local async function unsafe(a: T, b: T): boolean return await a:same(b) end",
        "T015",
    );
}
#[test]
fn async_named_table_methods_and_receiver_contracts_work() {
    accepts(
        "local m = {n = 1} async function m:read(add: number): number return self.n + add end local value: number = scheduler.run(m:read(2))",
    );
    rejects(
        "local m = {n = 1} async function m:read(add: number): number return self.n + add end m:read('x')",
        "T001",
    );
    accepts(
        "local m = {} async function m.read(n: number): number return n end local t: Task<number> = m.read(1)",
    );
}
#[test]
fn optional_async_calls_require_a_non_nil_task_before_awaiting() {
    accepts(
        "local async function invoke(f: (async function(): number)?): number local task = f?() if task ~= nil then return await task end return 0 end",
    );
    rejects(
        "local async function invoke(f: (async function(): number)?): number return await f?() end",
        "T016",
    );
    accepts(
        "local async function invoke(f: async function(): number): number return await f?() end",
    );
    accepts("local async function invoke(t: Task<{n: number}?>): number? return (await t)?.n end");
}
#[test]
fn await_in_table_tails_respects_return_expansion() {
    accepts("local async function values(t: Task<number, number>): {number} return {await t} end");
    rejects(
        "local async function values(t: Task<number, string>): {number} return {await t} end",
        "T001",
    );
    accepts(
        "local async function values(t: Task<number, string>): {number} return {(await t)} end",
    );
}
#[test]
fn nested_tasks_are_not_implicitly_flattened() {
    accepts(
        "local async function value(): number return 1 end local async function wrap(): Task<number> return value() end local async function main(): number local inner: Task<number> = await wrap() return await inner end",
    );
    rejects(
        "local async function value(): number return 1 end local async function wrap(): Task<number> return value() end local async function main(): number return await wrap() end",
        "T001",
    );
}
#[test]
fn task_name_is_reserved_only_for_type_declarations() {
    rejects("class Task end", "T014");
    rejects("trait Task end", "T014");
    accepts("local Task = 1 print(Task)");
}
#[test]
fn imported_async_interfaces_and_declarations_keep_task_contracts() {
    let module = compile("class C n: number = 1 end local async function make(): C return C {} end return {C = C, make = make}", Mode::Strict, true).unwrap().interface;
    let modules = BTreeMap::from([("model".into(), module), ("legacy".into(), declaration("export: {make: async function(number): (number, string), ready: function(): Task<boolean>}").unwrap())]);
    let good = prepare("import model from 'model' import legacy from 'legacy' local task: Task<model.C> = model.make() local c: model.C = scheduler.run(task) local n: number, text: string = scheduler.run(legacy.make(1)) local flag: boolean = scheduler.run(legacy.ready())", true).unwrap();
    good.compile(Mode::Strict, &modules).unwrap();
    let errors = prepare(
        "import legacy from 'legacy' local text: number = scheduler.run(legacy.ready())",
        true,
    )
    .unwrap()
    .compile(Mode::Strict, &modules)
    .unwrap_err();
    assert!(errors.iter().any(|e| e.code == "T001"));
}

#[test]
fn async_bodies_cannot_pump_the_scheduler_but_shadowing_is_ordinary() {
    rejects(
        "local async function bad(t: Task<number>): number return scheduler.run(t) end",
        "T016",
    );
    rejects(
        "local async function bad(): boolean return scheduler.step() end",
        "T016",
    );
    rejects(
        "local step = scheduler.step local async function bad(): boolean return step(1) end",
        "T016",
    );
    accepts(
        "local async function good(): boolean local scheduler = {step = function(): boolean return true end} return scheduler.step() end",
    );
    accepts(
        "local async function good(): number local function sync(t: Task<number>): number return scheduler.run(t) end return 1 end",
    );
}

#[test]
fn task_covariance_preserves_result_counts_including_nullable_slots() {
    rejects(
        "local async function empty(): () end local task: Task<number?> = empty()",
        "T001",
    );
    rejects(
        "local async function maybe(ok: boolean) if ok then return 1 end end local task: Task<number?> = maybe(false)",
        "T001",
    );
    accepts(
        "local async function nullable(): number? return nil end local task: Task<number?> = nullable()",
    );
    accepts("local async function empty(): () end local task: Task<...number> = empty()");
    rejects(
        "local async function empty(): () end local task: Task<number?, ...number> = empty()",
        "T001",
    );
}

#[test]
fn annotated_async_results_require_all_fixed_slots_including_nil() {
    rejects("local async function maybe(): number? end", "T006");
    rejects(
        "local async function maybe(ok: boolean): number? if ok then return 1 end end",
        "T006",
    );
    rejects(
        "local async function maybe(): (number?, ...string) return end",
        "T006",
    );
    accepts(
        "local async function maybe(ok: boolean): number? if ok then return 1 end return nil end local task: Task<number?> = maybe(false)",
    );
    accepts("local function legacy(): number? end");
    accepts(
        "local function legacy(): number? end local async function adapt(): number? return (legacy()) end",
    );
}
#[test]
fn forward_async_methods_cannot_hide_missing_nullable_result_slots() {
    rejects(
        "class C async function first(): number? return await self:second() end async function second(): number? end end",
        "T006",
    );
    accepts(
        "class C async function first(): number? return await self:second() end async function second(): number? return nil end end local task: Task<number?> = C {}:first()",
    );
}

#[test]
fn task_annotations_hold_at_explicit_dynamic_boundaries() {
    rejects(
        "local handle: any = require('legacy') local t: Task<number> = scheduler.from_worker(handle) local bad: string = scheduler.run(t)",
        "T001",
    );
    rejects(
        "local async function number(): number return 1 end local t: Task<number> = number() local handle: any = require('legacy') t = scheduler.from_worker(handle) local bad: string = scheduler.run(t)",
        "T001",
    );
    accepts(
        "local handle: any = require('legacy') local t: Task<number>? = scheduler.from_worker(handle) local good: number = scheduler.run(t)",
    );
    rejects(
        "local handle: any = require('legacy') local t: Task<number>? = scheduler.from_worker(handle) local bad: string = scheduler.run(t)",
        "T001",
    );
}
