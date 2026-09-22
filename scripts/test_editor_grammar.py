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
    chaining = GRAMMAR / 'test/fixtures/optional-chaining.tlj'
    captures = run('query', '--captures', QUERIES / 'highlights.scm', chaining)
    for kind, text in [('operator', '?.'), ('operator', '?['), ('operator', '?('), ('operator', '?:'),
                       ('operator', '?'), ('property', 'score'), ('property', 'children'),
                       ('function', 'callback'), ('function.method', 'update'), ('function.method', 'configure'),
                       ('function.method', 'label'), ('function.method', 'find'), ('type', 'Player')]:
        assert any(f' - {kind},' in line and f'text: `{text}`' in line for line in captures.splitlines()), (kind, text, captures)
    brackets = run('query', '--captures', QUERIES / 'brackets.scm', chaining)
    for text in ['?(', '?[']:
        assert any(' - open,' in line and f'text: `{text}`' in line for line in brackets.splitlines()), (text, brackets)
    asynchronous = GRAMMAR / 'test/fixtures/async-await.tlj'
    captures = run('query', '--captures', QUERIES / 'highlights.scm', asynchronous)
    for kind, text in [('keyword', 'async'), ('keyword', 'await'), ('type.builtin', 'Task'),
                       ('function', 'pair'), ('function.method', 'fetch'), ('variable.parameter', 'value')]:
        assert any(f' - {kind},' in line and f'text: `{text}`' in line for line in captures.splitlines()), (kind, text, captures)
    io_fixture = GRAMMAR / 'test/fixtures/async-io.tlj'
    captures = run('query', '--captures', QUERIES / 'highlights.scm', io_fixture)
    for kind, text in [('type.builtin', 'Socket'), ('type.builtin', 'Task'),
                       ('keyword', 'await'), ('function', 'exchange'),
                       ('variable.parameter', 'socket'), ('function.method', 'read'),
                       ('function.method', 'write'), ('function.method', 'close')]:
        assert any(f' - {kind},' in line and f'text: `{text}`' in line for line in captures.splitlines()), (kind, text, captures)
    with tempfile.TemporaryDirectory() as directory:
        shape_query = Path(directory) / 'optional-shapes.scm'
        shape_query.write_text('''
(dot_index_expression
  table: (parenthesized_expression (optional_dot_index_expression))) @grouped.chain
(dot_index_expression
  table: (optional_function_call
    name: (optional_method_index_expression
      table: (bracket_index_expression table: (optional_dot_index_expression))))) @mixed.chain
(optional_type (type (parenthesized_type (type (function_type))))) @optional.callback
(function_type (return_type (type (optional_type)))) @optional.return
(return_annotation
  (return_type (type (optional_type
    (type (parenthesized_type (type (function_type)))))))) @optional.callback.result
(return_annotation
  (return_type (type (optional_type
    (type (parenthesized_type (type (primitive_type)))))))) @optional.scalar.result
''')
        shapes = run('query', '--captures', shape_query, chaining)
        for kind, text in [('grouped.chain', '(maybe?.child).score'),
                           ('mixed.chain', 'maybe?.children[index()]?:find("name").score'),
                           ('optional.callback', '(function(number): number)?'),
                           ('optional.return', 'function(number): number?'),
                           ('optional.callback.result', ': (function(number): number)?'),
                           ('optional.scalar.result', ': (number)?')]:
            assert any(f' - {kind},' in line and f'text: `{text}`' in line for line in shapes.splitlines()), (kind, text, shapes)
        source = Path(directory) / 'capture.tlj'
        for invalid_result in ['(number, string)?', '()?']:
            source.write_text(f'local function invalid(): {invalid_result} return nil end\n')
            parsed = subprocess.run([str(CLI), 'parse', str(source)], cwd=GRAMMAR, capture_output=True, text=True)
            assert parsed.returncode != 0 and 'ERROR' in parsed.stdout, (invalid_result, parsed.stdout, parsed.stderr)
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
        source.write_text('local broken = player?.\nlocal next: string = "still highlighted"\n')
        captures = run('query', '--captures', QUERIES / 'highlights.scm', source)
        assert any('type.builtin' in line and 'text: `string`' in line for line in captures.splitlines()), captures
        source.write_text('local async function broken(): number\n local n = await\nend\nlocal next: string = "still highlighted"\n')
        captures = run('query', '--captures', QUERIES / 'highlights.scm', source)
        assert any('type.builtin' in line and 'text: `string`' in line for line in captures.splitlines()), captures
    print(f'Tree-sitter highlighting: {len(examples)} examples, {len(fixtures)} editor fixtures, all queries, captures and recovery passed')


if __name__ == '__main__':
    main()
