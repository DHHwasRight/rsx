//! The `.rsx` template compiler: parser, syntax tree, and Rust code generator.
//!
//! This crate has no opinion on how it is driven. The proc macro in
//! `rsx-macros` and the `rsx` binary both call the same entry points, and so
//! can anything else:
//!
//! ```
//! use rsx_core::{compile_source, Options};
//!
//! let rust = compile_source(
//!     "pub fn Hello(name: &str) -> Element { <p>Hi, {name}!</p> }",
//!     &Options::default(),
//! )?;
//! assert!(rust.contains("pub fn hello"));
//! # Ok::<(), rsx_core::Error>(())
//! ```

pub mod ast;
pub mod codegen;
pub mod error;
pub mod naming;
pub mod parser;
pub mod project;
pub mod registry;
mod scanner;
pub mod source_map;
pub mod span;

pub use ast::File;
pub use codegen::{generate, generate_at, generate_with_map, Options};
pub use error::{Error, Result};
pub use parser::parse;
pub use project::{
    compile_dir, compile_file, compile_source, Compilation, CompiledFile, EXTENSION,
};
pub use registry::{Registry, Scope};
pub use source_map::{Mapping, SourceMap};
pub use span::{Pos, Span};
