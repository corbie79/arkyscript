"""Rust frontend end-to-end regressions against the official pinned LuaJIT."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / "target/debug" / ("arky.exe" if os.name == "nt" else "arky")
REVISION = json.loads((ROOT / "dependencies.json").read_text())["luajit"]["revision"]
DEFAULT_RUNTIME = (ROOT / "build/windows-x64" / REVISION / "luajit.exe") if os.name == "nt" else (ROOT / "build/native" / REVISION / "src/luajit")
LUAJIT = Path(os.environ.get("TYNEX_LUAJIT", str(DEFAULT_RUNTIME)))


class RustLuaJITTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(["cargo", "build", "--locked", "-p", "tynex-cli"], cwd=ROOT, check=True)
        if not LUAJIT.is_file():
            raise RuntimeError("Build LuaJIT with make setup or set TYNEX_LUAJIT before runtime tests")

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="typed lua ")
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name).resolve()

    def file(self, name, source):
        path = self.directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(source).lstrip("\n"))
        return path

    def cli(self, command, path, *args, env=None):
        environment = {**os.environ, "TYNEX_LUAJIT": str(LUAJIT)}
        for name in ("LUA_INIT", "LOCAL_LUA_DEBUGGER_VSCODE", "TYNEX_EMMY_CORE_DIR"):
            environment.pop(name, None)
        environment.update(env or {})
        return subprocess.run([str(CLI), command, str(path), *args], cwd=self.directory,
                              env=environment, capture_output=True, text=True, timeout=30)

    def test_typed_example_executes(self):
        result = self.cli("run", ROOT / "examples/main.arky")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "LuaJIT score: 15\n")

    def test_table_control_flow_example_executes(self):
        result = self.cli("run", ROOT / "examples/control_flow.arky")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "Mina: 60\nhigh\tmissing\n5\t0\n")

    def test_new_lua_syntax_matches_stock_vm_execution(self):
        cases = [
            "local p={name='Mina',score=0} for i,v in ipairs({10,20,30}) do p.score=p.score+v end print(p.name,p.score)",
            "local n=0 for i=6,1,-2 do n=n+i end while n>3 do n=n-1 end repeat local done=n==0 n=n-1 until done print(n)",
            "local sum=0 for i=1,5 do if i==4 then break elseif i==2 then sum=sum+10 else sum=sum+i end end print(sum)",
            "local t={a=1,b=2,c=3} local sum=0 for k,v in pairs(t) do sum=sum+v+#k end print(sum)",
            "local fn=function(n) if n>2 then return n*2 else return n end end local t={f=fn} print(t.f(3),fn(1))",
            "local function choose(n) if n==nil then return 7 end return n+1 end print(choose(nil),choose(2))",
            "local a={n=1} local b=a b.n=4 do local a={n=10} print(a.n) end print(a.n)",
            "local t={['na\\x6de']=1,['\\0']=2} print(t.name,t['\\000'])",
            "local t={[ [=[\nname]=] ]=3} print(t.name)",
            "local n=0 repeat n=n+1 if n==2 then break end until false print(n)",
        ]
        for index, source in enumerate(cases):
            with self.subTest(index=index):
                lua = self.file(f"reference{index}.lua", source)
                typed = self.file(f"checked{index}.arky", source)
                stock = subprocess.run([str(LUAJIT), str(lua)], capture_output=True, text=True, timeout=10)
                result = self.cli("run", typed)
                self.assertEqual(stock.returncode, 0, stock.stderr)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, stock.stdout)

    def test_multiple_values_example_executes(self):
        result = self.cli("run", ROOT / "examples/multiple_values.arky")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "60\t3\n5\tnil\n")

    def test_multiple_values_and_methods_match_stock_luajit(self):
        cases = [
            "local function f() return 1,'s' end local a,b,c=f(),f() print(a,b,c) local x,y=(f()) print(x,y)",
            "local function f() return 1,2 end local a,b,c=f(),9 print(a,b,c) local t={f(),9} print(#t,t[1],t[2],t[3])",
            "local function f() return 1,2 end local t={0,f()} local u={0,(f())} print(#t,t[1],t[2],t[3],#u)",
            "local function f() end local function n(...) return select('#',...) end print(n(f()),n((f())),n(f(),1))",
            "local function f(...) return ... end local function g(...) return 0,f(...) end print(g(1,nil,3))",
            "local function f(...) local n=select('#',...) local t={...} return n,t end local n,t=f(1,nil,3) print(n,t[1],t[2],t[3])",
            "local a,b=1,2 local b,a=a,b print(a,b) a,b=b,a print(a,b)",
            "local i,t=1,{0,0} i,t[i]=2,9 print(i,t[1],t[2])",
            "local a=0 a,a=1,2 print(a) local t={x=0} t.x,t.x=1,2 print(t.x)",
            "local t={x=0} local old=t t,t.x={x=4},7 print(t.x,old.x)",
            "local calls=0 local function f() calls=calls+1 return 1,2 end local x=f(),f() print(x,calls)",
            "local m={n=0} function m:add(a,b) self.n=self.n+a+b return self.n,a end local x,y=m:add(2,3) print(x,y,m.add(m,4,5))",
            "local m={n=0} function m:add(n) self.n=self.n+n return self.n end local calls=0 local function get() calls=calls+1 return m end print(get():add(3),calls)",
            "local m={} function m.f(n) if n==0 then return 1,2 end return m.f(n-1) end print(m.f(3))",
            "function f(n) return n*2 end local function use() return f(4) end print(use())",
            "local s='hello' print(s:sub(2):upper(),s:len())",
            "local function f(...) return select('#',...),select(2,...) end print(f(1,nil,3)) print(unpack({3,4,5},2,3))",
        ]
        for index, source in enumerate(cases):
            with self.subTest(index=index):
                lua = self.file(f"values_reference{index}.lua", source)
                typed = self.file(f"values_checked{index}.arky", source)
                stock = subprocess.run([str(LUAJIT), str(lua)], capture_output=True, text=True, timeout=10)
                result = self.cli("run", typed)
                self.assertEqual(stock.returncode, 0, stock.stderr)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, stock.stdout)

    def test_multiple_value_type_errors_prevent_execution(self):
        for index, code in enumerate([
            "local function f() return 1,'s' end local a:number,b:number=f()",
            "local function f(...:number) end f(1,'s')",
            "local m={n=0} function m:f(n:number) self.n=n end m:f('s')",
            "local function f(): (number,string) return 1 end",
        ]):
            source = self.file(f"values_bad{index}.arky", "print('MUST NOT RUN')\n" + code)
            for command in ("check", "gen", "run", "debug"):
                result = self.cli(command, source)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_method_debug_hook_preserves_self_arguments_and_multi_locals(self):
        source = ROOT / "examples/multiple_values.arky"
        probe = self.file("method_probe.lua", '''
            jit.off()
            debug.sethook(function(_, line)
               local info=debug.getinfo(2,'S')
               if info.source=='@'..os.getenv('TEST_SOURCE') and line==14 then
                  local values={}
                  for i=1,30 do
                     local name,value=debug.getlocal(2,i)
                     if not name then break end
                     values[name]=value
                  end
                  assert(values.self.total==0)
                  assert(values.amount==60 and values.count==3)
                  io.stderr:write('METHOD_HOOK_OK\\n')
               end
            end, 'l')
        ''')
        result = self.cli("run", source, env={"LUA_INIT":"@"+str(probe), "TEST_SOURCE":str(source)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr.count("METHOD_HOOK_OK"), 1)

    def test_table_and_branch_type_errors_block_all_execution(self):
        cases = [
            "local p={score=1} p.score='bad'",
            "local xs:{number}={1} local n:number=xs[99]",
            "local function f(n:number):number if n>0 then return n end end",
            "for i='bad',3 do print(i) end",
        ]
        for index, code in enumerate(cases):
            source = self.file(f"bad{index}.arky", "print('MUST NOT EXECUTE')\n"+code)
            result = self.cli("run", source)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")

    def test_loop_debug_hook_sees_original_table_and_iterator_locals(self):
        source = ROOT / "examples/control_flow.arky"
        probe = self.file("loop_probe.lua", '''
            jit.off()
            debug.sethook(function(_, line)
               local info=debug.getinfo(2,'S')
               if info.source=='@'..os.getenv('TEST_SOURCE') and line==6 then
                  local values={}
                  for i=1,30 do
                     local name,value=debug.getlocal(2,i)
                     if not name then break end
                     values[name]=value
                  end
                  assert(values.player.name=='Mina')
                  assert(values.score==values.index*10)
                  io.stderr:write('LOOP_HOOK_OK\\n')
               end
            end, 'l')
        ''')
        result=self.cli("run", source, env={"LUA_INIT":"@"+str(probe), "TEST_SOURCE":str(source)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr.count("LOOP_HOOK_OK"), 3)

    def test_gradual_accepts_untyped_parameters(self):
        source = self.file("main.arky", "local function twice(x) return x*2 end print(twice(21))")
        result = self.cli("run", source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "42\n")

    def test_known_type_error_blocks_every_command_and_lua_init(self):
        source = self.file("bad.arky", "print('MUST NOT EXECUTE')\nlocal n: number = 'bad'")
        for command in ("check", "gen", "run", "debug"):
            result = self.cli(command, source, env={"LUA_INIT": "print('INIT MUST NOT EXECUTE')"})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, "")
            self.assertIn(f"{source}:2:7:", result.stderr)

    def test_legacy_metatables_globals_and_preloaded_modules_match_stock(self):
        source = self.file("main.lua", '''
            package.preload.marker = function() return 8 end
            local object = setmetatable({}, {__index = function(_, k) return #k end})
            total = 0
            for i=1,3 do total = total + i end
            print(total, object.hello, require('marker'))
        ''')
        expected = subprocess.run([str(LUAJIT), str(source)], capture_output=True, text=True, check=True)
        result = self.cli("run", source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected.stdout)

    def test_legacy_generation_is_byte_preserving_and_validated(self):
        source = self.file("main.lua", "#!/usr/bin/env luajit\n-- hello\nprint('한글')\n")
        result = self.cli("gen", source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, source.read_text())
        bad = self.file("bad.lua", "local = 1")
        self.assertNotEqual(self.cli("gen", bad).returncode, 0)

    def test_legacy_check_never_executes_entry_or_lua_init(self):
        source = self.file("main.lua", "print('MUST NOT EXECUTE') error('never')")
        result = self.cli("check", source, env={"LUA_INIT": "print('INIT MUST NOT EXECUTE')"})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("type checking skipped", result.stdout)
        self.assertNotIn("MUST NOT EXECUTE", result.stdout)

    def test_script_arguments_are_preserved_after_separator(self):
        source = self.file("args.lua", "local a,b=... print(arg[0]) print(arg[1],arg[2],a,b)")
        result = self.cli("run", source, "--", "hello world", "--strict")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"{source}\nhello world\t--strict\thello world\t--strict\n")

    def test_legacy_import_from_strict_typed_code_uses_explicit_any_boundary(self):
        self.file("legacy.lua", "return { double = function(x) return x*2 end }")
        source = self.file("main.arky", "--!typecheck strict\nlocal lib: any = require('legacy')\nprint(lib.double(21))")
        result = self.cli("run", source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "42\n")

    def test_generated_lua_executes_in_unmodified_engine(self):
        result = self.cli("gen", ROOT / "examples/main.arky")
        self.assertEqual(result.returncode, 0, result.stderr)
        generated = self.file("output.lua", result.stdout)
        executed = subprocess.run([str(LUAJIT), str(generated)], capture_output=True, text=True)
        self.assertEqual(executed.returncode, 0, executed.stderr)
        self.assertEqual(executed.stdout, "LuaJIT score: 15\n")

    def test_supported_lexical_forms_generate_vm_accepted_syntax(self):
        cases = [
            "local n: number = -2^2 + .5 + 0x1.fp2 + 1e-2 print(n)",
            "local s: string = 'hi\\x41\\255\\z   there' print(s)",
            "local s: string = [==[quotes ' and -- comments\n]==] print(s)",
            "local n: -- annotation\nnumber = 1; (print)(n)",
            "#!/usr/bin/env arky\nlocal function f(): number return 1 end print(f())",
        ]
        for index, code in enumerate(cases):
            with self.subTest(index=index):
                source = self.file(f"case{index}.arky", code)
                result = self.cli("gen", source)
                self.assertEqual(result.returncode, 0, result.stderr)
                generated = self.file(f"case{index}.lua", result.stdout)
                checked = self.cli("check", generated)
                self.assertEqual(checked.returncode, 0, checked.stderr)

    def test_typed_runtime_errors_use_original_source_lines(self):
        source = self.file("crash.arky", "local n: number = 1\nerror('runtime marker')")
        result = self.cli("run", source)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("crash.arky:2: runtime marker", result.stderr)

    def test_debug_hooks_see_original_file_locals_and_stack(self):
        source = self.file("hook.arky", '''
            local function double(value: number): number
               local total = value * 2
               return total
            end
            print(double(21))
        ''')
        probe = self.file("probe.lua", '''
            jit.off()
            debug.sethook(function(_, line)
               local info = debug.getinfo(2, 'S')
               if info.source == '@' .. os.getenv('TEST_SOURCE') and line == 3 then
                  local values = {}
                  for i=1,20 do
                     local name,value = debug.getlocal(2,i)
                     if not name then break end
                     values[name] = value
                  end
                  assert(values.value == 21 and values.total == 42)
                  assert(debug.getinfo(3,'S').source == info.source)
                  io.stderr:write('HOOK_OK\\n')
               end
            end, 'l')
        ''')
        result = self.cli("run", source, env={"LUA_INIT": "@"+str(probe), "TEST_SOURCE": str(source)})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("HOOK_OK", result.stderr)

    def test_debug_without_adapter_does_not_run_entry(self):
        result = self.cli("debug", ROOT / "examples/main.arky")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("docs/debugging.md", result.stderr)

    def test_unicode_and_spaces_in_source_path(self):
        source = self.file("한글 경로/main.arky", "print('hello')")
        result = self.cli("run", source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "hello\n")

    @unittest.skipIf(os.name == "nt", "requires local symlink creation permissions")
    def test_symlink_parent_resolution_uses_filesystem_semantics(self):
        self.file("inner/main.arky", "print('wrong')")
        self.file("elsewhere/main.arky", "print('expected')")
        (self.directory / "elsewhere/target").mkdir()
        (self.directory / "inner/link").symlink_to(self.directory / "elsewhere/target")
        result = self.cli("run", self.directory / "inner/link/../main.arky")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "expected\n")


if __name__ == "__main__":
    unittest.main()
