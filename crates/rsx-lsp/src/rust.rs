//! Answering Rust questions by asking `rust-analyzer`.
//!
//! This is the layer between the template server and the Rust one: it decides
//! whether a position is Rust at all, keeps the overlay in step with the
//! template, and translates what comes back.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

pub use rsx_core::project::Overlay;
use rsx_core::Options;

use crate::bridge::{self, Host};
use crate::document::Document;
use crate::protocol::{
    CompletionItem, CompletionItemKind, Hover, InsertTextFormat, Location, MarkupContent, Position,
    Range, TextEdit,
};
use crate::rust_analyzer::{path_to_uri, uri_to_path, RustAnalyzer};

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
    /// The last host built, reused while the text it came from is unchanged.
    ///
    /// Building one compiles a directory of templates. A single keystroke
    /// asks more than once — completion and hover both need a host, and an
    /// editor re-requests freely — so without this the cost is paid several
    /// times over for one answer.
    cache: Option<CachedHost>,
}

struct CachedHost {
    template: PathBuf,
    fingerprint: u64,
    host: Host,
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
            cache: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// The Rust standing in for this document, if its code reaches a crate.
    ///
    /// `overlay` carries the editor's unsaved buffers; without them the host
    /// is built from the last saved text and every offset after the first
    /// unsaved edit points at the wrong place.
    pub fn host(&mut self, document: &Document, overlay: &Overlay) -> Option<Host> {
        let path = uri_to_path(&document.uri)?;
        let fingerprint = overlay.fingerprint();

        if let Some(cached) = &self.cache {
            if cached.template == path && cached.fingerprint == fingerprint {
                return Some(cached.host.clone());
            }
        }

        let host = bridge::resolve_with(&path, &self.options, overlay)?;
        self.cache = Some(CachedHost {
            template: path,
            fingerprint,
            host: host.clone(),
        });
        Some(host)
    }

    /// Whether the position is inside Rust rather than markup.
    pub fn is_rust_position(
        &mut self,
        document: &Document,
        offset: usize,
        overlay: &Overlay,
    ) -> bool {
        self.host(document, overlay)
            .is_some_and(|host| host.to_host_offset(offset).is_some())
    }

    /// Completions for a position inside a Rust fragment.
    pub fn completion(
        &mut self,
        document: &Document,
        offset: usize,
        overlay: &Overlay,
    ) -> Option<Vec<CompletionItem>> {
        let (host, position) = self.locate(document, offset, overlay)?;
        let analyzer = self.analyzer_for(&host.path)?;
        analyzer.wait_until_primed();
        analyzer.overlay(&host.path, &host.text).ok()?;
        let result = ask_until_settled(
            analyzer,
            |a| a.completion(&host.path, position.0, position.1).ok(),
            |value| translate_completions(value, document, &host).is_empty(),
        )?;
        Some(translate_completions(&result, document, &host))
    }

    /// Where the thing at a position inside a Rust fragment is defined.
    pub fn definition(
        &mut self,
        document: &Document,
        offset: usize,
        overlay: &Overlay,
    ) -> Option<Location> {
        let (host, position) = self.locate(document, offset, overlay)?;
        let host_path = host.path.clone();
        let binary = self.settings.binary.clone();
        let Some(analyzer) = self.analyzer_for(&host_path) else {
            self.decline("definition", &format!("could not start `{binary}`"));
            return None;
        };
        if !analyzer.wait_until_primed() {
            eprintln!("rsx: rust-analyzer has not finished indexing; answering anyway");
        }
        analyzer.overlay(&host_path, &host.text).ok()?;
        let result = ask_until_settled(
            analyzer,
            |a| a.definition(&host_path, position.0, position.1).ok(),
            |value| translate_definition(value, document, &host).is_none(),
        )?;
        let found = translate_definition(&result, document, &host);
        if found.is_none() {
            self.decline(
                "definition",
                &format!(
                    "rust-analyzer answered {result} for {}",
                    host_path.display()
                ),
            );
        }
        found
    }

    /// Hover for a position inside a Rust fragment.
    pub fn hover(
        &mut self,
        document: &Document,
        offset: usize,
        overlay: &Overlay,
    ) -> Option<Hover> {
        let (host, position) = self.locate(document, offset, overlay)?;
        let host_path = host.path.clone();
        let analyzer = self.analyzer_for(&host_path)?;
        analyzer.wait_until_primed();
        analyzer.overlay(&host_path, &host.text).ok()?;
        let result = ask_until_settled(
            analyzer,
            |a| a.hover(&host_path, position.0, position.1).ok(),
            |value| translate_hover(value, document, &host).is_none(),
        )?;
        translate_hover(&result, document, &host)
    }

    /// Why the last Rust question could not be answered, for the log.
    ///
    /// Everything here fails quietly by design, which is right for an editor
    /// and unhelpful when something is wrong. One line on stderr reaches the
    /// client's output channel, where it can be read.
    fn decline(&self, what: &str, reason: &str) {
        eprintln!("rsx: {what} not forwarded to rust-analyzer: {reason}");
    }

