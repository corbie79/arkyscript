#!/usr/bin/env python3
"""Live mapped debugging against the installed, unmodified EmmyLua DAP/core."""
import argparse
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import threading

ROOT = Path(__file__).resolve().parents[1]
CLI = ROOT / 'target/debug' / ('arky.exe' if os.name == 'nt' else 'arky')


class Dap:
    def __init__(self, process):
        self.process = process
        self.queue = queue.Queue()
        self.pending = []
        self.sequence = 0
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        try:
            while True:
                length = None
                while line := self.process.stdout.readline():
                    if line == b'\r\n':
                        break
                    if line.lower().startswith(b'content-length:'):
                        length = int(line.split(b':')[1])
                if length is None:
                    raise EOFError('DAP exited')
                self.queue.put(json.loads(self.process.stdout.read(length)))
        except Exception as error:
            self.queue.put(error)

    def send(self, command, arguments=None):
        self.sequence += 1
        data = json.dumps({'seq': self.sequence, 'type': 'request', 'command': command, 'arguments': arguments or {}}).encode()
        self.process.stdin.write(f'Content-Length: {len(data)}\r\n\r\n'.encode() + data)
        self.process.stdin.flush()
        return self.sequence

    def wait(self, predicate):
        for index, message in enumerate(self.pending):
            if predicate(message):
                return self.pending.pop(index)
        while True:
            message = self.queue.get(timeout=20)
            if isinstance(message, Exception):
                raise message
            if predicate(message):
                return message
            self.pending.append(message)

    def request(self, command, arguments=None):
        seq = self.send(command, arguments)
        response = self.wait(lambda m: m.get('type') == 'response' and m.get('request_seq') == seq)
        assert response.get('success'), response
        return response.get('body', {})

    def event(self, event):
        return self.wait(lambda m: m.get('event') == event).get('body', {})


