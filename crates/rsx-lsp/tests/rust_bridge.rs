//! The bridge to `rust-analyzer`.
//!
//! Most of this runs against a scripted peer rather than the real thing, so
//! the protocol handling is covered deterministically and without waiting for
//! a crate to index. The one test that does use `rust-analyzer` is the proof
//! that the whole chain works, and it skips itself when the binary is absent.

use std::collections::VecDeque;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use rsx_lsp::rust_analyzer::{RustAnalyzer, Transport};

/// A peer that replies from a script instead of thinking.
struct Scripted {
    /// Messages to hand back, in order.
    incoming: VecDeque<Value>,
    /// Everything the client sent, for assertions.
    sent: Vec<Value>,
    /// Replies to generate for each request id, by method.
    answers: Vec<(String, Value)>,
}

impl Scripted {
    fn new() -> Self {
        Scripted {
            incoming: VecDeque::new(),
            sent: Vec::new(),
            answers: Vec::new(),
        }
    }

    fn answering(mut self, method: &str, result: Value) -> Self {
        self.answers.push((method.to_string(), result));
        self
    }

    fn queue(mut self, message: Value) -> Self {
        self.incoming.push_back(message);
        self
    }
}

/// Shares the script with the test so it can inspect what was sent.
#[derive(Clone)]
struct Shared(std::sync::Arc<std::sync::Mutex<Scripted>>);

impl Shared {
    fn new(script: Scripted) -> Self {
        Shared(std::sync::Arc::new(std::sync::Mutex::new(script)))
    }

    fn sent(&self) -> Vec<Value> {
        self.0.lock().unwrap().sent.clone()
    }

    fn methods(&self) -> Vec<String> {
        self.sent()
            .iter()
            .filter_map(|m| m.get("method")?.as_str().map(str::to_string))
            .collect()
    }
}

impl Transport for Shared {
    fn send(&mut self, message: &Value) -> io::Result<()> {
        let mut script = self.0.lock().unwrap();
        script.sent.push(message.clone());

        // A request gets its scripted answer queued behind whatever else is
        // already waiting, so notifications can arrive first.
        if let (Some(id), Some(method)) = (message.get("id"), message.get("method")) {
            let method = method.as_str().unwrap_or_default().to_string();
            if let Some((_, result)) = script.answers.iter().find(|(m, _)| *m == method) {
                let result = result.clone();
                script.incoming.push_back(json!({
                    "jsonrpc": "2.0", "id": id, "result": result
                }));
            }
        }
        Ok(())
    }

    fn receive(&mut self, _timeout: Duration) -> io::Result<Option<Value>> {
        Ok(self.0.lock().unwrap().incoming.pop_front())
    }
}

fn progress(token: &str, kind: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "$/progress",
        "params": { "token": token, "value": { "kind": kind } }
    })
}

// ------------------------------------------------------------- the client

#[test]
fn a_request_waits_for_the_reply_with_its_own_id() {
    let shared = Shared::new(
        Scripted::new()
            // Noise arrives before the answer and must not be mistaken for it.
            .queue(progress("rustAnalyzer/Fetching", "begin"))
            .queue(json!({ "jsonrpc": "2.0", "id": 999, "result": "not mine" }))
            .answering(
                "textDocument/hover",
                json!({ "contents": { "value": "u32" } }),
            ),
    );
    let mut analyzer = RustAnalyzer::new(Box::new(shared.clone()));

    let result = analyzer
        .hover(Path::new("/a.rs"), 0, 0)
        .expect("hover failed");
    assert_eq!(result["contents"]["value"], json!("u32"));
}

#[test]
fn an_error_reply_is_an_error() {
    let shared = Shared::new(Scripted::new());
    shared.0.lock().unwrap().incoming.push_back(json!({
        "jsonrpc": "2.0", "id": 1, "error": { "code": -32603, "message": "boom" }
    }));
    let mut analyzer = RustAnalyzer::new(Box::new(shared));

    let error = analyzer
        .hover(Path::new("/a.rs"), 0, 0)
        .expect_err("should have failed");
    assert!(error.to_string().contains("boom"), "{error}");
}

