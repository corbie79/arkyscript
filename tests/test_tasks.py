"""Real shared-module execution, isolated VMs, and examples through the CLI."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_cli as support


class TaskRuntimeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(["cargo", "build", "--locked", "-p", "tynex-cli"], cwd=support.ROOT, check=True)
        suffix = ".dll" if os.name == "nt" else ".so"
        module = support.LUAJIT.parent / ("tynex_task" + suffix)
        if not module.is_file():
            raise RuntimeError("Build the Task module with make setup / make task before testing: " + str(module))

    def run_process(self, *args):
        env = {**os.environ, "TYNEX_LUAJIT": str(support.LUAJIT),
               "LUA_CPATH": str(support.LUAJIT.parent / "?.dll") + ";" + str(support.LUAJIT.parent / "?.so") + ";;"}
        for name in ("LUA_INIT", "LOCAL_LUA_DEBUGGER_VSCODE", "TYNEX_EMMY_CORE_DIR"):
            env.pop(name, None)
        return subprocess.run(list(map(str, args)), cwd=support.ROOT, env=env,
                              capture_output=True, text=True, timeout=30)

    def test_dynamic_module_contract(self):
        result = self.run_process(support.LUAJIT, "tests/task/runtime.lua")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("isolation, errors and cancellation: passed", result.stdout)

    def test_typed_task_examples(self):
        for file, expected in (
            ("main.arky", "Parallel task sum: 250015000\nInput isolation: 1 / 10000\nTask status: succeeded\n"),
            ("cancellation.arky", "Cancelled: cancelled\nFailure: failed\nPool still works: 42\n"),
        ):
            with self.subTest(file=file):
                result = self.run_process(support.CLI, "run", support.ROOT / "examples/tasks" / file)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)

    def test_two_lua_vms_execute_on_distinct_overlapping_os_threads(self):
        # Files are only a rendezvous signal. A single sequential worker cannot
        # pass: both functions must reach their signal before either can return.
        with tempfile.TemporaryDirectory(prefix="tynex task ") as directory:
            script = Path(directory) / "parallel.lua"
            script.write_text(r'''
local task = require("tynex_task")
task.configure(2, 16)
local function work(data, context)
   local ffi = require("ffi")
   local id
   if ffi.os == "Windows" then
      ffi.cdef("unsigned long __stdcall GetCurrentThreadId(void);")
      id = tostring(ffi.C.GetCurrentThreadId())
   else
      ffi.cdef("uintptr_t pthread_self(void);")
      id = tostring(ffi.C.pthread_self())
   end
   local signal = assert(io.open(data.mine, "wb"))
   signal:write("ready"); signal:close()
   local deadline = os.time() + 10
   while true do
      context.checkpoint()
      local other = io.open(data.other, "rb")
      if other then other:close(); break end
      assert(os.time() < deadline, "workers did not overlap")
   end
   return id
end
local a = task.spawn(work, {mine=arg[1], other=arg[2]})
local b = task.spawn(work, {mine=arg[2], other=arg[1]})
assert(a:wait() ~= b:wait(), "Lua jobs must use distinct OS threads")
task.shutdown()
print("overlapping Lua workers passed")
''')
            result = self.run_process(support.LUAJIT, script, Path(directory) / "one", Path(directory) / "two")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "overlapping Lua workers passed\n")

    def test_type_mode_and_exception_examples(self):
        for mode in ("legacy", "gradual", "strict"):
            suffix = "lua" if mode == "legacy" else "arky"
            result = self.run_process(support.CLI, "run", support.ROOT / f"examples/type_modes/{mode}.{suffix}")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, f"{mode}\t42\n")
        for folder in ("tasks", "type_modes"):
            result = self.run_process(support.CLI, "check", "--project", support.ROOT / f"examples/{folder}/tynex.toml")
            self.assertEqual(result.returncode, 0, result.stderr)
        result = self.run_process(support.CLI, "check", "--mode", "strict", support.ROOT / "examples/type_modes/gradual.arky")
        self.assertNotEqual(result.returncode, 0)
        result = self.run_process(support.CLI, "run", support.ROOT / "examples/exceptions.arky")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "success: 42\nhandled failure\n")
