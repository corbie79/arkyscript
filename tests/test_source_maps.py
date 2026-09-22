"""Generated bundles keep original source locations without patching LuaJIT."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from test_cli import ROOT, CLI, LUAJIT


class SourceMapTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        subprocess.run(['cargo','build','--locked','-p','tynex-cli'], cwd=ROOT, check=True, capture_output=True)

    def cli(self, *args):
        return subprocess.run([str(CLI), *map(str,args)], cwd=ROOT, env={**os.environ,'TYNEX_LUAJIT':str(LUAJIT)}, capture_output=True, text=True)

    def test_bundle_maps_modules_and_errors_without_editing_original(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            source = directory / '한글 test.tlj'
            original = 'local n: number = 7\nerror("mapped failure")\n'
            source.write_text(original)
            output = directory / 'out'
            result = self.cli('bundle',source,'--out',output)
            self.assertEqual(result.returncode,0,result.stderr)
            mapping = json.loads((output/'debug-map.json').read_text())['files'][0]
            self.assertEqual(mapping['lines'][:2],[None,None])
            self.assertEqual(mapping['lines'][3]['line'],2)
            generated = Path(mapping['generated'])
            standard = json.loads(generated.with_suffix('.lua.map').read_text())
            self.assertEqual(standard['version'],3)
            self.assertEqual(standard['file'],generated.name)
            self.assertEqual(standard['sourcesContent'],[original])
            self.assertEqual(standard['sources'],[source.resolve().as_posix()])
            if LUAJIT.is_file():
                failed = self.cli('exec-bundle',output)
                self.assertEqual(failed.returncode,1)
                self.assertIn(f'{source.resolve()}:2: mapped failure',failed.stderr)
                self.assertNotIn(str(generated)+':4',failed.stderr)
            self.assertEqual(source.read_text(),original)

    def test_bundle_refuses_unowned_output_and_can_rebuild(self):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory)
            source=directory/'main.tlj'; source.write_text('print(1)\n')
            output=directory/'output'; output.mkdir(); (output/'keep.txt').write_text('keep')
            failed=self.cli('bundle',source,'--out',output)
            self.assertNotEqual(failed.returncode,0)
            self.assertEqual((output/'keep.txt').read_text(),'keep')
            owned=directory/'bundle'
            self.assertEqual(self.cli('bundle',source,'--out',owned).returncode,0)
            source.write_text('print(2)\n')
            self.assertEqual(self.cli('bundle',source,'--out',owned).returncode,0)
            if LUAJIT.is_file():
                self.assertEqual(self.cli('exec-bundle',owned).stdout,'2\n')

    def test_previous_brand_bundle_requires_regeneration_and_can_be_rebuilt(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            source = directory / 'main.tlj'
            source.write_text('try print(42) except err then print(err) end\n')
            output = directory / 'bundle'
            output.mkdir()
            (output / '.tljit-bundle').write_text('TLJIT bundle v1\n')
            (output / 'runner.lua').write_text('error("stale runner")\n')
            rejected = self.cli('exec-bundle', output)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertIn('not a Tynex bundle', rejected.stderr)
            rebuilt = self.cli('bundle', source, '--out', output)
            self.assertEqual(rebuilt.returncode, 0, rebuilt.stderr)
            self.assertTrue((output / '.tynex-bundle').is_file())
            self.assertFalse((output / '.tljit-bundle').exists())
            if LUAJIT.is_file():
                result = self.cli('exec-bundle', output)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, '42\n')


if __name__ == '__main__':
    unittest.main()
