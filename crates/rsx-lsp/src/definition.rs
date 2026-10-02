//! Go to definition.

use crate::document::Document;
use crate::find_node::{self, Found};
use crate::protocol::{Location, Position};
use crate::DocumentStore;

/// Resolves the component under the cursor to where it is declared.
///
/// HTML elements and everything else resolve to nothing: there is no
/// declaration to jump to, and offering one would be a lie.
pub fn handle(document: &Document, position: Position, store: &DocumentStore) -> Option<Location> {
    let file = document.ast.as_ref()?;
    let offset = document.offset(position);

    let name = match find_node::at(file, offset)? {
        Found::Call(call) => call.name().to_string(),
        _ => return None,
    };

    let (declaring, component) = store.find_component(&name, &document.uri)?;
    Some(Location {
        uri: declaring.uri.clone(),
        range: declaring.span_range(component.name_span),
    })
}
