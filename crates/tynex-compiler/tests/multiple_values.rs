use tynex_compiler::{Mode, compile};
fn good(source: &str) {
    compile(source, Mode::Strict, true).unwrap_or_else(|errors| panic!("{source}\n{errors:#?}"));
}
fn bad(source: &str, code: &str) {
    let errors = compile(source, Mode::Strict, true).expect_err(source);
    assert!(
        errors.iter().any(|e| e.code == code),
        "{source}\n{errors:#?}"
    );
}

#[test]
fn multiple_locals_assignments_and_shadowing_use_old_values() {
    good("local a,b=1,'s' local b,a=a,b local n:number=b local s:string=a");
    good("local a,b=1,2 a,b=b,a print(a,b)");
    bad("local a,b=1,'s' a,b=b,a", "T001");
    bad("local a:number,b:string=1,2", "T001");
    good("local a,b:number?=1 local n:number=a print(b)");
    bad("local a,b:number=1", "T001");
}
#[test]
fn missing_values_are_nil_and_extra_expressions_are_still_checked() {
    good("local function f() end local n:nil=f() local a,b=1 print(a,b)");
    bad("local a=1,math.abs('bad')", "T001");
    bad("local function f() end local n:number=f()", "T001");
}
#[test]
fn inferred_multiple_results_and_parentheses_adjust_differently() {
    good("local function f() return 1,'two' end local a:number,b:string=f()");
    good("local function f() return 1,'two' end local a:number,b:nil=(f())");
    bad(
        "local function f() return 1,'two' end local a:number,b:string=(f())",
        "T001",
    );
    good("local function f() return 1,'two' end local a:number,b:number,c:string=f(),f()");
    good("local function f() return 1,'two' end local a:number,b:number,c:nil=f(),3");
}
#[test]
fn return_annotations_cover_lists_zero_values_and_variadic_tails() {
    good("local function f(): (number,string) return 1,'s' end local a,b=f()");
    good("local function f(): () end f()");
    good("local function f(): (number,string?) return 1 end local a:number,b:nil=f()");
    bad("local function f(): (number,string) return 1 end", "T001");
    bad("local function f(): number return 1,2 end", "T006");
    bad("local function f(): () return nil end", "T006");
    good(
        "local function f(n:number,...:string): (number,...string) return n,... end print(f(1,'a','b'))",
    );
}
#[test]
fn branch_result_counts_preserve_missing_slots() {
    good(
        "local function f(b:boolean) if b then return 1,'s' else return 2 end end local a:number,b:string?=f(true)",
    );
    bad(
        "local function f(b:boolean) if b then return 1,'s' else return 2 end end local a:number,b:string=f(true)",
        "T001",
    );
    bad(
        "local function f(b:boolean): (number,string) if b then return 1,'s' end end",
        "T001",
    );
    good("local function f(b:boolean): number? if b then return 1 end end local n:number?=f(true)");
}
#[test]
fn calls_expand_only_the_final_argument_expression() {
    bad(
        "local function tables() return {1},{2} end for i,v in ipairs(tables()) do end",
        "T006",
    );
    good(
        "local function tables() return {1},{2} end for i,v in ipairs((tables())) do print(v+1) end",
    );
    good(
        "local function pair() return 1,'s' end local function use(n:number,s:string) end use(pair())",
    );
    bad(
        "local function pair() return 1,'s' end local function use(n:number,s:string) end use((pair()))",
        "T006",
    );
    good(
        "local function pair() return 1,'s' end local function use(n:number,s:string) end use(pair(),'x')",
    );
    bad(
        "local function pair() return 1,'s' end local function use(n:number) end use(pair())",
        "T006",
    );
    bad(
        "local function pair() return 1,'s' end math.max(pair())",
        "T001",
    );
}
#[test]
fn return_forwarding_preserves_expansion_and_grouping() {
    good(
        "local function pair() return 1,'s' end local function forward(): (number,string) return pair() end local a:number,b:string=forward()",
    );
    bad(
        "local function pair() return 1,'s' end local function forward(): (number,string) return (pair()) end",
        "T001",
    );
    good(
        "local function f():number return 1 end local function g(): (number,number) return f(),f() end",
    );
}
#[test]
fn varargs_are_typed_and_local_to_the_current_function() {
    good("local function f(...:number) return ... end local a:number?,b:number?=f(1,2)");
    good("local function f(...:number) local values:{number}={...} return values end f(1,2)");
    bad("local function f(...:number) end f(1,'bad')", "T001");
    bad("local function f(...) return ... end", "T002");
    bad("local function f() return ... end", "P001");
    bad(
        "local function f(...:number) local fn=function() return ... end end",
        "P001",
    );
    bad("local function f(...:number,x:number) end", "P001");
    good(
        "local function f(...:number) local fn=function(...:string) return ... end return ... end",
    );
    assert!(
        compile(
            "local function f(...) return ... end print(f(1,'s'))",
            Mode::Gradual,
            true
        )
        .is_ok()
    );
}
#[test]
fn varargs_in_single_value_positions_may_be_absent() {
    bad(
        "local function f(...:number):number return (...) end",
        "T001",
    );
    good("local function f(...:number):number? return (...) end");
    bad(
        "local function f(...:number) local n:number=... end",
        "T001",
    );
    bad("local function f(...:number) return ... + 1 end", "T001");
}
#[test]
fn table_final_list_fields_expand_and_named_fields_do_not() {
    bad(
        "local function f() return 1,'s' end local xs:{number}={f()}",
        "T001",
    );
    good("local function f() return 1,'s' end local xs:{number}={(f())}");
    good("local function f() return 1,'s' end local xs:{number}={f(),2}");
    good("local function f() return 1,'s' end local r:{x:number}={x=f()}");
    good("local function f() end local xs:{number}={f()}");
}
#[test]
fn general_function_declarations_and_qualified_recursion() {
    good("function double(n:number):number return n*2 end local n:number=double(2)");
    good("local m={} function m.double(n:number):number return n*2 end local n:number=m.double(2)");
    good(
        "local m={nested={}} function m.nested.f(n:number):number if n==0 then return 1 end return m.nested.f(n-1) end",
    );
    bad(
        "local m={} function m.f(n:number):number return n end m.f('s')",
        "T001",
    );
    bad("local m={f=1} function m.f() end", "T001");
    bad(
        "local function f(n:number):number return n end function f(n:string):string return n end",
        "T001",
    );
}
#[test]
fn methods_check_self_fields_explicit_arguments_and_dot_calls() {
    good(
        "local counter={n=0} function counter:add(amount:number):number self.n=self.n+amount return self.n end local n:number=counter:add(2) local m:number=counter.add(counter,3)",
    );
    bad(
        "local counter={n=0} function counter:add(amount:number):number self.n=self.n+amount return self.n end counter:add('s')",
        "T001",
    );
    bad(
        "local counter={n=0} function counter:add(amount:number) self.n='bad' end",
        "T001",
    );
    bad(
        "local counter={n=0} function counter:add(amount:number) self.n=self.n+amount end counter.add({n='bad'},1)",
        "T001",
    );
    bad("local m={} function m:f(self:number) end", "T004");
    good("local m:any={} function m:f(n:number) return n end m:f(1)");
}
#[test]
fn methods_on_preexisting_function_fields_and_strings() {
    good("local t={f=function(self:any,n:number):number return n end} local n:number=t:f(1)");
    good("local s:string=('abc'):upper() local n:number=s:len() print(s:sub(1,2))");
    bad("local s='abc' s:sub('bad')", "T001");
    bad("local t={f=1} t:f()", "T007");
}
#[test]
fn conditional_new_members_are_diagnosed_instead_of_claiming_definite_presence() {
    bad(
        "local m={} if false then function m.f() end end m.f()",
        "T011",
    );
    bad(
        "while false do function global_fn() end end global_fn()",
        "T011",
    );
    good(
        "local m={} function m.f():number return 1 end if true then function m.f():number return 2 end end",
    );
}
#[test]
fn signatures_and_varargs_preserve_source_bytes_lines_and_lua_method_syntax() {
    let source = "-- 한국어\nlocal m={}\nfunction m:f(n: number, ...: string): (number, ...string)\n return n,...\nend\nlocal a: number,b: string?=m:f(1,'s')\n";
    let generated = compile(source, Mode::Strict, true).unwrap().lua;
    assert_eq!(generated.len(), source.len());
    assert_eq!(
        source
            .match_indices('\n')
            .map(|(i, _)| i)
            .collect::<Vec<_>>(),
        generated
            .match_indices('\n')
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    );
    assert!(generated.contains("function m:f(n"));
    assert!(generated.contains("return n,..."));
    assert!(generated.contains("m:f(1,'s')"));
}
#[test]
fn gradual_multiple_values_still_reject_known_mismatches() {
    for source in [
        "local a:number,b:string=1,2",
        "local function f() return 1,'bad' end local a:number,b:number=f()",
        "local function f(...:number) end f(1,'bad')",
    ] {
        assert!(compile(source, Mode::Gradual, true).is_err(), "{source}");
    }
}

