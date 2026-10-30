//! A Language Server Protocol server over stdio.
//!
//! The transport is deliberately small: read a `Content-Length` header, read
//! that many bytes of JSON, dispatch, write a reply the same way. Everything
//! interesting lives in the analysis modules, which are testable without a
//! transport at all.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::protocol::Position;
use crate::rust::{RustBridge, Settings};
use crate::workspace::Index;
use crate::{completion, definition, hover, references, symbols, DocumentStore};

/// Runs the server on stdin and stdout until the client disconnects.
pub fn run() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_with(stdin.lock(), stdout.lock())
}

/// Runs the server over any pair of streams, which is what the tests drive.
pub fn run_with(mut reader: impl BufRead, mut writer: impl Write) -> io::Result<()> {
    let mut server = Server::new();

    while let Some(message) = read_message(&mut reader)? {
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue; // A response to something we sent; nothing to do.
        };
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if method == "exit" {
            return Ok(());
        }

        match server.handle(method, &params) {
            Outcome::Reply(result) => {
                if let Some(id) = id {
                    write_message(
                        &mut writer,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result,
                        }),
                    )?;
                }
            }
            Outcome::Notify(notifications) => {
                for notification in notifications {
                    write_message(&mut writer, &notification)?;
                }
            }
            Outcome::Ignore => {}
        }
    }

    Ok(())
}

enum Outcome {
    Reply(Value),
    Notify(Vec<Value>),
    Ignore,
}

struct Server {
    store: DocumentStore,
    /// Answers the questions that are about Rust rather than about markup.
    rust: RustBridge,
    /// The files the editor has not opened, which is most of the project.
    index: Index,
}

impl Server {
    fn new() -> Self {
        Server {
            store: DocumentStore::new(),
            rust: RustBridge::new(Settings::default()),
            index: Index::empty(),
        }
    }

    /// Reads the settings an editor passes in `initializationOptions`.
    fn configure(&mut self, params: &Value) {
        let mut settings = Settings::default();
        if let Some(options) = params.get("initializationOptions") {
            if let Some(analyzer) = options.get("rustAnalyzer") {
                if let Some(enabled) = analyzer.get("enabled").and_then(Value::as_bool) {
                    settings.enabled = enabled;
                }
                if let Some(path) = analyzer.get("path").and_then(Value::as_str) {
                    if !path.is_empty() {
                        settings.binary = path.to_string();
                    }
                }
            }
        }
        self.rust = RustBridge::new(settings);
    }

