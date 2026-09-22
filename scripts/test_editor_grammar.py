#!/usr/bin/env python3
"""Parse all examples and check captures, queries and recovery with pinned CLI."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
GRAMMAR = ROOT / 'editors/tree-sitter-tynex'
CLI = GRAMMAR / 'node_modules/.bin/tree-sitter'
QUERIES = ROOT / 'editors/zed/languages/tynex'


def run(*args):
    result = subprocess.run([str(CLI), *map(str, args)], cwd=GRAMMAR, capture_output=True, text=True)
    if result.returncode:
        raise AssertionError(result.stdout + result.stderr)
    return result.stdout


def main():
    examples = sorted((ROOT / 'examples').rglob('*.tlj'))
    run('parse', '--quiet', *examples)
    for query in QUERIES.glob('*.scm'):
        run('query', '--quiet', query, *examples)
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / 'capture.tlj'
        source.write_text('local function add(score: number, amount: number?): number\n'
                          '  -- 한글 😀\n  local text: string = "number"\n'
                          '  return score + amount\nend\n')
        captures = run('query', '--captures', QUERIES / 'highlights.scm', source)
        for kind, text in [('function', 'add'), ('variable.parameter', 'score'),
                           ('type.builtin', 'number'), ('comment', '-- 한글 😀'), ('string', '"number"')]:
            assert any(kind in line and f'text: `{text}`' in line for line in captures.splitlines()), (kind, captures)
        source.write_text('import api: any from "api"\ntry print(api.n) except err then print(err) else print(1) end\ndeclare n: number')
        captures = run('query', '--captures', QUERIES / 'highlights.scm', source)
        for word in ['import', 'from', 'try', 'except', 'declare']:
            assert any('keyword' in line and f'text: `{word}`' in line for line in captures.splitlines()), captures
        source.write_text('local broken: =\nlocal next: string = "still highlighted"\n')
        captures = run('query', '--captures', QUERIES / 'highlights.scm', source)
        assert any('type.builtin' in line and 'text: `string`' in line for line in captures.splitlines()), captures
    print(f'Tree-sitter highlighting: {len(examples)} examples, all queries, captures and recovery passed')


if __name__ == '__main__':
    main()
