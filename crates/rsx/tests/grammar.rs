//! The grammar the crate exposes is the one the editor ships.

#[test]
fn the_crate_grammar_is_the_one_the_vscode_extension_ships() {
    let editor = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../editors/vscode/syntaxes/rsx.tmLanguage.json"
    );
    // A published crate has no editors directory: there is nothing to compare against.
    let Ok(shipped) = std::fs::read_to_string(editor) else {
        return;
    };
    assert!(
        shipped == rsx::grammar::TEXTMATE,
        "editors/vscode/syntaxes/rsx.tmLanguage.json differs from crates/rsx/grammar/rsx.tmLanguage.json: \
         edit the crate copy, then copy it over the editor one"
    );
}

#[test]
fn the_grammar_names_its_scope() {
    assert!(rsx::grammar::TEXTMATE
        .contains(&format!("\"scopeName\": \"{}\"", rsx::grammar::SCOPE_NAME)));
}
