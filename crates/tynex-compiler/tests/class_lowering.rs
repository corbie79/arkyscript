use tynex_compiler::{Mode, compile, declaration, prepare, source_map::Position};

#[test]
fn class_and_trait_lowering_preserves_comments_and_lf_or_crlf_source_lines() {
    let source = "trait -- trait header\nNamed\n label: function(Self): string\n function -- method header\n describe(): string\n  return self:label()\n end\nend\nclass -- class header\nItem\n is -- implemented trait\n Named\n value: string = -- default expression\n  'hello'\n function -- method header\n label(): string\n  return self.value\n end\nend\nlocal item = Item {}\nprint(item:describe())\n";
    for source in [source.to_owned(), source.replace('\n', "\r\n")] {
        let output = compile(&source, Mode::Strict, true).unwrap();
        assert_eq!(source.lines().count(), output.lua.lines().count());
        assert_eq!(
            source.bytes().filter(|b| *b == b'\r').count(),
            output.lua.bytes().filter(|b| *b == b'\r').count()
        );
        assert!(output.source_map.mappings[0].original.is_none());
        for (line, text) in output.lua.lines().enumerate().skip(1) {
            assert_eq!(
                output
                    .source_map
                    .original_position(Position { line, column: 0 })
                    .unwrap()
                    .line,
                line,
                "{text}"
            );
        }
        for marker in ["'hello'", "return self:label()", "return self.value"] {
            let original_line = source
                .lines()
                .position(|line| line.contains(marker))
                .unwrap();
            let (line, text) = output
                .lua
                .lines()
                .enumerate()
                .find(|(_, text)| text.contains(marker))
                .unwrap();
            let column = text.find(marker).unwrap();
            assert_eq!(
                output
                    .source_map
                    .original_position(Position { line, column })
                    .unwrap()
                    .line,
                original_line
            );
        }
        assert!(output.lua.contains("-- class header"));
        assert!(output.lua.contains("-- method header"));
        assert!(output.lua.contains("-- default expression"));
    }
}

#[test]
fn parser_handles_empty_nested_and_qualified_declarations() {
    for source in [
        "class Empty end; trait EmptyTrait end; local empty = Empty {}",
        "local function build() class Local value = 1 end return Local {} end",
        "class Outer function build() class Inner value = 1 end return Inner {} end end",
        "class Container callback = function(...) return ... end end",
        "trait Named label: function(Self): string end class C is api.Named label: string end",
        "class Tree children: {Tree} = {} parent: Tree? end",
    ] {
        assert!(prepare(source, true).is_ok(), "{source}");
    }
}

#[test]
fn parser_rejects_incomplete_or_ambiguous_class_and_trait_forms() {
    for source in [
        "class end",
        "class Item value: number",
        "class Item is end",
        "class Item is A, end",
        "class Item value = end",
        "class Item function label(self) end end",
        "trait Named label: number end",
        "trait Named label end",
        "trait Named label: function(Self): string = function() end end",
        "trait Named function label(self) end end",
        "local function make(...) class Item value = ... end end",
    ] {
        assert!(prepare(source, true).is_err(), "{source}");
    }
    for source in ["class Item end", "trait Named end"] {
        assert!(prepare(source, false).is_err(), "{source}");
    }
}

#[test]
fn declaration_files_reject_unresolved_named_contracts_at_every_depth() {
    for source in [
        "export: Missing",
        "declare item: Missing?",
        "declare item: {Missing}",
        "declare item: {field: Missing}",
        "declare item: {[string]: Missing}",
        "declare make: function(): module.Missing",
        "declare use: function(Missing): ()",
    ] {
        assert!(declaration(source).is_err(), "{source}");
    }
}
