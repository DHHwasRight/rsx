use core::fmt;
use std::path::{Path, PathBuf};

use crate::span::Span;

/// A compile error with the source location that produced it.
#[derive(Clone)]
pub struct Error {
    pub message: String,
    pub span: Span,
    pub path: Option<PathBuf>,
    /// An optional second line suggesting what to do about it.
    pub help: Option<String>,
}

pub type Result<T> = core::result::Result<T, Error>;

impl Error {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Error {
            message: message.into(),
            span,
            path: None,
            help: None,
        }
    }

    /// Attaches a suggestion shown below the message.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Attaches the file the error came from.
    pub fn in_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Formats the error with the offending line and a caret, the way rustc
    /// does. `source` must be the text the span was produced from.
    pub fn render(&self, source: &str) -> String {
        let mut out = String::new();
        let location = match &self.path {
            Some(p) => format!("{}:{}", display_path(p), self.span.from),
            None => self.span.from.to_string(),
        };
        out.push_str(&format!("error: {}\n  --> {}\n", self.message, location));

        if let Some(line) = source
            .lines()
            .nth(self.span.from.line.saturating_sub(1) as usize)
        {
            let gutter = self.span.from.line.to_string();
            let pad = " ".repeat(gutter.len());
            out.push_str(&format!("{pad} |\n"));
            out.push_str(&format!("{gutter} | {}\n", line.replace('\t', "    ")));

            // Count the caret offset in characters, widening tabs the same way
            // the printed line does, so the caret lands under the right column.
            let col = self.span.from.col.saturating_sub(1) as usize;
            let prefix: String = line.chars().take(col).collect();
            let caret_col = prefix
                .chars()
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>();
            let width = if self.span.to.line == self.span.from.line {
                (self.span.to.col.saturating_sub(self.span.from.col) as usize).max(1)
            } else {
                1
            };
            out.push_str(&format!(
                "{pad} | {}{}\n",
                " ".repeat(caret_col),
                "^".repeat(width)
            ));
        }

        if let Some(help) = &self.help {
            out.push_str(&format!("  = help: {help}\n"));
        }
        out
    }
}

fn display_path(p: &Path) -> String {
    p.display().to_string()
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(p) => write!(
                f,
                "{}:{}: {}",
                display_path(p),
                self.span.from,
                self.message
            ),
            None => write!(f, "{}: {}", self.span.from, self.message),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for Error {}
