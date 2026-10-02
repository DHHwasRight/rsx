//! Language server behaviour, ported from the Crystal `crx` LSP specs.

use rsx_lsp::protocol::{CompletionItemKind, InsertTextFormat, Position};
use rsx_lsp::{completion, definition, find_node, hover, symbols, DocumentStore};

fn position(line: u32, character: u32) -> Position {
    Position::new(line, character)
}

// ---------------------------------------------------------- document store

mod document_store {
    use super::*;

    #[test]
    fn parses_valid_content_with_no_diagnostics() {
        let mut store = DocumentStore::new();
        let doc = store.open(
            "file:///test.rsx",
            "pub fn Hello(name: &str) -> Element {\n    <div>{name}</div>\n}",
            1,
        );

        assert!(doc.diagnostics.is_empty());
        let ast = doc.ast.as_ref().expect("should have parsed");
        assert_eq!(ast.components().count(), 1);
        assert_eq!(ast.components().next().unwrap().name, "Hello");
    }

    #[test]
    fn produces_diagnostics_for_invalid_content() {
        let mut store = DocumentStore::new();
        let doc = store.open("file:///bad.rsx", "pub fn Bad() -> Element { <div>", 1);

        assert_eq!(doc.diagnostics.len(), 1);
        assert!(doc.ast.is_none());
        assert_eq!(doc.diagnostics[0].source, "rsx");
    }

    #[test]
    fn produces_diagnostics_with_position_for_mismatched_tags() {
        let mut store = DocumentStore::new();
        let doc = store.open(
            "file:///mismatch.rsx",
            "pub fn Bad() -> Element {\n    <div></span>\n}",
            1,
        );

        assert_eq!(doc.diagnostics.len(), 1);
        assert!(
            doc.diagnostics[0].message.contains("does not match"),
            "{}",
            doc.diagnostics[0].message
        );
        // The caret belongs on the closing tag, which is on line 1.
        assert_eq!(doc.diagnostics[0].range.start.line, 1);
    }

    #[test]
    fn a_diagnostic_carries_its_help_text() {
        let mut store = DocumentStore::new();
        let doc = store.open("file:///bad.rsx", "pub fn card() -> Element { <p></p> }", 1);
        assert!(
            doc.diagnostics[0].message.contains("help:"),
            "{}",
            doc.diagnostics[0].message
        );
    }

    #[test]
    fn reparses_on_update() {
        let mut store = DocumentStore::new();
        let doc = store.open("file:///test.rsx", "pub fn Bad() -> Element { <div>", 1);
        assert_eq!(doc.diagnostics.len(), 1);

        let updated = store
            .update(
                "file:///test.rsx",
                "pub fn Fixed() -> Element {\n    <p>ok</p>\n}",
                2,
            )
            .expect("should update");

        assert!(updated.diagnostics.is_empty());
        assert!(updated.ast.is_some());
        assert_eq!(updated.version, 2);
    }

    #[test]
    fn returns_none_for_update_on_unknown_uri() {
        let mut store = DocumentStore::new();
        assert!(store.update("file:///unknown.rsx", "content", 1).is_none());
    }

    #[test]
    fn removes_a_document_on_close() {
        let mut store = DocumentStore::new();
        store.open("file:///test.rsx", "pub fn A() -> Element { <p></p> }", 1);
        assert_eq!(store.len(), 1);
        assert!(store.close("file:///test.rsx"));
        assert!(store.is_empty());
        assert!(!store.close("file:///test.rsx"));
    }

    #[test]
    fn collects_components_across_documents() {
        let mut store = DocumentStore::new();
        store.open("file:///a.rsx", "pub fn Alpha() -> Element { <p></p> }", 1);
        store.open("file:///b.rsx", "pub fn Beta() -> Element { <p></p> }", 1);

        let names: Vec<&str> = store
            .all_components()
            .iter()
            .map(|(_, c)| c.name.as_str())
            .collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn clears_old_diagnostics_when_valid_content_replaces_invalid() {
        let mut store = DocumentStore::new();
        store.open("file:///test.rsx", "pub fn Bad() -> Element { <div>", 1);
        let updated = store
            .update("file:///test.rsx", "pub fn Ok() -> Element { <p></p> }", 2)
            .unwrap();
        assert!(updated.diagnostics.is_empty());
    }

    #[test]
    fn converts_positions_across_non_ascii_lines() {
        let mut store = DocumentStore::new();
        // An emoji is two UTF-16 code units, which is what the protocol counts.
        let doc = store.open(
            "file:///emoji.rsx",
            "pub fn A() -> Element {\n    <p>\u{1f980} {x}</p>\n}",
            1,
        );
        let offset = doc.offset(position(1, 9));
        assert_eq!(doc.position(offset), position(1, 9));
        assert!(doc.line(1).contains('\u{1f980}'));
    }
}

// --------------------------------------------------------------- find node

mod find_node_tests {
    use super::*;
    use rsx_lsp::find_node::Found;

