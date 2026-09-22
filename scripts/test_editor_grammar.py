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
    fixtures = sorted((GRAMMAR / 'test/fixtures').rglob('*.tlj'))
    sources = examples + fixtures
    run('parse', '--quiet', *sources)
    for query in QUERIES.glob('*.scm'):
        run('query', '--quiet', query, *sources)
    classes = GRAMMAR / 'test/fixtures/class-trait.tlj'
    captures = run('query', '--captures', QUERIES / 'highlights.scm', classes)
    for kind, text in [('keyword', 'class'), ('keyword', 'trait'), ('keyword', 'is'),
                       ('type', 'Player'), ('type', 'Self'), ('type', 'api.Player'),
                       ('type', 'api.Serializable'), ('property', 'score'), ('property', 'active'),
                       ('property', 'legacy_value'),
                       ('function.method', 'label'), ('function.method', 'describe'),
                       ('variable.parameter', 'amount')]:
        assert any(f' - {kind},' in line and f'text: `{text}`' in line for line in captures.splitlines()), (kind, captures)
    outline = run('query', '--captures', QUERIES / 'outline.scm', classes)
    for name in ['Player', 'Named', 'Scorable', 'label', 'describe', 'score', 'active']:
        assert any(' - name,' in line and f'text: `{name}`' in line for line in outline.splitlines()), (name, outline)
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
        source.write_text('class Broken\n  field: =\nend\ntrait Recovery\n  label: function(Self): string\nend\n')
        captures = run('query', '--captures', QUERIES / 'highlights.scm', source)
        for word in ['Recovery', 'Self']:
            assert any(' - type,' in line and f'text: `{word}`' in line for line in captures.splitlines()), captures
    print(f'Tree-sitter highlighting: {len(examples)} examples, {len(fixtures)} editor fixtures, all queries, captures and recovery passed')


if __name__ == '__main__':
    main()
