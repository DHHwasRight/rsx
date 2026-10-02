//! Document symbols: the components a file declares, and their parameters.

use crate::document::Document;
use crate::protocol::{DocumentSymbol, SymbolKind};

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