    fn found_at(source: &str, line: u32, character: u32) -> Option<String> {
        let mut store = DocumentStore::new();
        let doc = store.open("file:///t.rsx", source, 1);
        let file = doc.ast.as_ref()?;
        let offset = doc.offset(position(line, character));
        find_node::at(file, offset).map(|found| match found {
            Found::ComponentName(c) => format!("name:{}", c.name),
            Found::Param(_, p) => format!("param:{}", p.name),
            Found::Call(c) => format!("call:{}", c.path),
            Found::Element(e) => format!("element:{}", e.tag),
            Found::Attribute(a) => format!("attr:{}", a.name().unwrap_or("spread")),
            Found::Expr(e) => format!("expr:{}", e.source),
            Found::Component(c) => format!("component:{}", c.name),
        })
    }

    #[test]
    fn finds_a_component_call_at_a_position() {
        let found = found_at("pub fn Page() -> Element {\n    <Card />\n}", 1, 6);
        assert_eq!(found.as_deref(), Some("call:Card"));
    }

    #[test]
    fn finds_an_element_at_a_position() {
        let found = found_at("pub fn Page() -> Element {\n    <div>hi</div>\n}", 1, 6);
        assert_eq!(found.as_deref(), Some("element:div"));
    }

    #[test]
    fn finds_a_param_at_a_position() {
        let found = found_at(
            "pub fn Hello(name: &str) -> Element {\n    <p>{name}</p>\n}",
            0,
            14,
        );
        assert_eq!(found.as_deref(), Some("param:name"));
    }

    #[test]
    fn finds_the_component_name() {
        let found = found_at("pub fn Hello() -> Element {\n    <p></p>\n}", 0, 8);
        assert_eq!(found.as_deref(), Some("name:Hello"));
    }

    #[test]
    fn finds_an_attribute() {
        let found = found_at(
            "pub fn A() -> Element {\n    <div class=\"x\"></div>\n}",
            1,
            10,
        );
        assert_eq!(found.as_deref(), Some("attr:class"));
    }

    #[test]
    fn finds_an_interpolated_expression() {
        let found = found_at("pub fn A(n: u8) -> Element {\n    <p>{n}</p>\n}", 1, 8);
        assert_eq!(found.as_deref(), Some("expr:n"));
    }

    #[test]
    fn finds_nodes_nested_in_control_flow() {
        let source = "pub fn A(f: bool) -> Element {\n    if f {\n        <Card />\n    }\n}";
        assert_eq!(found_at(source, 2, 10).as_deref(), Some("call:Card"));
    }

    #[test]
    fn returns_the_component_when_the_position_is_on_nothing_more_specific() {
        let found = found_at("pub fn Page() -> Element {\n\n    <p></p>\n}", 1, 0);
        assert_eq!(found.as_deref(), Some("component:Page"));
    }

    #[test]
    fn returns_none_outside_any_component() {
        let found = found_at(
            "use std::fmt;\n\npub fn A() -> Element {\n    <p></p>\n}",
            0,
            3,
        );
        assert_eq!(found, None);
    }
}

// ------------------------------------------------------------------- hover

mod hover_tests {
    use super::*;

    fn hover_at(store: &DocumentStore, uri: &str, line: u32, character: u32) -> Option<String> {
        let doc = store.get(uri)?;
        hover::handle(doc, position(line, character), store).map(|h| h.contents.value)
    }

    #[test]
    fn returns_hover_for_a_component_call() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///card.rsx",
            "pub fn Card(title: &str) -> Element { <p>{title}</p> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Page() -> Element {\n    <Card />\n}",
            1,
        );

