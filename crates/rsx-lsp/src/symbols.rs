//! Document symbols: the components a file declares, and their parameters.
//!
//! Two questions share this module. The outline is about one file and nests
//! parameters under the component that takes them. The workspace search is
//! about every file and is deliberately flat: a picker shows one line per
//! match, and a parameter is not something anybody searches for by name.

use std::collections::HashSet;

use crate::document::Document;
use crate::protocol::{DocumentSymbol, Location, SymbolInformation, SymbolKind};
use crate::workspace::Index;
use crate::DocumentStore;

/// Builds the outline for a document, or `None` when it does not parse.
pub fn handle(document: &Document) -> Option<Vec<DocumentSymbol>> {
    let file = document.ast.as_ref()?;

    Some(
        file.components()
            .map(|component| DocumentSymbol {
                name: component.name.clone(),
                detail: Some(format!(
                    "({}) -> Element",
                    component
                        .params
                        .iter()
                        .map(|p| format!("{}: {}", p.name, p.ty))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                kind: SymbolKind::Function,
                range: document.span_range(component.span),
                selection_range: document.span_range(component.name_span),
                children: component
                    .params
                    .iter()
                    .map(|param| DocumentSymbol {
                        name: param.name.clone(),
                        detail: Some(param.ty.clone()),
                        kind: SymbolKind::Variable,
                        range: document.span_range(param.span),
                        selection_range: document.span_range(param.span),
                        children: Vec::new(),
                    })
                    .collect(),
            })
            .collect(),
    )
}

/// Every component in the project whose name matches `query`.
///
/// The match is a case-insensitive substring rather than a prefix, because a
/// component is usually remembered by its noun and named with its context in
/// front: `AdminUserCard` is what somebody typing "card" is looking for.
///
/// An empty query asks for everything, which is what a client sends to fill a
/// picker before the author has typed.
pub fn workspace(query: &str, store: &DocumentStore, index: &Index) -> Vec<SymbolInformation> {
    let needle = query.to_lowercase();
    let open: HashSet<String> = store.documents().map(|doc| doc.uri.clone()).collect();

    let mut found: Vec<SymbolInformation> = store
        .documents()
        .chain(index.closed_files(&open))
        .flat_map(|document| {
            document
                .components()
                .map(move |component| SymbolInformation {
                    name: component.name.clone(),
                    kind: SymbolKind::Function,
                    location: Location {
                        uri: document.uri.clone(),
                        range: document.span_range(component.name_span),
                    },
                    container_name: file_name(&document.uri),
                })
        })
        .filter(|symbol| needle.is_empty() || symbol.name.to_lowercase().contains(&needle))
        .collect();

    found.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.location.uri.cmp(&b.location.uri))
    });
    found
}

/// The last segment of a URI, which is the part worth showing in a picker.
fn file_name(uri: &str) -> Option<String> {
    uri.rsplit('/').next().map(ToString::to_string)
}