#[test]
fn select_and_unpack_have_variadic_signatures() {
    good(
        "local function count(...:any):number return select('#',...) end local n:number=count(1,nil,'s')",
    );
    good("local a:number?,b:number?=unpack({1,2})");
    good("local n:number?=select(2,1,2,3)");
    good("local pick=select local n:number=pick('#',1,2)");
    bad("unpack(1)", "T005");
    bad("unpack({1},'bad')", "T001");
    bad("select()", "T006");
    bad("select('invalid',1)", "T001");
    bad("local a:number=unpack({1,2})", "T001");
}

#[test]
fn declared_return_contract_survives_dynamic_forwarding() {
    bad(
        "local api:any=require('legacy') local function f():string return api.f() end local n:number=f()",
        "T001",
    );
    good(
        "local api:any=require('legacy') local function f(): (number,string) return api.f() end local a:number,b:string=f()",
    );
}
#[test]
fn method_type_snapshots_are_shared_without_exponential_expansion() {
    let mut source = String::from("local t={n=1} ");
    for i in 0..80 {
        source.push_str(&format!("function t:m{i}() return self end "));
    }
    source.push_str("local result=t:m79() print(result.n)");
    good(&source);
}
#[test]
fn local_string_shadowing_does_not_change_string_metatable_methods() {
    good("local string=1 local s:string=('hi'):upper() print(s,string)");
}
