"""LSP legacy diagnostics use the pinned engine without running user code."""
import json
import os
import subprocess
import unittest
import test_cli as support


class LspRuntimeTests(unittest.TestCase):
    setUpClass = classmethod(support.RustLuaJITTests.setUpClass.__func__)
    setUp = support.RustLuaJITTests.setUp
    file = support.RustLuaJITTests.file

    def check_buffer(self, filename, text):
        path = self.directory / filename
        marker = self.directory / 'must-not-run'
        startup = self.file('startup.lua', f'local f=assert(io.open({json.dumps(str(marker))},"w")); f:write("bad"); f:close()')
        messages = [
            {'jsonrpc':'2.0','id':0,'method':'initialize','params':{'capabilities':{}}},
            {'jsonrpc':'2.0','method':'initialized','params':{}},
            {'jsonrpc':'2.0','method':'textDocument/didOpen','params':{'textDocument':{'uri':path.as_uri(),'version':1,'languageId':'lua','text':text}}},
            {'jsonrpc':'2.0','id':1,'method':'shutdown'},
            {'jsonrpc':'2.0','method':'exit'},
        ]
        frames = bytearray()
        for message in messages:
            body=json.dumps(message).encode()
            frames.extend(f'Content-Length: {len(body)}\r\n\r\n'.encode()+body)
        result = subprocess.run([str(support.CLI),'lsp'],input=frames,capture_output=True,timeout=15,
            env={**os.environ,'TYNEX_LUAJIT':str(support.LUAJIT),'LUA_INIT':'@'+str(startup)})
        self.assertEqual(result.returncode,0,result.stderr)
        data=result.stdout
        reports=[]
        while data:
            header, data=data.split(b'\r\n\r\n',1)
            length=int(header.split(b':')[1])
            message=json.loads(data[:length]);data=data[length:]
            if message.get('method')=='textDocument/publishDiagnostics': reports.append(message['params'])
        self.assertFalse(marker.exists())
        self.assertFalse(path.exists())
        return next(p['diagnostics'] for p in reports if p['uri']==path.as_uri())

    def test_unsaved_legacy_syntax_and_no_execution(self):
        errors=self.check_buffer('legacy.lua','error("BODY MUST NOT RUN")\nlocal x = )')
        self.assertEqual(errors[0]['code'],'LUA_SYNTAX')
        self.assertEqual(errors[0]['range']['start']['line'],1)

    def test_shebang_and_bom_match_loadfile_semantics(self):
        self.assertEqual(self.check_buffer('legacy.lua','\ufeff#!/usr/bin/env luajit\nerror("BODY MUST NOT RUN")\n'),[])

    def test_legacy_type_mismatch_is_not_a_type_error(self):
        self.assertEqual(self.check_buffer('legacy.lua','local value = 1\nvalue = "ok"\nerror("BODY MUST NOT RUN")'),[])

    def test_static_import_types_and_named_declarations_in_unsaved_buffer(self):
        self.file('api.lua', 'error("MODULE MUST NOT RUN")')
        self.file('api.d.tlj', 'declare n: number')
        source='import api from "api"\ntry\n  local n:string=api.n\nexcept err then print(err) end'
        errors=self.check_buffer('main.tlj',source)
        self.assertEqual(errors[0]['code'],'T001')
        self.assertEqual(errors[0]['range']['start']['line'],2)
        self.assertEqual(self.check_buffer('main.tlj',source.replace('n:string','n:number')),[])

    def test_named_declaration_buffer_rejects_executable_code(self):
        self.assertEqual(self.check_buffer('host.d.tlj','declare n: number'),[])
        errors=self.check_buffer('host.d.tlj','declare n: number = 1')
        self.assertEqual(errors[0]['code'],'P001')

    def test_class_and_trait_diagnostics_in_unsaved_buffer(self):
        source = ('--!typecheck strict\n'
                  'trait Named label:function(Self):string end\n'
                  'class User is Named\n'
                  '  name:string\n'
                  '  function label():string return self.name end\n'
                  'end\n'
                  'local user=User {name="Mina"}\n'
                  'user.name=42\n')
        errors = self.check_buffer('classes.tlj', source)
        self.assertTrue(errors)
        self.assertEqual(errors[0]['code'], 'T001')
        self.assertEqual(errors[0]['range']['start']['line'], 7)
        self.assertEqual(self.check_buffer('classes.tlj', source.replace('user.name=42', 'user.name="Joon"')), [])
