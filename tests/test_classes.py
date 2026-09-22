"""Script classes and traits run on the unchanged official LuaJIT."""
import json
from pathlib import Path
import unittest

import test_cli as support


class ClassTraitRuntimeTests(unittest.TestCase):
    setUpClass = classmethod(support.RustLuaJITTests.setUpClass.__func__)
    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli

    def run_source(self, source, expected):
        main = self.file("main.tlj", source)
        result = self.cli("run", main)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected)
        return main

    def test_shipped_examples(self):
        for name, expected in [
            ("class.tlj", "Mina\t43\nJoon\t5\n"),
            ("traits.tlj", "name: Mina\nname: device: sensor-7\n42\n"),
        ]:
            with self.subTest(name=name):
                result = self.cli("run", support.ROOT / "examples" / name)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)

    def test_defaults_are_lazy_ordered_and_inputs_are_copied(self):
        self.run_source('''
            --!typecheck strict
            local count = 0
            local function next_value(): number
                count = count + 1
                return count
            end
            class Item
                first: number = next_value()
                second: number = next_value()
                values: {number} = {}
                enabled: boolean = true
                note: string?
                function total(): number return self.first + self.second end
            end
            print(count)
            local input = {first=10, enabled=false}
            local a = Item(input)
            input.first = 100
            local b = Item {}
            a.values[1] = 42
            print(count, a:total(), b:total(), a.enabled, b.values[1], a.note)
        ''', '0\n3\t11\t5\tfalse\tnil\tnil\n')

    def test_self_recursive_and_forward_methods_with_multiple_returns(self):
        self.run_source('''
            --!typecheck strict
            class Counter
                n: number
                function sum(): number return self:walk(self.n) end
                function walk(n: number): number
                    if n == 0 then return 0 end
                    return n + self:walk(n - 1)
                end
                function pair(): (Self, number) return self, self:sum() end
            end
            local original = Counter {n=5}
            local same, total = original:pair()
            print(same == original, total)
        ''', 'true\t15\n')

    def test_trait_conflict_can_be_resolved_and_dispatches_to_class(self):
        self.run_source('''
            --!typecheck strict
            trait A
                label: function(Self): string
                function describe(): string return "A " .. self:label() end
            end
            trait B
                function describe(): string return "B" end
            end
            class Item is A, B
                function label(): string return "item" end
                function describe(): string return self:label() .. " resolved" end
            end
            local function show_a(value: A): () print(value:describe()) end
            local function show_b(value: B): () print(value:describe()) end
            local item = Item {}
            show_a(item)
            show_b(item)
        ''', 'item resolved\nitem resolved\n')

    def test_class_name_can_shadow_the_outer_trait_it_implements(self):
        self.run_source('''
            --!typecheck strict
            trait Named
                function label(): string return "outer trait" end
            end
            do
                class Named is Named end
                print(Named {}:label())
            end
            class Other is Named end
            print(Other {}:label())
        ''', 'outer trait\nouter trait\n')

    def test_imported_classes_traits_and_nominal_identity(self):
        self.file('models.tlj', '''
            --!typecheck strict
            trait Named
                label: function(Self): string
                function describe(): string return "name: " .. self:label() end
            end
            class User is Named
                name: string
                function label(): string return self.name end
            end
            return {User=User, Named=Named}
        ''')
        self.run_source('''
            --!typecheck strict
            import models from "models"
            import again from "models"
            class Device is models.Named
                function label(): string return "device" end
            end
            local user: models.User = again.User {name="Mina"}
            local function show(value: models.Named): () print(value:describe()) end
            show(user)
            show(Device {})
        ''', 'name: Mina\nname: device\n')
        self.file('other.tlj', 'class User name:string end return {User=User}')
        main = self.file('main.tlj', '''
            import models from "models"
            import other from "other"
            local user: models.User = other.User {name="wrong"}
        ''')
        failed = self.cli('check', main)
        self.assertNotEqual(failed.returncode, 0)
        self.assertIn('T001', failed.stderr)

    def test_invalid_definitions_and_calls_block_execution_in_both_modes(self):
        failures = [
            'class User name:string end local u=User {}',
            'class User n:number end local u=User {n="bad"}',
            'class User n:number=0 end local u=User {extra=1}',
            'class User n:number=0 end local u=User {} u.n="bad"',
            'class User end class Other end local u:User=Other {}',
            'trait Named label:function(Self):string end class User is Named end',
            'trait Named label:function(Self):string end class User is Named function label():number return 1 end end',
            'trait A function f():number return 1 end end trait B function f():number return 2 end end class C is A,B end',
            'trait A f:function(Self):number end class C function f():number return 1 end end local a:A=C {}',
            'class C function f(n:number):number return n end end local c=C {} c:f("bad")',
            'class C end C = 1',
            'class C x:number end local function pair(): ({x:number},number) return {x=1},2 end local c=C(pair())',
        ]
        for mode in ['gradual', 'strict']:
            for source in failures:
                with self.subTest(mode=mode, source=source):
                    main = self.file('main.tlj', f'--!typecheck {mode}\nprint("NEVER")\n' + source)
                    result = self.cli('run', main)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(result.stdout, '')

    def test_dynamic_constructor_shape_checks_are_catchable(self):
        self.run_source('''
            --!typecheck strict
            class Required name:string end
            local constructor:any=Required
            try constructor {} except err then print("missing") end
            try constructor {name="x", extra=1} except err then print("unknown") end
            try constructor(3) except err then print("table") end
            try constructor({name="x"}, {}) except err then print("arity") end
            print(constructor {name="valid"}.name)
        ''', 'missing\nunknown\ntable\narity\nvalid\n')

    def test_parenthesized_initializer_collapses_multiple_results(self):
        self.run_source('''
            --!typecheck strict
            class Item n:number end
            local function pair(): ({n:number}, number) return {n=42}, 7 end
            local item = Item((pair()))
            print(item.n)
        ''', '42\n')

    def test_shadowed_builtins_do_not_change_generated_helpers(self):
        self.run_source('''
            --!typecheck strict
            local setmetatable, pairs, next, rawget, type, error, select = 1,2,3,4,5,6,7
            trait Named
                function label(): string return "ok" end
            end
            class Item is Named
                n: number = 42
            end
            local item=Item {}
            print(item:label(), item.n, setmetatable+pairs+next+rawget+type+error+select)
        ''', 'ok\t42\t28\n')

    def test_legacy_class_trait_self_and_is_names_are_unchanged(self):
        main = self.file('legacy.lua', 'local class,trait,Self,is=1,2,3,4 print(class+trait+Self+is)')
        result = self.cli('run', main)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '10\n')

    def test_method_and_default_errors_keep_source_lines_in_bundles(self):
        for body, error_line in [
            ('class Item\n   function fail(): ()\n      error("METHOD_FAILURE")\n   end\nend\nItem {}:fail()\n', 3),
            ('local function fail(): number\n   error("DEFAULT_FAILURE")\nend\nclass Item\n   n: number = fail()\nend\nlocal item=Item {}\n', 2),
        ]:
            for newline in ['\n', '\r\n']:
                with self.subTest(body=body, newline=repr(newline)):
                    main = self.file('main.tlj', '')
                    original = body.replace('\n', newline)
                    main.write_bytes(original.encode('utf8'))
                    result = self.cli('run', main)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f'{main.name}:{error_line}:', result.stderr)
                    output = self.directory / 'bundle'
                    built = self.cli('bundle', main, '--out', str(output))
                    self.assertEqual(built.returncode, 0, built.stderr)
                    result = self.cli('exec-bundle', output)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(f'{main}:{error_line}:', result.stderr)
                    mapping = json.loads((output/'debug-map.json').read_text())['files'][0]
                    self.assertIn({'line': error_line}, [
                        {'line': p['line']} for p in mapping['lines'] if p
                    ])
                    self.assertEqual(main.read_bytes(), original.encode('utf8'))

    def test_class_inside_protected_block_preserves_method_returns(self):
        self.run_source('''
            --!typecheck strict
            try
                class Counter
                    n: number = 42
                    function get(): number
                        try return self.n except err then error(err) end
                    end
                end
                print(Counter {}:get())
            except err then print("unexpected")
            else print("done") end
        ''', '42\ndone\n')

    def test_debug_hook_sees_self_and_arguments_at_original_method_line(self):
        source = self.file('method.tlj', '''
            --!typecheck strict
            trait CounterLike
                add: function(Self, number): number
            end
            class Counter is CounterLike
                total: number = 2
                function add(amount: number): number
                    local result = self.total + amount
                    return result
                end
            end
            print(Counter {}:add(40))
        ''')
        probe = self.file('probe.lua', '''
            jit.off()
            debug.sethook(function(_, line)
                local info=debug.getinfo(2, 'S')
                if info.source=='@'..os.getenv('TEST_SOURCE') and line==9 then
                    local values={}
                    for i=1,30 do
                        local name,value=debug.getlocal(2,i)
                        if not name then break end
                        values[name]=value
                    end
                    assert(values.self.total==2)
                    assert(values.amount==40 and values.result==42)
                    io.stderr:write('CLASS_HOOK_OK\\n')
                end
            end, 'l')
        ''')
        result=self.cli('run', source, env={'LUA_INIT':'@'+str(probe), 'TEST_SOURCE':str(source)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '42\n')
        self.assertEqual(result.stderr.count('CLASS_HOOK_OK'), 1)

    def test_bundle_constructor_helpers_have_no_original_breakpoint_line(self):
        source = self.file('first-line.tlj', '''
            class Item
                value: number = 42
                function get(): number return self.value end
            end
            print(Item {}:get())
        ''')
        output = self.directory / 'bundle'
        built = self.cli('bundle', source, '--out', str(output))
        self.assertEqual(built.returncode, 0, built.stderr)
        mapping = json.loads((output/'debug-map.json').read_text())['files'][0]
        generated = Path(mapping['generated'])
        code = generated.read_text().splitlines()
        helper_line = next(i for i, line in enumerate(code) if 'constructor expects exactly one table' in line)
        self.assertIsNone(mapping['lines'][helper_line])
        class_line = next(i for i, line in enumerate(code) if 'local Item;' in line)
        self.assertEqual(mapping['lines'][class_line]['line'], 1)
        construction_line = next(i for i, line in enumerate(code) if 'print(Item {}:get())' in line)
        probe = self.file('bundle-probe.lua', '''
            jit.off()
            local constructing = false
            debug.sethook(function(_, line)
                local info=debug.getinfo(2,'S')
                if info.source=='@'..os.getenv('TEST_SOURCE') then
                    if line==tonumber(os.getenv('CONSTRUCTION_LINE')) then constructing=true end
                    if line==tonumber(os.getenv('TEST_LINE')) then
                        assert(not constructing, 'constructor helper triggered declaration breakpoint')
                        io.stderr:write('CLASS_DECLARATION_HOOK\\n')
                    end
                end
            end,'l')
        ''')
        result=self.cli('exec-bundle', output, env={
            'LUA_INIT':'@'+str(probe), 'TEST_SOURCE':str(generated), 'TEST_LINE':str(class_line+1),
            'CONSTRUCTION_LINE':str(construction_line+1),
        })
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '42\n')
        self.assertGreaterEqual(result.stderr.count('CLASS_DECLARATION_HOOK'), 1)
