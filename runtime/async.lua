-- One scheduler belongs to one Lua VM and must be pumped on its owner thread.
-- Hosts may pass a trusted native-module loader when loading this chunk.
local worker_loader = ...
local raw_require, protected_call, raise = require, pcall, error
local lua_type, set_meta, lua_pairs, lua_next, raw_equal = type, setmetatable, pairs, next, rawequal
local unpack_values, count_values = unpack, select
local coroutine_create, coroutine_resume = coroutine.create, coroutine.resume
local coroutine_yield, coroutine_status, coroutine_running = coroutine.yield, coroutine.status, coroutine.running
local traceback, stringify = debug.traceback, tostring
local floor = math.floor
local function pack(...) return {n=count_values("#", ...), ...} end
local function text(value)
   local ok, result = protected_call(stringify, value)
   return ok and result or "<error object>"
end

-- Capture the native adapter before user modules can replace require, loaders,
-- package.loaded, or the native module table. A missing worker library is fine
-- for programs which use only coroutine tasks.
local native_ok, native = protected_call(worker_loader or function() return raw_require("tynex_task") end)
local native_ready, native_wait, native_cancel, native_sleep
if native_ok and lua_type(native) == "table" then
   native_ready, native_wait = native._async_ready, native._async_wait
   native_cancel, native_sleep = native._async_cancel, native._async_sleep
end
local native_available = lua_type(native_ready) == "function" and lua_type(native_wait) == "function"
   and lua_type(native_cancel) == "function" and lua_type(native_sleep) == "function"
native = nil

-- Lua 5.1 weak-key tables are not ephemerons: storing a state which contains
-- its own Task result would otherwise keep that entire result graph alive.
local states = set_meta({}, {__mode="kv"})
local worker_tasks = set_meta({}, {__mode="kv"})
local workers = {}
local queue, first, last = {}, 1, 0
local active, current, pumping = 0, nil, false
local yield_marker = {}
local last_failure
local methods, scheduler = {}, {}
local function readonly_task()
   raise("Task objects are read-only", 2)
end
local function record(task)
   local state = states[task]
   if not state then raise("expected a managed Task", 3) end
   return state
end
local function finished(state)
   return state.status == "succeeded" or state.status == "failed" or state.status == "cancelled"
end
local function enqueue(state)
   state.status = "queued"
   last = last + 1
   queue[last] = state
end
local function new_task(state)
   state.methods = methods
   -- The Task owns its state through this closure; only active work is rooted
   -- by the scheduler. The registry merely validates a Task's identity.
   local task = set_meta({}, {__index=function(_, key) return state.methods[key] end,
      __metatable="Tynex Task", __newindex=readonly_task})
   state.waiters = {}
   states[task] = state
   active = active + 1
   return task
