use crate::Diagnostic;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Word,
    Number,
    String,
    Symbol,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub kind: Kind,
    pub text: String,
    pub span: Range<usize>,
}

fn long_open(b: &[u8], start: usize) -> Option<(usize, usize)> {
    if b.get(start) != Some(&b'[') {
        return None;
    }
    let mut i = start + 1;
    while b.get(i) == Some(&b'=') {
        i += 1;
    }
    (b.get(i) == Some(&b'[')).then_some((i - start - 1, i + 1))
}

fn long_end(b: &[u8], from: usize, level: usize) -> Option<usize> {
    (from..b.len()).find_map(|i| {
        if b[i] != b']' {
            return None;
        }
        let end = i + level + 1;
        (b.get(end) == Some(&b']') && b.get(i + 1..end)?.iter().all(|v| *v == b'='))
            .then_some(end + 1)
    })
}

pub fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    let b = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    if b.starts_with(b"#!") {
        while i < b.len() && b[i] != b'\n' {
            i += 1;
        }
    }
    while i < b.len() {
        if tokens.len() >= 4096 {
            return Err(Diagnostic::new(
                "L004",
                "initial frontend limit: 4096 tokens per file",
                i..i,
            ));
        }
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if b.get(i..i + 2) == Some(b"--") {
            i += 2;
            if let Some((level, from)) = long_open(b, i) {
                i = long_end(b, from, level).ok_or_else(|| {
                    Diagnostic::new("L001", "unterminated long comment", start..start + 2)
                })?;
            } else {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            continue;
        }
        let kind = if let Some((level, from)) = long_open(b, i) {
            i = long_end(b, from, level).ok_or_else(|| {
                Diagnostic::new("L001", "unterminated long string", start..start + 1)
            })?;
            Kind::String
        } else if b[i] == b'\'' || b[i] == b'"' {
            let quote = b[i];
            i += 1;
            while i < b.len() && b[i] != quote {
                if b[i] == b'\n' || b[i] == b'\r' {
                    break;
                }
                if b[i] == b'\\' {
                    i += 1;
                    if b.get(i) == Some(&b'z') {
                        i += 1;
                        while i < b.len() && b[i].is_ascii_whitespace() {
                            i += 1;
                        }
                        continue;
                    }
                    if b.get(i) == Some(&b'x') {
                        if !b
                            .get(i + 1..i + 3)
                            .is_some_and(|s| s.iter().all(u8::is_ascii_hexdigit))
                        {
                            return Err(Diagnostic::new(
                                "L001",
                                "expected two hexadecimal escape digits",
                                start..start + 1,
                            ));
                        }
                        i += 3;
                        continue;
                    }
                    if b.get(i).is_some_and(u8::is_ascii_digit) {
                        let mut n: u16 = 0;
                        for _ in 0..3 {
                            if !b.get(i).is_some_and(u8::is_ascii_digit) {
                                break;
                            }
                            n = n * 10 + u16::from(b[i] - b'0');
                            i += 1;
                        }
                        if n > 255 {
                            return Err(Diagnostic::new(
                                "L001",
                                "decimal escape exceeds 255",
                                start..start + 1,
                            ));
                        }
                        continue;
                    }
                    if !b.get(i).is_some_and(|c| b"abfnrtv\\\"'\r\n".contains(c)) {
                        return Err(Diagnostic::new(
                            "L001",
                            "invalid or unsupported string escape",
                            start..start + 1,
                        ));
                    }
                    if matches!(b.get(i..i + 2), Some(b"\r\n" | b"\n\r")) {
                        i += 1;
                    }
                }
                i += 1;
            }
            if b.get(i) != Some(&quote) {
                return Err(Diagnostic::new(
                    "L001",
                    "unterminated string",
                    start..start + 1,
                ));
            }
            i += 1;
            Kind::String
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            Kind::Word
        } else if b[i].is_ascii_digit()
            || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            // Syntax validity is checked below, not delegated to the VM.
            let hex = b.get(i..i + 2).is_some_and(|s| s == b"0x" || s == b"0X");
            if hex {
                i += 2;
            }
            let digits = i;
            while i < b.len()
                && if hex {
                    b[i].is_ascii_hexdigit()
                } else {
                    b[i].is_ascii_digit()
                }
            {
                i += 1;
            }
            let mut count = i - digits;
            if b.get(i) == Some(&b'.') && b.get(i + 1) != Some(&b'.') {
                i += 1;
                let digits = i;
                while i < b.len()
                    && if hex {
                        b[i].is_ascii_hexdigit()
                    } else {
                        b[i].is_ascii_digit()
                    }
                {
                    i += 1;
                }
                count += i - digits;
            }
            if count == 0 {
                return Err(Diagnostic::new("L002", "invalid number", start..i));
            }
            if b.get(i).is_some_and(|c| {
                if hex {
                    *c == b'p' || *c == b'P'
                } else {
                    *c == b'e' || *c == b'E'
                }
            }) {
                i += 1;
                if b.get(i).is_some_and(|c| *c == b'+' || *c == b'-') {
                    i += 1;
                }
                let digits = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if digits == i {
                    return Err(Diagnostic::new(
                        "L002",
                        "invalid numeric exponent",
                        start..i,
                    ));
                }
            }
            if b.get(i)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
            {
                return Err(Diagnostic::new(
                    "L002",
                    "unsupported numeric suffix or malformed number",
                    start..i,
                ));
            }
            if b.get(i..i + 2) == Some(b"..") {
                return Err(Diagnostic::new(
                    "L002",
                    "separate numbers and concatenation with whitespace",
                    start..i,
                ));
            }
            Kind::Number
        } else {
            if !b[i].is_ascii() {
                let end = i + source[i..].chars().next().unwrap().len_utf8();
                return Err(Diagnostic::new(
                    "L003",
                    "identifiers currently use ASCII; Unicode is supported in strings and comments",
                    i..end,
                ));
            }
            i += 1;
            if i < b.len()
                && matches!(
                    &b[start..i + 1],
                    b".." | b"==" | b"~=" | b"<=" | b">=" | b"::" | b"//"
                )
            {
                i += 1;
                if &b[start..i] == b".." && b.get(i) == Some(&b'.') {
                    i += 1;
                }
            }
            Kind::Symbol
        };
        tokens.push(Token {
            kind,
            text: source[start..i].into(),
            span: start..i,
        });
    }
    tokens.push(Token {
        kind: Kind::Eof,
        text: String::new(),
        span: b.len()..b.len(),
    });
    Ok(tokens)
}

