//! Parser behaviour that the end-to-end render tests cannot observe directly.

use rsx_core::ast::{Attribute, Item, Node};
use rsx_core::parse;

fn component(source: &str) -> rsx_core::ast::Component {
    let file = parse(source).expect("parse failed");
    let component = file.components().next().expect("no component").clone();
    component
}

#[test]
fn non_component_items_pass_through_verbatim() {
    let file = parse(
        r#"
use std::fmt::Display;

pub struct User { pub name: String }

impl User {
    fn shout(&self) -> String { self.name.to_uppercase() }
}

fn helper(x: u32) -> u32 { x + 1 }

pub fn Card() -> Element { <p>hi</p> }
"#,
    )
    .unwrap();

    let rust: Vec<&str> = file
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Rust(r) => Some(r.source.trim()),
            Item::Component(_) => None,
        })
        .filter(|s| !s.is_empty())
        .collect();

    assert!(rust.iter().any(|s| s.starts_with("use std::fmt")));
    assert!(rust.iter().any(|s| s.starts_with("pub struct User")));
    assert!(rust.iter().any(|s| s.starts_with("impl User")));
    assert!(rust.iter().any(|s| s.starts_with("fn helper")));
    assert_eq!(file.components().count(), 1);
}

#[test]
fn a_fn_returning_something_else_is_not_a_component() {
    let file = parse("fn helper() -> String { String::new() }").unwrap();
    assert_eq!(file.components().count(), 0);
}

#[test]
fn parses_visibility_generics_and_where_clauses() {
    let c = component(
        "pub(crate) fn List<T>(items: &[T]) -> Element where T: std::fmt::Display { <ul></ul> }",
    );
    assert_eq!(c.vis, "pub(crate)");
    assert_eq!(c.name, "List");
    assert_eq!(c.generics, "T");
    assert_eq!(c.where_clause, "T: std::fmt::Display");
    assert_eq!(c.params.len(), 1);
    assert_eq!(c.params[0].ty, "&[T]");
}

#[test]
fn parameter_types_may_contain_commas_inside_generics() {
    let c = component(
        "fn X(map: std::collections::HashMap<String, Vec<u8>>, n: usize) -> Element { <p></p> }",
    );
    assert_eq!(c.params.len(), 2);
    assert_eq!(c.params[0].ty, "std::collections::HashMap<String, Vec<u8>>");
    assert_eq!(c.params[1].ty, "usize");
}

#[test]
fn element_typed_parameters_are_children() {
    let c = component("fn X(a: &str, children: Element) -> Element { <p></p> }");
    assert!(!c.params[0].is_children());
    assert!(c.params[1].is_children());
    assert_eq!(c.children_params().count(), 1);
}

#[test]
fn collapses_whitespace_but_keeps_significant_spaces() {
    let c = component("fn X(name: &str) -> Element { <h1>Hello,   {name}  !</h1> }");
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    let Node::Text(before) = &el.children[0] else {
        panic!("expected text");
    };
    assert_eq!(before.value, "Hello, ");
    let Node::Text(after) = &el.children[2] else {
        panic!("expected text");
    };
    assert_eq!(after.value, " !");
}

#[test]
fn whitespace_only_text_between_elements_is_dropped() {
    let c = component("fn X() -> Element { <div>\n  <a></a>\n  <b></b>\n</div> }");
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    assert_eq!(el.children.len(), 2, "{:?}", el.children);
}

#[test]
fn recognises_every_attribute_form() {
    let c = component(
        r#"fn X(u: &str, m: &str) -> Element { <input class="a" value={u} required checked?={true} { ...m }> }"#,
    );
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    assert!(matches!(el.attrs[0], Attribute::Static { .. }));
    assert!(matches!(el.attrs[1], Attribute::Expr { .. }));
    assert!(matches!(el.attrs[2], Attribute::Bare { .. }));
    assert!(matches!(el.attrs[3], Attribute::Conditional { .. }));
    assert!(matches!(el.attrs[4], Attribute::Spread { .. }));
    assert!(el.void);
}

