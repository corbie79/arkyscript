These files are copied unchanged from tree-sitter-grammars/tree-sitter-lua,
revision 10fe0054734eec83049514ea2e718b2a56acd0c9 (MIT).

https://github.com/tree-sitter-grammars/tree-sitter-lua/tree/10fe0054734eec83049514ea2e718b2a56acd0c9

Tynex's grammar extends grammar.js; src/scanner.c renames the scanner's exported
symbols with preprocessor macros. Neither vendored file is patched. This is an
editor syntax grammar, not a dependency of the Rust parser or type checker.
