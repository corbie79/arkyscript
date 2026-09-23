"""Real file and loopback TCP operations through typed managed Tasks."""
from contextlib import contextmanager
import json
import os
import socket
import threading
import unittest

import test_cli as support
import test_async_runtime as runtime_support


class AsyncIOTests(unittest.TestCase):
    setUpClass = classmethod(support.RustLuaJITTests.setUpClass.__func__)
    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file
    cli = support.RustLuaJITTests.cli
    lua = runtime_support.AsyncRuntimeTests.lua

    def run_source(self, source, expected=None):
        result = self.cli('run', self.file('main.luax', '--!typecheck strict\n' + source))
        self.assertEqual(result.returncode, 0, result.stderr)
        if expected is not None:
            self.assertEqual(result.stdout, expected)
        return result

    @contextmanager
    def server(self, handler, family=socket.AF_INET, connections=1):
        listener = socket.socket(family, socket.SOCK_STREAM)
        self.addCleanup(listener.close)
        listener.bind(('::1' if family == socket.AF_INET6 else '127.0.0.1', 0))
        listener.listen(connections)
        listener.settimeout(10)
        failures = []

        def serve():
            try:
                clients = []
                try:
                    for _ in range(connections):
                        client, _ = listener.accept()
                        client.settimeout(10)
                        clients.append(client)
                    handler(*clients)
                finally:
                    for client in clients:
                        client.close()
            except Exception as error:
                failures.append(error)
            finally:
                listener.close()

        thread = threading.Thread(target=serve, daemon=True)
        thread.start()
        client_failed = False
        try:
            yield listener.getsockname()[1]
        except BaseException:
            client_failed = True
            raise
        finally:
            thread.join(12)
            if not client_failed:
                self.assertFalse(thread.is_alive(), 'loopback server did not finish')
                if failures:
                    raise failures[0]

    @staticmethod
    def receive(client, count):
        result = bytearray()
        while len(result) < count:
            chunk = client.recv(min(16384, count - len(result)))
            if not chunk:
                break
            result.extend(chunk)
        return bytes(result)

    def test_parallel_file_reads_binary_empty_and_task_identity(self):
        (self.directory / '데이터.bin').write_bytes(b'a\0b\r\n\xff')
        (self.directory / 'empty.dat').write_bytes(b'')
        self.run_source(r'''
            local a: Task<string> = fs.read_file('데이터.bin')
            local b: Task<string> = fs.read_file('empty.dat', 0)
            local async function main(): ()
                local bytes = await a
                print(#bytes, bytes:sub(2, 2) == '\0', bytes:sub(6, 6) == '\255', #(await b))
                print(bytes == await a, bytes == a:result())
            end
            scheduler.run(main())
            print(scheduler.pending())
        ''', '6\ttrue\ttrue\t0\ntrue\ttrue\n0\n')

    def test_file_errors_limits_cancel_and_deadline(self):
        (self.directory / 'data.txt').write_text('abcdef')
        self.run_source('''
            local function failed(t: Task<string>): boolean
                try scheduler.run(t) except err then return true end
                return false
            end
            print(failed(fs.read_file('missing.dat')))
            print(failed(fs.read_file('data.txt', 5)))
            print(failed(fs.read_file('.')))
            print(failed(fs.read_file('data.txt', nil, 0)))
            local pending = fs.read_file('data.txt')
            print(pending:cancel(), pending:cancel(), pending:status(), failed(pending))
            print(scheduler.pending())
        ''', 'true\ntrue\ntrue\ntrue\ntrue\tfalse\tcancelled\ttrue\n0\n')

    @unittest.skipIf(os.name == 'nt', 'POSIX special file test')
    def test_named_pipe_is_rejected_without_waiting_for_writer(self):
        os.mkfifo(self.directory / 'pipe')
        self.run_source('''
            try scheduler.run(fs.read_file('pipe'))
            except err then print('rejected') end
        ''', 'rejected\n')

    def test_large_binary_write_chunked_read_and_eof(self):
        payload = b'a\0b\xff' * (512 * 1024)

        def echo(client):
            self.assertEqual(self.receive(client, len(payload)), payload)
            client.sendall(b'hello\0world')
            client.shutdown(socket.SHUT_WR)

        with self.server(echo) as port:
            self.run_source(f'''
                local async function main(): ()
                    local connection: Socket = await net.connect('localhost', {port})
                    local payload = 'a\\0b\\255'
                    for i=1,19 do payload = payload .. payload end
                    print(await connection:write(payload))
                    local result = ''
                    while true do
                        local chunk: string? = await connection:read(3)
                        if chunk == nil then break end
                        result = result .. chunk
                    end
                    print(#result, result:sub(6,6) == '\\0', result:sub(7))
                    print(await connection:read(1) == nil)
                    print(connection:close(), connection:close())
                end
                scheduler.run(main())
                print(scheduler.pending())
            ''', '2097152\n11\ttrue\tworld\ntrue\ntrue\tfalse\n0\n')

    def test_waiting_socket_allows_file_and_other_coroutines_to_progress(self):
        (self.directory / 'release.txt').write_text('go')

        def rendezvous(client):
            self.assertEqual(self.receive(client, 2), b'go')
            client.sendall(b'ok')

        with self.server(rendezvous) as port:
            self.run_source(f'''
                local socket = scheduler.run(net.connect('127.0.0.1', {port}))
                local read = socket:read(16)
                print(scheduler.step(), read:ready())
                local async function release(): string?
                    local signal = await fs.read_file('release.txt')
                    await socket:write(signal)
                    return await read
                end
                print(scheduler.run(release()))
                print(socket:close(), scheduler.pending())
            ''', 'true\tfalse\nok\ntrue\t0\n')

    def test_independent_sockets_make_progress_together(self):
        def exchange(first, second):
            self.assertEqual(self.receive(first, 1), b'a')
            self.assertEqual(self.receive(second, 1), b'a')
            first.sendall(b'x')
            second.sendall(b'x')

        with self.server(exchange, connections=2) as port:
            self.run_source(f'''
                local async function request(): string?
                    local socket = await net.connect('127.0.0.1', {port})
                    await socket:write('a')
                    local result = await socket:read(1)
                    socket:close()
                    return result
                end
                local a, b = request(), request()
                print(scheduler.run(a), scheduler.run(b), scheduler.pending())
            ''', 'x\tx\t0\n')

    def test_read_timeout_cancellation_and_close_release_waiters(self):
        def wait_for_close(client):
            self.assertEqual(client.recv(1), b'')

        for operation in ('timeout', 'cancel', 'close'):
            with self.subTest(operation=operation), self.server(wait_for_close) as port:
                action = {
                    'timeout': 'local pending = socket:read(1, 0)',
                    'cancel': 'local pending = socket:read(1) if not pending:cancel() then error("cancel failed") end',
                    'close': 'local pending = socket:read(1) if not socket:close() then error("close failed") end',
                }[operation]
                self.run_source(f'''
                    local socket = scheduler.run(net.connect('127.0.0.1', {port}))
                    {action}
                    try scheduler.run(pending)
                    except err then print('failed') end
                    print(socket:close(), scheduler.pending())
                ''', 'failed\nfalse\t0\n')

    def test_overlapping_reads_rejected_but_pending_read_still_completes(self):
        def exchange(client):
            self.assertEqual(self.receive(client, 2), b'go')
            client.sendall(b'ok')

        with self.server(exchange) as port:
            self.run_source(f'''
                local socket = scheduler.run(net.connect('127.0.0.1', {port}))
                local first = socket:read(2)
                try socket:read(2) except err then print('busy') end
                scheduler.run(socket:write('go'))
                print(scheduler.run(first))
                socket:close()
                print(scheduler.pending())
            ''', 'busy\nok\n0\n')

    def test_refused_connection_fails_task_without_hanging(self):
        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1', 0))
            port = reserved.getsockname()[1]
            self.run_source(f'''
                try scheduler.run(net.connect('127.0.0.1', {port}, 1000))
                except err then print('refused') end
                print(scheduler.pending())
            ''', 'refused\n0\n')

    def test_socket_gc_keeps_pending_operations_alive_then_releases_connection(self):
        marker = self.directory / 'closed.txt'

        def exchange(client):
            self.assertEqual(self.receive(client, 2), b'go')
            client.sendall(b'ok')
            self.assertEqual(client.recv(1), b'')
            marker.write_text('closed')

        with self.server(exchange) as port:
            self.lua(f'''
                local socket_task = scheduler.net.connect('127.0.0.1', {port})
                local socket = scheduler.run(socket_task)
                assert(socket == socket_task:result())
                local pending_read = socket:read(2)
                local weak = setmetatable({{socket}}, {{__mode='v'}})
                local send = socket:write('go')
                socket, socket_task = nil, nil
                collectgarbage('collect') collectgarbage('collect')
                assert(weak[1] == nil)
                assert(scheduler.run(send) == 2)
                assert(scheduler.run(pending_read) == 'ok')
                send, pending_read = nil, nil
                collectgarbage('collect') collectgarbage('collect')
                local marker = {json.dumps(str(marker).replace(chr(92), '/'))}
                local sleep = require('tynex_io')._async_sleep
                local closed = false
                for i=1,2000 do
                    local file = io.open(marker, 'rb')
                    if file then file:close() closed = true break end
                    sleep(1)
                end
                assert(closed, 'unreferenced socket was not released before VM exit')
                assert(scheduler.pending() == 0)
            ''')

    def test_write_overlap_empty_write_and_runtime_receiver_validation(self):
        data = b'x' * (1024 * 1024)

        def consume(client):
            self.assertEqual(self.receive(client, len(data)), data)

        with self.server(consume) as port:
            self.lua(f'''
                local socket = scheduler.run(scheduler.net.connect('127.0.0.1', {port}))
                assert(not pcall(socket.read, {{}}, 1))
                assert(not pcall(socket.write, nil, 'x'))
                assert(not pcall(function() socket.close = function() end end))
                assert(not pcall(socket.read, socket, 0))
                assert(not pcall(socket.read, socket, 8388609))
                assert(not pcall(socket.write, socket, 42))
                assert(scheduler.run(socket:write('')) == 0)
                local pending = socket:write(string.rep('x', 1048576))
                assert(not pcall(socket.write, socket, 'extra'))
                assert(scheduler.run(pending) == 1048576)
                assert(socket:close() and not socket:close())
                assert(not pcall(socket.read, socket, 1))
                assert(not pcall(socket.write, socket, 'x'))
                assert(scheduler.pending() == 0)
            ''')

    def test_request_capacity_recovers_after_cancellation(self):
        # Native capacity bounds unpolled handles even after a worker finishes
        # its bytes; cancellation frees capacity without waiting for result GC.
        self.lua('''
            local native = require('tynex_io')
            local pending = {}
            for i=1,256 do pending[i] = native.read_file('tests/io/runtime.lua') end
            local ok, failure = pcall(native.read_file, 'tests/io/runtime.lua')
            assert(not ok and failure:find('request limit', 1, true))
            for i=1,#pending do assert(native._async_cancel(pending[i])) end
            local next_request = native.read_file('tests/io/runtime.lua')
            while not native._async_ready(next_request) do native._async_sleep(1) end
            assert(#native._async_wait(next_request) > 0)
        ''')

    def test_ipv6_loopback(self):
        try:
            with socket.socket(socket.AF_INET6, socket.SOCK_STREAM) as probe:
                probe.bind(('::1', 0))
        except OSError:
            self.skipTest('IPv6 loopback unavailable')

        with self.server(lambda client: client.sendall(b'v6'), family=socket.AF_INET6) as port:
            self.run_source(f'''
                local socket = scheduler.run(net.connect('::1', {port}))
                print(scheduler.run(socket:read(16)))
                socket:close()
            ''', 'v6\n')

    def test_validation_rejects_invalid_arguments_without_allocating_tasks(self):
        expressions = [
            "fs.read_file('bad\\0path')", "fs.read_file('x', -1)",
            "fs.read_file('x', 1.5)", "fs.read_file('x', 67108865)",
            "fs.read_file('x', nil, -1)", "net.connect('bad\\0host', 80)",
            "net.connect('', 80)", "net.connect('127.0.0.1', 0)",
            "net.connect('127.0.0.1', 65536)", "net.connect('127.0.0.1', 80, 0/0)",
        ]
        source = '\n'.join(f'try {expression} except err then print("rejected") end' for expression in expressions)
        source += '\nprint(scheduler.pending())\n'
        self.run_source(source, 'rejected\n' * len(expressions) + '0\n')

    def test_file_example_and_bundle(self):
        example = support.ROOT / 'examples/io/files.luax'
        result = self.cli('run', example)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'first\t6\nsecond\t7\n')
        path = self.file('bundle.luax', '''
            local async function main(): string return await fs.read_file('bundle-data.txt') end
            print(scheduler.run(main()))
        ''')
        (self.directory / 'bundle-data.txt').write_text('bundled IO')
        bundle = self.directory / 'bundle'
        result = self.cli('bundle', path, '--out', str(bundle))
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.cli('exec-bundle', bundle)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'bundled IO\n')

    def test_io_error_bundle_trace_includes_original_await_line(self):
        path = self.file('io-error.luax', '''
            local async function load(): string
                return await fs.read_file('does-not-exist')
            end
            scheduler.run(load())
        ''')
        bundle = self.directory / 'bundle'
        result = self.cli('bundle', path, '--out', str(bundle))
        self.assertEqual(result.returncode, 0, result.stderr)
        result = self.cli('exec-bundle', bundle)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(path) + ':2:', result.stderr)


if __name__ == '__main__':
    unittest.main()