        let value = hover_at(&store, "file:///page.rsx", 1, 6).expect("expected hover");
        assert!(value.contains("Card"), "{value}");
        assert!(value.contains("title: &str"), "{value}");
    }

    #[test]
    fn returns_hover_for_an_element() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <div>hi</div>\n}",
            1,
        );

        let value = hover_at(&store, "file:///t.rsx", 1, 6).expect("expected hover");
        assert!(value.contains("<div>"), "{value}");
        assert!(value.contains("block container"), "{value}");
    }

    #[test]
    fn notes_that_a_void_element_takes_no_closing_tag() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <br>\n}",
            1,
        );
        let value = hover_at(&store, "file:///t.rsx", 1, 6).expect("expected hover");
        assert!(value.contains("void element"), "{value}");
    }

    #[test]
    fn returns_hover_for_a_component_definition() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Hello(name: &str) -> Element {\n    <div>{name}</div>\n}",
            1,
        );

        let value = hover_at(&store, "file:///t.rsx", 0, 9).expect("expected hover");
        assert!(value.contains("Hello"), "{value}");
        assert!(value.contains("name: &str"), "{value}");
    }

    #[test]
    fn includes_the_doc_comment() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "/// A small card.\npub fn Card() -> Element {\n    <p></p>\n}",
            1,
        );
        let value = hover_at(&store, "file:///t.rsx", 1, 9).expect("expected hover");
        assert!(value.contains("A small card."), "{value}");
    }

    #[test]
    fn says_a_component_accepts_children() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Box(children: Element) -> Element {\n    <div>{children}</div>\n}",
            1,
        );
        let value = hover_at(&store, "file:///t.rsx", 0, 9).expect("expected hover");
        assert!(value.contains("Accepts children"), "{value}");
    }

    #[test]
    fn reports_a_component_it_cannot_find() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <Absent />\n}",
            1,
        );
        let value = hover_at(&store, "file:///t.rsx", 1, 7).expect("expected hover");
        assert!(value.contains("not found"), "{value}");
    }

    #[test]
    fn returns_none_for_a_position_with_no_tree() {
        let mut store = DocumentStore::new();
        store.open("file:///bad.rsx", "pub fn A() -> Element { <div>", 1);
        assert!(hover_at(&store, "file:///bad.rsx", 0, 0).is_none());
    }
}

// -------------------------------------------------------------- definition

mod definition_tests {
    use super::*;

    #[test]
    fn finds_a_component_definition_across_documents() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///card.rsx",
            "pub fn Card(title: &str) -> Element { <p>{title}</p> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Page() -> Element {\n    <Card />\n}",
            1,
        );

        let doc = store.get("file:///page.rsx").unwrap();
        let location = definition::handle(doc, position(1, 6), &store).expect("expected a jump");
        assert_eq!(location.uri, "file:///card.rsx");
        // `Card` starts at character 7 of line 0.
        assert_eq!(location.range.start, position(0, 7));
    }

    #[test]
    fn prefers_a_declaration_in_the_asking_document() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///other.rsx",
            "pub fn Card() -> Element { <p>other</p> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Card() -> Element { <p>mine</p> }\npub fn Page() -> Element {\n    <Card />\n}",
            1,
        );

        let doc = store.get("file:///page.rsx").unwrap();
        let location = definition::handle(doc, position(2, 6), &store).unwrap();
        assert_eq!(location.uri, "file:///page.rsx");
    }

    #[test]
    fn returns_none_for_html_elements() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <div></div>\n}",
            1,
        );
        let doc = store.get("file:///t.rsx").unwrap();
        assert!(definition::handle(doc, position(1, 6), &store).is_none());
    }

    #[test]
    fn returns_none_for_an_undefined_component() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <Absent />\n}",
            1,
        );
        let doc = store.get("file:///t.rsx").unwrap();
        assert!(definition::handle(doc, position(1, 7), &store).is_none());
    }
}

// ----------------------------------------------------------------- symbols

mod symbol_tests {
    use super::*;

