//! Open documents, their syntax trees, and their diagnostics.

use std::collections::HashMap;

use rsx_core::ast::{Component, File};
use rsx_core::project::Overlay;
use rsx_core::span::Span;
use rsx_core::{generate, parse, Error, Options, Registry};

use crate::protocol::{Diagnostic, DiagnosticSeverity, Position, Range};

/// One open `.rsx` file.
pub struct Document {
    pub uri: String,
    pub version: i32,
    pub text: String,
    /// `None` when the text does not parse; the diagnostics say why.
    ///
    /// Features that must agree with what is on screen — hover, go to
    /// definition, the outline — use this and answer nothing while the file is
    /// broken, rather than pointing at stale positions.
    pub ast: Option<File>,
    /// The last tree that did parse.
    ///
    /// Completion is asked for mid-keystroke, which is exactly when the file
    /// is broken. It works from the text, so an out-of-date tree is still a
    /// good source for "what components does this file declare".
    last_good_ast: Option<File>,
    pub diagnostics: Vec<Diagnostic>,
    /// Byte offset where each line starts, for position conversion.
    line_starts: Vec<usize>,
}

impl Document {
    pub fn new(uri: impl Into<String>, text: impl Into<String>, version: i32) -> Self {
        let text = text.into();
        let line_starts = line_starts(&text);
        let mut doc = Document {
            uri: uri.into(),
            version,
            text,
            ast: None,
            last_good_ast: None,
            diagnostics: Vec::new(),
            line_starts,
        };
        doc.reparse();
        doc
    }

    fn reparse(&mut self) {
        match parse(&self.text) {
            Ok(file) => {
                self.last_good_ast = Some(file.clone());
                self.ast = Some(file);
                self.diagnostics.clear();
            }
            Err(error) => {
                self.ast = None;
                self.diagnostics = vec![self.diagnostic(&error)];
            }
        }
    }

    /// Adds the errors that only code generation can find — an unknown prop,
    /// children passed to a component that takes none, a lowercase component
    /// name — using `registry` to resolve components declared elsewhere.
    ///
    /// Only the document that changed is re-checked. An edit in one file can
    /// invalidate a call in another, and that second file catches up when it
    /// is next touched.
    pub(crate) fn validate(&mut self, registry: &Registry) {
        let Some(file) = &self.ast else {
            return;
        };
        if let Err(error) = generate(file, registry, &Options::default()) {
            self.diagnostics.push(self.diagnostic(&error));
        }
    }

    fn diagnostic(&self, error: &Error) -> Diagnostic {
        let mut message = error.message.clone();
        if let Some(help) = &error.help {
            message.push_str("\n\nhelp: ");
            message.push_str(help);
        }
        Diagnostic {
            range: self.span_range(error.span),
            severity: DiagnosticSeverity::Error,
            message,
            source: "rsx".to_string(),
        }
    }

    /// Replaces the text and re-parses.
    pub fn update(&mut self, text: impl Into<String>, version: i32) {
        self.text = text.into();
        self.line_starts = line_starts(&self.text);
        self.version = version;
        self.reparse();
    }

    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.ast.iter().flat_map(|file| file.components())
    }

    /// The components this file declares, from the last tree that parsed.
    pub fn known_components(&self) -> impl Iterator<Item = &Component> {
        self.last_good_ast.iter().flat_map(|file| file.components())
    }

    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components().find(|c| c.name == name)
    }

    /// Converts a protocol position to a byte offset.
    ///
    /// `character` counts UTF-16 code units, so a line containing an emoji
    /// does not land the cursor in the wrong place.
    pub fn offset(&self, position: Position) -> usize {
        let line_start = match self.line_starts.get(position.line as usize) {
            Some(start) => *start,
            None => return self.text.len(),
        };
        let line_end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        let line = &self.text[line_start..line_end];

        let mut utf16 = 0u32;
        for (offset, ch) in line.char_indices() {
            if utf16 >= position.character {
                return line_start + offset;
            }
            utf16 += ch.len_utf16() as u32;
        }
        line_end
    }

    /// Converts a byte offset to a protocol position.
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        let character = self.text[line_start..offset]
            .chars()
            .map(|c| c.len_utf16() as u32)
            .sum();
        Position {
            line: line as u32,
            character,
        }
    }

    pub fn span_range(&self, span: Span) -> Range {
        Range::new(
            self.position(span.from.offset),
            self.position(span.to.offset),
        )
    }

    /// The text of a line, without its terminator.
    pub fn line(&self, line: u32) -> &str {
        let start = match self.line_starts.get(line as usize) {
            Some(start) => *start,
            None => return "",
        };
        let end = self
            .line_starts
            .get(line as usize + 1)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (offset, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(offset + 1);
        }
    }
    starts
}

