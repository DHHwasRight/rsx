use core::fmt;

/// A byte offset in a source file, with the line and column it falls on.
///
/// Lines and columns are 1-based so they can be printed straight into a
/// diagnostic. `offset` is a byte index, not a character index.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Pos {
    pub offset: usize,
    pub line: u32,
    pub col: u32,
}

impl Pos {
    pub const fn start() -> Self {
        Pos {
            offset: 0,
            line: 1,
            col: 1,
        }
    }
}

impl fmt::Debug for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// A half-open range of source text.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub from: Pos,
    pub to: Pos,
}

impl Span {
    pub const fn new(from: Pos, to: Pos) -> Self {
        Span { from, to }
    }

    /// A zero-width span at `pos`.
    pub const fn at(pos: Pos) -> Self {
        Span { from: pos, to: pos }
    }

    /// The span covering both `self` and `other`.
    pub fn join(self, other: Span) -> Span {
        Span {
            from: if self.from.offset <= other.from.offset {
                self.from
            } else {
                other.from
            },
            to: if self.to.offset >= other.to.offset {
                self.to
            } else {
                other.to
            },
        }
    }

    /// The source text this span covers.
    pub fn slice<'s>(&self, source: &'s str) -> &'s str {
        let end = self.to.offset.min(source.len());
        let start = self.from.offset.min(end);
        &source[start..end]
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}..{:?}", self.from, self.to)
    }
}
