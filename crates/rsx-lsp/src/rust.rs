//! Answering Rust questions by asking `rust-analyzer`.
//!
//! This is the layer between the template server and the Rust one: it decides
//! whether a position is Rust at all, keeps the overlay in step with the
//! template, and translates what comes back.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use rsx_core::Options;

use crate::bridge::{self, Host};
use crate::document::Document;
use crate::protocol::{
    CompletionItem, CompletionItemKind, Hover, InsertTextFormat, MarkupContent, Range,
};
use crate::rust_analyzer::{uri_to_path, RustAnalyzer};

/// How the bridge is configured.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Whether to consult `rust-analyzer` at all.
    pub enabled: bool,
    /// The binary to run.
    pub binary: String,
    /// How long to wait for a single answer.
    pub timeout: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            binary: "rust-analyzer".to_string(),
            // Generous: the first request after a cold start waits for the
            // crate graph to load. Later ones return in milliseconds.
            timeout: Duration::from_secs(20),
        }
    }
}

/// Keeps one `rust-analyzer` per crate root and routes questions to it.
#[derive(Default)]
pub struct RustBridge {
    settings: Settings,
    /// Started on first use: a template that never needs Rust should never
    /// pay for an indexing run.
    analyzers: Vec<(PathBuf, Option<RustAnalyzer>)>,
    options: Options,
}