    #[test]
    fn returns_document_symbols_for_components() {
        let mut store = DocumentStore::new();
        let doc = store.open(
            "file:///t.rsx",
            "pub fn Card(title: &str) -> Element { <p>{title}</p> }\npub fn Page() -> Element { <p></p> }",
            1,
        );

        let symbols = symbols::handle(doc).expect("expected symbols");
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "Card");
        assert_eq!(symbols[1].name, "Page");
        assert_eq!(symbols[0].children.len(), 1);
        assert_eq!(symbols[0].children[0].name, "title");
        assert_eq!(symbols[0].children[0].detail.as_deref(), Some("&str"));
    }

    #[test]
    fn returns_none_when_there_is_no_tree() {
        let mut store = DocumentStore::new();
        let doc = store.open("file:///bad.rsx", "pub fn A() -> Element { <div>", 1);
        assert!(symbols::handle(doc).is_none());
    }
}

// -------------------------------------------------------------- completion

mod completion_tests {
    use super::*;

    fn complete(
        store: &DocumentStore,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Vec<rsx_lsp::protocol::CompletionItem> {
        let doc = store.get(uri).unwrap();
        completion::handle(doc, position(line, character), store)
    }

    #[test]
    fn returns_component_completions_with_snippet_params() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///card.rsx",
            "pub fn Card(title: &str, size: usize) -> Element { <p>{title}{size}</p> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Page() -> Element {\n    <\n}",
            1,
        );

        let items = complete(&store, "file:///page.rsx", 1, 5);
        assert!(!items.is_empty());

        let card = items.iter().find(|i| i.label == "Card").expect("no Card");
        assert_eq!(card.kind, CompletionItemKind::Snippet);
        assert_eq!(card.insert_text_format, InsertTextFormat::Snippet);
        let insert = card.insert_text.as_deref().unwrap();
        assert!(insert.contains("title="), "{insert}");
        assert!(insert.contains("size="), "{insert}");
        assert!(card.detail.as_deref().unwrap().contains("title: &str"));
        // Components sort before HTML tags.
        assert!(card.sort_text.as_deref().unwrap().starts_with('0'));
    }

    #[test]
    fn returns_html_tag_completions() {
        let mut store = DocumentStore::new();
        store.open("file:///t.rsx", "pub fn Page() -> Element {\n    <\n}", 1);

        let items = complete(&store, "file:///t.rsx", 1, 5);
        let div = items.iter().find(|i| i.label == "div").expect("no div");
        assert!(div.insert_text.as_deref().unwrap().contains("</div>"));
        assert!(div.sort_text.as_deref().unwrap().starts_with('1'));
    }

    #[test]
    fn returns_void_elements_as_self_closing() {
        let mut store = DocumentStore::new();
        store.open("file:///t.rsx", "pub fn Page() -> Element {\n    <\n}", 1);

        let items = complete(&store, "file:///t.rsx", 1, 5);
        let img = items.iter().find(|i| i.label == "img").expect("no img");
        let insert = img.insert_text.as_deref().unwrap();
        assert!(insert.contains("/>"), "{insert}");
        assert!(!insert.contains("</img>"), "{insert}");
    }

    #[test]
    fn a_component_taking_children_completes_with_a_closing_tag() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///box.rsx",
            "pub fn Panel(children: Element) -> Element { <div>{children}</div> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Page() -> Element {\n    <\n}",
            1,
        );

        let items = complete(&store, "file:///page.rsx", 1, 5);
        let panel = items.iter().find(|i| i.label == "Panel").unwrap();
        assert!(panel.insert_text.as_deref().unwrap().contains("</Panel>"));
    }

    #[test]
    fn returns_empty_when_the_cursor_is_not_after_an_open_angle() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    hello\n}",
            1,
        );
        assert!(complete(&store, "file:///t.rsx", 1, 9).is_empty());
    }

    #[test]
    fn returns_empty_inside_a_finished_tag() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <p>x\n}",
            1,
        );
        assert!(complete(&store, "file:///t.rsx", 1, 9).is_empty());
    }

    #[test]
    fn completes_component_props_inside_a_call() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///card.rsx",
            "pub fn Card(title: &str, count: usize) -> Element { <p>{title}{count}</p> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Page() -> Element {\n    <Card \n}",
            1,
        );

        let items = complete(&store, "file:///page.rsx", 1, 10);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"title"), "{labels:?}");
        assert!(labels.contains(&"count"), "{labels:?}");
        // A children parameter is filled by writing markup, not an attribute.
        assert!(!labels.contains(&"children"), "{labels:?}");
    }

    #[test]
    fn completes_html_attributes_inside_an_element() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <img \n}",
            1,
        );

        let items = complete(&store, "file:///t.rsx", 1, 9);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"src"), "{labels:?}");
        assert!(labels.contains(&"alt"), "{labels:?}");
        assert!(labels.contains(&"class"), "{labels:?}");
    }

    #[test]
    fn does_not_complete_inside_an_open_string() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///t.rsx",
            "pub fn Page() -> Element {\n    <img src=\"a \n}",
            1,
        );
        assert!(complete(&store, "file:///t.rsx", 1, 16).is_empty());
    }

    #[test]
    fn completes_while_the_document_does_not_parse() {
        // Completion is asked for mid-keystroke, which is exactly when the
        // file is broken. The file's own components still have to be offered,
        // which means falling back to the last tree that parsed.
        let mut store = DocumentStore::new();
        store.open(
            "file:///page.rsx",
            "pub fn Card() -> Element { <p></p> }\npub fn Page() -> Element {\n    <p></p>\n}",
            1,
        );
        let doc = store
            .update(
                "file:///page.rsx",
                "pub fn Card() -> Element { <p></p> }\npub fn Page() -> Element {\n    <Ca\n}",
                2,
            )
            .unwrap();
        assert!(doc.ast.is_none(), "this edit should not parse");

        let items = complete(&store, "file:///page.rsx", 2, 7);
        assert!(items.iter().any(|i| i.label == "Card"), "no Card offered");
    }

    #[test]
    fn offers_a_local_component_before_one_from_elsewhere() {
        let mut store = DocumentStore::new();
        store.open(
            "file:///other.rsx",
            "pub fn Card(a: &str) -> Element { <p>{a}</p> }",
            1,
        );
        store.open(
            "file:///page.rsx",
            "pub fn Card(b: usize) -> Element { <p>{b}</p> }\npub fn Page() -> Element {\n    <p></p>\n}",
            1,
        );
        store
            .update(
                "file:///page.rsx",
                "pub fn Card(b: usize) -> Element { <p>{b}</p> }\npub fn Page() -> Element {\n    <\n}",
                2,
            )
            .unwrap();

        let items = complete(&store, "file:///page.rsx", 2, 5);
        let card = items.iter().find(|i| i.label == "Card").unwrap();
        assert!(
            card.detail.as_deref().unwrap().contains("b: usize"),
            "{:?}",
            card.detail
        );
    }
}

