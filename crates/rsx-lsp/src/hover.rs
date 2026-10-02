//! Hover text.

use rsx_core::ast::{Component, Param};

use crate::document::Document;
use crate::find_node::{self, Found};
use crate::html;
use crate::protocol::{Hover, MarkupContent, Position};
use crate::DocumentStore;

/// Builds hover text for the position, or `None` when there is nothing to say.
pub fn handle(document: &Document, position: Position, store: &DocumentStore) -> Option<Hover> {
    let file = document.ast.as_ref()?;
    let offset = document.offset(position);
    let found = find_node::at(file, offset)?;

    let value = match found {
        Found::ComponentName(component) | Found::Component(component) => {
            signature_markdown(component)
        }
        Found::Param(_, param) => param_markdown(param),
        Found::Call(call) => {
            let name = call.name();
            match store.find_component(name, &document.uri) {
                Some((_, component)) => signature_markdown(component),
                // A component from handwritten Rust or another crate is not in
                // any open document; say what is known rather than nothing.
                None => format!(
                    "```rust\n{}\n```\n\nComponent (not found in any open file)",
                    call.path
                ),
            }
        }
        Found::Element(element) => element_markdown(&element.tag),
        Found::Attribute(attr) => {
            let name = attr.name()?;
            format!("```html\n{name}\n```\n\nAttribute")
        }
        Found::Expr(expr) => format!("```rust\n{}\n```\n\nInterpolated expression", expr.source),
    };

    Some(Hover {
        contents: MarkupContent::markdown(value),
        range: Some(document.span_range(found.span())),
    })
}

/// A component rendered the way its declaration reads.
pub fn signature_markdown(component: &Component) -> String {
    let params = component
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect::<Vec<_>>()
        .join(", ");
    let generics = if component.generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", component.generics)
    };

    let mut out = format!(
        "```rust\nfn {}{generics}({params}) -> Element\n```",
        component.name
    );

    if component.params.iter().any(Param::is_children) {
        out.push_str("\n\nAccepts children.");
    }
    if let Some(doc) = doc_comment(&component.attrs) {
        out.push_str("\n\n");
        out.push_str(&doc);
    }
    out
}

fn param_markdown(param: &Param) -> String {
    let mut out = format!("```rust\n{}: {}\n```", param.name, param.ty);
    if param.is_children() {
        out.push_str("\n\nReceives the markup written inside the component call.");
    }
    out
}

fn element_markdown(tag: &str) -> String {
    let mut out = format!("```html\n<{tag}>\n```");
    if let Some(description) = html::describe(tag) {
        out.push_str("\n\n");
        out.push_str(description);
    }
    if rsx_core::ast::is_void_element(tag) {
        out.push_str("\n\nA void element: it takes no closing tag.");
    }
    out
}

/// Pulls the text out of `///` lines above a component.
fn doc_comment(attrs: &str) -> Option<String> {
    let lines: Vec<&str> = attrs
        .lines()
        .filter_map(|line| line.trim().strip_prefix("///"))
        .map(str::trim)
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}
