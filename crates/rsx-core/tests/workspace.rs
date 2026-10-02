//! The workspace is wired together by relative path.
//!
//! When a crate moves, a stale path surfaces as a confusing build failure far
//! from the edit that caused it. Asserting the paths resolve catches a move
//! here instead.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<name> should have a workspace above it")
        .to_path_buf()
}

/// Every `path = "…"` in the workspace manifest.
fn path_dependencies(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .filter_map(|line| line.split_once("path = \""))
        .filter_map(|(_, rest)| rest.split('"').next())
        .map(str::to_string)
        .collect()
}

#[test]
fn every_path_dependency_resolves() {
    let root = workspace_root();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).expect("no workspace manifest");
    let paths = path_dependencies(&manifest);

    assert!(!paths.is_empty(), "no path dependencies declared");

    for relative in paths {
        let resolved = root.join(&relative);
        assert!(
            resolved.is_dir(),
            "Cargo.toml points at {relative}, which does not exist"
        );
        assert!(
            resolved.join("Cargo.toml").is_file(),
            "{relative} has no Cargo.toml"
        );
    }
}

#[test]
fn every_member_is_a_real_crate() {
    let root = workspace_root();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();

    let members: Vec<String> = manifest
        .lines()
        .skip_while(|l| !l.starts_with("members"))
        .take_while(|l| !l.trim_start().starts_with(']'))
        .filter_map(|line| line.split_once('"'))
        .filter_map(|(_, rest)| rest.split('"').next())
        .map(str::to_string)
        .collect();

    assert!(members.len() >= 5, "found only {members:?}");
    for member in members {
        assert!(
            root.join(&member).join("Cargo.toml").is_file(),
            "workspace member {member} has no Cargo.toml"
        );
    }
}