/// Decode already validated tokens, including Lua's newline normalization.
/// Byte keys preserve embedded NUL and non-UTF-8 decimal escapes exactly.
pub fn decode_string(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut output = Vec::new();
    if let Some((level, mut i)) = long_open(bytes, 0) {
        let end = bytes.len() - level - 2;
        if bytes.get(i).is_some_and(|b| *b == b'\n' || *b == b'\r') {
            let first = bytes[i];
            i += 1;
            if bytes
                .get(i)
                .is_some_and(|b| (*b == b'\n' || *b == b'\r') && *b != first)
            {
                i += 1;
            }
        }
        while i < end {
            let b = bytes[i];
            i += 1;
            if b == b'\n' || b == b'\r' {
                if i < end && (bytes[i] == b'\n' || bytes[i] == b'\r') && bytes[i] != b {
                    i += 1;
                }
                output.push(b'\n');
            } else {
                output.push(b);
            }
        }
        return output;
    }
    let mut i = 1;
    while i < bytes.len() - 1 {
        let b = bytes[i];
        i += 1;
        if b != b'\\' {
            output.push(b);
            continue;
        }
        let escape = bytes[i];
        i += 1;
        match escape {
            b'z' => {
                while i < bytes.len() - 1 && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            b'x' => {
                let value = u8::from_str_radix(&text[i..i + 2], 16).expect("validated escape");
                output.push(value);
                i += 2;
            }
            b'0'..=b'9' => {
                let mut value = u16::from(escape - b'0');
                for _ in 0..2 {
                    if !bytes[i].is_ascii_digit() {
                        break;
                    }
                    value = value * 10 + u16::from(bytes[i] - b'0');
                    i += 1;
                }
                output.push(value as u8);
            }
            b'\r' | b'\n' => {
                if (bytes[i] == b'\n' || bytes[i] == b'\r') && bytes[i] != escape {
                    i += 1;
                }
                output.push(b'\n');
            }
            _ => output.push(match escape {
                b'a' => 7,
                b'b' => 8,
                b'f' => 12,
                b'n' => 10,
                b'r' => 13,
                b't' => 9,
                b'v' => 11,
                c => c,
            }),
        }
    }
    output
}
