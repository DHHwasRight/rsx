//! The slice of the Language Server Protocol this server speaks.
//!
//! Only the messages the server actually handles are modelled. A full protocol
//! crate would be a large dependency for a handful of request types, and the
//! shapes below are stable parts of the specification.

use serde::{Deserialize, Serialize};

/// A position in a document. Both fields are 0-based, and `character` counts
/// UTF-16 code units, as the protocol requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Position { line, character }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Range { start, end }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// Defines a protocol enum that serialises as the number the specification
/// gives it.
///
/// `serde`'s default for a fieldless enum is the variant *name*, and a client
/// rejects `"Snippet"` where the protocol says `15`. Spelling the numbers out
/// here keeps that mistake impossible without taking on `serde_repr`.
macro_rules! numeric_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($variant:ident = $value:expr),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant = $value),*
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u8(*self as u8)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                match u8::deserialize(deserializer)? {
                    $($value => Ok($name::$variant),)*
                    other => Err(serde::de::Error::custom(format!(
                        concat!("unknown ", stringify!($name), " value: {}"), other
                    ))),
                }
            }
        }
    };
}

numeric_enum! {
    DiagnosticSeverity {
        Error = 1,
        Warning = 2,
        Information = 3,
        Hint = 4,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: DiagnosticSeverity,
    pub message: String,
    /// Always `"rsx"`, so an editor can tell these from `rustc`'s.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkupContent {
    pub kind: String,
    pub value: String,
}

impl MarkupContent {
    pub fn markdown(value: impl Into<String>) -> Self {
        MarkupContent {
            kind: "markdown".to_string(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hover {
    pub contents: MarkupContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

numeric_enum! {
    CompletionItemKind {
        Function = 3,
        Field = 5,
        Property = 10,
        Snippet = 15,
    }
}

numeric_enum! {
    InsertTextFormat {
        PlainText = 1,
        Snippet = 2,
    }
}

/// A replacement of a range of the document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: Range,
    #[serde(rename = "newText")]
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionItemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<MarkupContent>,
    #[serde(rename = "insertText", skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    /// Replaces an explicit range rather than inserting at the cursor.
    ///
    /// Needed whenever the text already typed is part of what the completion
    /// replaces: inserting at the cursor instead would leave that prefix
    /// behind, turning `user.na` into `user.naname`.
    #[serde(rename = "textEdit", skip_serializing_if = "Option::is_none")]
    pub text_edit: Option<TextEdit>,
    #[serde(rename = "insertTextFormat")]
    pub insert_text_format: InsertTextFormat,
    /// Orders the list: components before HTML tags.
    #[serde(rename = "sortText", skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
}

numeric_enum! {
    SymbolKind {
        Function = 12,
        Variable = 13,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: SymbolKind,
    pub range: Range,
    #[serde(rename = "selectionRange")]
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub children: Vec<DocumentSymbol>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_serialise_as_the_numbers_the_protocol_defines() {
        let json = serde_json::to_string(&CompletionItemKind::Snippet).unwrap();
        assert_eq!(json, "15");
        assert_eq!(serde_json::to_string(&SymbolKind::Function).unwrap(), "12");
        assert_eq!(
            serde_json::to_string(&DiagnosticSeverity::Error).unwrap(),
            "1"
        );
        assert_eq!(
            serde_json::to_string(&InsertTextFormat::Snippet).unwrap(),
            "2"
        );
    }

    #[test]
    fn enums_round_trip() {
        let kind: CompletionItemKind = serde_json::from_str("5").unwrap();
        assert_eq!(kind, CompletionItemKind::Field);
        assert!(serde_json::from_str::<CompletionItemKind>("99").is_err());
    }

    #[test]
    fn optional_fields_are_omitted_rather_than_sent_as_null() {
        let hover = Hover {
            contents: MarkupContent::markdown("hi"),
            range: None,
        };
        let json = serde_json::to_string(&hover).unwrap();
        assert!(!json.contains("range"), "{json}");
    }
}
