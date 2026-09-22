"""Static imports, declaration contracts and protected control flow on stock LuaJIT."""
import json
import subprocess
import unittest
import test_cli as support


class LanguageExtensionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        support.RustLuaJITTests.setUpClass.__func__(cls)

    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli

    def run_source(self, source, expected, **kwargs):
        main = self.file('main.tlj', source)
        result = self.cli('run', main, **kwargs)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected)
        return main

    def test_import_ignores_reassigned_require_preloads_and_public_cache(self):
        self.file('api.tlj', "print('ONCE') return {n=42}")
        self.file('other.tlj', "return 23")
        self.run_source('''
            local globals:any = _G
            local packages:any = package
            globals.require = function() error('replaced global') end
            local require:any = function() error('replaced local') end
            packages.loaded.api = {n='wrong'}
            packages.preload.api = function() error('preload') end
            packages.loaders = {function() error('replaced loaders') end}
            import api from 'api'
            packages.loaded = nil
            packages.preload = nil
            packages.loaders = nil
            import again from 'api'
            import other from 'other'
            print(api == again, api.n, other)
        ''', 'ONCE\ntrue\t42\t23\n')

    def test_import_initializer_once_false_and_module_arguments(self):
        self.file('f.tlj', "print('FALSE') return false")
        self.file('empty.tlj', "print('EMPTY')")
        self.file('api.tlj', "local name:string=... return {name=name}")
        self.run_source('''
            --!typecheck strict
            import a from 'f' import b from 'f'
            import x from 'empty' import y from 'empty'
            import api from 'api'
            print(a, b, x, y, api.name)
        ''', 'FALSE\nEMPTY\nfalse\tfalse\ttrue\ttrue\tapi\n')

    def test_import_and_require_share_checked_instances(self):
        self.file('api.tlj', "print('ONCE') return {n=42}")
        for source in [
            "import a from 'api' local b=require('api') print(a==b)",
            "local a=require('api') import b from 'api' print(a==b)",
        ]:
            self.run_source(source, 'ONCE\ntrue\n')

    def test_import_contract_and_immutable_binding_enforced_before_execution(self):
        self.file('api.tlj', 'return {n=42}')
        for tail, code in [
            ('api = {n=3}', 'T012'),
            ('local api = 3', 'T012'),
            ('local function api() end', 'T012'),
            ('function api() end', 'T012'),
            ('local n:string=api.n', 'T001'),
            ('local function f() api = {n=3} end', 'T012'),
        ]:
            with self.subTest(tail=tail):
                main = self.file('main.tlj', "print('NEVER') import api from 'api' " + tail)
                result = self.cli('run', main)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(code, result.stderr)
                self.assertEqual(result.stdout, '')
        self.run_source("import api from 'api' do local api=9 print(api) end print(api.n)", '9\n42\n')

    def test_named_declarations_describe_legacy_exports_without_execution(self):
        self.file('old.lua', 'return {double=function(n) return n*2 end, version="1.0"}')
        stub = self.file('old.d.tlj', 'declare double: function(number): number\ndeclare version: string')
        self.run_source('''
            --!typecheck strict
            import old from 'old'
            print(old.double(21), old.version)
        ''', '42\t1.0\n')
        for source in ['declare n: number = 1', 'declare n: number\ndeclare n:string', 'declare n', 'print("NEVER")']:
            stub.write_text(source)
            result = self.cli('check', self.directory / 'main.tlj')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('old.d.tlj', result.stderr)

    def test_import_requires_literal_prepared_implementation_and_checks_cycles(self):
        self.file('tynex.toml', "external_modules=['host']")
        for source, message in [
            ("local name='host' import x from name", 'literal'),
            ("import x from 'host'", 'M110'),
            ("local x:any=require('host') import y from 'host'", 'M110'),
            ("import x from 'missing'", 'M101'),
        ]:
            main = self.file('main.tlj', source)
            result = self.cli('check', main)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(message, result.stderr)
        self.file('a.tlj', "import b from 'b' return b")
        self.file('b.tlj', "import a from 'a' return a")
        main = self.file('main.tlj', "print('NEVER') import a from 'a'")
        result = self.cli('run', main)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('M103', result.stderr)
        self.assertEqual(result.stdout, '')

    def test_except_receives_original_error_object_else_only_on_normal_completion(self):
        self.run_source('''
            --!typecheck strict
            local token={reason='failed'}
            try
                error(token)
            except err then
                print(err == token, err.reason)
            else
                print('NEVER')
            end
            try
                print('body')
            except err then
                print('NEVER')
            else
                print('else')
            end
            try error(nil) except err then print(err == nil) end
        ''', 'true\tfailed\nbody\nelse\ntrue\n')

    def test_handler_and_else_errors_propagate_to_outer_try(self):
        self.run_source('''
            try
                try error('inner',0) except err then error('handler',0) end
            except err then print(err) end
            try
                try print('body') except err then print('NEVER') else error('else',0) end
            except err then print(err) end
        ''', 'handler\nbody\nelse\n')

    def test_return_nil_packs_varargs_nested_functions_and_early_else_skip(self):
        self.run_source('''
            local function f(...:any): (...any)
                try
                    try
                        local function inner():number return 42 end
                        print(inner())
                        return ...
                    except err then error(err) else print('NEVER') end
                except err then error(err) else print('NEVER') end
            end
            local a,b,c,d=f(1,nil,3,nil)
            print(a,b,c,d,select('#',f(1,nil,3,nil)))
            local function empty():()
                try return except err then error(err) else print('NEVER') end
            end
            print(select('#',empty()))
            local function handler(): (number,nil)
                try error('x') except err then return 4,nil end
            end
            local function otherwise(): number
                try print('ok') except err then error(err) else return 5 end
            end
            print(handler()) print(otherwise())
        ''', '42\n42\n1\tnil\t3\tnil\t4\n0\n4\tnil\nok\n5\n')

    def test_break_keeps_target_across_nested_protected_blocks(self):
        self.run_source('''
            local n=0
            for i=1,4 do
                try
                    for j=1,4 do
                        try break except err then error(err) end
                    end
                    n=n+1
                    try break except err then error(err) end
                except err then error(err) else print('NEVER') end
                print('NEVER')
            end
            print(n)
            while true do
                try error('x') except err then break end
            end
            repeat
                try print('repeat') except err then error(err) else break end
            until false
        ''', '1\nrepeat\n')

    def test_try_flow_types_scopes_and_exceptional_partial_assignment(self):
        for source in [
            'local function f():number try return 1 except err then return "bad" end end',
            'try local hidden=1 except err then print(hidden) else print(hidden) end',
            'local n:number?=1 try n=nil error(1) except err then local x:number=n end',
        ]:
            main = self.file('main.tlj', '--!typecheck strict\n' + source)
            result = self.cli('check', main)
            self.assertNotEqual(result.returncode, 0, source)
        self.run_source('''
            --!typecheck strict
            local n:number?=nil
            try n=1 except err then n=2 else n=3 end
            local result:number=n
            print(result)
        ''', '3\n')

    def test_protected_calls_can_yield_on_stock_luajit(self):
        self.run_source('''
            local co:any=coroutine
            local thread=co.create(function()
                try
                    co.yield('pause')
                    print('resumed')
                except err then print('NEVER') else print('else') end
            end)
            print(co.resume(thread))
            print(co.resume(thread))
        ''', 'true\tpause\nresumed\nelse\ntrue\n')

    def test_comments_multiline_imports_and_original_error_lines_survive_lowering(self):
        self.file('api.tlj', 'return {n=42}')
        source='''\
import -- module
    api: -- contract
    any from -- path
    [=[api]=]
try -- body
    print(api.n)
except -- handler
    err then -- binding
    print(err)
else -- no exception
    error('EXPECTED', 0)
end
'''
        main = self.file('main.tlj', source)
        generated = self.cli('gen', main)
        self.assertEqual(generated.returncode, 0, generated.stderr)
        self.assertEqual(generated.stdout.count('\n'), main.read_text().count('\n'))
        result = self.cli('run', main)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, '42\n')
        self.assertIn('main.tlj:11:', result.stderr)
        bundle=self.directory / 'bundle'
        result=self.cli('bundle', main, '--out', str(bundle))
        self.assertEqual(result.returncode, 0, result.stderr)
        result=self.cli('exec-bundle', bundle)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(main) + ':11:', result.stderr)
        mapping=json.loads((bundle/'debug-map.json').read_text())
        entry=next(f for f in mapping['files'] if f['original']==str(main))
        generated_lines = (bundle / entry['generated']).read_text().splitlines()
        error_line = next(i for i, line in enumerate(generated_lines) if "error('EXPECTED', 0)" in line)
        self.assertEqual(entry['lines'][error_line]['line'], 11)

    def test_failed_static_initialization_can_retry_without_poisoning_private_cache(self):
        self.file('api.tlj', "local globals:any=_G if globals.ready ~= true then error('retry',0) end return {n=42}")
        self.run_source("try import api from 'api' except err then print(err) end local globals:any=_G globals.ready=true import api from 'api' print(api.n)", 'retry\n42\n')

    def test_legacy_require_preserves_module_registration_without_return(self):
        self.file('old.lua', 'module(..., package.seeall) n=42')
        self.run_source("local a:any=require('old') local b:any=require('old') print(a.n,a==b)", '42\ttrue\n')

    def test_new_keywords_remain_ordinary_names_in_legacy_lua(self):
        main=self.file('main.lua','local import,try,except,declare=1,2,3,4 print(import+try+except+declare)')
        result=self.cli('run',main)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout,'10\n')


class ZedConfigurationTests(unittest.TestCase):
    def test_workspace_lsp_arguments_and_debug_schema_are_shipped(self):
        import tomllib
        root=support.ROOT
        settings=json.loads((root/'.zed/settings.json').read_text())
        self.assertEqual(settings['lsp']['tynex']['binary']['arguments'],['lsp'])
        extension=tomllib.loads((root/'editors/zed/extension.toml').read_text())
        schema=extension['debug_adapters']['tynex-emmy']['schema_path']
        data=json.loads((root/'editors/zed'/schema).read_text())
        self.assertIn('request',data['required'])
        self.assertIn('tynexMap',data['required'])
