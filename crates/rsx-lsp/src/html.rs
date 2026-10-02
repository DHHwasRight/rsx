//! The HTML vocabulary the language server offers.
//!
//! Enough of it to be useful in completion and hover without pulling in a
//! specification dump: the elements and attributes an author reaches for.

/// Elements offered in tag completion, with a one-line description.
pub const ELEMENTS: &[(&str, &str)] = &[
    ("a", "A hyperlink."),
    (
        "abbr",
        "An abbreviation, with an optional expansion in `title`.",
    ),
    (
        "address",
        "Contact information for its nearest article or body.",
    ),
    ("article", "A self-contained composition."),
    ("aside", "Content tangential to what surrounds it."),
    ("audio", "Embedded sound."),
    ("b", "Text drawn attention to without extra importance."),
    ("blockquote", "A quotation from another source."),
    ("body", "The document's content."),
    ("br", "A line break."),
    ("button", "A clickable control."),
    ("canvas", "A bitmap the page can draw into."),
    ("caption", "A table's title."),
    ("code", "A fragment of computer code."),
    ("col", "A column in a table."),
    ("colgroup", "A group of table columns."),
    ("datalist", "A set of suggested values for an input."),
    ("dd", "The description in a description list."),
    ("details", "A disclosure widget."),
    ("div", "A generic block container."),
    ("dl", "A description list."),
    ("dt", "The term in a description list."),
    ("em", "Stressed emphasis."),
    ("embed", "An external application or interactive content."),
    ("fieldset", "A group of form controls."),
    ("figcaption", "A caption for a figure."),
    ("figure", "Self-contained content with an optional caption."),
    ("footer", "A footer for its nearest sectioning content."),
    (
        "form",
        "A section with interactive controls for submission.",
    ),
    ("h1", "A top-level heading."),
    ("h2", "A second-level heading."),
    ("h3", "A third-level heading."),
    ("h4", "A fourth-level heading."),
    ("h5", "A fifth-level heading."),
    ("h6", "A sixth-level heading."),
    ("head", "Metadata about the document."),
    ("header", "Introductory content for its nearest section."),
    ("hr", "A thematic break."),
    ("html", "The document root."),
    ("i", "Text in an alternate voice or mood."),
    ("iframe", "A nested browsing context."),
    ("img", "An image."),
    ("input", "A form control."),
    ("label", "A caption for a form control."),
    ("legend", "A caption for a fieldset."),
    ("li", "A list item."),
    ("link", "A link to an external resource."),
    ("main", "The dominant content of the document."),
    ("mark", "Text marked for reference."),
    ("meta", "Metadata that other elements cannot express."),
    ("nav", "A section with navigation links."),
    ("noscript", "Content for when scripting is unavailable."),
    ("ol", "An ordered list."),
    ("optgroup", "A group of options."),
    ("option", "An option in a select or datalist."),
    ("p", "A paragraph."),
    ("picture", "An image with multiple sources."),
    ("pre", "Preformatted text."),
    ("progress", "A progress indicator."),
    ("script", "Executable code. Its content is character data."),
    ("section", "A generic section of the document."),
    ("select", "A control for choosing among options."),
    ("small", "Side comments such as fine print."),
    ("source", "A media or image source."),
    ("span", "A generic inline container."),
    ("strong", "Strong importance."),
    ("style", "Style information. Its content is character data."),
    ("sub", "Subscript."),
    ("summary", "A summary for a details element."),
    ("sup", "Superscript."),
    ("svg", "An SVG fragment."),
    ("table", "Tabular data."),
    ("tbody", "A block of table rows."),
    ("td", "A table cell."),
    ("textarea", "A multi-line text control."),
    ("tfoot", "A table's footer rows."),
    ("th", "A table header cell."),
    ("thead", "A table's header rows."),
    ("time", "A machine-readable date or time."),
    ("title", "The document's title."),
    ("tr", "A table row."),
    ("ul", "An unordered list."),
    ("video", "Embedded video."),
    ("wbr", "A line break opportunity."),
];

/// Attributes offered on any element.
pub const GLOBAL_ATTRIBUTES: &[&str] = &[
    "accesskey",
    "autocapitalize",
    "class",
    "contenteditable",
    "dir",
    "draggable",
    "hidden",
    "id",
    "inert",
    "lang",
    "role",
    "slot",
    "spellcheck",
    "style",
    "tabindex",
    "title",
    "translate",
];

/// Attributes worth offering on particular elements.
pub const ELEMENT_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("a", &["href", "target", "rel", "download", "hreflang"]),
    ("audio", &["src", "controls", "autoplay", "loop", "muted"]),
    ("button", &["type", "disabled", "name", "value", "form"]),
    (
        "form",
        &["action", "method", "enctype", "novalidate", "target"],
    ),
    (
        "img",
        &["src", "alt", "width", "height", "loading", "srcset"],
    ),
    (
        "input",
        &[
            "type",
            "name",
            "value",
            "placeholder",
            "required",
            "disabled",
            "checked",
            "readonly",
            "min",
            "max",
            "step",
            "pattern",
            "autocomplete",
        ],
    ),
    ("label", &["for"]),
    ("link", &["rel", "href", "type", "media", "as"]),
    ("meta", &["name", "content", "charset", "property"]),
    ("option", &["value", "selected", "disabled"]),
    ("script", &["src", "type", "defer", "async", "nonce"]),
    ("select", &["name", "required", "disabled", "multiple"]),
    ("source", &["src", "srcset", "type", "media"]),
    ("td", &["colspan", "rowspan", "headers"]),
    (
        "textarea",
        &["name", "rows", "cols", "placeholder", "required"],
    ),
    ("th", &["colspan", "rowspan", "scope"]),
    ("video", &["src", "controls", "poster", "width", "height"]),
];

/// The description for an element, if it is one this server knows.
pub fn describe(tag: &str) -> Option<&'static str> {
    ELEMENTS
        .iter()
        .find(|(name, _)| *name == tag)
        .map(|(_, description)| *description)
}

/// Attribute names to offer inside `<tag …>`.
pub fn attributes_for(tag: &str) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = ELEMENT_ATTRIBUTES
        .iter()
        .find(|(name, _)| *name == tag)
        .map(|(_, attrs)| attrs.to_vec())
        .unwrap_or_default();
    names.extend_from_slice(GLOBAL_ATTRIBUTES);
    names.sort_unstable();
    names.dedup();
    names
}
