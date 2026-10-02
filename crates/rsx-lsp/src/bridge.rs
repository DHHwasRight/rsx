//! Finding the Rust file that stands in for a template.
//!
//! `rust-analyzer` can only answer questions about code that belongs to a
//! crate it has loaded. Generated code held only in this process is invisible
//! to it, so the question has to be asked about a file the crate really
//! contains, with its contents overlaid in memory.
//!
//! There are two ways a template's code gets into a crate, and each gives a
//! file to stand in:
//!
//! * `rsx generate` writes `card.rs` beside `card.rsx`, and something `mod`s
//!   it. That file is the host, and the generated code is all of it.
//! * `rsx::load_dir!("src/views")` expands inside whichever `.rs` holds the
//!   invocation. That file is the host, and the generated code replaces the
//!   invocation inside it.

use std::fs;
use std::path::{Path, PathBuf};

use rsx_core::project::{self, Compilation};
use rsx_core::{Options, SourceMap};

/// A real Rust file to ask `rust-analyzer` about, and what to overlay on it.
#[derive(Debug, Clone)]
pub struct Host {
    /// The file in the user's crate.
    pub path: PathBuf,
    /// The contents to overlay.
    pub text: String,
    /// Byte offset in `text` where this template's generated code begins.
    pub shift: usize,
    /// Map from the template to its generated code, before the shift.
    pub map: SourceMap,
}

impl Host {
    /// Where a position in the template lands in the host file.
    pub fn to_host_offset(&self, template_offset: usize) -> Option<usize> {
        self.map
            .to_generated(template_offset)
            .map(|offset| offset + self.shift)
    }

    /// Where a position in the host file came from in the template.
    pub fn to_template_offset(&self, host_offset: usize) -> Option<usize> {
        self.map.to_source(host_offset.checked_sub(self.shift)?)
    }
}

/// The crate root above `path`: the nearest ancestor with a `Cargo.toml`.
pub fn crate_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| dir.join("Cargo.toml").is_file())
        .map(Path::to_path_buf)
}

/// Builds the host for `template`, or `None` when its code reaches no crate.
///
/// The macro is tried first. A generated `.rs` sitting beside a template is
/// not proof that anything compiles it — a golden fixture looks exactly like
/// output that is `mod`-ed in — whereas a macro invocation naming the
/// template is proof, because the invocation is itself inside a crate.
pub fn resolve(template: &Path, options: &Options) -> Option<Host> {
    macro_host(template, options).or_else(|| sibling_host(template, options))
}

/// The outermost `Cargo.toml` above `path` that declares a workspace.
pub fn workspace_root(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .skip(1)
        .find(|dir| {
            fs::read_to_string(dir.join("Cargo.toml"))
                .is_ok_and(|manifest| manifest.contains("[workspace]"))
        })
        .map(Path::to_path_buf)
}

/// `rsx generate` mode: the generated file sits beside the template.
fn sibling_host(template: &Path, options: &Options) -> Option<Host> {
    let sibling = template.with_extension("rs");
    if !sibling.is_file() {
        return None;
    }
    let compiled = project::compile_file(template, options).ok()?;
    Some(Host {
        path: sibling,
        text: compiled.code,
        shift: 0,
        map: compiled.map,
    })
}

