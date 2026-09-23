"""Async syntax, checked Task values, VM scheduling and native worker integration."""
import json
from pathlib import Path
import unittest

import test_cli as support


class AsyncAwaitTests(unittest.TestCase):
    setUpClass = classmethod(support.RustLuaJITTests.setUpClass.__func__)
    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli

    def run_source(self, source, expected):
        path = self.file('main.luax', source)
        result = self.cli('run', path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, expected)
        return path

    def test_shipped_examples(self):
        for source, expected in [
            ('examples/async.luax', 'scheduled\tqueued\nresult\t72\nframe result\t20\n'),
            ('examples/tasks/async.luax', 'Async worker sum: 250015000\n'),
        ]:
            with self.subTest(source=source):
                result = self.cli('run', support.ROOT / source)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, expected)

    def test_async_creation_is_scheduled_and_frames_share_captured_values(self):
        self.run_source('''
            --!typecheck strict
            local total=0
            local async function add(n:number):number total=total+n return total end
            local a=add(10)
            local b=add(32)
            print(total,a:ready(),scheduler.pending())
            print(scheduler.step(1),total,a:result(),b:ready())
            print(scheduler.step(1),total,b:result(),scheduler.pending())
        ''', '0\tfalse\t2\ntrue\t10\t10\tfalse\nfalse\t42\t42\t0\n')

    def test_multiwaiters_nested_await_and_result_identity(self):
        self.run_source('''
            --!typecheck strict
            local calls=0
            local async function build():{n:number} calls=calls+1 return {n=40} end
            local shared=build()
            local async function first():number
                local value=await shared
                value.n=value.n+1
                return value.n
            end
            local async function second():number
                local value=await shared
                return value.n+1
            end
            local a,b=first(),second()
            print(scheduler.run(a),scheduler.run(b),calls)
            print(shared:result()==shared:result())
        ''', '41\t42\t1\ntrue\n')

    def test_varargs_multi_returns_empty_and_grouping(self):
        self.run_source('''
            --!typecheck strict
            local async function echo(...:any):(...any) return ... end
            local async function forward(...:any):(...any) return await echo(...) end
            local async function empty():() end
            local async function main():()
                print(select('#',await forward(1,nil,3,nil)),await forward(1,nil,3,nil))
                print(select('#',await empty()))
                print(select('#',(await forward(1,2,3))))
                local values={await forward(1,2,3)}
                print(#values,values[1],values[3])
                await empty()
            end
            scheduler.run(main())
        ''', '4\t1\tnil\t3\tnil\n0\n1\n3\t1\t3\n')

    def test_try_else_errors_preserve_original_values_across_yields(self):
        self.run_source('''
            --!typecheck strict
            local marker={message="original"}
            local async function fail(value:any):() error(value) end
            local async function ok():number return 42 end
            local async function main():()
                try await fail(marker) except err then print(err==marker) else print("bad") end
                try await fail(false) except err then print(err==false) end
                try await fail(nil) except err then print(err==nil) end
                try print(await ok()) except err then print("bad") else print("success") end
            end
            scheduler.run(main())
        ''', 'true\ntrue\ntrue\n42\nsuccess\n')

    def test_classes_traits_anonymous_and_recursive_async_functions(self):
        self.run_source('''
            --!typecheck strict
            trait Compute
                value: async function(Self,number):number
                async function doubled(n:number):number return (await self:value(n))*2 end
            end
            class Counter is Compute
                base:number=1
                async function value(n:number):number return self.base+n end
                async function recursive(n:number):number
                    if n==0 then return self.base end
                    return 1+await self:recursive(n-1)
                end
            end
            local async function recurse(n:number):number
                if n==0 then return 0 end
                return 1+await recurse(n-1)
            end
            local callback:async function(number):number=async function(n:number):number return n+1 end
            local item=Counter{}
            print(scheduler.run(item:doubled(20)),scheduler.run(item:recursive(4)))
            print(scheduler.run(recurse(5)),scheduler.run(callback(41)))
        ''', '42\t5\n5\t42\n')

    def test_optional_chain_awaits_and_async_factory_defaults(self):
        self.run_source('''
            --!typecheck strict
            local async function value():{n:number} return {n=42} end
            class Holder
                callback:async function():{n:number}=async function():{n:number} return await value() end
            end
            local async function main():()
                local holder=Holder{}
                print((await holder.callback?())?.n)
                local f:(async function():{n:number})?=nil
                local pending=f?()
                if pending~=nil then print((await pending)?.n) else print("skipped") end
            end
            scheduler.run(main())
        ''', '42\nskipped\n')

    def test_imports_share_scheduler_and_task_type_metadata(self):
        self.file('provider.luax', '''
            local async function get():number return 42 end
            return {get=get,pending=get()}
        ''')
        self.run_source('''
            --!typecheck strict
            import provider from "provider"
            local async function main():number
                local pending:Task<number>=provider.pending
                return await pending
            end
            print(scheduler.run(main()),scheduler.run(provider.get()))
        ''', '42\t42\n')

    def test_cancelled_task_does_not_run_and_waiter_can_handle_failure(self):
        self.run_source('''
            --!typecheck strict
            local calls=0
            local async function work():number calls=calls+1 return 42 end
            local pending=work()
            print(pending:cancel(),pending:cancel(),pending:status())
            local async function main():()
                try print(await pending) except err then print("cancelled",calls) end
            end
            scheduler.run(main())
        ''', 'true\tfalse\tcancelled\ncancelled\t0\n')

    def test_native_await_returns_control_before_job_finishes(self):
        self.file('tynex.toml','type_mode="strict"\nexternal_modules=["tynex_task"]\n[[overrides]]\nfiles=["*.lua"]\nmode="legacy"\n')
        self.file('task.lua','return require("tynex_task")')
        self.file('task.d.luax','declare spawn:function(function(any,any):any,any):any\ndeclare shutdown:function():()')
        release=self.directory/'release'
        self.file('host.lua', '''
            return {
                release=function() local f=assert(io.open(PATH,"w")); f:close() end,
                work=function(path,ctx)
                    local deadline=os.time()+10
                    while true do
                        ctx.checkpoint()
                        local f=io.open(path,"r")
                        if f then f:close() break end
                        assert(os.time()<deadline,"main loop blocked")
                    end
                    return 42
                end
            }
        '''.replace('PATH',json.dumps(str(release))))
        self.file('host.d.luax','declare release:function():()\ndeclare work:function(any,any):any')
        self.run_source('''
            --!typecheck strict
            import task from "task"
            import host from "host"
            local worker=task.spawn(host.work, PATH)
            local async function main():number return await scheduler.from_worker(worker) end
            local result=main()
            scheduler.step(1)
            print("frame",result:ready())
            host.release()
            print(scheduler.run(result))
            task.shutdown()
        '''.replace('PATH',json.dumps(str(release))), 'frame\tfalse\n42\n')

    def test_known_errors_and_illegal_awaits_block_execution(self):
        cases=[
            'local n=await 42',
            'local function f():number return await other() end',
            'local async function f():number return await 42 end',
            'local async function f():number return "bad" end',
            'local async function f():number return 1 end local n:number=f()',
            'local async function f():number return 1 end local n:string=scheduler.run(f())',
            'local async function f():number return 1 end local t:Task<string>=f()',
            'local async function f():() local function g():() await f() end end',
            'local async function f():() class X value:any=await f() end end',
            'local async function f():() local p:Task<number>?=nil print(await p) end',
        ]
        for mode in ('gradual','strict'):
            for source in cases:
                with self.subTest(mode=mode,source=source):
                    path=self.file('invalid.luax','print("MUST NOT RUN")\n'+source)
                    result=self.cli('run',path,'--mode',mode)
                    self.assertNotEqual(result.returncode,0)
                    self.assertNotIn('MUST NOT RUN',result.stdout)

    def test_original_error_lines_and_bundle_mapping(self):
        source='local async function fail():number\n   error("ASYNC_FAILURE")\nend\nlocal async function main():number\n   return await fail()\nend\nscheduler.run(main())\n'
        for newline in ('\n','\r\n'):
            with self.subTest(newline=repr(newline)):
                path=self.directory/'errors.luax'
                path.write_bytes(source.replace('\n',newline).encode())
                generated=self.cli('gen',path)
                self.assertEqual(generated.returncode,0,generated.stderr)
                self.assertEqual(len(generated.stdout.splitlines()),len(source.splitlines()))
                result=self.cli('run',path)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(path.name+':2:',result.stderr)
                self.assertIn(path.name+':5:',result.stderr)
                bundle=self.directory/('bundle'+str(len(newline)))
                result=self.cli('bundle',path,'--out',str(bundle))
                self.assertEqual(result.returncode,0,result.stderr)
                result=self.cli('exec-bundle',bundle)
                self.assertNotEqual(result.returncode,0)
                self.assertIn(str(path)+':2:',result.stderr)
                self.assertIn(str(path)+':5:',result.stderr)

    def test_legacy_async_and_await_identifiers_still_work(self):
        path=self.file('legacy.lua','local async,await=20,22; print(async+await)')
        result=self.cli('run',path)
        self.assertEqual(result.returncode,0,result.stderr)
        self.assertEqual(result.stdout,'42\n')
