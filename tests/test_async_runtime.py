"""Scheduler contract tests against stock LuaJIT, independent of syntax lowering."""
import json
import os
from pathlib import Path
import subprocess
import textwrap
import unittest

from test_cli import LUAJIT, ROOT


class AsyncRuntimeTests(unittest.TestCase):
    def lua(self, source):
        runtime = json.dumps(str(ROOT / "runtime/async.lua").replace("\\", "/"))
        code = f"local scheduler = assert(loadfile({runtime}))()\n" + textwrap.dedent(source)
        environment = {**os.environ, "LUA_CPATH": str(LUAJIT.parent / "?.so") + ";" + str(LUAJIT.parent / "?.dll") + ";;"}
        environment.pop("LUA_INIT", None)
        result = subprocess.run([str(LUAJIT), "-"], input=code, env=environment, cwd=ROOT,
                                capture_output=True, text=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def io_contract(self, source):
        # A trusted injected adapter lets these tests control readiness exactly.
        # Native userdata validation and actual I/O have separate integration tests.
        return self.lua(r"""
            local requests=setmetatable({}, {__mode='k'})
            local sockets=setmetatable({}, {__mode='k'})
            local waits,cancels,sleeps,collected=0,0,0,0
            local latest
            local function request(value)
                local handle=newproxy(true)
                requests[handle]={ready=false,value=value}
                latest=handle
                return handle
            end
            local native={}
            function native._async_ready(handle) return assert(requests[handle],'foreign request').ready end
            function native._async_wait(handle)
                local state=assert(requests[handle],'foreign request')
                assert(state.ready,'wait blocked')
                waits=waits+1
                return state.value
            end
            function native._async_cancel(handle)
                local state=assert(requests[handle],'foreign request')
                cancels=cancels+1
                state.cancelled=true
                return true
            end
            function native._async_sleep(milliseconds)
                assert(milliseconds == 1)
                sleeps=sleeps+1
                for _,state in pairs(requests) do state.ready=true end
            end
            function native.read_file(path,max_bytes,timeout_ms)
                assert(path == 'data.bin' and max_bytes == 100 and timeout_ms == 10)
                return request('a\0b')
            end
            function native.connect(host,port,timeout_ms)
                assert(host == 'localhost' and port == 1234 and timeout_ms == 10)
                local socket=newproxy(true)
                sockets[socket]={closed=false}
                getmetatable(socket).__gc=function() collected=collected+1 end
                return request(socket)
            end
            function native.read(socket,max_bytes,timeout_ms)
                assert(sockets[socket] and not sockets[socket].closed)
                assert(max_bytes == 100 and timeout_ms == 10)
                return request(nil)
            end
            function native.write(socket,data,timeout_ms)
                assert(sockets[socket] and not sockets[socket].closed)
                assert(data == 'payload' and timeout_ms == 10)
                return request(#data)
            end
            function native.close(socket)
                local state=assert(sockets[socket],'foreign socket')
                if state.closed then return false end
                state.closed=true
                return true
            end
            local isolated=assert(loadfile('runtime/async.lua'))(
                function() error('no worker library') end, function() return native end)
        """ + textwrap.dedent(source))

    def test_io_adapter_capture_eof_pending_cancel_and_private_socket_receivers(self):
        self.io_contract("""
            local fs,net=isolated.fs,isolated.net
            local file=fs.read_file('data.bin',100,10)
            assert(isolated.step() and waits == 0)
            assert(file:cancel() and cancels == 1 and not file:cancel())
            local cancelled=requests[latest]
            assert(cancelled.cancelled and isolated.pending() == 0)
            -- Later native module table mutation cannot redirect captured methods.
            for name in pairs(native) do native[name]=function() error('mutated native method') end end
            local data=isolated.run(fs.read_file('data.bin',100,10))
            assert(data == 'a\\0b' and sleeps >= 1 and waits == 1)
            local connected=net.connect('localhost',1234,10)
            local socket=isolated.run(connected)
            assert(connected:result() == socket and getmetatable(socket) == 'Tynex Socket')
            assert(not pcall(function() socket.read=function() end end))
            assert(not pcall(function() fs.read_file=function() end end))
            assert(not pcall(function() net.connect=function() end end))
            assert(not pcall(socket.read,{},100,10))
            assert(not pcall(socket.write,{},'payload',10))
            assert(not pcall(socket.close,{}))
            local eof=socket:read(100,10)
            assert(select('#',isolated.run(eof)) == 1 and eof:result() == nil)
            assert(isolated.run(socket:write('payload',10)) == 7)
            assert(socket:close() and not socket:close())
            assert(not pcall(socket.read,socket,100,10))
            assert(waits == 4 and isolated.pending() == 0)
            assert(not isolated.step() and waits == 4)
        """)

    def test_io_waiter_cancellation_keeps_shared_request_and_socket_graphs_collect(self):
        self.io_contract("""
            local dependency=isolated.fs.read_file('data.bin',100,10)
            local wait=isolated.create(function() return isolated.await(dependency) end)
            local first,second=wait(),wait()
            assert(isolated.step() and first:status() == 'waiting' and second:status() == 'waiting')
            assert(first:cancel() and cancels == 0)
            assert(isolated.run(second) == 'a\\0b' and cancels == 0 and waits == 1)
            local weak=setmetatable({}, {__mode='v'})
            do
                local socket=isolated.run(isolated.net.connect('localhost',1234,10))
                weak[1]=socket
                collectgarbage() collectgarbage()
                assert(weak[1] and collected == 0)
                local holder=isolated.create(function() return socket end)()
                isolated.run(holder)
                weak[2]=holder
                -- An arbitrary user cycle must not root the native socket.
                rawset(socket,'cycle',holder)
            end
            latest=nil
            for i=1,4 do collectgarbage() end
            assert(weak[1] == nil and weak[2] == nil and collected == 1)
        """)

    def test_missing_io_module_does_not_affect_coroutines(self):
        self.lua("""
            local absent=function() error('not installed') end
            local isolated=assert(loadfile('runtime/async.lua'))(absent,absent)
            assert(isolated.run(isolated.create(function() return 42 end)()) == 42)
            local ok,err=pcall(isolated.fs.read_file,'data.bin')
            assert(not ok and err:find('I/O adapter is unavailable',1,true))
            ok,err=pcall(isolated.net.connect,'localhost',1234)
            assert(not ok and err:find('I/O adapter is unavailable',1,true))
        """)

    def test_scheduling_is_lazy_fifo_and_step_budget_is_bounded(self):
        self.assertEqual(self.lua("""
            local calls = {}
            local fn = scheduler.create(function(n) calls[#calls+1] = n return n end)
            local a,b,c = fn(1),fn(2),fn(3)
            assert(a:status() == 'queued' and scheduler.pending() == 3 and #calls == 0)
            local ok, err = pcall(a.result,a)
            assert(not ok and err:find('still pending',1,true))
            assert(scheduler.step(1) and a:ready() and not b:ready() and #calls == 1)
            assert(scheduler.step(1) and b:ready() and not c:ready())
            assert(not scheduler.step(1) and c:ready() and scheduler.pending() == 0)
            assert(table.concat(calls,',') == '1,2,3')
            print(a:result(),b:result(),c:result())
        """), "1\t2\t3\n")

    def test_lua_values_nil_slots_and_zero_returns_survive_nested_await(self):
        self.lua("""
            local marker, callback, userdata = {}, function() end, newproxy(true)
            local first = scheduler.create(function(...) return ... end)
            local second = scheduler.create(function(task) return scheduler.await(task) end)
            local task = second(second(first(marker,nil,callback,userdata,false,nil)))
            local function verify(...)
                assert(select('#',...) == 6)
                local a,b,c,d,e,f = ...
                assert(a == marker and b == nil and c == callback and d == userdata and e == false and f == nil)
            end
            verify(scheduler.run(task))
            verify(task:result())
            local empty = scheduler.create(function() end)
            assert(select('#',scheduler.run(second(empty()))) == 0)
        """)

    def test_error_objects_are_exact_and_tracebacks_keep_async_origin(self):
        self.lua("""
            local function verify(value)
                local origin = scheduler.create(function() error(value,0) end)
                local forward = scheduler.create(function(task) return scheduler.await(task) end)
                local task = forward(origin())
                local ok, failure = pcall(scheduler.run,task)
                assert(not ok and failure == value and task:status() == 'failed')
                local trace = scheduler.__traceback(failure)
                assert(type(trace) == 'string' and trace:find('stdin:',1,true))
                ok, failure = pcall(task.result, task)
                assert(not ok and failure == value)
            end
            verify(nil) verify(false) verify({}) verify('failure')
            local value = setmetatable({}, {__tostring=function() error('bad tostring') end})
            verify(value)
            local comparison = {__eq=function() error('error equality must never run') end}
            local first,second = setmetatable({},comparison),setmetatable({},comparison)
            local failed=scheduler.create(function() error(first,0) end)()
            assert(not pcall(scheduler.run,failed))
            assert(scheduler.__traceback(second) == nil)
            -- Await an already-failed dependency and recover its exact value.
            local caught=scheduler.create(function()
                local ok,failure=pcall(scheduler.await,failed)
                assert(not ok and rawequal(failure,first))
                error(second,0)
            end)()
            local ok,failure=pcall(scheduler.run,caught)
            assert(not ok and rawequal(failure,second))
        """)

    def test_shared_waiters_cancel_independently_and_never_resume_twice(self):
        self.lua("""
            local dependency, calls
            calls = 0
            local waiter = scheduler.create(function()
                local n = scheduler.await(dependency)
                calls = calls+1
                return n
            end)
            local a,b = waiter(),waiter()
            dependency = scheduler.create(function() return 42 end)()
            assert(scheduler.step(2) and a:status() == 'waiting' and b:status() == 'waiting')
            assert(a:cancel() and not a:cancel() and not dependency:ready())
            assert(scheduler.run(b) == 42 and calls == 1 and dependency:status() == 'succeeded')
            assert(not scheduler.step() and not scheduler.step() and calls == 1)
            local ok,err=pcall(a.result,a)
            assert(not ok and err == 'async task cancelled' and scheduler.pending() == 0)
        """)

    def test_dependency_cycles_and_raw_yields_are_diagnosed(self):
        self.lua("""
            local a,b
            a=scheduler.create(function() return scheduler.await(b) end)()
            b=scheduler.create(function() return scheduler.await(a) end)()
            local ok,err=pcall(scheduler.run,a)
            assert(not ok and err:find('dependency cycle',1,true))
            assert(a:ready() and b:ready() and scheduler.pending() == 0)
            local raw=scheduler.create(function() coroutine.yield('arbitrary') end)()
            ok,err=pcall(scheduler.run,raw)
            assert(not ok and err:find('must suspend using await',1,true))
            local ok,err=pcall(scheduler.await,a)
            assert(not ok and err:find('only valid inside',1,true))
            local self
            self=scheduler.create(function() return scheduler.await(self) end)()
            ok,err=pcall(scheduler.run,self)
            assert(not ok and err:find('dependency cycle',1,true))
        """)

    def test_pump_reentrancy_and_invalid_budgets_are_rejected(self):
        self.lua("""
            local task=scheduler.create(function()
                local ok,err=pcall(scheduler.step)
                assert(not ok and err:find('inside an async',1,true))
                ok,err=pcall(scheduler.run,{})
                assert(not ok and err:find('inside an async',1,true))
                return 10
            end)()
            assert(scheduler.run(task) == 10)
            for _,value in ipairs({0,-1,1.5,1000001,math.huge,0/0,'1',false}) do
                local ok=pcall(scheduler.step,value)
                assert(not ok)
            end
            assert(not pcall(scheduler.run,{}))
            assert(not pcall(scheduler.create,{}))
            assert(not pcall(function() task.custom = 1 end))
        """)

    def test_native_bridge_is_exact_and_captures_c_functions(self):
        self.lua("""
            local task=require('tynex_task')
            task.configure(1,8)
            local forged_called=false
            local forged={ready=function() forged_called=true return true end,wait=function() return 99 end}
            assert(not pcall(scheduler.from_worker,forged) and not forged_called)
            assert(not pcall(scheduler.from_worker,newproxy(true)))
            local worker=task.spawn(function(n) return {result=n*2} end,21)
            local managed=scheduler.from_worker(worker)
            assert(managed == scheduler.from_worker(worker))
            task._async_ready=function() error('mutated ready') end
            task._async_wait=function() error('mutated wait') end
            task._async_cancel=function() error('mutated cancel') end
            task._async_sleep=function() error('mutated sleep') end
            package.loaded.tynex_task={}
            local join=scheduler.create(function() return scheduler.await(managed).result end)
            assert(scheduler.run(join()) == 42 and managed:result().result == 42)
            assert(scheduler.pending() == 0)
            task.shutdown()
        """)

    def test_native_waiting_step_never_blocks_and_cancel_is_cooperative(self):
        self.lua("""
            local task=require('tynex_task')
            task.configure(1,8)
            local handle=task.spawn(function(_,control) while true do control.checkpoint() end end)
            local managed=scheduler.from_worker(handle)
            local waiter=scheduler.create(function() return scheduler.await(managed) end)()
            assert(scheduler.step() and not managed:ready() and waiter:status() == 'waiting')
            assert(waiter:cancel() and not managed:ready())
            assert(managed:cancel() and managed:status() == 'cancelled')
            assert(scheduler.pending() == 0)
            assert(not scheduler.step())
            task.shutdown()
        """)

    def test_native_errors_propagate_and_coroutine_only_needs_no_library(self):
        self.lua("""
            local task=require('tynex_task')
            local managed=scheduler.from_worker(task.spawn(function() error('WORKER_ORIGIN') end))
            local ok,err=pcall(scheduler.run,managed)
            assert(not ok and err:find('WORKER_ORIGIN',1,true) and managed:status() == 'failed')
            task.shutdown()
            local fresh=assert(loadfile('runtime/async.lua'))(function() error('missing') end)
            assert(fresh.run(fresh.create(function() return 7 end)()) == 7)
            ok,err=pcall(fresh.from_worker,{})
            assert(not ok and err:find('adapter is unavailable',1,true))
        """)

    def test_runtime_captures_mutable_lua_builtins(self):
        self.lua("""
            local original_pcall, original_error = pcall,error
            local done=scheduler.create(function() return 9 end)()
            pcall=nil error=nil pairs=nil next=nil unpack=nil select=nil type=nil
            coroutine.resume=nil coroutine.status=nil coroutine.running=nil
            assert(scheduler.run(done) == 9 and scheduler.pending() == 0)
            local ok=original_pcall(done.result,done)
            assert(ok)
        """)

    def test_completed_and_cancelled_cyclic_task_graphs_are_collected(self):
        self.lua("""
            local references=setmetatable({}, {__mode='v'})
            do
                local self
                self=scheduler.create(function() return self end)()
                references[1]=self
                assert(scheduler.run(self) == self)
                self=nil
            end
            do
                local task
                task=scheduler.create(function() return task end)()
                references[2]=task
                assert(task:cancel())
                task=nil
            end
            assert(not scheduler.step())
            collectgarbage() collectgarbage()
            assert(references[1] == nil and references[2] == nil)
            local retained=scheduler.create(function() return 42 end)()
            scheduler.run(retained)
            collectgarbage() collectgarbage()
            assert(retained:result() == 42)
        """)


if __name__ == '__main__':
    unittest.main()
