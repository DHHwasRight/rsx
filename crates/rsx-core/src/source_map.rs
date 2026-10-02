//! Maps lines of generated Rust back to the `.rsx` source that produced them.
//!
//! Rust has no `#line` directive, so a compiler error always names the
//! generated code — the file under `rsx generate`, or the macro call site when
//! the proc macro is driving. A consumer that keeps the map alongside the
//! generated code can translate those positions into something the author can
//! actually edit.

use core::ops::Range;

use crate::span::{Pos, Span};

/// One generated line and the source position it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapping {
    /// Line of generated Rust, 1-based.
    pub generated_line: u32,
    /// Line in the `.rsx` file, 1-based.
    pub line: u32,
    /// Column in the `.rsx` file, 1-based.
    pub col: u32,
}

/// One run of Rust copied verbatim from the template into the output.
///
/// Because the text is copied unchanged, the mapping inside it is a shift:
/// the nth byte of the fragment in the template is the nth byte of the
/// fragment in the generated code. That is what makes it possible to ask
/// `rust-analyzer` about a cursor in the middle of `{ user.posts }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// Byte range in the generated Rust.
    pub generated: Range<usize>,
    /// Byte range in the `.rsx` source.
    pub source: Range<usize>,
}

impl Fragment {
    pub fn len(&self) -> usize {
        self.generated.len()
    }

    pub fn is_empty(&self) -> bool {
        self.generated.is_empty()
    }
}

/// Generated lines in increasing order, each with its source position, plus
/// the byte-exact ranges of the Rust fragments copied into the output.
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    mappings: Vec<Mapping>,
    fragments: Vec<Fragment>,
}

impl SourceMap {
    pub fn new() -> Self {
        SourceMap::default()
    }

    /// Records that `generated_line` was emitted while generating the node at
    /// `pos`.
    ///
    /// Repeated calls for one generated line keep the first, which is the
    /// outermost node still being generated — the one an author would rather
    /// be pointed at.
    pub fn record(&mut self, generated_line: u32, pos: Pos) {
        if self.mappings.last().map(|m| m.generated_line) == Some(generated_line) {
            return;
        }
        self.mappings.push(Mapping {
            generated_line,
            line: pos.line,
            col: pos.col,
        });
    }

    /// The source position for a line of generated code.
    ///
    /// Lines the generator emitted outside any node — the banner, a blank line
    /// between items — resolve to the nearest mapped line before them, or to
    /// `None` when there is none.
    pub fn lookup(&self, generated_line: u32) -> Option<Mapping> {
        let index = self
            .mappings
            .partition_point(|m| m.generated_line <= generated_line);
        index.checked_sub(1).map(|i| self.mappings[i])
    }

    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    /// Records a fragment of `len` bytes copied from `source` to `generated`.
    pub fn record_fragment(&mut self, generated: usize, source: Span, len: usize) {
        if len == 0 {
            return;
        }
        self.fragments.push(Fragment {
            generated: generated..generated + len,
            source: source.from.offset..source.from.offset + len,
        });
    }

    pub fn fragments(&self) -> &[Fragment] {
        &self.fragments
    }

    /// The offset in the generated Rust for a position in the template.
    ///
    /// Only positions inside a copied Rust fragment have one; markup has no
    /// counterpart in the output beyond the string literals it became.
    pub fn to_generated(&self, source_offset: usize) -> Option<usize> {
        // An offset at the very end of a fragment still belongs to it: that
        // is where a cursor sits after typing a `.`.
        let fragment = self
            .fragments
            .iter()
            .find(|f| source_offset >= f.source.start && source_offset <= f.source.end)?;
        Some(fragment.generated.start + (source_offset - fragment.source.start))
    }

    /// The offset in the template for a position in the generated Rust.
    pub fn to_source(&self, generated_offset: usize) -> Option<usize> {
        let fragment = self.fragments.iter().find(|f| {
            generated_offset >= f.generated.start && generated_offset <= f.generated.end
        })?;
        Some(fragment.source.start + (generated_offset - fragment.generated.start))
    }

    /// Whether `source_offset` falls inside a Rust fragment.
    pub fn is_rust(&self, source_offset: usize) -> bool {
        self.to_generated(source_offset).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }

    pub fn len(&self) -> usize {
        self.mappings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, col: u32) -> Pos {
        Pos {
            offset: 0,
            line,
            col,
        }
    }

    #[test]
    fn keeps_the_first_mapping_for_a_line() {
        let mut map = SourceMap::new();
        map.record(3, pos(10, 1));
        map.record(3, pos(11, 5));
        assert_eq!(map.len(), 1);
        assert_eq!(map.lookup(3).unwrap().line, 10);
    }

    #[test]
    fn falls_back_to_the_nearest_preceding_mapping() {
        let mut map = SourceMap::new();
        map.record(2, pos(4, 1));
        map.record(6, pos(9, 1));
        assert_eq!(map.lookup(2).unwrap().line, 4);
        assert_eq!(map.lookup(5).unwrap().line, 4);
        assert_eq!(map.lookup(6).unwrap().line, 9);
        assert_eq!(map.lookup(600).unwrap().line, 9);
    }

    #[test]
    fn has_no_mapping_before_the_first_entry() {
        let mut map = SourceMap::new();
        map.record(4, pos(1, 1));
        assert!(map.lookup(1).is_none());
        assert!(SourceMap::new().lookup(1).is_none());
    }
}
