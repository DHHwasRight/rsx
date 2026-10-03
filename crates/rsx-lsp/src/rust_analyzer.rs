//! Delegating Rust questions to `rust-analyzer`.
//!
//! The template language server knows about components, elements and
//! attributes. It knows nothing about Rust types, and the moment a cursor
//! lands inside `{ user.posts.| }` that is the only thing that matters.
//!
//! Rather than build a second type checker, this forwards the question. Every
//! Rust fragment in a template is copied verbatim into the generated code, and
//! the [`SourceMap`](rsx_core::SourceMap) records where each one landed, so a
//! position in the template has an exact counterpart in real Rust. That
//! counterpart is handed to `rust-analyzer` as a position in a file it already
//! analyses, and the answer comes back with its ranges translated home.
//!
//! The file it is asked about is one that genuinely belongs to the user's
//! crate — the generated `.rs` beside the template, or the file holding the
//! `load_dir!` that produced it — with its contents overlaid in memory. That
//! is what makes the surrounding types, imports and dependencies real.

use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// A JSON-RPC channel to a language server.
///
/// Abstracted so the tests can drive a scripted peer instead of spawning a
/// real `rust-analyzer`, which takes seconds to index and is not installed
/// everywhere.
pub trait Transport: Send {
    fn send(&mut self, message: &Value) -> io::Result<()>;

    /// Waits up to `timeout` for the next message.
    ///
    /// `Ok(None)` means the time ran out and the peer may still speak;
    /// an error means it will not.
    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Value>>;
}

/// A child process spoken to over stdin and stdout.
///
/// Reading happens on its own thread feeding a channel. A language server
/// must never block on a peer that has gone quiet, and a plain blocking read
/// on the child's stdout offers no way to give up.
pub struct ChildTransport {
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Value>,
}

impl ChildTransport {
    pub fn spawn(command: &str, args: &[&str], cwd: &Path) -> io::Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Its progress chatter is not ours to relay.
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");

        let (sender, incoming) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(message)) = read_message(&mut reader) {
                if sender.send(message).is_err() {
                    break; // Nobody is listening any more.
                }
            }
        });

        Ok(ChildTransport {
            child,
            stdin,
            incoming,
        })
    }
}

impl Drop for ChildTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Transport for ChildTransport {
    fn send(&mut self, message: &Value) -> io::Result<()> {
        let body = serde_json::to_vec(message)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        self.stdin.flush()
    }

    fn receive(&mut self, timeout: Duration) -> io::Result<Option<Value>> {
        match self.incoming.recv_timeout(timeout) {
            Ok(message) => Ok(Some(message)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "rust-analyzer closed the connection",
            )),
        }
    }
}

