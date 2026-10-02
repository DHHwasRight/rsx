//! Golden-file tests over `testdata/`.
//!
//! Every `.rsx` fixture has a committed `.rs` beside it holding exactly what
//! the generator produces. A change in generated output then shows up as a
//! reviewable diff rather than as a silent behaviour change, and the fixtures
//! double as readable documentation of what the compiler emits.
//!
//! Run with `UPDATE_GOLDEN=1 cargo test -p rsx-core --test golden` to rewrite
//! them after an intentional change, then read the diff before committing.

use std::fs;
use std::path::{Path, PathBuf};

use rsx_core::project;
use rsx_core::Options;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn generated_output_matches_the_golden_files() {
    let root = testdata();
    let compilation =
        project::compile_dir(&root, &Options::default()).expect("testdata should compile");

    assert!(
        compilation.files.len() >= 8,
        "expected the full fixture set, found {}",
        compilation.files.len()
    );

    let updating = std::env::var("UPDATE_GOLDEN").as_deref() == Ok("1");
    let mut stale = Vec::new();

    for file in &compilation.files {
        let golden = file.path.with_extension("rs");

        if updating {
            fs::write(&golden, &file.code).expect("could not write golden file");
            continue;
        }

        let want = match fs::read_to_string(&golden) {
            Ok(want) => want,
            Err(_) => {
                stale.push(format!(
                    "{}: golden file missing",
                    golden.file_name().unwrap().to_string_lossy()
                ));
                continue;
            }
        };

        if want != file.code {
            stale.push(format!(
                "{}: generated output differs\n--- generated ---\n{}\n--- golden ---\n{}",
                golden.file_name().unwrap().to_string_lossy(),
                file.code,
                want,
            ));
        }
    }

    assert!(
        stale.is_empty(),
        "{}\n\nrun with UPDATE_GOLDEN=1 to rewrite the golden files",
        stale.join("\n\n")
    );
}

#[test]
fn every_fixture_has_a_golden_file() {
    let root = testdata();
    let sources = project::find_sources(&root).expect("could not list fixtures");
    assert!(
        !sources.is_empty(),
        "no fixtures found in {}",
        root.display()
    );

    for source in sources {
        let golden = source.with_extension("rs");
        assert!(
            golden.exists(),
            "{} has no golden file; run with UPDATE_GOLDEN=1",
            source.display()
        );
    }
}

#[test]
fn the_fixtures_cover_every_node_kind() {
    // A fixture set that quietly stops exercising a feature is worse than no
    // fixture set, so the coverage itself is asserted.
    let root = testdata();
    let sources = project::find_sources(&root).unwrap();
    let all: String = sources
        .iter()
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();

    for (feature, needle) in [
        ("elements", "<div"),
        ("void elements", "<br>"),
        ("self-closing", "/>"),
        ("fragments", "<>"),
        ("doctype", "<!DOCTYPE"),
        ("static attributes", "class=\""),
        ("expression attributes", "href={"),
        ("bare attributes", " active "),
        ("conditional attributes", "?={"),
        ("spread attributes", "{ ...attrs }"),
        ("interpolation", "{title}"),
        ("raw interpolation", "{!"),
        ("if", "if featured {"),
        ("else if", "} else if "),
        ("for", "for tag in tags {"),
        ("while", "while n > 0 {"),
        ("match", "match status {"),
        ("let statements", "let "),
        ("component calls", "<Card "),
        ("children", "children: Element"),
        ("imports", "use super::"),
        ("generics-free script", "<script>"),
        ("style", "<style>"),
        ("character references", "&amp;"),
        ("mut parameters", "mut n: u32"),
    ] {
        assert!(
            all.contains(needle),
            "no fixture exercises {feature} (looked for {needle:?})"
        );
    }
}
