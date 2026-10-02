//! What a template author sees when a template is wrong.

use rsx_core::{compile_source, Options};

/// Compiles and returns the error message, failing if compilation succeeds.
fn error(source: &str) -> String {
    match compile_source(source, &Options::default()) {
        Ok(code) => panic!("expected an error, got:\n{code}"),
        Err(e) => e.message,
    }
}

/// The full rendered diagnostic, with the source line and caret.
fn rendered(source: &str) -> String {
    match compile_source(source, &Options::default()) {
        Ok(code) => panic!("expected an error, got:\n{code}"),
        Err(e) => e.render(source),
    }
}

#[test]
fn reports_an_unclosed_element() {
    let message = error("fn X() -> Element { <div><p>hi</p> }");
    assert!(message.contains("unclosed `<div>`"), "{message}");
}

#[test]
fn reports_a_mismatched_closing_tag() {
    let message = error("fn X() -> Element { <div>hi</span> }");
    assert!(
        message.contains("closing tag `</span>` does not match `<div>`"),
        "{message}"
    );
}

#[test]
fn reports_an_unknown_component_parameter() {
    let message = error(
        "fn Card(title: &str) -> Element { <p>{title}</p> }\n\
         fn Page() -> Element { <Card titel=\"typo\" /> }",
    );
    assert!(
        message.contains("component `Card` has no parameter `titel`"),
        "{message}"
    );
}

#[test]
fn lists_the_parameters_a_component_does_accept() {
    let source = "fn Card(title: &str, count: usize) -> Element { <p>{title}</p> }\n\
                  fn Page() -> Element { <Card nope=\"x\" /> }";
    let full = rendered(source);
    assert!(full.contains("accepts: title, count"), "{full}");
}

#[test]
fn reports_children_passed_to_a_component_that_takes_none() {
    let source = "fn Badge(label: &str) -> Element { <b>{label}</b> }\n\
                  fn Page() -> Element { <Badge label=\"a\"><p>child</p></Badge> }";
    let full = rendered(source);
    assert!(full.contains("does not accept children"), "{full}");
    assert!(full.contains("type `Element`"), "{full}");
}

#[test]
fn reports_a_duplicate_prop() {
    let message = error(
        "fn Card(title: &str) -> Element { <p>{title}</p> }\n\
         fn Page() -> Element { <Card title=\"a\" title=\"b\" /> }",
    );
    assert!(message.contains("set twice"), "{message}");
}

#[test]
fn requires_component_names_to_be_capitalised() {
    let full = rendered("fn card(title: &str) -> Element { <p>{title}</p> }");
    assert!(
        full.contains("must start with an uppercase letter"),
        "{full}"
    );
    assert!(full.contains("`<Card />` is a component"), "{full}");
}

#[test]
fn rejects_impl_trait_parameters_with_a_suggestion() {
    let full = rendered("fn X(value: impl std::fmt::Display) -> Element { <p>{value}</p> }");
    assert!(full.contains("`impl Trait`"), "{full}");
    assert!(full.contains("named generic parameter"), "{full}");
}

#[test]
fn reports_an_empty_expression() {
    let message = error("fn X() -> Element { <p>{}</p> }");
    assert!(message.contains("empty `{}` expression"), "{message}");
}

#[test]
fn reports_a_parameter_with_no_type() {
    let message = error("fn X(name) -> Element { <p></p> }");
    assert!(message.contains("needs a type"), "{message}");
}

#[test]
fn reports_an_unterminated_expression() {
    let message = error("fn X(a: u8) -> Element { <p>{a");
    assert!(message.contains("unterminated"), "{message}");
}

#[test]
fn reports_an_unclosed_fragment() {
    let message = error("fn X() -> Element { <><p>hi</p> }");
    assert!(message.contains("fragment"), "{message}");
}

#[test]
fn reports_a_non_identifier_prop_name() {
    let message = error(
        "fn Card(title: &str) -> Element { <p>{title}</p> }\n\
         fn Page() -> Element { <Card data-x=\"1\" /> }",
    );
    assert!(message.contains("not a valid parameter name"), "{message}");
}

#[test]
fn a_diagnostic_points_at_the_offending_line() {
    let source = "fn Card(title: &str) -> Element {\n    <p>{title}</p>\n}\n\
                  fn Page() -> Element {\n    <Card wrong=\"x\" />\n}";
    let full = rendered(source);
    assert!(
        full.contains("--> 5:"),
        "expected a line 5 location:\n{full}"
    );
    assert!(
        full.contains("<Card wrong="),
        "expected the source line:\n{full}"
    );
    assert!(full.contains('^'), "expected a caret:\n{full}");
}

#[test]
fn unknown_components_are_left_for_rustc() {
    // A component defined in handwritten Rust or another crate is not in the
    // registry; generating the call anyway keeps those usable.
    let code = compile_source(
        "fn Page() -> Element { <External title=\"a\" /> }",
        &Options::default(),
    )
    .expect("should compile");
    assert!(code.contains("External { title: \"a\" }"), "{code}");
}