end
local function detach_waiter(state)
   local dependency = state.waiting_on
   if dependency then
      local retained = {}
      for i=1,#dependency.waiters do
         local waiter = dependency.waiters[i]
         if waiter ~= state then retained[#retained+1] = waiter end
      end
      dependency.waiters = retained
      state.waiting_on = nil
   end
end
local function settle(state, status, values, failure, trace)
   if finished(state) then return false end
   detach_waiter(state)
   state.status, state.values, state.failure, state.trace = status, values, failure, trace
   state.thread, state.arguments, state.worker = nil, nil, nil
   workers[state] = nil
   active = active - 1
   local waiters = state.waiters
   state.waiters = {}
   for i=1,#waiters do
      local waiter = waiters[i]
      if waiter.status == "waiting" and waiter.waiting_on == state then
         waiter.waiting_on = nil
         enqueue(waiter)
      end
   end
   return true
end
local function result(state)
   if state.status == "succeeded" then return unpack_values(state.values, 1, state.values.n) end
   if state.status == "failed" or state.status == "cancelled" then
      last_failure = state
      raise(state.failure, 0)
   end
   raise("async task is still pending; use await or scheduler.run", 3)
end
function methods:ready() return finished(record(self)) end
function methods:status() return record(self).status end
function methods:result() return result(record(self)) end
function methods:cancel()
   local state = record(self)
   if finished(state) then return false end
   if state.worker then
      -- Cancellation belongs to this explicit wrapper, never to an awaited
      -- dependency merely because one of its waiters was cancelled.
      local ok, failure = protected_call(native_cancel, state.worker)
      if not ok then
         settle(state, "failed", nil, failure, traceback(text(failure), 2))
         return false
      end
   end
   return settle(state, "cancelled", nil, "async task cancelled", traceback("async task cancelled", 2))
end
function scheduler.create(fn)
   if lua_type(fn) ~= "function" then raise("async expects a function", 2) end
   return function(...)
      local state = {status="created", thread=coroutine_create(fn), arguments=pack(...)}
      local task = new_task(state)
      enqueue(state)
      return task
   end
end
function scheduler.await(task)
   if not current or coroutine_running() ~= current.thread then
      raise("await is only valid inside a managed async function", 2)
   end
   if finished(current) then raise("async task cancelled", 2) end
   local dependency = record(task)
   if finished(dependency) then return result(dependency) end
   local cursor = dependency
   while cursor do
      if cursor == current then raise("async await dependency cycle", 2) end
      cursor = cursor.waiting_on
   end
   coroutine_yield(yield_marker, dependency)
   return result(dependency)
end
function scheduler.from_worker(handle)
   if not native_available then raise("native task adapter is unavailable; rebuild tynex_task", 2) end
   -- The C adapter checks exact userdata storage, metatable, and private tag.
   -- Never invoke arbitrary ready/wait/cancel methods supplied by the caller.
   local ok, ready = protected_call(native_ready, handle)
   if not ok then raise(ready, 2) end
   local previous = worker_tasks[handle]
   if previous then return previous end
   local state = {status="waiting", worker=handle}
   local task = new_task(state)
   worker_tasks[handle] = task
   workers[state] = true
   return task
end
local function poll_workers()
   for state in lua_pairs(workers) do
      local ok, ready = protected_call(native_ready, state.worker)
      if not ok then
         settle(state, "failed", nil, ready, traceback(text(ready), 2))
      elseif ready then
         local values = pack(protected_call(native_wait, state.worker))
         if values[1] then
            local returns = {n=values.n-1}
            for i=2,values.n do returns[i-1] = values[i] end
            settle(state, "succeeded", returns)
         else
            settle(state, "failed", nil, values[2], traceback(text(values[2]), 2))
         end
      end
   end
end
local function resume_one(state)
   if state.status ~= "queued" then return end
   state.status = "running"
   current = state
   last_failure = nil
   local arguments = state.arguments
   state.arguments = nil
   local values
   if arguments then values = pack(coroutine_resume(state.thread, unpack_values(arguments, 1, arguments.n)))
   else values = pack(coroutine_resume(state.thread)) end
   current = nil
   if finished(state) then return end
   if not values[1] then
      local trace = traceback(state.thread, text(values[2]), 0)
      if last_failure and raw_equal(last_failure.failure, values[2]) and last_failure.trace then
         trace = last_failure.trace .. "\nawaited from:\n" .. trace
      end
      last_failure = nil
      settle(state, "failed", nil, values[2], trace)
   elseif coroutine_status(state.thread) == "dead" then
      local returns = {n=values.n-1}
      for i=2,values.n do returns[i-1] = values[i] end
      settle(state, "succeeded", returns)
   elseif values[2] == yield_marker and values[3] then
      local dependency = values[3]
      state.status, state.waiting_on = "waiting", dependency
      dependency.waiters[#dependency.waiters+1] = state
   else
      local failure = "async functions must suspend using await, not coroutine.yield"
      settle(state, "failed", nil, failure, traceback(state.thread, failure, 0))
   end
end
local function step(max_resumes)
   if pumping or current then raise("scheduler.step cannot run inside an async function or another pump", 2) end
   if max_resumes == nil then max_resumes = 64 end
   if lua_type(max_resumes) ~= "number" or max_resumes < 1 or max_resumes > 1000000 or max_resumes ~= floor(max_resumes) then
      raise("scheduler.step expects an integer budget from 1 to 1000000", 2)
   end
   pumping = true
   poll_workers()
   local resumed = 0
   while first <= last and resumed < max_resumes do
      local state = queue[first]
      queue[first] = nil
      first = first + 1
      if state.status == "queued" then
         resumed = resumed + 1
         resume_one(state)
      end
   end
   if first > last then queue, first, last = {}, 1, 0 end
   pumping = false
   return active > 0
end
scheduler.step = step
function scheduler.pending() return active end
function scheduler.run(task)
   if pumping or current then raise("scheduler.run cannot run inside an async function or another pump", 2) end
   local state = record(task)
   while not finished(state) do
      step()
      if not finished(state) and first > last then
         if lua_next(workers) then native_sleep(1)
         else raise("async scheduler deadlock: no runnable task or pending native job", 2) end
      end
   end
   return result(state)
end
-- The runner captures this before executing user code. Keep error objects intact
-- for pcall/try while reporting the suspended coroutine's original stack.
function scheduler.__traceback(failure)
   local state = last_failure
   last_failure = nil
   if state and raw_equal(state.failure, failure) then return state.trace end
end
return scheduler