    fn handle(&mut self, method: &str, params: &Value) -> Outcome {
        match method {
            "initialize" => {
                self.configure(params);
                // Reading the project costs one pass over the `.rsx` files,
                // and it happens here rather than on the first question so
                // that the first question is not the slow one.
                self.index = Index::from_initialize(params);
                Outcome::Reply(json!({
                "capabilities": {
                    "textDocumentSync": 1, // full text on every change
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "documentSymbolProvider": true,
                    "referencesProvider": true,
                    "completionProvider": {
                        // `.` matters most: without it an editor never
                        // asks after `user.`, which is exactly where the
                        // answer has to come from rust-analyzer. `<` opens a
                        // tag, a space starts an attribute, `:` a path.
                        "triggerCharacters": ["<", " ", ":", "."],
                    },
                },
                "serverInfo": { "name": "rsx-lsp", "version": env!("CARGO_PKG_VERSION") },
                }))
            }
            "initialized" => Outcome::Reply(Value::Null),
            "shutdown" => {
                self.rust.shutdown();
                Outcome::Reply(Value::Null)
            }

            "textDocument/didOpen" => {
                let Some(doc) = params.get("textDocument") else {
                    return Outcome::Ignore;
                };
                let uri = string(doc, "uri");
                let text = string(doc, "text");
                let version = doc.get("version").and_then(Value::as_i64).unwrap_or(1) as i32;
                self.store.open(&uri, &text, version);
                Outcome::Notify(vec![self.diagnostics(&uri)])
            }

            "textDocument/didChange" => {
                let Some(doc) = params.get("textDocument") else {
                    return Outcome::Ignore;
                };
                let uri = string(doc, "uri");
                let version = doc.get("version").and_then(Value::as_i64).unwrap_or(0) as i32;
                // Full sync: the last change carries the whole document.
                let text = params
                    .get("contentChanges")
                    .and_then(Value::as_array)
                    .and_then(|changes| changes.last())
                    .map(|change| string(change, "text"))
                    .unwrap_or_default();

                if self.store.update(&uri, &text, version).is_none() {
                    self.store.open(&uri, &text, version);
                }
                Outcome::Notify(vec![self.diagnostics(&uri)])
            }

            "textDocument/didClose" => {
                let uri = params
                    .get("textDocument")
                    .map(|doc| string(doc, "uri"))
                    .unwrap_or_default();
                self.store.close(&uri);
                Outcome::Ignore
            }

            "textDocument/hover" => {
                let (uri, position) = location(params);
                let Some(document) = self.store.get(&uri) else {
                    return Outcome::Reply(Value::Null);
                };
                let offset = document.offset(position);

                // A cursor inside a Rust fragment is asking about Rust, and
                // `rust-analyzer` knows more about that than this server ever
                // will. Markup positions, and anything it cannot answer, fall
                // back to what the template itself says.
                let overlay = self.store.overlay();
                let from_rust = self
                    .rust
                    .is_enabled()
                    .then(|| self.rust.hover(document, offset, &overlay))
                    .flatten();

                let result = match from_rust {
                    Some(hover) => Some(hover),
                    None => {
                        let document = self.store.get(&uri).expect("still open");
                        hover::handle(document, position, &self.store)
                    }
                };
                Outcome::Reply(to_value(result))
            }

            "textDocument/definition" => {
                let (uri, position) = location(params);
                let Some(document) = self.store.get(&uri) else {
                    return Outcome::Reply(Value::Null);
                };

                // A component call is a template concept, and the template
                // knows where it is declared — in the `.rsx`, which is where
                // the author wants to land. Everything else is Rust.
                if let Some(found) = definition::handle(document, position, &self.store) {
                    return Outcome::Reply(to_value(Some(found)));
                }

                let offset = document.offset(position);
                let overlay = self.store.overlay();
                let result = self
                    .rust
                    .is_enabled()
                    .then(|| self.rust.definition(document, offset, &overlay))
                    .flatten();
                Outcome::Reply(to_value(result))
            }

            "textDocument/documentSymbol" => {
                let uri = params
                    .get("textDocument")
                    .map(|doc| string(doc, "uri"))
                    .unwrap_or_default();
                let result = self.store.get(&uri).and_then(symbols::handle);
                Outcome::Reply(to_value(result))
            }

            "textDocument/references" => {
                let (uri, position) = location(params);
                let Some(document) = self.store.get(&uri) else {
                    return Outcome::Reply(Value::Null);
                };
                // The flag is the client's, and leaving it out means "no":
                // an editor that wanted the declaration would have said so.
                let include_declaration = params
                    .get("context")
                    .and_then(|c| c.get("includeDeclaration"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let result = references::handle(
                    document,
                    position,
                    &self.store,
                    &self.index,
                    include_declaration,
                );
                Outcome::Reply(to_value(result))
            }

            // A save is the moment the file on disk and the file on screen
            // agree again, which is the cheapest honest point to re-read the
            // rest of the project.
            "textDocument/didSave" => {
                self.index.refresh();
                Outcome::Ignore
            }

            "textDocument/completion" => {
                let (uri, position) = location(params);
                let Some(document) = self.store.get(&uri) else {
                    return Outcome::Reply(to_value(Some(Vec::<Value>::new())));
                };
                let offset = document.offset(position);

                let overlay = self.store.overlay();
                let from_rust = self
                    .rust
                    .is_enabled()
                    .then(|| self.rust.completion(document, offset, &overlay))
                    .flatten()
                    .filter(|items| !items.is_empty());

                let items = match from_rust {
                    Some(items) => items,
                    None => {
                        let document = self.store.get(&uri).expect("still open");
                        completion::handle(document, position, &self.store)
                    }
                };
                Outcome::Reply(to_value(Some(items)))
            }

            _ => Outcome::Reply(Value::Null),
        }
    }

    fn diagnostics(&self, uri: &str) -> Value {
        let diagnostics = self
            .store
            .get(uri)
            .map(|doc| doc.diagnostics.clone())
            .unwrap_or_default();
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "diagnostics": diagnostics },
        })
    }
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn location(params: &Value) -> (String, Position) {
    let uri = params
        .get("textDocument")
        .map(|doc| string(doc, "uri"))
        .unwrap_or_default();
    let position = params
        .get("position")
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .unwrap_or(Position::new(0, 0));
    (uri, position)
}

fn to_value<T: serde::Serialize>(value: Option<T>) -> Value {
    value
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or(Value::Null)
}

/// Reads one `Content-Length`-framed message, or `None` at end of input.
fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length: Option<usize> = None;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break; // End of headers.
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }

    let Some(length) = length else {
        return Ok(None);
    };
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body).ok())
}

fn write_message(writer: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}
