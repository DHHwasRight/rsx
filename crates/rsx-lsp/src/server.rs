//! A Language Server Protocol server over stdio.
//!
//! The transport is deliberately small: read a `Content-Length` header, read
//! that many bytes of JSON, dispatch, write a reply the same way. Everything
//! interesting lives in the analysis modules, which are testable without a
//! transport at all.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

use crate::protocol::Position;
use crate::{completion, definition, hover, symbols, DocumentStore};

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
}

impl Server {
    fn new() -> Self {
        Server {
            store: DocumentStore::new(),
        }
    }

    fn handle(&mut self, method: &str, params: &Value) -> Outcome {
        match method {
            "initialize" => Outcome::Reply(json!({
                "capabilities": {
                    "textDocumentSync": 1, // full text on every change
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "documentSymbolProvider": true,
                    "completionProvider": {
                        "triggerCharacters": ["<", " ", ":"],
                    },
                },
                "serverInfo": { "name": "rsx-lsp", "version": env!("CARGO_PKG_VERSION") },
            })),
            "initialized" | "shutdown" => Outcome::Reply(Value::Null),

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
                let result = self
                    .store
                    .get(&uri)
                    .and_then(|doc| hover::handle(doc, position, &self.store));
                Outcome::Reply(to_value(result))
            }

            "textDocument/definition" => {
                let (uri, position) = location(params);
                let result = self
                    .store
                    .get(&uri)
                    .and_then(|doc| definition::handle(doc, position, &self.store));
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

            "textDocument/completion" => {
                let (uri, position) = location(params);
                let items = self
                    .store
                    .get(&uri)
                    .map(|doc| completion::handle(doc, position, &self.store))
                    .unwrap_or_default();
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