def run(backend, core, directory, error=False, extensions=False):
    with socket.socket() as port_socket:
        port_socket.bind(('127.0.0.1', 0))
        port = port_socket.getsockname()[1]
    env = {**os.environ, 'TYNEX_EMMY_CORE_DIR': str(core), 'TYNEX_DEBUG_PORT': str(port)}
    engine = subprocess.Popen([str(CLI), 'exec-bundle', str(directory), '--debug'], cwd=ROOT,
                              env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    adapter = None
    try:
        assert b'Waiting for EmmyLua' in engine.stderr.readline()
        adapter = subprocess.Popen([str(CLI), 'dap', '--map', str(directory / 'debug-map.json'), '--backend', str(backend)],
                                   cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        dap = Dap(adapter)
        capabilities = dap.request('initialize', {'clientID':'tynex-test', 'adapterID':'emmylua_new', 'linesStartAt1':True, 'columnsStartAt1':True, 'pathFormat':'path'})
        dap.request('launch', {'host':'127.0.0.1', 'port':port, 'sourcePaths':[str(directory)], 'ext':['.lua'], 'ideConnectDebugger':True})
        main = ROOT / 'examples/debug' / ('error.arky' if error else 'main.arky')
        worker = ROOT / 'examples/debug/worker.arky'
        if extensions:
            main = ROOT / 'examples/imports/main.arky'
            worker = ROOT / 'examples/imports/worker.arky'
        first_line = 6 if extensions else 2
        dap.request('setBreakpoints', {'source':{'path':str(main)}, 'breakpoints':[{'line':first_line}]})
        dap.request('configurationDone')
        stop = dap.event('stopped')
        tid = stop['threadId']
        frames = dap.request('stackTrace', {'threadId':tid})['stackFrames']
        assert frames[0]['source']['path'] == str(main) and frames[0]['line'] == first_line, frames
        dap.request('stepIn', {'threadId':tid})
        dap.event('stopped')
        frames = dap.request('stackTrace', {'threadId':tid})['stackFrames']
        expected = 4 if extensions else (6 if error else 2)
        assert frames[0]['source']['path'] == str(worker) and frames[0]['line'] == expected, frames
        if extensions:
            assert dap.request('evaluate', {'frameId':frames[0]['id'],'expression':'value','context':'watch'})['result'] == '21'
            dap.request('setBreakpoints', {'source':{'path':str(main)}, 'breakpoints':[{'line':10}]})
            dap.request('continue', {'threadId':tid})
            dap.event('stopped')
            top = dap.request('stackTrace', {'threadId':tid})['stackFrames'][0]
            assert top['line'] == 10 and top['source']['path'] == str(main), top
            assert dap.request('evaluate', {'frameId':top['id'],'expression':'score','context':'watch'})['result'] == '42'
            dap.request('setBreakpoints', {'source':{'path':str(main)}, 'breakpoints':[]})
        elif not error:
            frame = frames[0]['id']
            scopes = dap.request('scopes', {'frameId':frame})['scopes']
            variables = []
            for scope in scopes:
                if not scope.get('expensive'):
                    variables += dap.request('variables', {'variablesReference':scope['variablesReference']})['variables']
            assert any(v['name'] == 'score' and v['value'] == '10' for v in variables), variables
            assert dap.request('evaluate', {'frameId':frame,'expression':'score + amount','context':'watch'})['result'] == '15'
            dap.request('setBreakpoints', {'source':{'path':str(worker)}, 'breakpoints':[{'line':2,'condition':'score == 15'}]})
            dap.request('next', {'threadId':tid})
            dap.event('stopped')
            top = dap.request('stackTrace', {'threadId':tid})['stackFrames'][0]
            assert top['line'] == 3 and top['source']['path'] == str(worker), top
            assert dap.request('evaluate', {'frameId':top['id'],'expression':'total','context':'watch'})['result'] == '15'
            dap.request('continue', {'threadId':tid})
            dap.event('stopped')
            top = dap.request('stackTrace', {'threadId':tid})['stackFrames'][0]
            assert top['line'] == 2 and top['source']['path'] == str(worker), top
            assert dap.request('evaluate', {'frameId':top['id'],'expression':'score','context':'watch'})['result'] == '15'
        dap.request('continue', {'threadId':tid})
        engine.wait(timeout=20)
        # EmmyLua may close transport without a terminated event; inspect the process exit too.
        assert engine.returncode == (1 if error else 0), engine.stderr.read().decode()
        if not error:
            assert ('score: 42' if extensions else 'second total: 30') in engine.stdout.read().decode()
        return {'stepInto': expected, 'originalFile': str(worker), 'exitCode': engine.returncode,
                'staticImportTryElse': extensions, 'watch': 42 if extensions else (None if error else 15), 'conditionalBreakpoint': not error and not extensions, 'capabilities':capabilities}
    finally:
        for process in [adapter, engine]:
            if process:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=5)
                for stream in [process.stdin, process.stdout, process.stderr]:
                    if stream:
                        stream.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', type=Path, required=True)
    parser.add_argument('--core', type=Path, required=True)
    args = parser.parse_args()
    results = []
    for error in [False, True, False]:
        name = 'error' if error else 'main'
        directory = ROOT / 'build/debug' / ('error' if error else 'modules')
        subprocess.run([str(CLI), 'bundle', f'examples/debug/{name}.arky', '--out', str(directory)], cwd=ROOT, check=True)
        results.append(run(args.backend.resolve(), args.core.resolve(), directory, error))
    directory = ROOT / 'build/debug/imports'
    subprocess.run([str(CLI), 'bundle', 'examples/imports/main.arky', '--out', str(directory)], cwd=ROOT, check=True)
    results.append(run(args.backend.resolve(), args.core.resolve(), directory, extensions=True))
    destination = ROOT / 'build/editor-test/emmy-mapped-result.json'
    destination.write_text(json.dumps(results, indent=2))
    print(f'EmmyLua mapped debug, module error and restart passed: {destination}')


if __name__ == '__main__':
    main()
