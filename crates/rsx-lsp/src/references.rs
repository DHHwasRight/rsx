//! Find references: every call to the component under the cursor.
//!
//! The question is asked from two places — on a component's declaration, and
//! on one of its calls — and both mean the same thing, so both resolve to a
//! name first and the search is the same afterwards.
//!
//! Only the template language is searched. A component is an ordinary Rust
//! function once generated, so it can also be called from `.rs`; finding those
//! is `rust-analyzer`'s job over the generated code, and claiming to find them
//! here would mean answering with a list that is quietly incomplete.

use std::collections::HashSet;

use rsx_core::ast::{Call, Node};

use crate::document::Document;
use crate::find_node::{self, Found};
use crate::protocol::{Location, Position, Range};
use crate::workspace::Index;
use crate::DocumentStore;

/// Every place the component under the cursor is called.
///
/// `include_declaration` follows the request's own flag: an editor that asks
/// for the declaration as well gets the `pub fn` line, and one that does not
/// gets call sites only.
pub fn handle(
    document: &Document,
    position: Position,
    store: &DocumentStore,
    index: &Index,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let file = document.ast.as_ref()?;
    let offset = document.offset(position);

    // A declaration and a call are two ways of pointing at one name. An
    // attribute or a bare element is not a component at all, and answering
    // with an empty list there is more honest than answering with the
    // enclosing component's references.
    let name = match find_node::at(file, offset)? {
        Found::ComponentName(component) => component.name.clone(),
        Found::Call(call) => call.name().to_string(),
        _ => return None,
    };

    let open: HashSet<String> = store.documents().map(|doc| doc.uri.clone()).collect();
    let mut found = Vec::new();
    for doc in store.documents().chain(index.closed_files(&open)) {
        collect(doc, &name, include_declaration, &mut found);
    }

    // A deterministic order, so a list the author reads twice reads the same
    // way twice. `HashMap` iteration in the store does not give one.
    found.sort_by(|a, b| {
        a.uri.cmp(&b.uri).then_with(|| {
            (a.range.start.line, a.range.start.character)
                .cmp(&(b.range.start.line, b.range.start.character))
        })
    });
    found.dedup();
    Some(found)
}

fn collect(document: &Document, name: &str, include_declaration: bool, out: &mut Vec<Location>) {
    let Some(file) = &document.ast else {
        return;
    };
    for component in file.components() {
        if include_declaration && component.name == name {
            out.push(Location {
                uri: document.uri.clone(),
                range: document.span_range(component.name_span),
            });
        }
        each_call(&component.body, &mut |call| {
            // The final segment, so `<Blog::Card>` and `<Card>` are the same
            // component — which is what the registry already decided.
            if call.name() == name {
                out.push(Location {
                    uri: document.uri.clone(),
                    range: name_range(document, call),
                });
            }
        });
    }
}

/// The range covering the path a call was written with.
///
/// [`Call`] keeps the span of the whole element, `<Card>…</Card>`. Highlighting
/// that would select the children too, and the name is always the text right
/// after `<`, so the shorter range is derived rather than stored.
fn name_range(document: &Document, call: &Call) -> Range {
    let start = call.span.from.offset + 1;
    Range::new(
        document.position(start),
        document.position(start + call.path.len()),
    )
}

/// Visits every component call in a tree, however deeply nested.
fn each_call(nodes: &[Node], f: &mut impl FnMut(&Call)) {
    for node in nodes {
        match node {
            Node::Call(call) => {
                f(call);
                each_call(&call.children, f);
            }
            Node::Element(element) => each_call(&element.children, f),
            Node::Fragment(fragment) => each_call(&fragment.children, f),
            Node::If(node) => {
                each_call(&node.then_body, f);
                for branch in &node.else_ifs {
                    each_call(&branch.body, f);
                }
                if let Some(body) = &node.else_body {
                    each_call(body, f);
                }
            }
            Node::For(node) => each_call(&node.body, f),
            Node::While(node) => each_call(&node.body, f),
            Node::Match(node) => {
                for arm in &node.arms {
                    each_call(&arm.body, f);
                }
            }
            Node::Expr(_)
            | Node::RawExpr(_)
            | Node::Text(_)
            | Node::Doctype(_)
            | Node::Comment(_)
            | Node::Stmt(_) => {}
        }
    }
}