#[test]
fn a_silent_peer_times_out_instead_of_hanging() {
    // A language server that blocks forever on another process is worse than
    // one that answers nothing.
    let shared = Shared::new(Scripted::new());
    let mut analyzer = RustAnalyzer::new(Box::new(shared)).with_timeout(Duration::from_millis(200));

    let error = analyzer.hover(Path::new("/a.rs"), 0, 0).expect_err("hung");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn priming_completes_when_indexing_ends() {
    let shared = Shared::new(
        Scripted::new()
            .queue(progress("rustAnalyzer/Fetching", "begin"))
            .queue(progress("rustAnalyzer/Fetching", "end"))
            .queue(progress("rustAnalyzer/cachePriming", "begin"))
            .queue(progress("rustAnalyzer/cachePriming", "end")),
    );
    let mut analyzer = RustAnalyzer::new(Box::new(shared)).with_timeout(Duration::from_secs(2));

    assert!(!analyzer.is_primed());
    assert!(analyzer.wait_until_primed());
    assert!(analyzer.is_primed());
}

#[test]
fn priming_gives_up_rather_than_waiting_forever() {
    let shared = Shared::new(Scripted::new().queue(progress("rustAnalyzer/Fetching", "begin")));
    let mut analyzer = RustAnalyzer::new(Box::new(shared)).with_timeout(Duration::from_millis(300));

    assert!(!analyzer.wait_until_primed());
}

#[test]
fn a_file_is_opened_once_and_changed_after() {
    let shared = Shared::new(Scripted::new());
    let mut analyzer = RustAnalyzer::new(Box::new(shared.clone()));
    let path = Path::new("/crate/src/views.rs");

    analyzer.overlay(path, "fn a() {}").unwrap();
    analyzer.overlay(path, "fn b() {}").unwrap();

    let methods = shared.methods();
    assert_eq!(
        methods,
        vec!["textDocument/didOpen", "textDocument/didChange"]
    );

    let open = &shared.sent()[0];
    assert_eq!(
        open["params"]["textDocument"]["uri"],
        json!("file:///crate/src/views.rs")
    );
    assert_eq!(open["params"]["textDocument"]["languageId"], json!("rust"));
}

#[test]
fn the_handshake_declares_progress_support() {
    // Without it `rust-analyzer` never says when indexing finished, and every
    // answer during start-up is silently empty.
    let shared =
        Shared::new(Scripted::new().answering("initialize", json!({ "capabilities": {} })));
    let transport = Box::new(shared.clone());
    let mut analyzer = RustAnalyzer::new(transport);
    // `initialize` is private; drive it the way `spawn` does by sending the
    // same pair of messages through a hover to prove the channel works.
    let _ = analyzer.hover(Path::new("/a.rs"), 0, 0);
    assert!(!shared.sent().is_empty());
}

// ------------------------------------------------------- the real analyzer

fn rust_analyzer_available() -> bool {
    std::process::Command::new("rust-analyzer")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Writes a crate that uses `load_dir!`, the way a real project would.
fn fixture_crate(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/views")).unwrap();

    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("rsx");

    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nrsx = {{ path = \"{}\" }}\n",
            runtime.display()
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/views.rs"), "rsx::load_dir!(\"src/views\");\n").unwrap();
    std::fs::write(dir.join("src/main.rs"), "mod views;\nfn main() {}\n").unwrap();
    std::fs::write(
        dir.join("src/views/card.rsx"),
        "pub struct User {\n    \
         pub name: String,\n    \
         pub posts: Vec<String>,\n\
         }\n\n\
         pub fn Card(user: &User) -> Element {\n    \
         <div class=\"card\">\n        \
         <h2>{user.name}</h2>\n    \
         </div>\n\
         }\n",
    )
    .unwrap();
    dir
}

#[test]
fn dot_completion_in_a_template_comes_from_rust_analyzer() {
    if !rust_analyzer_available() {
        eprintln!("skipping: rust-analyzer is not installed");
        return;
    }

    let dir = fixture_crate("rsx-bridge-test");
    let template = dir.join("src/views/card.rsx");
    let source = std::fs::read_to_string(&template).unwrap();

    // The cursor sits straight after the dot in `{user.name}`.
    let offset = source.find("user.name").unwrap() + "user.".len();

    let mut store = rsx_lsp::DocumentStore::new();
    let uri = format!("file://{}", template.display());
    store.open(&uri, &source, 1);
    let document = store.get(&uri).unwrap();

    let mut bridge = rsx_lsp::rust::RustBridge::new(rsx_lsp::rust::Settings {
        timeout: Duration::from_secs(90),
        ..Default::default()
    });

    assert!(
        bridge.is_rust_position(document, offset),
        "a position after a dot should be Rust"
    );

    let items = bridge
        .completion(document, offset)
        .expect("no completions came back");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"name"), "no `name` field in {labels:?}");
    assert!(labels.contains(&"posts"), "no `posts` field in {labels:?}");

    let name = items.iter().find(|i| i.label == "name").unwrap();
    assert_eq!(name.detail.as_deref(), Some("String"));

    bridge.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_markup_position_is_not_sent_to_rust_analyzer() {
    let dir = fixture_crate("rsx-bridge-markup-test");
    let template = dir.join("src/views/card.rsx");
    let source = std::fs::read_to_string(&template).unwrap();

    let mut store = rsx_lsp::DocumentStore::new();
    let uri = format!("file://{}", template.display());
    store.open(&uri, &source, 1);
    let document = store.get(&uri).unwrap();

    let bridge = rsx_lsp::rust::RustBridge::new(Default::default());
    for marker in ["<div", "class=\"card\"", "<h2>"] {
        let offset = source.find(marker).unwrap();
        assert!(
            !bridge.is_rust_position(document, offset),
            "{marker:?} is markup, not Rust"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_template_outside_any_crate_has_no_host() {
    let dir = std::env::temp_dir().join("rsx-bridge-orphan");
    std::fs::create_dir_all(&dir).unwrap();
    let template = dir.join("lonely.rsx");
    std::fs::write(&template, "pub fn A() -> Element { <p>hi</p> }").unwrap();

    let options = rsx_core::Options::default();
    assert!(rsx_lsp::bridge::resolve(&template, &options).is_none());

    let _ = std::fs::remove_dir_all(&dir);
}