impl RustBridge {
    pub fn new(settings: Settings) -> Self {
        RustBridge {
            settings,
            analyzers: Vec::new(),
            options: Options {
                header: false,
                ..Options::default()
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// The Rust standing in for this document, if its code reaches a crate.
    pub fn host(&self, document: &Document) -> Option<Host> {
        let path = uri_to_path(&document.uri)?;
        bridge::resolve(&path, &self.options)
    }

    /// Whether the position is inside Rust rather than markup.
    pub fn is_rust_position(&self, document: &Document, offset: usize) -> bool {
        self.host(document)
            .is_some_and(|host| host.to_host_offset(offset).is_some())
    }

    /// Completions for a position inside a Rust fragment.
    pub fn completion(
        &mut self,
        document: &Document,
        offset: usize,
    ) -> Option<Vec<CompletionItem>> {
        let (host, position) = self.locate(document, offset)?;
        let analyzer = self.analyzer_for(&host.path)?;
        analyzer.wait_until_primed();
        analyzer.overlay(&host.path, &host.text).ok()?;
        let result = analyzer
            .completion(&host.path, position.0, position.1)
            .ok()?;
        Some(translate_completions(&result))
    }

    /// Hover for a position inside a Rust fragment.
    pub fn hover(&mut self, document: &Document, offset: usize) -> Option<Hover> {
        let (host, position) = self.locate(document, offset)?;
        let host_path = host.path.clone();
        let analyzer = self.analyzer_for(&host_path)?;
        analyzer.wait_until_primed();
        analyzer.overlay(&host_path, &host.text).ok()?;
        let result = analyzer.hover(&host_path, position.0, position.1).ok()?;
        translate_hover(&result, document, &host)
    }

    /// Resolves a template offset to a position in the host file.
    fn locate(&self, document: &Document, offset: usize) -> Option<(Host, (u32, u32))> {
        if !self.settings.enabled {
            return None;
        }
        let host = self.host(document)?;
        let host_offset = host.to_host_offset(offset)?;
        let position = offset_to_position(&host.text, host_offset);
        Some((host, position))
    }

    /// The analyzer for the crate containing `path`, started if need be.
    ///
    /// A failure to start is remembered, so an editor without
    /// `rust-analyzer` installed does not retry a spawn on every keystroke.
    fn analyzer_for(&mut self, path: &Path) -> Option<&mut RustAnalyzer> {
        // The crate holding the host file, not the workspace above it. A
        // crate can be excluded from the workspace it sits inside, and
        // starting the analyzer at that workspace would leave the file in no
        // project at all. Given the crate's own manifest, rust-analyzer finds
        // whatever workspace does contain it.
        let root = bridge::crate_root(path)?;

        let index = match self.analyzers.iter().position(|(r, _)| *r == root) {
            Some(index) => index,
            None => {
                let started = RustAnalyzer::spawn(&self.settings.binary, &root)
                    .ok()
                    .map(|a| a.with_timeout(self.settings.timeout));
                self.analyzers.push((root, started));
                self.analyzers.len() - 1
            }
        };
        self.analyzers[index].1.as_mut()
    }

    /// Stops every analyzer that was started.
    pub fn shutdown(&mut self) {
        for (_, analyzer) in &mut self.analyzers {
            if let Some(analyzer) = analyzer {
                analyzer.shutdown();
            }
        }
        self.analyzers.clear();
    }
}

/// Converts a byte offset into a line and UTF-16 character position.
pub fn offset_to_position(text: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = text[..line_start].bytes().filter(|b| *b == b'\n').count() as u32;
    let character = text[line_start..offset]
        .chars()
        .map(|c| c.len_utf16() as u32)
        .sum();
    (line, character)
}

/// Converts a line and UTF-16 character position into a byte offset.
pub fn position_to_offset(text: &str, line: u32, character: u32) -> usize {
    let mut start = 0usize;
    for _ in 0..line {
        match text[start..].find('\n') {
            Some(index) => start += index + 1,
            None => return text.len(),
        }
    }
    let rest = &text[start..];
    let mut utf16 = 0u32;
    for (index, ch) in rest.char_indices() {
        if utf16 >= character {
            return start + index;
        }
        utf16 += ch.len_utf16() as u32;
    }
    text.len()
}

/// Turns `rust-analyzer`'s completion list into ours.
///
/// The text edits are dropped rather than translated. They carry ranges in
/// the host file, and a range that lands outside the fragment — which is what
/// an edit spanning a whole statement does — would rewrite markup. Letting
/// the editor insert the label at the cursor is both simpler and safe.
fn translate_completions(result: &Value) -> Vec<CompletionItem> {
    let items = match result {
        Value::Array(items) => items.as_slice(),
        Value::Object(object) => match object.get("items").and_then(Value::as_array) {
            Some(items) => items.as_slice(),
            None => &[],
        },
        _ => &[],
    };

    items
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            Some(CompletionItem {
                kind: completion_kind(item),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                documentation: documentation(item),
                // Insert what is on the label; see above.
                insert_text: Some(
                    item.get("insertText")
                        .and_then(Value::as_str)
                        .unwrap_or(&label)
                        .to_string(),
                ),
                insert_text_format: InsertTextFormat::PlainText,
                sort_text: item
                    .get("sortText")
                    .and_then(Value::as_str)
                    .map(|s| format!("0{s}"))
                    .or_else(|| Some(format!("0{label}"))),
                label,
            })
        })
        .collect()
}

fn completion_kind(item: &Value) -> CompletionItemKind {
    match item.get("kind").and_then(Value::as_u64) {
        Some(3) | Some(2) => CompletionItemKind::Function,
        Some(5) => CompletionItemKind::Field,
        _ => CompletionItemKind::Property,
    }
}

fn documentation(item: &Value) -> Option<MarkupContent> {
    match item.get("documentation")? {
        Value::String(text) => Some(MarkupContent::markdown(text.clone())),
        Value::Object(object) => object
            .get("value")
            .and_then(Value::as_str)
            .map(MarkupContent::markdown),
        _ => None,
    }
}

/// Turns `rust-analyzer`'s hover into ours, with the range mapped home.
fn translate_hover(result: &Value, document: &Document, host: &Host) -> Option<Hover> {
    let contents = result.get("contents")?;
    let value = match contents {
        Value::String(text) => text.clone(),
        Value::Object(object) => object.get("value")?.as_str()?.to_string(),
        _ => return None,
    };
    if value.trim().is_empty() {
        return None;
    }

    // A range in the host file only means something here if both ends came
    // from the template; otherwise the hover is reported without one.
    let range = result.get("range").and_then(|range| {
        let start = position_of(range.get("start")?, &host.text)?;
        let end = position_of(range.get("end")?, &host.text)?;
        let from = host.to_template_offset(start)?;
        let to = host.to_template_offset(end)?;
        Some(Range::new(document.position(from), document.position(to)))
    });

    Some(Hover {
        contents: MarkupContent::markdown(value),
        range,
    })
}

fn position_of(position: &Value, text: &str) -> Option<usize> {
    let line = position.get("line")?.as_u64()? as u32;
    let character = position.get("character")?.as_u64()? as u32;
    Some(position_to_offset(text, line, character))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_offsets_and_positions_both_ways() {
        let text = "one\ntwo\nthree";
        assert_eq!(offset_to_position(text, 0), (0, 0));
        assert_eq!(offset_to_position(text, 4), (1, 0));
        assert_eq!(offset_to_position(text, 6), (1, 2));
        assert_eq!(position_to_offset(text, 1, 2), 6);
        assert_eq!(position_to_offset(text, 2, 0), 8);
    }

    #[test]
    fn counts_characters_in_utf16_units() {
        let text = "a\u{1f980}b";
        // The emoji is two UTF-16 units, so `b` is at character 3.
        assert_eq!(offset_to_position(text, text.len() - 1), (0, 3));
        assert_eq!(position_to_offset(text, 0, 3), text.len() - 1);
    }

    #[test]
    fn translates_a_completion_list() {
        let result = json!({
            "isIncomplete": false,
            "items": [
                { "label": "name", "kind": 5, "detail": "String" },
                { "label": "len", "kind": 2, "detail": "fn() -> usize",
                  "documentation": { "kind": "markdown", "value": "the length" } },
            ]
        });

        let items = translate_completions(&result);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].label, "name");
        assert_eq!(items[0].kind, CompletionItemKind::Field);
        assert_eq!(items[0].detail.as_deref(), Some("String"));
        assert_eq!(items[1].kind, CompletionItemKind::Function);
        assert!(items[1].documentation.is_some());
        // Rust completions sort ahead of everything the template offers.
        assert!(items[0].sort_text.as_deref().unwrap().starts_with('0'));
    }

    #[test]
    fn accepts_a_bare_array_of_completions() {
        let items = translate_completions(&json!([{ "label": "x", "kind": 5 }]));
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn ignores_an_empty_hover() {
        let document = Document::new("file:///a.rsx", "", 1);
        let host = Host {
            path: PathBuf::from("/a.rs"),
            text: String::new(),
            shift: 0,
            map: Default::default(),
        };
        assert!(
            translate_hover(&json!({ "contents": { "value": "  " } }), &document, &host).is_none()
        );
    }
}
