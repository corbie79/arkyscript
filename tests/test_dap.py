"""Exercise real framed proxy transport with a deterministic backend process."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from test_cli import ROOT, CLI
sys.path.insert(0, str(ROOT / 'scripts'))
from test_emmy_debug import Dap

BACKEND = '''import json,sys
while True:
    header=sys.stdin.buffer.readline()
    if not header: break
    length=int(header.split(b':')[1])
    assert sys.stdin.buffer.readline()==b'\\r\\n'
    request=json.loads(sys.stdin.buffer.read(length))
    command=request['command']
    if command=='setBreakpoints':
        assert request['arguments']['source']['path']==sys.argv[1]
        point=request['arguments']['breakpoints'][0]
        assert point['line']==4 and point['condition']=='score == 15'
        body={'breakpoints':[{'line':4,'verified':True}]}
    elif command=='stackTrace':
        body={'stackFrames':[{'id':1,'name':'add','line':4,'source':{'path':sys.argv[1],'sourceReference':9}}]}
    else: body={}
    response={'seq':1,'type':'response','request_seq':request['seq'],'command':command,'success':True,'body':body}
    payload=json.dumps(response).encode()
    sys.stdout.buffer.write(f'Content-Length: {len(payload)}\\r\\n\\r\\n'.encode()+payload)
    sys.stdout.buffer.flush()
'''


class DapTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(['cargo','build','--locked','-p','tynex-cli'],cwd=ROOT,check=True,capture_output=True)

    def test_backend_crash_is_not_success(self):
        with tempfile.TemporaryDirectory() as directory:
            mapping=Path(directory)/'map.json'
            mapping.write_text('{"version":1,"files":[]}')
            process=subprocess.Popen([str(CLI),'dap','--map',str(mapping),'--backend',sys.executable,'--','-c','raise SystemExit(7)'],
                stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            try:
                self.assertNotEqual(process.wait(timeout=5),0)
                self.assertIn('before terminating',process.stderr.read().decode())
            finally:
                if process.poll() is None: process.kill()
                process.wait(timeout=5)
                for stream in [process.stdin,process.stdout,process.stderr]: stream.close()

    def test_stdio_mapping_source_and_shutdown(self):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory)
            source=directory/'한글.luax'; source.write_text('local x: number = 1\nprint(x)\n')
            generated=directory/'generated.lua'
            mapping=directory/'map.json'
            mapping.write_text(json.dumps({'version':1,'files':[{'original':str(source),'generated':str(generated),
                'lines':[None,None,{'line':1,'column':1},{'line':2,'column':1}]}]}))
            backend=directory/'backend.py'; backend.write_text(BACKEND)
            process=subprocess.Popen([str(CLI),'dap','--map',str(mapping),'--backend',sys.executable,'--',str(backend),str(generated)],
                stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            try:
                dap=Dap(process)
                dap.request('initialize')
                points=dap.request('setBreakpoints',{'source':{'path':str(source)},'breakpoints':[{'line':2,'condition':'score == 15'}]})
                self.assertEqual(points['breakpoints'][0]['line'],2)
                self.assertEqual(points['breakpoints'][0]['source']['path'],str(source))
                frame=dap.request('stackTrace')['stackFrames'][0]
                self.assertEqual(frame['line'],2)
                self.assertEqual(frame['source']['sourceReference'],0)
                self.assertEqual(frame['source']['name'],source.name)
                content=dap.request('source',{'source':{'path':str(source)}})['content']
                self.assertEqual(content,source.read_bytes().decode('utf-8'))
                process.stdin.close()
                self.assertEqual(process.wait(timeout=5),0,process.stderr.read().decode())
            finally:
                if process.poll() is None: process.kill()
                process.wait(timeout=5)
                for stream in [process.stdin,process.stdout,process.stderr]:
                    stream.close()


if __name__ == '__main__': unittest.main()
