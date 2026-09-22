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
fn record_fields_and_array_elements_are_checked() {
    good("local p={name='Mina',score=1} p.score=2 print(p.name .. p.score)");
    bad("local p={score=1} p.score='bad'", "T001");
    bad("local p={score=1} print(p.missing)", "T005");
    good("local xs: {number}={} xs[1]=4 local n=xs[1] if n~=nil then print(n+1) end");
    bad("local xs: {number}={} xs[1]='bad'", "T001");
    bad("local xs: {number}={1} local n:number=xs[10]", "T001");
}
#[test]
fn fresh_literals_are_contextualized_but_mutable_aliases_are_invariant() {
    good("local p:{score:number?}={score=1} p.score=nil");
    good("local p:{child:{score:number?}}={child={score=1}}");
    bad("local p={score=1} local wider:{score:number?}=p", "T001");
    bad(
        "local p={score=1} local outer:{child:{score:number?}}={child=p}",
        "T001",
    );
    bad("local xs:{number}={1} local ys:{number?}=xs", "T001");
}
#[test]
fn table_contracts_check_missing_extra_and_nested_fields() {
    good("local p:{name:string,score:number?}={name='Mina'} print(p.name)");
    bad("local p:{name:string}={}", "T001");
    bad("local p:{name:string}={name='Mina',extra=1}", "T005");
    bad(
        "local p:{child:{score:number}}={child={score='bad'}}",
        "T001",
    );
    bad("local t:{[number?]:string}={}", "P001");
}
#[test]
fn string_keys_are_decoded_like_lua_not_compared_as_raw_tokens() {
    good("local p={name=1} print(p['na\\x6de']+p['\\110ame'])");
    good("local p={['\\0']=1} print(p['\\000'])");
    good("local p={[ [[name]] ]=1} print(p.name+1)");
    bad("local p={name=1,['na\\x6de']=2}", "T005");
    bad("local p={[nil]=1}", "T005");
}
#[test]
fn dictionaries_use_optional_reads_and_checked_writes() {
    good(
        "local counts:{[string]:number}={} counts.foo=1 local n=counts['bar'] if n~=nil then print(n+1) end",
    );
    bad("local counts:{[string]:number}={} counts.foo=false", "T001");
    bad("local counts:{[string]:number}={} counts[1]=2", "T001");
    good("local t={a=1,b=2} local key='a' local n=t[key] if n then print(n+1) end");
}
#[test]
fn branches_infer_returns_and_detect_fallthrough() {
    good(
        "local function f(n:number):number if n>0 then return n elseif n==0 then return 1 else return -n end end print(f(1))",
    );
    bad(
        "local function f(n:number):number if n>0 then return n end end",
        "T001",
    );
    bad(
        "local function f(n:number):number if n>0 then return n else return 'bad' end end",
        "T001",
    );
    bad(
        "local function f(n:number) if n>0 then return n else return 'bad' end end local n:number=f(1)",
        "T001",
    );
    good("local function f(n:number):number if n<0 then error('negative') end return n end");
}
#[test]
fn nil_guards_and_early_returns_narrow_locals() {
    good("local function f(n:number?):number if n==nil then return 0 end return n+1 end");
    good("local function f(n:number?):number if nil~=n then return n+1 else return 0 end end");
    good("local function f(n:number?):number if not n then return 0 else return n+1 end end");
    bad(
        "local function f(n:number?):number if n~=nil then n=nil return n+1 end return 0 end",
        "T001",
    );
}
#[test]
fn branch_merges_preserve_only_guaranteed_refinements() {
    good(
        "local function f(flag:boolean,n:number?):number if flag then n=1 else n=2 end return n+1 end",
    );
    bad(
        "local function f(flag:boolean,n:number?):number if flag then n=1 end return n+1 end",
        "T001",
    );
    bad(
        "local n:number?=1 if n~=nil then print(n) print(n+1) end",
        "T001",
    );
}
#[test]
fn deferred_closures_and_calls_do_not_reuse_stale_narrowing() {
    bad(
        "local n:number?=1 local fn=function():number return n+1 end",
        "T001",
    );
    bad(
        "local n:number?=1 local function clear() n=nil end if n~=nil then clear() print(n+1) end",
        "T001",
    );
    bad(
        "local n:number?=1 local function clear():boolean n=nil return true end if n~=nil and clear() then print(n+1) end",
        "T001",
    );
    good(
        "local n:number?=1 local function clear() n=nil end if n~=nil then local saved=n clear() print(saved+1) end",
    );
}
#[test]
fn short_circuit_expressions_check_the_guarded_rhs() {
    good("local function f(n:number?):number return n or 0 end");
    good("local function f(n:number?):number? return n and n+1 end");
    good("local function f(n:number?):boolean return n~=nil and n>0 end");
    bad(
        "local function f(n:number?):boolean return n==nil and n>0 end",
        "T001",
    );
}
#[test]
fn loops_have_lexical_scopes_and_check_conditions_and_bounds() {
    good(
        "local sum=0 for i=1,10,2 do sum=sum+i end while sum>0 do sum=sum-1 end repeat local done=true until done print(sum)",
    );
    bad("for i='one',3 do print(i) end", "T001");
    bad("for i=1,3 do end print(i)", "T003");
    bad("repeat local done=true until done print(done)", "T003");
    bad("if true then local x=1 end print(x)", "T003");
    bad("do local x=1 end print(x)", "T003");
}
#[test]
fn break_is_scoped_to_the_current_function_and_loop() {
    good("for i=1,3 do if i==2 then break end print(i) end");
    bad("break", "P001");
    bad("while true do local fn=function() break end end", "P001");
    bad("while true do break print(1) end", "P001");
}
#[test]
fn loop_returns_do_not_hide_possible_fallthrough() {
    bad(
        "local function f(n:number):number while n>0 do return n end end",
        "T001",
    );
    bad(
        "local function f():number for i=1,0 do return i end end",
        "T001",
    );
    good("local function f():number repeat return 1 until true end");
    good("local function f():number while true do return 1 end end");
    bad(
        "local function f(n:boolean):number while true do if n then break end return 1 end end",
        "T001",
    );
}
#[test]
fn ipairs_and_pairs_infer_loop_values_without_leaking_loop_names() {
    good(
        "local xs:{number}={1,2,3} local total=0 for i,value in ipairs(xs) do total=total+i+value end print(total)",
    );
    good("local t={a=1,b=2} for key,value in pairs(t) do print(key .. value) end");
    bad(
        "for i,value in ipairs({1}) do local n:string=value end",
        "T001",
    );
    bad("for key,value in pairs({a=1}) do end print(value)", "T003");
    bad("for key,value in pairs(1) do end", "T005");
    good("local xs:{number?}={1,nil} for i,value in ipairs(xs) do print(value+1) end");
}
#[test]
fn shadowing_a_builtin_iterator_does_not_get_special_types() {
    bad(
        "local ipairs=function(x:any) return 1 end for i,value in ipairs({1}) do end",
        "T010",
    );
    good("local iterate=ipairs for i,value in iterate({1}) do print(value+1) end");
}
#[test]
fn anonymous_functions_table_functions_and_call_sugar_work() {
    good("local fn=function(n:number):number return n+1 end print(fn(1))");
    good("local t={f=function(n:number):number return n+1 end} print(t.f(1))");
    good("print 'hello' print {1,2,3} print((function():number return 42 end)())");
    bad(
        "local t={f=function(n:number):number return n end} t.f('bad')",
        "T001",
    );
    bad("local x={a=1}.a", "P001");
}
#[test]
fn standard_numeric_and_string_helpers_have_signatures() {
    good(
        "local n=math.abs(-1)+math.max(1,2,3) local s=string.upper('hi') print(string.sub(s,1),n)",
    );
    bad("print(math.abs('bad'))", "T001");
    bad("print(math.max(1,'bad'))", "T001");
    bad("print(string.sub('hello',true))", "T001");
}
#[test]
fn gradual_mode_does_not_suppress_known_errors_in_new_constructs() {
    assert!(compile("local p={score=1} p.score='bad'", Mode::Gradual, true).is_err());
    assert!(compile("for i='bad',3 do end", Mode::Gradual, true).is_err());
    assert!(
        compile(
            "local t={} t.dynamic=1 print(t.dynamic)",
            Mode::Gradual,
            true
        )
        .is_ok()
    );
}

#[test]
fn unknown_paths_do_not_mask_known_mismatches_and_nested_unknowns_are_explicit() {
    let source = "local function f(flag) if flag then return external() else return 'bad' end end local n:number=f(true)";
    assert!(compile(source, Mode::Gradual, true).is_err());
    bad("local t={data=require('legacy')}", "T002");
    bad(
        "local function f(flag:boolean) if flag then return require('legacy') else return 1 end end",
        "T002",
    );
    good("local t:{data:any}={data=require('legacy')}");
    good("local t:{score:number?}?={score=1}");
    bad("ipairs(1)", "T005");
    bad("pairs()", "T006");
}
