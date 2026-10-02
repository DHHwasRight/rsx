//! End-to-end tests: real `.rsx` files compiled by the macro and rendered.

use std::collections::BTreeMap;

use rsx::Template;

rsx::load_dir!("tests/templates");

fn render(t: impl Template) -> String {
    t.render_to_string().expect("render failed")
}

// ------------------------------------------------------------------ basics

#[test]
fn renders_text_and_expressions() {
    assert_eq!(
        render(basics::Hello { name: "Max" }),
        "<div class=\"greeting\"><h1>Hello, Max!</h1></div>"
    );
}

#[test]
fn renders_any_display_type() {
    assert_eq!(
        render(basics::Counts {
            count: 3,
            ratio: 1.5,
            flag: true
        }),
        "<ul><li>3</li><li>1.5</li><li>true</li></ul>"
    );
}

#[test]
fn escapes_interpolated_values_in_text_and_attributes() {
    assert_eq!(
        render(basics::Escaping {
            value: "<script>&'\""
        }),
        "<p title=\"&lt;script&gt;&amp;&#39;&#34;\">&lt;script&gt;&amp;&#39;&#34;</p>"
    );
}

#[test]
fn raw_output_is_not_escaped() {
    assert_eq!(
        render(basics::RawHtml {
            html: "<b>bold</b>"
        }),
        "<div class=\"prose\"><b>bold</b></div>"
    );
}

#[test]
fn void_elements_have_no_closing_tag() {
    assert_eq!(
        render(basics::Voids { src: "/a.png" }),
        "<div><img src=\"/a.png\" alt=\"a\"><br><hr><input type=\"text\" disabled></div>"
    );
}

#[test]
fn conditional_attributes_appear_only_when_true() {
    assert_eq!(
        render(basics::Conditional {
            active: true,
            name: "n"
        }),
        "<input name=\"n\" checked class=\"field\">"
    );
    assert_eq!(
        render(basics::Conditional {
            active: false,
            name: "n"
        }),
        "<input name=\"n\" class=\"field\">"
    );
}

#[test]
fn fragments_render_without_a_wrapper() {
    assert_eq!(
        render(basics::Fragmented { a: "x", b: "y" }),
        "<span>x</span><span>y</span>"
    );
}

#[test]
fn passes_through_doctype_comments_and_raw_text_elements() {
    let html = render(basics::Document {});
    assert!(html.starts_with("<!DOCTYPE html><html>"), "{html}");
    assert!(html.contains("<!-- a comment -->"), "{html}");
    // Braces and `<` inside script and style are content, not markup.
    assert!(
        html.contains("<style>body { color: red; }</style>"),
        "{html}"
    );
    assert!(
        html.contains("<script>if (a < b && c) { go(\"x\"); }</script>"),
        "{html}"
    );
}

#[test]
fn prose_starting_with_a_keyword_stays_text() {
    assert_eq!(
        render(basics::Prose {}),
        "<p>if you ask me, text that starts with a keyword is still text</p>"
    );
}

#[test]
fn whitespace_between_words_survives_but_indentation_does_not() {
    // Indentation around a text run is layout and goes; a space that sits
    // between two words, or between two elements on one line, is content.
    assert_eq!(
        render(basics::Spacing {
            count: 3,
            name: "Max"
        }),
        "<p>3 items</p><div><b>a</b> <i>b</i></div><p>Hello, Max!</p>"
    );
}

// ----------------------------------------------------------------- control

#[test]
fn if_else_if_else_picks_one_branch() {
    assert_eq!(render(control::Branches { role: "admin" }), "<b>Admin</b>");
    assert_eq!(
        render(control::Branches { role: "mod" }),
        "<i>Moderator</i>"
    );
    assert_eq!(
        render(control::Branches { role: "other" }),
        "<span>Member</span>"
    );
}

#[test]
fn if_without_else_may_render_nothing() {
    assert_eq!(
        render(control::NoElse { show: true }),
        "<div><p>visible</p></div>"
    );
    assert_eq!(render(control::NoElse { show: false }), "<div></div>");
}

#[test]
fn for_loops_over_a_slice() {
    let items = vec!["a".to_string(), "b".to_string()];
    assert_eq!(
        render(control::Loop { items: &items }),
        "<ul><li>a</li><li>b</li></ul>"
    );
    assert_eq!(render(control::Loop { items: &[] }), "<ul></ul>");
}

#[test]
fn for_loops_support_destructuring() {
    let items = vec!["a".to_string(), "b".to_string()];
    assert_eq!(
        render(control::Indexed { items: &items }),
        "<ol><li data-index=\"0\">a</li><li data-index=\"1\">b</li></ol>"
    );
}

