//! A language server for `.rsx` templates.
//!
//! The analysis is separated from the transport: [`DocumentStore`] and the
//! feature modules are ordinary functions over a parsed file, so they are
//! tested directly, and [`server::run`] is a thin stdio loop on top.
//!
//! # Scope
//!
//! Everything here is about the template language — components, elements,
//! attributes, children. Questions about Rust *types* are deliberately not
//! answered: what `user.posts` is, whether a method exists, what `String`
//! means. `rust-analyzer` answers those for the generated code, and competing
//! with it from here would mean reimplementing a type checker.
//!
//! ```
//! use rsx_lsp::{DocumentStore, protocol::Position};
//!
//! let mut store = DocumentStore::new();
//! let doc = store.open("file:///card.rsx", "pub fn Card() -> Element { <p>hi</p> }", 1);
//! assert!(doc.diagnostics.is_empty());
//! ```

pub mod bridge;
pub mod completion;
pub mod definition;
pub mod document;
pub mod find_node;
pub mod hover;
pub mod html;
pub mod protocol;
pub mod rust;
pub mod rust_analyzer;
pub mod server;
pub mod symbols;

pub use document::{Document, DocumentStore};
pub use server::run;
