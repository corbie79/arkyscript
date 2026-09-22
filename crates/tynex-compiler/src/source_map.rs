//! Source locations use zero-based lines and UTF-16 columns, as in Source Map v3.
//! A copied range tracks line changes; a lowered range can map several generated
//! lines to one original position. Helpers remain explicitly unmapped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}
impl Position {
    pub fn advance(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                self.line += 1;
                self.column = 0;
            } else {
                self.column += ch.len_utf16();
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub generated: Position,
    pub original: Option<Position>,
}
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    pub mappings: Vec<Mapping>,
    pub end: Position,
}
impl SourceMap {
    /// Same-line greatest lower bound; never borrows a mapping from a helper line.
    pub fn original_position(&self, generated: Position) -> Option<Position> {
        if generated >= self.end {
            return None;
        }
        self.mappings
            .iter()
            .rev()
            .find(|m| m.generated <= generated)
            .filter(|m| m.generated.line == generated.line)
            .and_then(|m| m.original)
    }
    /// All generated occurrences, including expansions of a single source line.
    pub fn generated_positions(&self, original_line: usize) -> Vec<Position> {
        self.mappings
            .iter()
            .filter(|m| m.original.is_some_and(|p| p.line == original_line))
            .map(|m| m.generated)
            .collect()
    }
    /// Standard v3 Base64 VLQ segments, with one-field segments for helpers.
    pub fn v3_mappings(&self) -> String {
        let mut out = String::new();
        let (mut line, mut column, mut source_line, mut source_column) = (0, 0, 0, 0);
        let mut first = true;
        for mapping in &self.mappings {
            while line < mapping.generated.line {
                out.push(';');
                line += 1;
                column = 0;
                first = true;
            }
            if !first {
                out.push(',');
            }
            first = false;
            vlq(mapping.generated.column as i64 - column, &mut out);
            column = mapping.generated.column as i64;
            if let Some(original) = mapping.original {
                vlq(0, &mut out); // a compilation unit has one source
                vlq(original.line as i64 - source_line, &mut out);
                vlq(original.column as i64 - source_column, &mut out);
                source_line = original.line as i64;
                source_column = original.column as i64;
            }
        }
        out
    }
}
fn vlq(value: i64, out: &mut String) {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = (value.unsigned_abs() << 1) | u64::from(value < 0);
    loop {
        let mut digit = (bits & 31) as usize;
        bits >>= 5;
        if bits != 0 {
            digit |= 32;
        }
        out.push(ALPHABET[digit] as char);
        if bits == 0 {
            break;
        }
    }
}
#[derive(Default)]
pub struct Emitter {
    pub code: String,
    pub map: SourceMap,
}
impl Emitter {
    fn mark(&mut self, original: Option<Position>) {
        let generated = self.map.end;
        if self
            .map
            .mappings
            .last()
            .is_some_and(|m| m.generated == generated)
        {
            self.map.mappings.pop();
        }
        self.map.mappings.push(Mapping {
            generated,
            original,
        });
    }
    fn push(&mut self, text: &str, mut original: Option<Position>, copied: bool) {
        if text.is_empty() {
            return;
        }
        for part in text.split_inclusive('\n') {
            self.mark(original);
            self.code.push_str(part);
            self.map.end.advance(part);
            if copied && let Some(ref mut position) = original {
                position.advance(part);
            }
        }
    }
    pub fn copied(&mut self, text: &str, original: Position) {
        self.push(text, Some(original), true);
    }
    pub fn lowered(&mut self, text: &str, original: Position) {
        self.push(text, Some(original), false);
    }
    pub fn helper(&mut self, text: &str) {
        self.push(text, None, false);
    }
    pub fn append(&mut self, code: &str, map: &SourceMap) {
        let offset = self.map.end;
        for mapping in &map.mappings {
            let mut mapping = mapping.clone();
            if mapping.generated.line == 0 {
                mapping.generated.column += offset.column;
            }
            mapping.generated.line += offset.line;
            if self
                .map
                .mappings
                .last()
                .is_some_and(|m| m.generated == mapping.generated)
            {
                self.map.mappings.pop();
            }
            self.map.mappings.push(mapping);
        }
        self.code.push_str(code);
        self.map.end.advance(code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(line: usize, column: usize) -> Position {
        Position { line, column }
    }
    #[test]
    fn expansions_helpers_and_bidirectional_lookup() {
        let mut e = Emitter::default();
        e.helper("-- helper\nlocal internal = 1\n");
        e.lowered("local a = value\nuse(a)\n", p(4, 2));
        e.copied("print(a)\nreturn a", p(5, 0));
        assert_eq!(e.map.original_position(p(1, 4)), None);
        assert_eq!(e.map.original_position(p(2, 4)), Some(p(4, 2)));
        assert_eq!(e.map.original_position(p(3, 0)), Some(p(4, 2)));
        assert_eq!(e.map.generated_positions(4), vec![p(2, 0), p(3, 0)]);
        assert_eq!(e.map.original_position(p(5, 0)), Some(p(6, 0)));
        assert_eq!(e.map.original_position(p(5, 8)), None);
    }
    #[test]
    fn utf16_columns_crlf_and_same_line_transitions() {
        let mut e = Emitter::default();
        e.copied("--😀\r\n", p(0, 0));
        e.helper("prefix ");
        e.copied("한글", p(1, 3));
        assert_eq!(e.map.end, p(1, 9));
        assert_eq!(e.map.original_position(p(1, 6)), None);
        assert_eq!(e.map.original_position(p(1, 7)), Some(p(1, 3)));
        assert_eq!(e.map.v3_mappings(), "AAAA;A,OACG");
    }
    #[test]
    fn append_shifts_generated_positions_only() {
        let mut source = Emitter::default();
        source.copied("x\ny", p(0, 0));
        let mut bundle = Emitter::default();
        bundle.helper("-- generated\n");
        bundle.append(&source.code, &source.map);
        assert_eq!(bundle.map.original_position(p(1, 0)), Some(p(0, 0)));
        assert_eq!(bundle.map.original_position(p(2, 0)), Some(p(1, 0)));
        assert_eq!(bundle.map.v3_mappings(), "A;AAAA;AACA");
    }
}
