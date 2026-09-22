-- Also run against the dynamically loaded DLL/shared module through test_tasks.py.
local task = task or require("tynex_task")
local function fails(fn, text)
   local ok, err = pcall(fn)
   assert(not ok and tostring(err):find(text, 1, true), tostring(err))
end
fails(function() task.configure(0, 10) end, "1..64")
task.configure(2, 64)
local input = {name = "Mina", scores = {10, 20}, binary = "a\0b", [true] = false}
local work = task.spawn(function(data)
   data.scores[1] = 99
   data.total = data.scores[1] + data.scores[2]
   return data
end, input)
input.name = "changed"
local result = work:wait()
assert(result.name == "Mina" and result.total == 119 and input.scores[1] == 10)
assert(result.binary == "a\0b" and result[true] == false)
result.name = "local edit"
assert(work:wait().name == "Mina")
assert(work:ready() and work:status() == "succeeded" and not work:cancel())
assert(task.spawn(function() return nil end):wait() == nil)
assert(task.spawn(function() return jit.status() end):wait() == jit.status())
assert(task.spawn(function() return 1, 2 end):wait() == 1) -- one transferred result
_G.only_in_parent = 99
assert(task.spawn(function() return only_in_parent == nil end):wait())
task.spawn(function() leaked = 12 end):wait()
assert(task.spawn(function() return leaked == nil end):wait())
local bad = task.spawn(function() error("worker exploded") end)
fails(function() bad:wait() end, "worker exploded")
assert(bad:ready() and bad:status() == "failed")
fails(function() task.configure(1, 1) end, "before the first task")
local captured = 42
fails(function() task.spawn(function() return captured end) end, "upvalues")
fails(function() task.spawn(print) end, "Lua function")
local cycle = {}; cycle.self = cycle
local shared = {}; local alias = {shared, shared}
for _, value in ipairs({cycle, alias, setmetatable({}, {}), function() end, io.stdout, coroutine.create(function() end)}) do
   fails(function() task.spawn(function(x) return x end, value) end, "cannot cross VMs")
end
fails(function() task.spawn(function(x) return x end, math.huge) end, "finite")
fails(function() task.spawn(function(x) return x end, string.rep("x", 9 * 1024 * 1024)) end, "8 MiB")
fails(function() task.spawn(function() return function() end end):wait() end, "cannot cross VMs")
fails(function() task.spawn(function() return setmetatable({}, {}) end):wait() end, "cannot cross VMs")
-- These loops have no captured values. Cancellation can interrupt their JIT loops
-- at the explicit checkpoint without disabling the JIT or touching another VM.
local endless = {}
for i = 1, 8 do
   endless[i] = task.spawn(function(_, context)
      while true do context.checkpoint() end
   end)
end
for _, item in ipairs(endless) do assert(item:cancel()) end
for _, item in ipairs(endless) do
   fails(function() item:wait() end, "cancelled")
   assert(item:status() == "cancelled")
end
assert(task.spawn(function() return 42 end):wait() == 42)
task.shutdown()
task.shutdown()
fails(function() task.spawn(function() end) end, "shut down")
assert(work:wait().total == 119) -- completed handles survive shutdown
local methods = debug.getmetatable(work)
fails(function() methods.__index.wait({}) end, "tynex.task.handle")
local fake = newproxy(true)
debug.setmetatable(fake, methods)
fails(function() fake:wait() end, "storage")
debug.setmetatable(fake, nil) -- avoid intentionally invalid __gc at VM close
methods.__gc(work)
methods.__gc(work)
fails(function() work:wait() end, "released")
print("Lua task transfer, isolation, errors and cancellation: passed")
