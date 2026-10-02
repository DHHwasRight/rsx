//! Syntax cases ported from the Crystal `crx` specs this descends from.
//!
//! Each group is one of that suite's `describe` blocks, translated to Rust
//! syntax. They are the cases that were worth a regression test there, so they
//! are worth one here.

use rsx_core::{compile_source, Options};

/// Compiles a component body and returns the generated Rust.
fn generate(body: &str) -> String {
    let source = format!("pub fn Test() -> Element {{\n{body}\n}}");
    compile_source(&source, &Options::default()).unwrap_or_else(|e| panic!("{}", e.render(&source)))
}

/// Compiles a whole file and returns the generated Rust.
fn generate_file(source: &str) -> String {
    compile_source(source, &Options::default()).unwrap_or_else(|e| panic!("{}", e.render(source)))
}

fn error(source: &str) -> String {
    match compile_source(source, &Options::default()) {
        Ok(code) => panic!("expected an error, got:\n{code}"),
        Err(e) => e.message,
    }
}

// --------------------------------------------------- attributes across lines

// An element carrying several attributes is unreadable on one line, and every
// HTML author wraps them.
mod attributes_across_lines {
    use super::*;

    #[test]
    fn parses_an_element_whose_attributes_are_wrapped() {
        let code =
            generate("<div class=\"card\"\n     id=\"one\"\n     data-role=\"panel\"></div>");
        assert!(code.contains(r#"class=\"card\""#), "{code}");
        assert!(code.contains(r#"id=\"one\""#), "{code}");
        assert!(code.contains(r#"data-role=\"panel\""#), "{code}");
    }

    #[test]
    fn parses_wrapped_expression_attributes() {
        let code = generate_file(
            "pub fn Test(source: &str, name: &str) -> Element {\n\
             <div\n  data-src={source}\n  data-name={name}></div>\n}",
        );
        assert!(code.contains("__w.attr(&(source))"), "{code}");
        assert!(code.contains("__w.attr(&(name))"), "{code}");
    }

    #[test]
    fn parses_a_wrapped_self_closing_element() {
        let code = generate("<img\n  src=\"a.png\"\n  alt=\"A\" />");
        assert!(code.contains(r#"alt=\"A\""#), "{code}");
    }

    #[test]
    fn parses_a_wrapped_component_call() {
        let code = generate("<Card\n  title=\"One\"\n  icon=\"i\" />");
        assert!(
            code.contains(r#"Card { title: "One", icon: "i" }"#),
            "{code}"
        );
    }

    #[test]
    fn still_parses_attributes_on_one_line() {
        let code = generate(r#"<div class="a" id="b"></div>"#);
        assert!(code.contains(r#"<div class=\"a\" id=\"b\">"#), "{code}");
    }

    #[test]
    fn still_ends_the_tag_where_it_should() {
        let code = generate("<div\n  class=\"a\">text</div>");
        assert!(code.contains("text</div>"), "{code}");
    }
}

// ------------------------------------------------------- component signatures

mod component_signatures {
    use super::*;

    #[test]
    fn parses_a_signature_wrapped_across_lines_with_a_trailing_comma() {
        let code = generate_file(
            "pub fn Test(\n    title: &str,\n    count: usize,\n) -> Element {\n    <p>{title}{count}</p>\n}",
        );
        assert!(code.contains("title: &str"), "{code}");
        assert!(code.contains("count: usize"), "{code}");
    }

    #[test]
    fn parses_a_wrapped_signature_without_a_trailing_comma() {
        let code = generate_file(
            "pub fn Test(\n    title: &str,\n    count: usize\n) -> Element {\n    <p>{title}{count}</p>\n}",
        );
        assert!(code.contains("count: usize"), "{code}");
    }

    #[test]
    fn parses_a_trailing_comma_on_one_line() {
        let code = generate_file("pub fn Test(title: &str,) -> Element { <p>{title}</p> }");
        assert!(code.contains("title: &str"), "{code}");
    }

    #[test]
    fn parses_a_where_clause_on_its_own_line() {
        let code = generate_file(
            "pub fn Test<T>(value: T) -> Element\nwhere\n    T: std::fmt::Display,\n{\n    <p>{value}</p>\n}",
        );
        assert!(code.contains("where T: std::fmt::Display"), "{code}");
    }
}

// ------------------------------------------------------ namespaced component calls

mod namespaced_component_calls {
    use super::*;

    #[test]
    fn parses_a_call_qualified_with_a_path() {
        let code = generate("<shared::Card title=\"a\"></shared::Card>");
        assert!(code.contains("shared::Card { title: \"a\" }"), "{code}");
    }

    #[test]
    fn parses_a_self_closing_namespaced_call() {
        let code = generate("<shared::Card title=\"a\" />");
        assert!(code.contains("shared::Card {"), "{code}");
    }

    #[test]
    fn parses_more_than_one_level() {
        let code = generate("<views::shared::Card title=\"a\" />");
        assert!(code.contains("views::shared::Card {"), "{code}");
    }

    #[test]
    fn reports_a_closing_tag_that_names_a_different_component() {
        let message = error("pub fn Test() -> Element { <shared::Card></other::Card> }");
        assert!(message.contains("does not match"), "{message}");
    }

    #[test]
    fn still_parses_a_bare_component_call() {
        let code = generate("<Card title=\"a\" />");
        assert!(code.contains("Card { title: \"a\" }"), "{code}");
    }

    #[test]
    fn does_not_treat_a_colon_in_a_lowercase_tag_as_a_namespace() {
        // `xlink:href` on an SVG element is an HTML name, not a Rust path.
        let code = generate("<svg><use xlink:href=\"#icon\"></use></svg>");
        assert!(code.contains(r##"xlink:href=\"#icon\""##), "{code}");
        assert!(!code.contains("Template::render"), "{code}");
    }
}

// --------------------------------------------- a statement before control flow

mod statements_before_control_flow {
    use super::*;

    #[test]
    fn parses_an_assignment_followed_by_an_if() {
        let code = generate_file(
            "pub fn Test(flag: bool) -> Element {\n    let shown = flag;\n    if shown {\n        <p>yes</p>\n    }\n}",
        );
        assert!(code.contains("let shown = flag;"), "{code}");
        assert!(code.contains("if shown {"), "{code}");
    }

    #[test]
    fn parses_a_destructuring_binding_followed_by_a_loop() {
        let code = generate_file(
            "pub fn Test(pair: (u8, u8), items: &[u8]) -> Element {\n    let (a, b) = pair;\n    <p>{a}{b}</p>\n    for item in items {\n        <li>{item}</li>\n    }\n}",
        );
        assert!(code.contains("let (a, b) = pair;"), "{code}");
        assert!(code.contains("for item in items {"), "{code}");
    }

    #[test]
    fn parses_a_bare_expression_statement() {
        let code = generate_file(
            "pub fn Test(mut n: u32) -> Element {\n    <ul>\n        while n > 0 {\n            <li>{n}</li>\n            n -= 1;\n        }\n    </ul>\n}",
        );
        assert!(code.contains("n -= 1;"), "{code}");
        assert!(
            !code.contains("n -= 1;\\\""),
            "statement leaked into HTML:\n{code}"
        );
    }

    #[test]
    fn parses_a_method_call_statement() {
        let code = generate_file(
            "pub fn Test(mut seen: Vec<u8>) -> Element {\n    seen.push(1);\n    <p>{seen.len()}</p>\n}",
        );
        assert!(code.contains("seen.push(1);"), "{code}");
    }

    #[test]
    fn still_treats_prose_before_markup_as_text() {
        let code = generate("<div>\n    plain words here\n    <p>and markup</p>\n</div>");
        assert!(code.contains("plain words here"), "{code}");
    }

    #[test]
    fn prose_with_a_parenthetical_is_not_a_call() {
        // `Bash (the shell); see below` has a path, a paren and a semicolon,
        // but the space before the paren says it is a sentence.
        let code = generate("<p>\n    Bash (the shell); see below\n</p>");
        assert!(code.contains("Bash (the shell); see below"), "{code}");
    }
}

// -------------------------------------------------- keywords appearing in text

mod keywords_in_text {
    use super::*;

    #[test]
    fn does_not_treat_if_in_text_as_control_flow() {
        let code = generate("<p>\n    if you like\n</p>");
        assert!(code.contains("if you like"), "{code}");
    }

    #[test]
    fn does_not_treat_for_in_text_as_control_flow() {
        let code = generate("<p>\n    for everyone\n</p>");
        assert!(code.contains("for everyone"), "{code}");
    }

    #[test]
    fn does_not_treat_match_in_text_as_control_flow() {
        let code = generate("<p>\n    match the colours\n</p>");
        assert!(code.contains("match the colours"), "{code}");
    }

    #[test]
    fn a_keyword_mid_line_is_always_text() {
        let code = generate("<p>ask if you like</p>");
        assert!(code.contains("ask if you like"), "{code}");
    }

    #[test]
    fn a_comparison_in_a_condition_is_not_a_tag() {
        let code = generate_file(
            "pub fn Test(a: usize, b: usize) -> Element {\n    if a < b {\n        <p>less</p>\n    }\n}",
        );
        assert!(code.contains("if a < b {"), "{code}");
    }
}

// --------------------------------------------------------- iteration shapes

// crx had a dedicated group for the shapes `.each` can take. Rust's `for`
// accepts an arbitrary expression, so the equivalent is checking that the
// header survives whatever is written in it.
mod iteration_shapes {
    use super::*;

    #[test]
    fn iterates_a_method_call() {
        let code = generate_file(
            "pub fn Test(items: &[String]) -> Element { <ul> for item in items.iter().rev() { <li>{item}</li> } </ul> }",
        );
        assert!(code.contains("for item in items.iter().rev() {"), "{code}");
    }

    #[test]
    fn iterates_a_closure_argument() {
        let code = generate_file(
            "pub fn Test(items: &[String]) -> Element { <ul> for item in items.iter().filter(|i| !i.is_empty()) { <li>{item}</li> } </ul> }",
        );
        assert!(
            code.contains("items.iter().filter(|i| !i.is_empty())"),
            "{code}"
        );
    }

    #[test]
    fn iterates_a_chain_of_narrowing_calls() {
        let code = generate_file(
            "pub fn Test(items: &[String]) -> Element { <ul> for item in items.iter().skip(1).take(2) { <li>{item}</li> } </ul> }",
        );
        assert!(code.contains("items.iter().skip(1).take(2)"), "{code}");
    }

    #[test]
    fn iterates_an_index_into_a_collection() {
        let code = generate_file(
            "pub fn Test(groups: &[Vec<String>]) -> Element { <ul> for item in &groups[0] { <li>{item}</li> } </ul> }",
        );
        assert!(code.contains("for item in &groups[0] {"), "{code}");
    }

    #[test]
    fn keeps_a_bracket_inside_a_string_argument() {
        let code = generate_file(
            "pub fn Test(items: &[String]) -> Element { <ul> for item in items.iter().filter(|i| i.contains(\"]\")) { <li>{item}</li> } </ul> }",
        );
        assert!(code.contains(r#"i.contains("]")"#), "{code}");
    }

    #[test]
    fn still_iterates_a_bare_binding() {
        let code = generate_file(
            "pub fn Test(items: &[String]) -> Element { <ul> for item in items { <li>{item}</li> } </ul> }",
        );
        assert!(code.contains("for item in items {"), "{code}");
    }
}

// ----------------------------------------------------- static text escaping

mod static_text_escaping {
    use super::*;

    #[test]
    fn escapes_a_bare_ampersand() {
        assert!(generate("<p>AT&T</p>").contains("AT&amp;T"));
    }

    #[test]
    fn escapes_angle_brackets_written_as_text() {
        assert!(generate("<p>a &lt; b</p>").contains("a &lt; b"));
    }

    #[test]
    fn leaves_a_named_character_reference_alone() {
        assert!(generate("<p>Tom &amp; Jerry</p>").contains("Tom &amp; Jerry"));
    }

    #[test]
    fn leaves_a_non_breaking_space_alone() {
        assert!(generate("<p>1&nbsp;000</p>").contains("1&nbsp;000"));
    }

    #[test]
    fn leaves_numeric_and_hexadecimal_references_alone() {
        assert!(generate("<p>&#8212; &#x2014;</p>").contains("&#8212; &#x2014;"));
    }

    #[test]
    fn escapes_an_ampersand_that_only_looks_like_a_reference() {
        assert!(generate("<p>R&D and more</p>").contains("R&amp;D and more"));
    }

    #[test]
    fn applies_the_same_rule_inside_a_static_attribute() {
        let code = generate(r#"<a title="Tom &amp; Jerry" data-x="AT&T"></a>"#);
        assert!(code.contains(r#"title=\"Tom &amp; Jerry\""#), "{code}");
        assert!(code.contains(r#"data-x=\"AT&amp;T\""#), "{code}");
    }

    #[test]
    fn leaves_script_and_style_content_untouched() {
        // A browser does not resolve references in character data, so escaping
        // here would corrupt the JavaScript it is meant to protect.
        let code = generate("<script>if (a && b < c) { go(); }</script>");
        assert!(code.contains("if (a && b < c) { go(); }"), "{code}");
    }
}
