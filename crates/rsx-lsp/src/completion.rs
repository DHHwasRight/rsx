//! Completion after `<` and inside a tag.

use rsx_core::ast::{is_void_element, Component};

use crate::document::Document;
use crate::html;
use crate::protocol::{
    CompletionItem, CompletionItemKind, InsertTextFormat, MarkupContent, Position,
};
use crate::DocumentStore;

/// Components sort before HTML tags, which is almost always what is wanted in
/// a file that defines components.
const COMPONENT_SORT: &str = "0";
const HTML_SORT: &str = "1";

/// What the text before the cursor is asking for.
enum Context {
    /// Just after `<`, optionally with a partial name typed.
    TagName,
    /// Inside the attribute area of `<tag …`.
    Attributes { tag: String },
    /// Nothing to complete.
    None,
}

/// Builds the completion list for a position.
pub fn handle(
    document: &Document,
    position: Position,
    store: &DocumentStore,
) -> Vec<CompletionItem> {
    match context(document, position) {
        Context::TagName => tag_names(document, store),
        Context::Attributes { tag } => attributes(&tag, document, store),
        Context::None => Vec::new(),
    }
}

/// Reads backwards from the cursor to work out what is being typed.
///
/// This is deliberately textual rather than tree-based: completion is asked
/// for while the text is half-written, which is exactly when it does not
/// parse and there is no tree to consult.
fn context(document: &Document, position: Position) -> Context {
    let offset = document.offset(position);
    let before = &document.text[..offset];

    let open = match before.rfind('<') {
        Some(index) => index,
        None => return Context::None,
    };

    // A `>` after the last `<` means that tag is already finished.
    if before[open..].contains('>') {
        return Context::None;
    }
    // `</` is a closing tag, which the editor can complete on its own.
    if before[open..].starts_with("</") {
        return Context::None;
    }

    let after_open = &before[open + 1..];
    let name_len = after_open
        .char_indices()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ':')
        .map(|(i, c)| i + c.len_utf8())
        .last()
        .unwrap_or(0);

    if name_len == after_open.len() {
        // Still inside the tag name.
        return Context::TagName;
    }

    // Past the name and into the attributes, as long as nothing has closed
    // the tag and no string literal is open.
    let rest = &after_open[name_len..];
    if rest.matches('"').count() % 2 == 1 {
        return Context::None;
    }

    Context::Attributes {
        tag: after_open[..name_len].to_string(),
    }
}

fn tag_names(document: &Document, store: &DocumentStore) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (_, component) in components_in_scope(document, store) {
        items.push(component_item(component));
    }

    for (tag, description) in html::ELEMENTS {
        let insert = if is_void_element(tag) {
            format!("{tag} $0/>")
        } else {
            format!("{tag}>$0</{tag}>")
        };
        items.push(CompletionItem {
            label: (*tag).to_string(),
            kind: CompletionItemKind::Property,
            detail: Some("HTML element".to_string()),
            documentation: Some(MarkupContent::markdown(*description)),
            insert_text: Some(insert),
            insert_text_format: InsertTextFormat::Snippet,
            text_edit: None,
            sort_text: Some(format!("{HTML_SORT}{tag}")),
        });
    }

    items
}

/// A component as a snippet that fills in its parameters.
fn component_item(component: &Component) -> CompletionItem {
    let value_params: Vec<&str> = component
        .params
        .iter()
        .filter(|p| !p.is_children())
        .map(|p| p.name.as_str())
        .collect();
    let takes_children = component.params.iter().any(|p| p.is_children());

    let mut insert = component.name.clone();
    for (index, name) in value_params.iter().enumerate() {
        insert.push_str(&format!(" {name}={{${}}}", index + 1));
    }
    if takes_children {
        let tab = value_params.len() + 1;
        insert.push_str(&format!(">${tab}</{}>", component.name));
    } else {
        insert.push_str(" />$0");
    }

    let detail = component
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect::<Vec<_>>()
        .join(", ");

    CompletionItem {
        label: component.name.clone(),
        kind: CompletionItemKind::Snippet,
        detail: Some(format!("fn {}({detail}) -> Element", component.name)),
        documentation: Some(MarkupContent::markdown(crate::hover::signature_markdown(
            component,
        ))),
        insert_text: Some(insert),
        insert_text_format: InsertTextFormat::Snippet,
        text_edit: None,
        sort_text: Some(format!("{COMPONENT_SORT}{}", component.name)),
    }
}

fn attributes(tag: &str, document: &Document, store: &DocumentStore) -> Vec<CompletionItem> {
    let is_component = tag
        .rsplit("::")
        .next()
        .and_then(|s| s.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase());

    if is_component {
        let name = tag.rsplit("::").next().unwrap_or(tag);
        let Some((_, component)) = store.find_component(name, &document.uri) else {
            return Vec::new();
        };
        return component
            .params
            .iter()
            .filter(|p| !p.is_children())
            .map(|param| CompletionItem {
                label: param.name.clone(),
                kind: CompletionItemKind::Field,
                detail: Some(param.ty.clone()),
                documentation: None,
                insert_text: Some(format!("{}={{$1}}", param.name)),
                insert_text_format: InsertTextFormat::Snippet,
                text_edit: None,
                sort_text: Some(format!("{COMPONENT_SORT}{}", param.name)),
            })
            .collect();
    }

    html::attributes_for(tag)
        .into_iter()
        .map(|name| CompletionItem {
            label: name.to_string(),
            kind: CompletionItemKind::Property,
            detail: Some("HTML attribute".to_string()),
            documentation: None,
            insert_text: Some(format!("{name}=\"$1\"")),
            insert_text_format: InsertTextFormat::Snippet,
            text_edit: None,
            sort_text: Some(format!("{HTML_SORT}{name}")),
        })
        .collect()
}

/// Components this document can offer: its own first, then every other open
/// document's, without duplicating a name.
fn components_in_scope<'a>(
    document: &'a Document,
    store: &'a DocumentStore,
) -> Vec<(&'a Document, &'a Component)> {
    let mut seen = Vec::new();
    let mut out: Vec<(&Document, &Component)> = Vec::new();

    // The last tree that parsed, because the file being completed in is
    // usually the file that does not parse right now.
    for component in document.known_components() {
        seen.push(component.name.clone());
        out.push((document, component));
    }
    for (doc, component) in store.all_components() {
        if doc.uri == document.uri || seen.contains(&component.name) {
            continue;
        }
        seen.push(component.name.clone());
        out.push((doc, component));
    }

    out.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
    out
}
