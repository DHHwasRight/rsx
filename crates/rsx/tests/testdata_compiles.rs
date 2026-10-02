//! The golden fixtures have to be valid Rust, not merely stable text.
//!
//! `rsx-core`'s golden test compares the generated output byte for byte, which
//! catches a change in what the generator emits but says nothing about whether
//! what it emits compiles. Loading the whole fixture tree here puts it through
//! `rustc`, so a code generation bug that produces well-formed nonsense fails
//! the build rather than passing a diff.

use rsx::Template;

rsx::load_dir!("../rsx-core/testdata");

#[test]
fn the_fixtures_render() {
    assert_eq!(
        hello::Hello { name: "Max" }.render_to_string().unwrap(),
        "<div class=\"greeting\"><h1>Hello, Max!</h1></div>"
    );

    assert_eq!(
        control_flow::IfOnly { show: true }
            .render_to_string()
            .unwrap(),
        "<p>Visible</p>"
    );

    assert!(children::Page {}
        .render_to_string()
        .unwrap()
        .contains("card-body"));
}

#[test]
fn the_showcase_renders() {
    let html = examples::Greeting { name: "Max" }
        .render_to_string()
        .unwrap();
    assert!(html.contains("Hello, Max!"), "{html}");

    // Static text keeps the entities an author wrote and escapes the rest.
    let footer = examples::Copyright { year: 2026 }
        .render_to_string()
        .unwrap();
    assert!(footer.contains("AT&amp;T &amp; others"), "{footer}");
}