/// Macro mode: the generated code lives inside whatever file invokes the
/// macro, so that file is overlaid with the expansion in place of the call.
fn macro_host(template: &Path, options: &Options) -> Option<Host> {
    let root = crate_root(template)?;
    let (host_path, invocation) = find_invocation(&root, template)?;
    let host_source = fs::read_to_string(&host_path).ok()?;

    let compilation: Compilation = match invocation.kind {
        // Lossy: a sibling template that does not parse must not take Rust
        // support away from this one.
        InvocationKind::Dir => project::compile_dir_lossy(&invocation.target, options).0,
        InvocationKind::File => {
            let file = project::compile_file(&invocation.target, options).ok()?;
            Compilation {
                files: vec![file],
                ..Compilation::default()
            }
        }
    };

    // The map belongs to this template, so find its code in the expansion.
    let template_canonical = canonical(template);
    let this = compilation
        .files
        .iter()
        .find(|f| canonical(&f.path) == template_canonical)?;
    let map = this.map.clone();

    let (expansion, offsets) = match invocation.kind {
        InvocationKind::Dir => compilation.into_module_tree_with_offsets(),
        InvocationKind::File => {
            let file = compilation.files.into_iter().next()?;
            let mut offsets = std::collections::BTreeMap::new();
            offsets.insert(file.path.clone(), 0usize);
            (file.code, offsets)
        }
    };

    let within = offsets
        .iter()
        .find(|(path, _)| canonical(path) == template_canonical)
        .map(|(_, offset)| *offset)?;

    // Replace the macro call with what it expands to.
    let mut text = String::with_capacity(host_source.len() + expansion.len());
    text.push_str(&host_source[..invocation.range.0]);
    let shift = text.len() + within;
    text.push_str(&expansion);
    text.push_str(&host_source[invocation.range.1..]);

    Some(Host {
        path: host_path,
        text,
        shift,
        map,
    })
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvocationKind {
    File,
    Dir,
}

#[derive(Debug, Clone)]
struct Invocation {
    kind: InvocationKind,
    /// The path the macro was given, resolved against the crate root.
    target: PathBuf,
    /// Byte range of the whole `…load_dir!("…");` call in its file.
    range: (usize, usize),
}

/// Searches for the macro call that compiles `template`.
///
/// The call need not be in the same crate, let alone in its `src`: a
/// workspace commonly keeps templates in one crate and loads them from a test
/// or an example in another. The crate is searched first, since that is where
/// it usually is, and the workspace only if that comes up empty.
fn find_invocation(root: &Path, template: &Path) -> Option<(PathBuf, Invocation)> {
    if let Some(found) = search_for_invocation(root, template) {
        return Some(found);
    }
    let workspace = workspace_root(template)?;
    if workspace == root {
        return None;
    }
    search_for_invocation(&workspace, template)
}

fn search_for_invocation(root: &Path, template: &Path) -> Option<(PathBuf, Invocation)> {
    let template = canonical(template);
    for candidate in rust_files(root) {
        let Ok(source) = fs::read_to_string(&candidate) else {
            continue;
        };
        // Macro paths are relative to the manifest of the crate holding the
        // invocation, which is not necessarily the one being searched.
        let Some(manifest_dir) = crate_root(&candidate) else {
            continue;
        };
        for invocation in invocations(&source, &manifest_dir) {
            let covers = match invocation.kind {
                InvocationKind::File => canonical(&invocation.target) == template,
                InvocationKind::Dir => template.starts_with(canonical(&invocation.target)),
            };
            if covers {
                return Some((candidate, invocation));
            }
        }
    }
    None
}

/// Every `load!` and `load_dir!` call in a Rust source file.
fn invocations(source: &str, root: &Path) -> Vec<Invocation> {
    let mut out = Vec::new();
    for (macro_name, kind) in [
        ("load_dir!", InvocationKind::Dir),
        ("load!", InvocationKind::File),
    ] {
        let mut from = 0usize;
        while let Some(index) = source[from..].find(macro_name) {
            let start = from + index;
            from = start + macro_name.len();

            // Back up over an optional `rsx::` qualifier.
            let call_start = source[..start]
                .rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
                .map(|i| i + 1)
                .unwrap_or(0);

            let Some(open) = source[start..].find('(') else {
                continue;
            };
            let Some(close) = source[start + open..].find(')') else {
                continue;
            };
            let args = &source[start + open + 1..start + open + close];
            let Some(path) = args.trim().trim_matches(|c| c == '"' || c == ',').get(..) else {
                continue;
            };
            if path.is_empty() {
                continue;
            }

            // Swallow a trailing semicolon so the expansion replaces the whole
            // statement rather than leaving a stray `;`.
            let mut end = start + open + close + 1;
            if source[end..].starts_with(';') {
                end += 1;
            }

            out.push(Invocation {
                kind,
                target: root.join(path),
                range: (call_start, end),
            });
        }
    }
    out
}

/// Every `.rs` file under `dir`, skipping the places a crate never keeps one.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rust(dir, &mut out, 0);
    out.sort();
    out
}

/// Directories inside a crate that can hold a macro invocation.
const SOURCE_DIRS: &[&str] = &["src", "tests", "examples", "benches"];

fn collect_rust(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    // A crate's module tree is not deep, and this runs on every template
    // change; a bound keeps a stray symlink from costing a scan of the disk.
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            // At the top of a crate only the directories that can hold code
            // are worth descending into; deeper down, everything is.
            if depth == 0 && !SOURCE_DIRS.contains(&name.as_ref()) && !is_crate_dir(&path) {
                continue;
            }
            collect_rust(&path, out, depth + 1);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Whether a directory holds a crate, or crates beneath it.
fn is_crate_dir(path: &Path) -> bool {
    path.join("Cargo.toml").is_file()
        || fs::read_dir(path).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().join("Cargo.toml").is_file())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_both_macro_forms() {
        let source = r#"
            rsx::load_dir!("src/views");
            load!("src/one.rsx");
        "#;
        let found = invocations(source, Path::new("/crate"));
        assert_eq!(found.len(), 2);

        let dir = found
            .iter()
            .find(|i| i.kind == InvocationKind::Dir)
            .unwrap();
        assert_eq!(dir.target, Path::new("/crate/src/views"));

        let file = found
            .iter()
            .find(|i| i.kind == InvocationKind::File)
            .unwrap();
        assert_eq!(file.target, Path::new("/crate/src/one.rsx"));
    }

    #[test]
    fn the_range_covers_the_whole_statement() {
        let source = "before\nrsx::load_dir!(\"src/views\");\nafter";
        let found = invocations(source, Path::new("/crate"));
        let range = found[0].range;
        assert_eq!(&source[range.0..range.1], "rsx::load_dir!(\"src/views\");");
    }

    #[test]
    fn ignores_a_call_with_no_path() {
        assert!(invocations("load_dir!();", Path::new("/crate")).is_empty());
    }
}