// ------------------------------------------------------------------ server

mod server_tests {
    use serde_json::{json, Value};

    /// Frames requests, runs the server over them, and returns the replies.
    fn exchange(requests: &[Value]) -> Vec<Value> {
        let mut input = Vec::new();
        for request in requests {
            let body = serde_json::to_vec(request).unwrap();
            input.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
            input.extend_from_slice(&body);
        }

        let mut output = Vec::new();
        rsx_lsp::server::run_with(std::io::Cursor::new(input), &mut output).expect("server failed");

        parse_messages(&String::from_utf8(output).unwrap())
    }

    fn parse_messages(raw: &str) -> Vec<Value> {
        let mut messages = Vec::new();
        let mut rest = raw;
        while let Some(index) = rest.find("\r\n\r\n") {
            let header = &rest[..index];
            let length: usize = header
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length:"))
                .and_then(|v| v.trim().parse().ok())
                .expect("missing Content-Length");
            let body = &rest[index + 4..index + 4 + length];
            messages.push(serde_json::from_str(body).unwrap());
            rest = &rest[index + 4 + length..];
        }
        messages
    }

    #[test]
    fn announces_its_capabilities_on_initialize() {
        let replies = exchange(&[json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
        })]);

        let capabilities = &replies[0]["result"]["capabilities"];
        assert_eq!(capabilities["hoverProvider"], json!(true));
        assert_eq!(capabilities["definitionProvider"], json!(true));
        assert_eq!(capabilities["documentSymbolProvider"], json!(true));
        assert_eq!(
            capabilities["completionProvider"]["triggerCharacters"][0],
            json!("<")
        );
    }

    #[test]
    fn publishes_diagnostics_when_a_document_opens() {
        let replies = exchange(&[json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///t.rsx",
                    "version": 1,
                    "text": "pub fn A() -> Element { <div></span> }"
                }
            }
        })]);

        assert_eq!(
            replies[0]["method"],
            json!("textDocument/publishDiagnostics")
        );
        let diagnostics = replies[0]["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0]["source"], json!("rsx"));
    }

    #[test]
    fn clears_diagnostics_when_the_document_is_fixed() {
        let replies = exchange(&[
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": "file:///t.rsx", "version": 1,
                    "text": "pub fn A() -> Element { <div>" } }
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": "file:///t.rsx", "version": 2 },
                    "contentChanges": [{ "text": "pub fn A() -> Element { <div></div> }" }]
                }
            }),
        ]);

        let last = replies.last().unwrap();
        assert_eq!(last["params"]["diagnostics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn answers_hover_definition_and_symbols() {
        let open = |uri: &str, text: &str| {
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": { "uri": uri, "version": 1, "text": text } }
            })
        };

        let replies = exchange(&[
            open(
                "file:///card.rsx",
                "pub fn Card(title: &str) -> Element { <p>{title}</p> }",
            ),
            open(
                "file:///page.rsx",
                "pub fn Page() -> Element {\n    <Card />\n}",
            ),
            json!({
                "jsonrpc": "2.0", "id": 10, "method": "textDocument/hover",
                "params": {
                    "textDocument": { "uri": "file:///page.rsx" },
                    "position": { "line": 1, "character": 6 }
                }
            }),
            json!({
                "jsonrpc": "2.0", "id": 11, "method": "textDocument/definition",
                "params": {
                    "textDocument": { "uri": "file:///page.rsx" },
                    "position": { "line": 1, "character": 6 }
                }
            }),
            json!({
                "jsonrpc": "2.0", "id": 12, "method": "textDocument/documentSymbol",
                "params": { "textDocument": { "uri": "file:///card.rsx" } }
            }),
        ]);

        let reply = |id: i64| {
            replies
                .iter()
                .find(|m| m["id"] == json!(id))
                .unwrap_or_else(|| panic!("no reply for {id}"))
        };

        let hover = reply(10)["result"]["contents"]["value"].as_str().unwrap();
        assert!(hover.contains("title: &str"), "{hover}");

        assert_eq!(reply(11)["result"]["uri"], json!("file:///card.rsx"));

        let symbols = reply(12)["result"].as_array().unwrap();
        assert_eq!(symbols[0]["name"], json!("Card"));
    }

    #[test]
    fn answers_completion_with_components_first() {
        let replies = exchange(&[
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": "file:///t.rsx", "version": 1,
                    "text": "pub fn Card() -> Element { <p></p> }\npub fn Page() -> Element {\n    <p></p>\n}" } }
            }),
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/didChange",
                "params": {
                    "textDocument": { "uri": "file:///t.rsx", "version": 2 },
                    "contentChanges": [{ "text": "pub fn Card() -> Element { <p></p> }\npub fn Page() -> Element {\n    <\n}" }]
                }
            }),
            json!({
                "jsonrpc": "2.0", "id": 20, "method": "textDocument/completion",
                "params": {
                    "textDocument": { "uri": "file:///t.rsx" },
                    "position": { "line": 2, "character": 5 }
                }
            }),
        ]);

        let items = replies.iter().find(|m| m["id"] == json!(20)).unwrap()["result"]
            .as_array()
            .unwrap()
            .clone();

        let card = items
            .iter()
            .find(|i| i["label"] == json!("Card"))
            .expect("no Card");
        assert_eq!(card["kind"], json!(15)); // Snippet
        assert!(card["sortText"].as_str().unwrap().starts_with('0'));
    }

    #[test]
    fn reports_a_bad_prop_as_a_diagnostic() {
        // Only code generation can find this, so it proves the store runs it.
        let replies = exchange(&[json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": "file:///t.rsx", "version": 1,
                "text": "pub fn Card(title: &str) -> Element { <p>{title}</p> }\npub fn Page() -> Element { <Card titel=\"x\" /> }" } }
        })]);

        let diagnostics = replies[0]["params"]["diagnostics"].as_array().unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0]["message"]
                .as_str()
                .unwrap()
                .contains("has no parameter `titel`"),
            "{:?}",
            diagnostics[0]["message"]
        );
    }

    #[test]
    fn stops_on_exit() {
        let replies = exchange(&[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "shutdown", "params": {} }),
            json!({ "jsonrpc": "2.0", "method": "exit" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {} }),
        ]);

        // The request after `exit` is never answered.
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["id"], json!(1));
    }
}
