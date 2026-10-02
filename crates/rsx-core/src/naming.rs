//! Name conversions shared by the parser, generator and registry.

/// Prefix for lifetimes the generator invents for elided references.
///
/// It is deliberately unpronounceable so it cannot collide with a lifetime
/// written in a template.
pub const SYNTH_LIFETIME_PREFIX: &str = "'__rsx_a";

/// Every Rust keyword, including the ones reserved for future use.
///
/// A component named `Loop` would otherwise generate `fn loop`, so names that
/// land on a keyword are emitted as raw identifiers instead.
const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where",
    "while", "async", "await", "abstract", "become", "box", "do", "final", "macro", "override",
    "priv", "try", "typeof", "unsized", "virtual", "yield",
];

/// `self`, `Self` and `_` cannot be raw identifiers at all.
const UNESCAPABLE: &[&str] = &["self", "Self", "_"];

/// Returns whether `name` needs `r#` to be used as an identifier.
pub fn is_keyword(name: &str) -> bool {
    KEYWORDS.contains(&name)
}

/// Wraps a generated name in `r#` when it collides with a keyword.
pub fn escape_ident(name: &str) -> String {
    if is_keyword(name) {
        format!("r#{name}")
    } else if UNESCAPABLE.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// Converts a component name to the name of its writer function.
///
/// `Card` becomes `card` and `BlogPost` becomes `blog_post`. Runs of capitals
/// stay together, so `HTMLPage` becomes `html_page` rather than `h_t_m_l_page`.
/// A result that collides with a keyword comes back as a raw identifier.
pub fn to_snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len() + 4);

    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            out.push('_');
            continue;
        }
        if ch.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            let next = chars.get(i + 1).copied();
            // A capital starts a new word when it follows a lowercase letter or
            // digit, or when it is the last capital of a run before a lowercase
            // one: the `P` in `HTMLPage`.
            let after_lower = prev.is_lowercase() || prev.is_numeric();
            let ends_acronym = prev.is_uppercase() && next.is_some_and(|n| n.is_lowercase());
            if after_lower || ends_acronym {
                out.push('_');
            }
        }
        out.extend(ch.to_lowercase());
    }

    escape_ident(&out)
}

/// Converts a file stem to a Rust module name.
pub fn to_module_name(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    for ch in stem.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|c| c.is_numeric()) {
        out.insert(0, '_');
    }
    escape_ident(&out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_pascal_case() {
        assert_eq!(to_snake_case("Card"), "card");
        assert_eq!(to_snake_case("BlogPost"), "blog_post");
        assert_eq!(to_snake_case("A"), "a");
    }

    #[test]
    fn keeps_acronyms_together() {
        assert_eq!(to_snake_case("HTMLPage"), "html_page");
        assert_eq!(to_snake_case("HTML"), "html");
        assert_eq!(to_snake_case("ParseURL"), "parse_url");
    }

    #[test]
    fn handles_digits_and_underscores() {
        assert_eq!(to_snake_case("Heading2"), "heading2");
        assert_eq!(to_snake_case("Grid2Col"), "grid2_col");
        assert_eq!(to_snake_case("My_Card"), "my_card");
    }

    #[test]
    fn sanitises_module_names() {
        assert_eq!(to_module_name("blog-post"), "blog_post");
        assert_eq!(to_module_name("404"), "_404");
        assert_eq!(to_module_name("match"), "r#match");
    }

    #[test]
    fn escapes_names_that_land_on_keywords() {
        assert_eq!(to_snake_case("Loop"), "r#loop");
        assert_eq!(to_snake_case("Match"), "r#match");
        assert_eq!(to_snake_case("Card"), "card");
    }
}
