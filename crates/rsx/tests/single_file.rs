//! `load!` compiles one file's items where the macro is written.

use rsx::Template;

rsx::load!("tests/templates/shared/button.rsx");

#[test]
fn a_single_file_loads_at_the_invocation_site() {
    assert_eq!(
        Button { label: "Save" }.render_to_string().unwrap(),
        "<button type=\"button\">Save</button>"
    );
}

#[test]
fn the_writer_function_is_available_too() {
    let mut w = rsx::Writer::new();
    button(&mut w, "Go").unwrap();
    assert_eq!(
        w.into_string().unwrap(),
        "<button type=\"button\">Go</button>"
    );
}
