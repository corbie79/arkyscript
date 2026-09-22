"""Project graph and mixed module execution against the unmodified LuaJIT."""
import json
from pathlib import Path
import subprocess
import unittest
import test_cli as support


class ModuleTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        support.RustLuaJITTests.setUpClass.__func__(cls)

    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli

    def ok(self, result, stdout=None):
        self.assertEqual(result.returncode, 0, result.stderr)
        if stdout is not None:
            self.assertEqual(result.stdout, stdout)

    def config(self, text=""):
        return self.file("tynex.toml", text)

    def test_typed_legacy_declarations_and_inferred_exports_work_together(self):
        self.config("type_mode='strict'\n[[overrides]]\nfiles=['**/*.lua']\nmode='legacy'")
        legacy = self.file("legacy.lua", "return {double=function(n) return n*2 end}")
        original = legacy.read_bytes()
        self.file("legacy.d.tlj", "export: {double:function(number):number}")
        self.file("api.tlj", "local old=require('legacy') local api={} function api.score(n:number):number return old.double(n)+1 end return api")
        main = self.file("main.tlj", "local api=require('api') print(api.score(20))")
        self.ok(self.cli("run", main), "41\n")
        self.ok(self.cli("check", main))
        self.assertEqual(legacy.read_bytes(), original)
        self.file("main.tlj", "local api=require('api') api.score('bad')")
        result = self.cli("run", main)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("T001", result.stderr)

    def test_dependency_errors_block_entry_and_lua_init_for_every_command(self):
        self.file("api.tlj", "--!typecheck strict\nlocal n:number='bad' return n")
        main = self.file("main.tlj", "print('ENTRY') local api=require('api')")
        for command in ("check", "gen", "run", "debug"):
            with self.subTest(command=command):
                result = self.cli(command, main, "--mode", "gradual", env={"LUA_INIT":"print('INIT')"})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("api.tlj:2:", result.stderr)

    def test_legacy_syntax_is_checked_without_executing_legacy_or_init(self):
        legacy = self.file("legacy.lua", "print('LEGACY') return {n=1}")
        main = self.file("main.tlj", "local m:any=require('legacy') print(m.n)")
        self.ok(self.cli("check", main, env={"LUA_INIT":"print('INIT')"}))
        legacy.write_text("-- valid header\n\nlocal =\nprint('NEVER')")
        result = self.cli("run", main, env={"LUA_INIT":"print('INIT')"})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("LUA_SYNTAX", result.stderr)
        diagnostic = json.loads(self.cli("check", main, "--format", "json").stdout)["diagnostics"][0]
        self.assertEqual(diagnostic["line"], 3)

    def test_invalid_declaration_is_reported_at_declaration_path(self):
        self.file("legacy.lua", "print('NEVER') return 1")
        self.file("legacy.d.tlj", "export: unknown_type")
        main = self.file("main.tlj", "local n=require('legacy')")
        result = self.cli("check", main, "--format", "json")
        self.assertNotEqual(result.returncode, 0)
        diagnostics = json.loads(result.stdout)["diagnostics"]
        self.assertTrue(any(d["file"].endswith("legacy.d.tlj") for d in diagnostics))
        self.assertNotIn("NEVER", result.stdout)

    def test_dynamic_project_modules_require_preparation(self):
        self.file("plugin.tlj", "--!typecheck strict\nreturn {n=42}")
        main = self.file("main.tlj", "--!typecheck strict\nlocal name='plugin' local p:any=require(name) print(p.n)")
        self.ok(self.cli("check", main))
        result = self.cli("run", main)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("dynamic_modules", result.stderr)
        self.config("dynamic_modules=['plugin']")
        self.ok(self.cli("run", main), "42\n")
        self.file("plugin.tlj", "--!typecheck strict\nlocal n:number='bad' return n")
        result = self.cli("run", main, env={"LUA_INIT":"print('NEVER')"})
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")

    def test_legacy_entry_can_import_prepared_typed_modules(self):
        self.config("dynamic_modules=['typed']")
        self.file("typed.tlj", "--!typecheck strict\nlocal m={} function m.double(n:number):number return n*2 end return m")
        main = self.file("main.lua", "local m=require('typed') print(m.double(21))")
        self.ok(self.cli("run", main), "42\n")

    def test_runtime_cache_false_nil_and_module_name_follow_lua_require(self):
        self.file("once.tlj", "print('ONCE') local name:string=... return {name=name}")
        self.file("empty.tlj", "print('EMPTY')")
        self.file("false_value.tlj", "print('FALSE') return false")
        main = self.file("main.tlj", "local a=require('once') local b=require('once') print(a==b,a.name) local x=require('empty') local y=require('empty') print(x,y) local f=require('false_value') local g=require('false_value') print(f,g)")
        self.ok(self.cli("run", main), "ONCE\ntrue\tonce\nEMPTY\ntrue\ttrue\nFALSE\nFALSE\nfalse\tfalse\n")

    def test_checked_interfaces_never_silently_use_conflicting_preloads_or_cache(self):
        self.file("api.tlj", "return {n=42}")
        main = self.file("main.tlj", "local api=require('api') print(api.n)")
        for init in ["package.preload.api=function() return {n='wrong'} end", "package.loaded.api={n='wrong'}"]:
            result = self.cli("run", main, env={"LUA_INIT":init})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")
            self.assertIn("project module 'api'", result.stderr)

    def test_registered_external_host_module_uses_stock_preload(self):
        self.config("external_modules=['host.api']")
        main = self.file("main.tlj", "--!typecheck strict\nlocal host:any=require('host.api') print(host.n)")
        self.ok(self.cli("run", main, env={"LUA_INIT":"package.preload['host.api']=function() return {n=9} end"}), "9\n")

    def test_search_roots_and_unicode_module_names_are_independent_of_cwd(self):
        self.config("module_paths=['소스 공간','fallback']")
        self.file("소스 공간/도구/init.tlj", "return {n=42}")
        self.file("fallback/도구.tlj", "return {n='wrong'}")
        main = self.file("entries/main.tlj", "--!typecheck strict\nlocal lib=require('도구') local n:number=lib.n print(n)")
        self.ok(self.cli("run", main), "42\n")

    def test_module_debug_hook_and_errors_keep_original_file_and_lines(self):
        module = self.file("lib.tlj", "local m={}\nfunction m.double(n:number):number\n   local result=n*2\n   return result\nend\nreturn m\n")
        main = self.file("main.tlj", "local m=require('lib') print(m.double(21))")
        probe = self.file("probe.lua", '''
            jit.off()
            debug.sethook(function(_,line)
               local info=debug.getinfo(2,'S')
               if info.source=='@'..os.getenv('TEST_SOURCE') and line==4 then
                  local values={}
                  for i=1,20 do
                     local name,value=debug.getlocal(2,i)
                     if not name then break end
                     values[name]=value
                  end
                  assert(values.n==21 and values.result==42)
                  io.stderr:write('MODULE_HOOK_OK\\n')
               end
            end,'l')
        ''')
        result = self.cli("run", main, env={"LUA_INIT":"@"+str(probe), "TEST_SOURCE":str(module)})
        self.ok(result, "42\n")
        self.assertEqual(result.stderr.count("MODULE_HOOK_OK"), 1)
        module.write_text("local n=1\nerror('MODULE_CRASH')")
        main.write_text("require('lib')")
        result = self.cli("run", main)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("lib.tlj:2: MODULE_CRASH", result.stderr)

    def test_project_check_includes_unused_legacy_without_execution(self):
        project = self.config("include=['src/**']")
        self.file("src/unused.lua", "print('NEVER') local =")
        self.file("src/main.tlj", "print('ENTRY')")
        result = subprocess.run([str(support.CLI), "check", "--project", str(project), "--format", "json"], cwd=self.directory, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(json.loads(result.stdout)["diagnostics"][0]["code"], "LUA_SYNTAX")
        self.assertNotIn("NEVER", result.stdout)

    def test_typed_lua_policy_and_legacy_transitive_modules_keep_their_boundaries(self):
        self.config("[[overrides]]\nfiles=['typed.lua']\nmode='strict'")
        self.file("typed.lua", "return {n=42}")
        main = self.file("main.tlj", "--!typecheck strict\nlocal t=require('typed') local n:number=t.n print(n)")
        self.ok(self.cli("run", main), "42\n")
        self.file("legacy.lua", "return require('helper')")
        self.file("helper.lua", "return {n=8}")
        main.write_text("local t:any=require('legacy') print(t.n)")
        self.ok(self.cli("run", main), "8\n")

    def test_cycle_and_ambiguity_prevent_all_user_execution(self):
        self.file("one.tlj", "return require('two')")
        self.file("two.tlj", "return require('one')")
        main = self.file("main.tlj", "print('ENTRY') local m=require('one')")
        result = self.cli("run", main, env={"LUA_INIT":"print('INIT')"})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("M103", result.stderr)
        self.assertEqual(result.stdout, "")
        self.file("one.lua", "return 1")
        result = self.cli("run", main)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ambiguous", result.stderr)
        self.assertEqual(result.stdout, "")

    def test_imported_loader_alias_and_independent_global_writes_execute(self):
        self.file("loader.tlj", "return require")
        self.file("data.tlj", "return 42")
        main = self.file("main.tlj", "--!typecheck strict\nlocal load=require('loader') local n:number=load('data') print(n)")
        self.ok(self.cli("run", main), "42\n")
        self.file("global.tlj", "function twice(n:number):number return n*2 end return true")
        self.config("dynamic_modules=['global']")
        main = self.file("legacy_entry.lua", "require('global') print(twice(21))")
        self.ok(self.cli("run", main), "42\n")