#[test]
fn match_arms_render_markup() {
    use control::Status;
    assert_eq!(
        render(control::Matched {
            status: &Status::Ok
        }),
        "<div><span class=\"ok\">ok</span></div>"
    );
    assert_eq!(
        render(control::Matched {
            status: &Status::Pending
        }),
        "<div><span class=\"pending\">pending</span></div>"
    );
    assert_eq!(
        render(control::Matched {
            status: &Status::Failed(7)
        }),
        "<div><span class=\"failed\">failed</span><code>7</code></div>"
    );
}

#[test]
fn let_bindings_run_before_markup() {
    assert_eq!(
        render(control::WithLet {
            first: "a",
            second: "b"
        }),
        "<p>a and b</p>"
    );
}

#[test]
fn control_flow_nests() {
    let groups = vec![vec!["a".to_string(), String::new()], vec!["b".to_string()]];
    assert_eq!(
        render(control::Nested {
            groups: &groups,
            show_empty: true
        }),
        "<div><ul><li>a</li></ul><ul><li>b</li></ul><p class=\"hint\">some were hidden</p></div>"
    );
}

#[test]
fn less_than_in_a_condition_is_not_a_tag() {
    assert_eq!(
        render(control::Comparison { a: 1, b: 2 }),
        "<div><span>less</span></div>"
    );
    assert_eq!(render(control::Comparison { a: 2, b: 1 }), "<div></div>");
}

#[test]
fn owned_fields_behind_a_reference_are_borrowed_not_moved() {
    let rows = vec![
        control::Row {
            id: 1,
            name: "a & b".to_string(),
        },
        control::Row {
            id: 2,
            name: "c".to_string(),
        },
    ];
    assert_eq!(
        render(control::Rows { rows: &rows }),
        "<table>\
         <tr data-id=\"1\"><td>a &amp; b</td></tr>\
         <tr data-id=\"2\"><td>c</td></tr>\
         </table>"
    );
}

// --------------------------------------------------------------- composing

#[test]
fn components_nest_through_children() {
    assert_eq!(
        render(compose::Page { name: "Max" }),
        "<html><head><title>Home</title></head><body>\
         <div class=\"card\"><h2>First</h2><div class=\"body\">\
         <p>one</p>\
         <div class=\"card\"><h2>Nested</h2><div class=\"body\"><p>Max</p></div></div>\
         </div></div>\
         <span class=\"badge\">done</span>\
         </body></html>"
    );
}

#[test]
fn a_component_called_without_children_renders_none() {
    assert_eq!(
        render(compose::ChildlessCall {}),
        "<div class=\"card\"><h2>empty</h2><div class=\"body\"></div></div>"
    );
}

#[test]
fn spread_attributes_expand_in_order() {
    let mut attrs = BTreeMap::new();
    attrs.insert("data-id".to_string(), "7".to_string());
    attrs.insert("title".to_string(), "a \"quote\"".to_string());
    assert_eq!(
        render(compose::Spread { attrs: &attrs }),
        "<button class=\"btn\" data-id=\"7\" title=\"a &#34;quote&#34;\">press</button>"
    );
}

#[test]
fn spread_booleans_render_bare_or_not_at_all() {
    let mut attrs = BTreeMap::new();
    attrs.insert("checked".to_string(), true);
    attrs.insert("disabled".to_string(), false);
    assert_eq!(
        render(compose::Bools { attrs: &attrs }),
        "<input type=\"checkbox\" checked>"
    );
}

#[test]
fn components_may_be_generic() {
    assert_eq!(
        render(compose::Generic { items: &[1, 2, 3] }),
        "<ul><li>1</li><li>2</li><li>3</li></ul>"
    );
}

#[test]
fn components_resolve_across_modules() {
    assert_eq!(
        render(imports::Toolbar {}),
        "<div class=\"toolbar\">\
         <button type=\"button\">Save</button>\
         <button type=\"button\">Cancel</button>\
         </div>"
    );
}

// ----------------------------------------------------------------- runtime

#[test]
fn the_constructor_function_is_equivalent_to_the_struct() {
    assert_eq!(
        render(basics::Hello("Max")),
        render(basics::Hello { name: "Max" })
    );
}

#[test]
fn templates_stream_into_an_io_write() {
    let mut out: Vec<u8> = Vec::new();
    basics::Hello { name: "Max" }
        .render_to(&mut out)
        .expect("render failed");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "<div class=\"greeting\"><h1>Hello, Max!</h1></div>"
    );
}

#[test]
fn templates_can_be_boxed_for_heterogeneous_collections() {
    let items: Vec<Box<dyn rsx::DynTemplate>> = vec![
        Box::new(basics::Hello { name: "a" }),
        Box::new(compose::Badge { label: "b" }),
    ];
    let mut out = String::new();
    for item in items {
        out.push_str(&render(item));
    }
    assert!(out.contains("Hello, a!"));
    assert!(out.contains("badge"));
}