/// Every document the editor has open.
#[derive(Default)]
pub struct DocumentStore {
    documents: HashMap<String, Document>,
}

impl DocumentStore {
    pub fn new() -> Self {
        DocumentStore::default()
    }

    pub fn open(&mut self, uri: &str, text: &str, version: i32) -> &Document {
        let document = Document::new(uri, text, version);
        self.documents.insert(uri.to_string(), document);
        self.validate(uri);
        &self.documents[uri]
    }

    /// Re-parses an open document. Returns `None` for a URI never opened.
    pub fn update(&mut self, uri: &str, text: &str, version: i32) -> Option<&Document> {
        self.documents.get_mut(uri)?.update(text, version);
        self.validate(uri);
        self.documents.get(uri)
    }

    /// Runs the generator over one document to surface the errors parsing
    /// alone cannot find.
    fn validate(&mut self, uri: &str) {
        let registry = self.registry_for(uri);
        if let Some(document) = self.documents.get_mut(uri) {
            document.validate(&registry);
        }
    }

    /// Every component the given document could be calling: its own, and the
    /// ones declared in the other open documents.
    fn registry_for(&self, uri: &str) -> Registry {
        let mut registry = Registry::new();
        if let Some(document) = self.documents.get(uri) {
            if let Some(file) = &document.ast {
                registry.add_file(file);
            }
        }
        for (other, document) in &self.documents {
            if other == uri {
                continue;
            }
            if let Some(file) = &document.ast {
                registry.add_file(file);
            }
        }
        registry
    }

    pub fn close(&mut self, uri: &str) -> bool {
        self.documents.remove(uri).is_some()
    }

    pub fn get(&self, uri: &str) -> Option<&Document> {
        self.documents.get(uri)
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub fn documents(&self) -> impl Iterator<Item = &Document> {
        self.documents.values()
    }

    /// Every component across every open document, with the document it is in.
    ///
    /// Sorted by name so completion lists and tests are deterministic, which a
    /// `HashMap` iteration order is not.
    pub fn all_components(&self) -> Vec<(&Document, &Component)> {
        let mut all: Vec<(&Document, &Component)> = self
            .documents
            .values()
            .flat_map(|doc| doc.components().map(move |c| (doc, c)))
            .collect();
        all.sort_by(|(a_doc, a), (b_doc, b)| {
            a.name.cmp(&b.name).then_with(|| a_doc.uri.cmp(&b_doc.uri))
        });
        all
    }

    /// The open buffers, as contents standing in for what is on disk.
    ///
    /// A question about a template is a question about what the author is
    /// looking at, which is rarely the saved copy.
    pub fn overlay(&self) -> Overlay {
        let mut overlay = Overlay::new();
        for document in self.documents.values() {
            if let Some(path) = crate::rust_analyzer::uri_to_path(&document.uri) {
                overlay.insert(path, document.text.clone());
            }
        }
        overlay
    }

    /// Finds where a component is declared, preferring the document asking.
    pub fn find_component(&self, name: &str, from: &str) -> Option<(&Document, &Component)> {
        if let Some(doc) = self.get(from) {
            if let Some(component) = doc.component(name) {
                return Some((doc, component));
            }
        }
        self.all_components()
            .into_iter()
            .find(|(_, component)| component.name == name)
    }
}
