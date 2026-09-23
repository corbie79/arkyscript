"""Nil-only optional access/calls preserve Lua evaluation and return-list rules."""
import json
from pathlib import Path
import unittest

import test_cli as support


class OptionalChainingTests(unittest.TestCase):
    setUpClass = classmethod(support.RustLuaJITTests.setUpClass.__func__)
    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli

    def run_source(self, source, expected):
        main = self.file('main.luax', source)
        result = self.cli('run', main)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected)
        return main

    def test_shipped_example(self):
        result = self.cli('run', support.ROOT/'examples/optional_chaining.luax')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'Mina\t10\nnil\tnil\ndone\n42\tnil\n')

    def test_receivers_once_and_skipped_keys_arguments_are_lazy(self):
        self.run_source('''
            --!typecheck strict
            local reads, arguments, keys = 0,0,0
            local function receiver(present:boolean): any
                reads=reads+1
                if present then return {child={n=42}} end
                return nil
            end
            local function key():string keys=keys+1 return "child" end
            local function argument():number arguments=arguments+1 return 10 end
            local function call(callback:(function(number):number)?):number?
                return callback?(argument())
            end
            print(receiver(false)?[key()].n)
            print(receiver(true)?[key()].n)
            print(call(nil))
            print(call(function(n:number):number return n+1 end))
            print(reads,arguments,keys)
        ''', 'nil\n42\nnil\n11\n2\t1\t1\n')

    def test_methods_keep_self_and_multiple_returns(self):
        self.run_source('''
            --!typecheck strict
            trait Named
                label:function(Self):string
            end
            class Player is Named
                name:string
                function label():string return self.name end
                function values(): (string,nil,number,nil) return self.name,nil,42,nil end
            end
            local function show(player:Player?):()
                print(select('#',player?:values()),player?:values())
            end
            local function named(value:Named?):string?
                return value?:label()
            end
            show(Player{name="Mina"})
            show(nil)
            print(named(Player{name="Joon"}),named(nil))
        ''', '4\tMina\tnil\t42\tnil\n1\tnil\nJoon\tnil\n')

    def test_false_is_never_treated_as_nil_and_errors_are_not_swallowed(self):
        self.run_source('''
            --!typecheck strict
            local f:any=false
            try f?() except err then print("false function rejected") end
            try print(f?.value) except err then print("false receiver rejected") end
            local t:any={flag=false}
            print(t?.flag)
            local broken:any=function() error("body failed",0) end
            try broken?() except err then print(err) end
            local empty:any={}
            try empty?:missing() except err then print("missing method rejected") end
        ''', 'false function rejected\nfalse receiver rejected\nfalse\nbody failed\nmissing method rejected\n')

    def test_short_circuit_spans_only_the_contiguous_chain(self):
        self.run_source('''
            --!typecheck strict
            local missing:any=nil
            local empty:any={}
            print(missing?.child.value)
            print(empty?.child?.value)
            try print(empty?.child.value) except err then print("unguarded child") end
            try print((missing?.child).value) except err then print("group ends chain") end
            print(missing?.child?.call?())
        ''', 'nil\nnil\nunguarded child\ngroup ends chain\nnil\n')

    def test_dot_call_and_optional_callable_field_use_lua_function_rules(self):
        self.run_source('''
            --!typecheck strict
            local function call(api:{run:(function(number):number)?}?):number?
                return api?.run?(21)
            end
            print(call(nil))
            print(call({run=nil}))
            print(call({run=function(n:number):number return n*2 end}))
            local api={run=function(n:number):number return n+1 end}
            print(api?.run(41))
            local function factory(present:boolean):(function(number):number)?
                if present then
                    return function(n:number):number return n+1 end
                end
                return nil
            end
            print(factory(true)?(41),factory(false)?(41))
        ''', 'nil\nnil\n42\n42\n42\tnil\n')

    def test_varargs_nil_slots_zero_returns_and_grouped_results(self):
        self.run_source('''
            --!typecheck strict
            local function forward(callback:(function(...any):(...any))?, ...:any):(...any)
                return callback?(...)
            end
            local function echo(...:any):(...any) return ... end
            local function empty():() end
            print(select('#',forward(echo,1,nil,3,nil)),forward(echo,1,nil,3,nil))
            print(select('#',forward(nil,1,nil)),forward(nil,1,nil))
            print(select('#',empty?()))
            print(select('#',(echo?(1,nil,3))))
            local t={echo?(1,2,3)}
            print(#t,t[1],t[3])
            local function nested(callback:(function(...any):(...any))?, ...:any):(...any)
                return callback?(echo?(...))
            end
            print(select('#',nested(echo,1,nil,3,nil)),nested(echo,1,nil,3,nil))
        ''', '4\t1\tnil\t3\tnil\n1\tnil\n0\n1\n3\t1\t3\n4\t1\tnil\t3\tnil\n')

    def test_nested_chains_class_defaults_and_try_returns(self):
        self.run_source('''
            --!typecheck strict
            local callback:any=function(n:number):{n:number} return {n=n} end
            local inputs:any={n=40}
            class Result
                n:number=callback?(inputs?.n)?.n + 2
                function get():number
                    try return self?.n except err then error(err) end
                end
            end
            local result=Result{}
            print(result?:get())
            local function sum(...:number):number
                local values={...}
                local n=0
                for i,v in ipairs(values) do n=n+v end
                return n
            end
            local function apply(...:number):number
                try return sum?(...) except err then error(err) end
            end
            print(apply(20,22))
        ''', '42\n42\n')

    def test_metamethod_receiver_and_method_lookup_only_once(self):
        self.file('meta.lua', 'return {create=setmetatable}')
        self.file('meta.d.luax', 'declare create: function(any, any): any')
        self.run_source('''
            --!typecheck strict
            import meta from "meta"
            local reads, calls=0,0
            local object:any=meta.create({n=40},{__index=function(self:any,key:string):any
                reads=reads+1
                return function(receiver:any,amount:number):number
                    calls=calls+1
                    return receiver.n+amount
                end
            end})
            print(object?:add(2),reads,calls)
        ''', '42\t1\t1\n')

    def test_static_module_discovery_still_covers_optional_calls(self):
        self.file('api.luax','print("loaded") return {n=42}')
        self.run_source('''
            --!typecheck strict
            local callback:(function(any):())?=nil
            callback?(require("api"))
            print("after skipped")
            local api=require?("api")
            print(api.n)
        ''', 'after skipped\nloaded\n42\n')

    def test_known_errors_stop_before_execution(self):
        failures = [
            'local n=1 n?()',
            'local flag=false flag?()',
            'local function f(n:number):() end f?("bad")',
            'class C function f(n:number):() end end local c=C{} c?:f("bad")',
            'local function f(c:{n:number}?):number return c?.n end',
            'local function f(c:{child:{n:number}?}?):number? return c?.child.n end',
            'local t:any={} t?.n=1',
            'local t:any={} t?[1]=1',
            'local t:any=nil local n=t ?? 42',
        ]
        for mode in ['gradual','strict']:
            for source in failures:
                with self.subTest(mode=mode,source=source):
                    main=self.file('main.luax', f'--!typecheck {mode}\nprint("NEVER")\n'+source)
                    result=self.cli('run',main)
                    self.assertNotEqual(result.returncode,0)
                    self.assertEqual(result.stdout,'')

    def test_original_lines_and_debug_bundle_survive_multiline_chain(self):
        source='''\
local api:any={run=function(n) error("OPTIONAL_FAILURE") end}
local n=api
  ?.
  run
  ?(42)
'''
        for newline in ['\n','\r\n']:
            with self.subTest(newline=repr(newline)):
                main=self.file('main.luax','')
                main.write_bytes(source.replace('\n',newline).encode('utf8'))
                generated=self.cli('gen',main)
                self.assertEqual(generated.returncode,0,generated.stderr)
                self.assertEqual(generated.stdout.count('\n'),source.count('\n'))
                direct=self.cli('run',main)
                self.assertNotEqual(direct.returncode,0)
                self.assertIn('main.luax:1:',direct.stderr)
                output=self.directory/'bundle'
                built=self.cli('bundle',main,'--out',str(output))
                self.assertEqual(built.returncode,0,built.stderr)
                result=self.cli('exec-bundle',output)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(f'{main}:1:',result.stderr)
                mapping=json.loads((output/'debug-map.json').read_text())['files'][0]
                code=Path(mapping['generated']).read_text().splitlines()
                error_line=next(i for i,line in enumerate(code) if 'error("OPTIONAL_FAILURE")' in line)
                self.assertEqual(mapping['lines'][error_line]['line'],1)

    def test_legacy_loader_does_not_accept_new_syntax(self):
        main=self.file('legacy.lua','local f=nil f?()')
        result=self.cli('run',main)
        self.assertNotEqual(result.returncode,0)