    /// Resolves a template offset to a position in the host file.
    fn locate(
        &mut self,
        document: &Document,
        offset: usize,
        overlay: &Overlay,
    ) -> Option<(Host, (u32, u32))> {
        if !self.settings.enabled {
            return None;
        }
        let Some(host) = self.host(document, overlay) else {
            self.decline(
                "request",
                "no Rust file compiles this template; is it covered by a \
                 `load!`/`load_dir!` in this crate, or does it have a \
                 generated `.rs` beside it?",
            );
            return None;
        };
        let Some(host_offset) = host.to_host_offset(offset) else {
            // Ordinary for markup, so this is only worth a word when the
            // author is somewhere that looks like Rust.
            return None;
        };
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
        self.cache = None;
        for (_, analyzer) in &mut self.analyzers {
            if let Some(analyzer) = analyzer {
                analyzer.shutdown();
            }
        }
        self.analyzers.clear();
    }
}

/// Asks `rust-analyzer` repeatedly while it is still working.
///
/// An empty answer means either "nothing to suggest" or "ask me later", and
/// the two look identical. So an empty one is only believed once the server
/// has gone quiet: if anything is still arriving, the question is put again.
/// When it is genuinely idle this costs one short wait and nothing else.
fn ask_until_settled<T>(
    analyzer: &mut RustAnalyzer,
    mut ask: impl FnMut(&mut RustAnalyzer) -> Option<T>,
    is_empty: impl Fn(&T) -> bool,
) -> Option<T> {
    /// Long enough to catch a server mid-index, short enough not to be felt
    /// when there is genuinely nothing to say.
    const QUIET: Duration = Duration::from_millis(250);
    /// A bound, so a server that never stops talking still answers.
    const ATTEMPTS: usize = 4;

    let mut answer = ask(analyzer)?;
    for _ in 0..ATTEMPTS {
        if !is_empty(&answer) {
            break;
        }
        if !analyzer.progressed(QUIET) {
            break;
        }
        answer = ask(analyzer)?;
    }
    Some(answer)
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
/// Two things need care, and getting either wrong is worse than offering no
/// completion at all.
///
/// The text to insert comes from the `textEdit`, not the label. For a field
/// whose expected type is a reference, `rust-analyzer` labels the item `&name`
/// while the text to insert is just `name` — the `&` is a separate edit. Using
/// the label would write `user.&name`.
///
/// And an item is only offered if every edit it carries lands inside the
/// template. The `&` above is placed inside the generated `__w.attr(&(`
/// wrapper, which the author cannot see and did not write; an auto-import is
/// placed in generated code too. Such an item cannot be applied faithfully, so
/// it is dropped rather than applied in part.
fn translate_completions(result: &Value, document: &Document, host: &Host) -> Vec<CompletionItem> {
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

            // An edit outside the template cannot be carried across.
            if item
                .get("additionalTextEdits")
                .and_then(Value::as_array)
                .is_some_and(|edits| !edits.is_empty())
            {
                return None;
            }

            let edit = item.get("textEdit");
            let new_text = edit
                .and_then(|e| e.get("newText"))
                .and_then(Value::as_str)
                .or_else(|| item.get("insertText").and_then(Value::as_str))
                .unwrap_or(&label)
                .to_string();

            // Prefer replacing the range the server named; fall back to
            // inserting at the cursor when it does not map home.
            let text_edit = edit
                .and_then(|e| translate_range(e.get("range")?, document, host))
                .map(|range| TextEdit {
                    range,
                    new_text: new_text.clone(),
                });

            Some(CompletionItem {
                kind: completion_kind(item),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                documentation: documentation(item),
                insert_text: Some(new_text),
                insert_text_format: InsertTextFormat::PlainText,
                sort_text: item
                    .get("sortText")
                    .and_then(Value::as_str)
                    .map(|s| format!("0{s}"))
                    .or_else(|| Some(format!("0{label}"))),
                text_edit,
                label,
            })
        })
        .collect()
}

/// Turns `rust-analyzer`'s definition answer into one location.
///
/// A target in another file — a type from a dependency, or from handwritten
/// Rust — is passed through as it stands. A target inside the host file only
/// means something to the author if it came from the template, so it is
/// translated home; a target in generated code has nowhere to point and is
/// dropped rather than sending the editor into a file nobody wrote.
fn translate_definition(result: &Value, document: &Document, host: &Host) -> Option<Location> {
    let first = match result {
        Value::Array(targets) => targets.first()?,
        Value::Object(_) => result,
        _ => return None,
    };

    // A LocationLink spells its fields differently from a Location.
    let (uri, range) = match first.get("targetUri") {
        Some(uri) => (
            uri.as_str()?,
            first
                .get("targetSelectionRange")
                .or_else(|| first.get("targetRange"))?,
        ),
        None => (first.get("uri")?.as_str()?, first.get("range")?),
    };

    let target = uri_to_path(uri)?;
    if target != host.path {
        return Some(Location {
            uri: path_to_uri(&target),
            range: protocol_range(range)?,
        });
    }

    Some(Location {
        uri: document.uri.clone(),
        range: translate_range(range, document, host)?,
    })
}

