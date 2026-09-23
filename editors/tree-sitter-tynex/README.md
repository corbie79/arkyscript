# Arky editor grammar

An independent editor grammar extending a pinned, unmodified MIT Lua grammar.
Arky source files use `.arky`; declaration files use `.d.arky`.
The Rust compiler remains authoritative for valid syntax and types. This grammar
recognizes optional and parenthesized types, records, arrays, maps, function
signatures, variadic parameters/results, class/trait declarations, async functions,
await expressions, builtin Task result packs, and optional
field/index/function/method chains (`?.`, `?[`, `?(`, `?:`). It recovers while editing incomplete
code. Future language features must add compiler tests and editor fixtures together.

`npm ci && npm run generate` regenerates src/parser.c and src/node-types.json.
`python3 ../../scripts/test_editor_grammar.py` checks all examples, highlighting
queries, and editing fixtures. Run `python3 ../../scripts/prepare_zed.py` after
changing the generated grammar or Zed extension. No upstream files are modified.