fn read_message(reader: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let Some(length) = length else {
        return Ok(None);
    };
    let mut body = vec![0u8; length];
    io::Read::read_exact(reader, &mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// A connection to `rust-analyzer`.
pub struct RustAnalyzer {
    transport: Box<dyn Transport>,
    next_id: i64,
    /// Files this server has been told about, so a second edit is a change
    /// rather than another open.
    opened: Vec<PathBuf>,
    /// How long to wait for a single reply before giving up on it.
    timeout: Duration,
    ready: bool,
    /// Whether the crate has been indexed. Answers before that are empty,
    /// which is indistinguishable from "nothing to suggest".
    primed: bool,
    /// Whether the priming cycle under way has reported any progress.
    priming_reported: bool,
}

impl RustAnalyzer {
    /// Wraps an already-connected transport.
    pub fn new(transport: Box<dyn Transport>) -> Self {
        RustAnalyzer {
            transport,
            next_id: 1,
            opened: Vec::new(),
            timeout: Duration::from_secs(10),
            ready: false,
            primed: false,
            priming_reported: false,
        }
    }

    /// Starts `rust-analyzer` for `root` and completes the handshake.
    pub fn spawn(binary: &str, root: &Path) -> io::Result<Self> {
        let transport = ChildTransport::spawn(binary, &[], root)?;
        let mut analyzer = RustAnalyzer::new(Box::new(transport));
        analyzer.initialize(root)?;
        Ok(analyzer)
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Whether the crate has finished indexing.
    pub fn is_primed(&self) -> bool {
        self.primed
    }

    /// Waits for indexing to finish, so the first answer is not an empty one.
    ///
    /// `rust-analyzer` replies to a request made during start-up with an
    /// empty list rather than an error, which would look exactly like "this
    /// expression has no members". Waiting once, on the first question that
    /// needs it, is the difference between the feature working and the
    /// feature silently doing nothing.
    pub fn wait_until_primed(&mut self) -> bool {
        if self.primed {
            return true;
        }
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            match self.transport.receive(Duration::from_millis(250)) {
                Ok(Some(message)) => self.note(&message),
                Ok(None) => continue,
                Err(_) => return false,
            }
            if self.primed {
                return true;
            }
        }
        false
    }

    /// Waits briefly to see whether the server is still working.
    ///
    /// Returns true if anything arrived, which means an answer just given may
    /// have been premature and is worth asking for again.
    pub fn progressed(&mut self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            match self.transport.receive(Duration::from_millis(50)) {
                Ok(Some(message)) => {
                    self.note(&message);
                    return true;
                }
                Ok(None) => continue,
                Err(_) => return false,
            }
        }
        false
    }

    /// Watches a message for the signal that indexing has finished.
    fn note(&mut self, message: &Value) {
        if message.get("method").and_then(Value::as_str) != Some("$/progress") {
            return;
        }
        let Some(params) = message.get("params") else {
            return;
        };
        let token = params.get("token").and_then(Value::as_str).unwrap_or("");
        let kind = params
            .get("value")
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("");
        // `rust-analyzer` runs two priming cycles: a trivial one before the
        // crate graph exists, which ends almost at once, and the real one
        // after. Only the real one reports progress, so a cycle that reported
        // nothing is not readiness — treating it as such means asking while
        // the index is empty and being told, indistinguishably, that there is
        // nothing to suggest.
        if !token.contains("cachePriming") {
            return;
        }
        match kind {
            "begin" => self.priming_reported = false,
            "report" => self.priming_reported = true,
            "end" if self.priming_reported => self.primed = true,
            _ => {}
        }
    }

    fn initialize(&mut self, root: &Path) -> io::Result<()> {
        let uri = path_to_uri(root);
        let params = json!({
            "processId": std::process::id(),
            "rootUri": uri,
            "workspaceFolders": [{ "uri": uri, "name": "root" }],
            "capabilities": {
                // Required for `$/progress`, which is how indexing announces
                // that it has finished.
                "window": { "workDoneProgress": true },
                "textDocument": {
                    "completion": {
                        "completionItem": { "snippetSupport": false },
                        "contextSupport": true,
                    },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                },
            },
        });

        self.request("initialize", params)?;
        self.notify("initialized", json!({}))?;
        self.ready = true;
        Ok(())
    }

    /// Makes `rust-analyzer` see `text` as the contents of `path`.
    pub fn overlay(&mut self, path: &Path, text: &str) -> io::Result<()> {
        let uri = path_to_uri(path);
        if self.opened.iter().any(|p| p == path) {
            self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": uri, "version": self.next_id },
                    "contentChanges": [{ "text": text }],
                }),
            )
        } else {
            self.opened.push(path.to_path_buf());
            self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": "rust",
                        "version": 1,
                        "text": text,
                    },
                }),
            )
        }
    }

    /// Asks for completions at a position in an overlaid file.
    pub fn completion(&mut self, path: &Path, line: u32, character: u32) -> io::Result<Value> {
        self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line, "character": character },
                "context": { "triggerKind": 1 },
            }),
        )
    }

    /// Asks where the thing at a position is defined.
    pub fn definition(&mut self, path: &Path, line: u32, character: u32) -> io::Result<Value> {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line, "character": character },
            }),
        )
    }

    /// Asks for hover at a position in an overlaid file.
    pub fn hover(&mut self, path: &Path, line: u32, character: u32) -> io::Result<Value> {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": path_to_uri(path) },
                "position": { "line": line, "character": character },
            }),
        )
    }

    pub fn shutdown(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
    }

    fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.transport.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    /// Sends a request and waits for the reply with the matching id.
    ///
    /// Anything else arriving in the meantime — progress notifications,
    /// diagnostics, requests back at us — is discarded: this is a one-way
    /// consumer of answers, not a full client.
    fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        self.transport.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;

        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            let Some(message) = self.transport.receive(Duration::from_millis(250))? else {
                continue;
            };
            if message.get("id").and_then(Value::as_i64) != Some(id) {
                self.note(&message);
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(io::Error::other(format!("rust-analyzer: {error}")));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }

        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("rust-analyzer did not answer {method} in time"),
        ))
    }
}

/// Converts a path to a `file://` URI.
///
/// Percent-encodes the characters that would otherwise change how the URI
/// parses; a path with a space in it is common enough to matter.
pub fn path_to_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'/' | b'-' | b'.' | b'_' | b'~' => out.push(byte as char),
            b if b.is_ascii_alphanumeric() => out.push(b as char),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Converts a `file://` URI back to a path.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(String::from_utf8(out).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_paths_through_uris() {
        for path in ["/a/b.rs", "/with space/c.rs", "/caf\u{e9}/d.rs"] {
            let uri = path_to_uri(Path::new(path));
            assert!(!uri.contains(' '), "{uri}");
            assert_eq!(uri_to_path(&uri).unwrap(), PathBuf::from(path));
        }
    }

    #[test]
    fn rejects_a_uri_that_is_not_a_file() {
        assert!(uri_to_path("http://example.com/a.rs").is_none());
    }
}
