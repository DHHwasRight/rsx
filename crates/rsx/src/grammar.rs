//! The TextMate grammar for `.rsx` files.
//!
//! The same grammar the VS Code extension ships (`editors/vscode/syntaxes`), here so that
//! anything rendering rsx source can color it the way the editor does: a documentation site, a
//! code review tool, a highlighter such as `giallo` or `syntect`. It embeds the Rust, CSS, and
//! JavaScript grammars by scope name (`source.rust`, `source.css`, `source.js`), so the host has
//! to provide those too.

/// The grammar's scope name.
pub const SCOPE_NAME: &str = "source.rsx";

/// The grammar, as TextMate JSON.
pub const TEXTMATE: &str = include_str!("../grammar/rsx.tmLanguage.json");
