"""Detached trait implementations, imported native contracts and real C++ userdata."""
import subprocess
import sys
import unittest
import test_cli as support


class ImplTests(unittest.TestCase):
    setUpClass = classmethod(support.RustLuaJITTests.setUpClass.__func__)
    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli

    def test_script_impl_and_existing_instances(self):
        main = self.file('main.arky', '''--!typecheck strict
class C n: number = 42 end
local value = C {}
trait T
 read: function(Self): number
 function describe(): string return "value: " .. self:read() end
end
impl T for C
 function read(): number return self.n end
end
local function show(v: T): () print(v:describe()) end
show(value)
''')
        result = self.cli('run', main)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'value: 42\n')

        generated = self.cli('gen', main)
        self.assertEqual(generated.returncode, 0, generated.stderr)
        restricted = self.file('restricted.lua', 'debug = nil\n' + generated.stdout)
        result = subprocess.run([str(support.DEFAULT_RUNTIME), str(restricted)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'value: 42\n')

    def test_module_impl_and_optional_dispatch(self):
        self.file('model.arky', 'class C end return {C=C}')
        self.file('ext.arky', '''import m from "model"
trait T function value(): (number, string) return 42, "ok" end end
impl T for m.C end
return {T=T}
''')
        main = self.file('main.arky', '''--!typecheck strict
import model from "model"
import ext from "ext"
local c: model.C? = model.C {}
print(c?:value())
local function show(v: ext.T): () print(v:value()) end
show(model.C {})
''')
        result = self.cli('run', main)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '42\tok\n42\tok\n')

    def test_async_impl_runs_with_shared_scheduler(self):
        main = self.file('main.arky', '''--!typecheck strict
class C end
trait T f: async function(Self): number end
impl T for C async function f(): number return 42 end end
print(scheduler.run(C {}:f()))
''')
        result = self.cli('run', main)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, '42\n')

    def test_generated_native_impl_on_real_cpp_objects(self):
        sys.path.insert(0, str(support.ROOT / 'scripts'))
        from test_binder import native, dependencies
        revision = dependencies()['luajit']['revision']
        # Exercise actual userdata, pointer adjustment and the host submission boundary.
        native(revision, False, (support.ROOT / 'examples/cpp_rendering.cpp',))
        generated = self.cli('gen', support.ROOT / 'examples/native_traits/main.arky')
        self.assertEqual(generated.returncode, 0, generated.stderr)
        script = self.file('generated.lua', generated.stdout)
        binary = support.ROOT / 'build/binder/native' / revision / 'cpp_rendering'
        result = subprocess.run([str(binary), str(script)], cwd=support.ROOT, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'name: sprite 7\nNative render queue: 1 commands\n')
