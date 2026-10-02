//! A byte-oriented cursor over `.rsx` source.
//!
//! The source is scanned as bytes rather than tokenised with a Rust lexer:
//! markup bodies are full of text that is not valid Rust (`don't`, `<div>`), so
//! there is no point at which the whole file could be tokenised. Columns still
//! count characters, so diagnostics line up on non-ASCII lines.

use crate::span::{Pos, Span};

#[derive(Clone)]
pub struct Scanner<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Scanner<'a> {
    pub fn new(src: &'a str) -> Self {
        Scanner {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn offset(&self) -> usize {
        self.pos
    }

    pub fn pos(&self) -> Pos {
        Pos {
            offset: self.pos,
            line: self.line,
            col: self.col,
        }
    }

    pub fn eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    pub fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    pub fn peek_at(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    pub fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    pub fn starts_with(&self, s: &str) -> bool {
        self.bytes[self.pos..].starts_with(s.as_bytes())
    }

    /// Advances one byte, tracking line and column.
    pub fn bump(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else if b & 0xC0 != 0x80 {
            // Continuation bytes share a column with the byte that started the
            // character, so only leading bytes advance the count.
            self.col += 1;
        }
        Some(b)
    }

    pub fn advance(&mut self, n: usize) {
        for _ in 0..n {
            if self.bump().is_none() {
                break;
            }
        }
    }

    /// Consumes `s` and returns true if it is next.
    pub fn eat(&mut self, s: &str) -> bool {
        if self.starts_with(s) {
            self.advance(s.len());
            true
        } else {
            false
        }
    }

    /// Consumes `s` only when it is a whole word, so `format` does not match
    /// the start of `formatting`.
    pub fn eat_keyword(&mut self, kw: &str) -> bool {
        if self.at_keyword(kw) {
            self.advance(kw.len());
            true
        } else {
            false
        }
    }

    pub fn at_keyword(&self, kw: &str) -> bool {
        if !self.starts_with(kw) {
            return false;
        }
        match self.peek_at(kw.len()) {
            Some(b) => !is_ident_byte(b),
            None => true,
        }
    }

    pub fn restore(&mut self, pos: Pos) {
        self.pos = pos.offset;
        self.line = pos.line;
        self.col = pos.col;
    }

    pub fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// Skips whitespace and Rust comments.
    pub fn skip_trivia(&mut self) {
        loop {
            self.skip_ws();
            if self.starts_with("//") {
                while let Some(b) = self.peek() {
                    self.bump();
                    if b == b'\n' {
                        break;
                    }
                }
            } else if self.starts_with("/*") {
                self.skip_block_comment();
            } else {
                return;
            }
        }
    }

    /// Consumes a `/* … */` comment, honouring nesting the way Rust does.
    pub fn skip_block_comment(&mut self) {
        if !self.eat("/*") {
            return;
        }
        let mut depth = 1usize;
        while depth > 0 && !self.eof() {
            if self.starts_with("/*") {
                self.advance(2);
                depth += 1;
            } else if self.starts_with("*/") {
                self.advance(2);
                depth -= 1;
            } else {
                self.bump();
            }
        }
    }

    /// Whether only whitespace separates the cursor from the start of its line.
    pub fn at_line_start(&self) -> bool {
        let mut i = self.pos;
        while i > 0 {
            let b = self.bytes[i - 1];
            if b == b'\n' {
                return true;
            }
            if b != b' ' && b != b'\t' && b != b'\r' {
                return false;
            }
            i -= 1;
        }
        true
    }

    pub fn read_ident(&mut self) -> String {
        let start = self.pos;
        if let Some(b) = self.peek() {
            if b.is_ascii_alphabetic() || b == b'_' {
                self.bump();
                while let Some(b) = self.peek() {
                    if is_ident_byte(b) {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
        }
        self.src[start..self.pos].to_string()
    }

    /// Consumes a string, char literal, or lifetime starting at the cursor,
    /// returning true if one was there.
    ///
    /// Lifetimes have to be recognised here: `'a` would otherwise look like the
    /// start of an unterminated character literal and swallow the rest of the
    /// file.
    pub fn skip_literal(&mut self) -> bool {
        match self.peek() {
            Some(b'"') => {
                self.skip_double_quoted();
                true
            }
            Some(b'r') if matches!(self.peek_at(1), Some(b'"') | Some(b'#')) => {
                self.skip_raw_string()
            }
            Some(b'b') if matches!(self.peek_at(1), Some(b'"')) => {
                self.bump();
                self.skip_double_quoted();
                true
            }
            Some(b'\'') => {
                self.skip_char_or_lifetime();
                true
            }
            _ => false,
        }
    }

    fn skip_double_quoted(&mut self) {
        self.bump(); // opening quote
        while let Some(b) = self.bump() {
            match b {
                b'\\' => {
                    self.bump();
                }
                b'"' => return,
                _ => {}
            }
        }
    }

    /// Handles `r"…"` and `r#"…"#` with any number of hashes.
    fn skip_raw_string(&mut self) -> bool {
        let save = self.pos();
        self.bump(); // r
        let mut hashes = 0usize;
        while self.peek() == Some(b'#') {
            self.bump();
            hashes += 1;
        }
        if self.peek() != Some(b'"') {
            self.restore(save);
            return false;
        }
        self.bump();
        let terminator = format!("\"{}", "#".repeat(hashes));
        while !self.eof() {
            if self.starts_with(&terminator) {
                self.advance(terminator.len());
                return true;
            }
            self.bump();
        }
        true
    }

    fn skip_char_or_lifetime(&mut self) {
        let save = self.pos();
        self.bump(); // opening quote

        // A lifetime is a quote, an identifier, and no closing quote.
        if matches!(self.peek(), Some(b) if b.is_ascii_alphabetic() || b == b'_') {
            let mut probe = self.clone();
            probe.read_ident();
            if probe.peek() != Some(b'\'') {
                self.restore(probe.pos());
                return;
            }
        }

        self.restore(save);
        self.bump(); // opening quote
        while let Some(b) = self.bump() {
            match b {
                b'\\' => {
                    self.bump();
                }
                b'\'' => return,
                _ => {}
            }
        }
    }

    pub fn span_from(&self, from: Pos) -> Span {
        Span::new(from, self.pos())
    }

    pub fn slice(&self, from: usize, to: usize) -> &'a str {
        &self.src[from..to.min(self.src.len())]
    }
}

pub fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_line_and_column() {
        let mut s = Scanner::new("ab\ncd");
        assert_eq!((s.pos().line, s.pos().col), (1, 1));
        s.advance(3);
        assert_eq!((s.pos().line, s.pos().col), (2, 1));
        s.advance(1);
        assert_eq!((s.pos().line, s.pos().col), (2, 2));
    }

    #[test]
    fn counts_characters_not_bytes_in_columns() {
        let mut s = Scanner::new("caf\u{e9}x");
        s.advance(5); // c a f é(2 bytes)
        assert_eq!(s.pos().col, 5);
    }

    #[test]
    fn keyword_match_requires_a_word_boundary() {
        assert!(Scanner::new("if x").at_keyword("if"));
        assert!(!Scanner::new("iffy").at_keyword("if"));
        assert!(Scanner::new("if(x)").at_keyword("if"));
    }

    #[test]
    fn distinguishes_lifetimes_from_char_literals() {
        let mut s = Scanner::new("'a, u8");
        assert!(s.skip_literal());
        assert_eq!(s.rest(), ", u8");

        let mut s = Scanner::new("'x' rest");
        assert!(s.skip_literal());
        assert_eq!(s.rest(), " rest");

        let mut s = Scanner::new(r"'\'' rest");
        assert!(s.skip_literal());
        assert_eq!(s.rest(), " rest");
    }

    #[test]
    fn skips_raw_strings_with_hashes() {
        let mut s = Scanner::new(r##"r#"a "quoted" b"# rest"##);
        assert!(s.skip_literal());
        assert_eq!(s.rest(), " rest");
    }

    #[test]
    fn skips_nested_block_comments() {
        let mut s = Scanner::new("/* a /* b */ c */ rest");
        s.skip_trivia();
        assert_eq!(s.rest(), "rest");
    }

    #[test]
    fn at_line_start_ignores_indentation() {
        let s = Scanner::new("  x");
        assert!(s.at_line_start());
        let mut s = Scanner::new("a\n   b");
        s.advance(5);
        assert!(s.at_line_start());
        s.advance(1);
        assert!(!s.at_line_start());
    }
}