/// A range as the protocol gives it, with no translation.
fn protocol_range(range: &Value) -> Option<Range> {
    let point = |key: &str| -> Option<Position> {
        let value = range.get(key)?;
        Some(Position::new(
            value.get("line")?.as_u64()? as u32,
            value.get("character")?.as_u64()? as u32,
        ))
    };
    Some(Range::new(point("start")?, point("end")?))
}

/// Maps a range in the host file back into the template.
fn translate_range(range: &Value, document: &Document, host: &Host) -> Option<Range> {
    let start = position_of(range.get("start")?, &host.text)?;
    let end = position_of(range.get("end")?, &host.text)?;
    Some(Range::new(
        document.position(host.to_template_offset(start)?),
        document.position(host.to_template_offset(end)?),
    ))
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

    /// A document and host whose fragment is the whole text, so ranges map
    /// one to one and the assertions are about translation, not mapping.
    fn fixture() -> (Document, Host) {
        let text = "pub fn A(user: &User) -> Element { <p>{user.name}</p> }";
        let document = Document::new("file:///a.rsx", text, 1);
        let mut map = rsx_core::SourceMap::new();
        map.record_fragment(
            0,
            rsx_core::Span::new(
                rsx_core::Pos {
                    offset: 0,
                    line: 1,
                    col: 1,
                },
                rsx_core::Pos {
                    offset: text.len(),
                    line: 1,
                    col: 1,
                },
            ),
            text.len(),
        );
        let host = Host {
            path: PathBuf::from("/a.rs"),
            text: text.to_string(),
            shift: 0,
            map,
        };
        (document, host)
    }

    #[test]
    fn translates_a_completion_list() {
        let (document, host) = fixture();
        let result = json!({
            "isIncomplete": false,
            "items": [
                { "label": "name", "kind": 5, "detail": "String" },
                { "label": "len", "kind": 2, "detail": "fn() -> usize",
                  "documentation": { "kind": "markdown", "value": "the length" } },
            ]
        });

        let items = translate_completions(&result, &document, &host);
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
        let (document, host) = fixture();
        let items = translate_completions(&json!([{ "label": "x", "kind": 5 }]), &document, &host);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn inserts_the_edit_text_rather_than_the_label() {
        // `rust-analyzer` labels a field whose expected type is a reference
        // `&name`, while the text to insert is `name`. Inserting the label
        // writes `user.&name`.
        let (document, host) = fixture();
        let items = translate_completions(
            &json!({"items": [{
                "label": "&name",
                "kind": 5,
                "textEdit": {
                    "newText": "name",
                    "range": {"start": {"line": 0, "character": 10},
                              "end": {"line": 0, "character": 10}}
                }
            }]}),
            &document,
            &host,
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert_text.as_deref(), Some("name"));
        assert_eq!(items[0].text_edit.as_ref().unwrap().new_text, "name");
    }

    #[test]
    fn drops_an_item_whose_extra_edit_lands_in_generated_code() {
        // The `&` of a reference completion is placed inside the generated
        // `__w.text(&(` wrapper. The author cannot see it, so the item cannot
        // be applied as the server meant it.
        let (document, host) = fixture();
        let items = translate_completions(
            &json!({"items": [
                {"label": "&name", "kind": 5,
                 "textEdit": {"newText": "name",
                              "range": {"start": {"line": 0, "character": 10},
                                        "end": {"line": 0, "character": 10}}},
                 "additionalTextEdits": [
                    {"newText": "&", "range": {"start": {"line": 0, "character": 2},
                                               "end": {"line": 0, "character": 2}}}]},
                {"label": "name", "kind": 5}
            ]}),
            &document,
            &host,
        );

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["name"], "the reference variant should be gone");
    }

    #[test]
    fn a_replacement_range_is_mapped_back_into_the_template() {
        // Completing after a typed prefix replaces that prefix. Inserting at
        // the cursor instead would turn `user.na` into `user.naname`.
        let (document, host) = fixture();
        let items = translate_completions(
            &json!({"items": [{
                "label": "name", "kind": 5,
                "textEdit": {"newText": "name",
                             "range": {"start": {"line": 0, "character": 44},
                                       "end": {"line": 0, "character": 46}}}
            }]}),
            &document,
            &host,
        );

        let edit = items[0].text_edit.as_ref().expect("no edit");
        assert_eq!(edit.range.start.character, 44);
        assert_eq!(edit.range.end.character, 46);
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
