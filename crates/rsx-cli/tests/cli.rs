//! Command line behaviour.

use std::process::{Command, Stdio};

fn rsx() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rsx"))
}

#[test]
fn lsp_accepts_the_stdio_flag() {
    // `vscode-languageclient` appends `--stdio` to whatever command it is
    // told to spawn. Rejecting it means every editor fails to start the
    // server, with an error that points nowhere near the cause.
    let output = rsx()
        .args(["lsp", "--stdio"])
        .stdin(Stdio::null())
        .output()
        .expect("could not run rsx");

    assert!(
        output.status.success(),
        "rsx lsp --stdio failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn an_unknown_option_is_still_rejected() {
    let output = rsx()
        .args(["lsp", "--nonsense"])
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown option"));
}

#[test]
fn check_reports_a_clean_tree() {
    let testdata = concat!(env!("CARGO_MANIFEST_DIR"), "/../rsx-core/testdata");
    let output = rsx().args(["check", testdata]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("checked"), "{stdout}");
}

#[test]
fn check_fails_on_a_broken_template() {
    let dir = std::env::temp_dir().join("rsx-cli-test-broken");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("bad.rsx"),
        "pub fn A() -> Element { <div></span> }",
    )
    .unwrap();

    let output = rsx()
        .args(["check", dir.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("does not match"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn print_writes_generated_rust_to_stdout() {
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rsx-core/testdata/hello.rsx"
    );
    let output = rsx().args(["print", file]).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pub fn hello"), "{stdout}");
    assert!(stdout.contains("DO NOT EDIT"), "{stdout}");
}

#[test]
fn help_lists_every_subcommand() {
    let output = rsx().arg("help").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    for command in ["generate", "check", "print", "lsp"] {
        assert!(stdout.contains(command), "help omits {command}:\n{stdout}");
    }
}