#[test]
fn attribute_expressions_may_contain_braces_and_strings() {
    let c = component(
        r#"fn X(n: u8) -> Element { <p title={ if n > 0 { "a}b" } else { "c" } }></p> }"#,
    );
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    let Attribute::Expr { expr, .. } = &el.attrs[0] else {
        panic!("expected an expression attribute");
    };
    assert_eq!(expr, r#"if n > 0 { "a}b" } else { "c" }"#);
}

#[test]
fn hyphenated_and_namespaced_attribute_names_are_kept() {
    let c = component(
        r#"fn X() -> Element { <div data-id="1" @click="go" :class="c" aria-label="l"></div> }"#,
    );
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    let names: Vec<&str> = el.attrs.iter().filter_map(|a| a.name()).collect();
    assert_eq!(names, vec!["data-id", "@click", ":class", "aria-label"]);
}

#[test]
fn apostrophes_in_text_are_not_character_literals() {
    let c = component("fn X() -> Element { <p>don't stop; it isn't a lifetime</p> }");
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    let Node::Text(text) = &el.children[0] else {
        panic!("expected text");
    };
    assert_eq!(text.value, "don't stop; it isn't a lifetime");
}

#[test]
fn script_and_style_bodies_are_character_data() {
    let c = component("fn X() -> Element { <script>if (a < b) { f(\"</div>\"); }</script> }");
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    assert!(el.raw_text);
    let Node::Text(text) = &el.children[0] else {
        panic!("expected text");
    };
    assert_eq!(text.value, "if (a < b) { f(\"</div>\"); }");
}

#[test]
fn match_arms_accept_blocks_and_single_nodes() {
    let c = component(
        "fn X(s: u8) -> Element { match s { 0 => <a></a>, 1 => { <b></b> <i></i> } _ => <c></c>, } }",
    );
    let Node::Match(m) = &c.body[0] else {
        panic!("expected a match");
    };
    assert_eq!(m.arms.len(), 3);
    assert_eq!(m.arms[0].body.len(), 1);
    assert_eq!(m.arms[1].body.len(), 2);
    assert_eq!(m.arms[2].pattern, "_");
}

#[test]
fn if_chains_keep_every_branch() {
    let c = component(
        "fn X(n: u8) -> Element { if n == 1 { <a></a> } else if n == 2 { <b></b> } else if n == 3 { <c></c> } else { <d></d> } }",
    );
    let Node::If(node) = &c.body[0] else {
        panic!("expected an if");
    };
    assert_eq!(node.else_ifs.len(), 2);
    assert!(node.else_body.is_some());
}

#[test]
fn component_calls_are_told_from_elements_by_case() {
    let c =
        component("fn X() -> Element { <div><Card title=\"a\" /><my-widget></my-widget></div> }");
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    assert!(matches!(el.children[0], Node::Call(_)));
    assert!(matches!(el.children[1], Node::Element(_)));
}

#[test]
fn qualified_paths_are_component_calls() {
    let c = component("fn X() -> Element { <shared::Card title=\"a\" /> }");
    let Node::Call(call) = &c.body[0] else {
        panic!("expected a call");
    };
    assert_eq!(call.path, "shared::Card");
    assert_eq!(call.name(), "Card");
}

#[test]
fn raw_expressions_are_marked() {
    let c = component("fn X(h: &str) -> Element { <div>{!h}{h}</div> }");
    let Node::Element(el) = &c.body[0] else {
        panic!("expected an element");
    };
    assert!(matches!(el.children[0], Node::RawExpr(_)));
    assert!(matches!(el.children[1], Node::Expr(_)));
}

#[test]
fn rust_comments_on_their_own_line_are_dropped() {
    let c = component("fn X() -> Element {\n    // a note\n    <p>text</p>\n}");
    assert_eq!(c.body.len(), 1);
    assert!(matches!(c.body[0], Node::Element(_)));
}
