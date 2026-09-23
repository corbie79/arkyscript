#!/usr/bin/env python3
"""Stage the unpublished Zed extension and a local, versioned editor grammar.

The generated Git commit belongs only to build/editors/grammar, never the project
or upstream repositories. Install build/editors/zed with Zed's Install Dev Extension.
"""
import argparse
from pathlib import Path
import json
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'build/editors'


def run(*args, cwd):
    return subprocess.run(args, cwd=cwd, check=True, text=True, capture_output=True).stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--emmy-dap', type=Path, help='Copy an installed EmmyLua DAP executable for the mapped debugger')
    args = parser.parse_args()
    if args.emmy_dap:
        binary_dir = OUT / 'bin'
        binary_dir.mkdir(parents=True, exist_ok=True)
        suffix = '.exe' if args.emmy_dap.suffix.lower() == '.exe' else ''
        shutil.copy2(args.emmy_dap, binary_dir / ('emmylua_dap' + suffix))
    grammar = OUT / 'grammar'
    grammar.mkdir(parents=True, exist_ok=True)
    for name in ['src', 'vendor']:
        if (grammar / name).exists():
            shutil.rmtree(grammar / name)
        shutil.copytree(ROOT / 'editors/tree-sitter-tynex' / name, grammar / name, dirs_exist_ok=True)
    for name in ['grammar.js', 'package.json', 'tree-sitter.json']:
        shutil.copy2(ROOT / 'editors/tree-sitter-tynex' / name, grammar / name)
    if not (grammar / '.git').exists():
        run('git', 'init', '-q', cwd=grammar)
    run('git', 'add', '.', cwd=grammar)
    changed = subprocess.run(['git', 'diff', '--cached', '--quiet'], cwd=grammar).returncode
    if changed:
        hooks = OUT / 'empty-hooks'
        hooks.mkdir(exist_ok=True)
        run('git', '-c', 'user.name=Tynex build', '-c', 'user.email=build@tynex.invalid',
            '-c', 'commit.gpgsign=false', '-c', f'core.hooksPath={hooks}',
            'commit', '-qm', 'Generated Arky editor grammar snapshot', cwd=grammar)
    revision = run('git', 'rev-parse', 'HEAD', cwd=grammar)
    extension = OUT / 'zed'
    for name in ['src', 'languages', 'debug_adapter_schemas']:
        if (extension / name).exists():
            shutil.rmtree(extension / name)
    shutil.copytree(ROOT / 'editors/zed', extension, dirs_exist_ok=True,
        ignore=shutil.ignore_patterns('target', '.git', 'extension.wasm', 'grammars'))
    manifest = (ROOT / 'editors/zed/extension.toml').read_text()
    manifest += f'\n[grammars.tynex]\nrepository = {json.dumps(grammar.as_uri())}\nrev = "{revision}"\n'
    (extension / 'extension.toml').write_text(manifest)
    print(f'Zed Install Dev Extension: {extension}')
    print(f'Grammar snapshot: {revision}')


if __name__ == '__main__':
    main()
