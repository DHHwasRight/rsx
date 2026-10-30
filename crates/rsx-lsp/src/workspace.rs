//! Every `.rsx` file in the project, parsed once.
//!
//! [`DocumentStore`](crate::DocumentStore) holds what the editor has open,
//! which is a handful of files. That is enough for hover, for the outline and
//! for go to definition, because each of those answers about one place. "Where
//! is this component used" is a question about the whole project, and most of
//! the project is only on disk.
//!
//! The index is built at `initialize` and refreshed on save. A file edited
//! outside the editor — a branch switch, a generator run — is therefore stale
//! until the next save. That is a deliberate trade: the alternative is file
//! watching, and a wrong answer that arrives late is worse than one the author
//! can refresh by pressing save.
//!
//! An open buffer always wins over the indexed copy. The index stores whole
//! [`Document`]s rather than extracted positions precisely so the two can be
//! treated the same way at query time.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use rsx_core::project::EXTENSION;

use crate::document::Document;
use crate::rust_analyzer::{path_to_uri, uri_to_path};

/// Directories that never hold templates worth indexing.
///
/// `target` and `node_modules` are build output, and walking the second of
/// them can cost more than the rest of the project put together.
const SKIP: [&str; 2] = ["target", "node_modules"];

/// The project's `.rsx` files, parsed.
#[derive(Default)]
pub struct Index {
    root: Option<PathBuf>,
    files: Vec<Document>,
}

impl Index {
    /// An index over nothing, which every feature treats as "no project".
    ///
    /// This is what a client that sends no root gets, and the features above
    /// still answer from the open documents alone.
    pub fn empty() -> Self {
        Index::default()
    }

    /// Reads and parses every `.rsx` file under `root`.
    pub fn build(root: impl Into<PathBuf>) -> Self {
        let mut index = Index {
            root: Some(root.into()),
            files: Vec::new(),
        };
        index.refresh();
        index
    }

    /// Takes the root from whichever field the client used.
    ///
    /// `rootUri` is deprecated and `workspaceFolders` is the replacement, but
    /// clients in the wild send one, the other, or both, so all three spellings
    /// are read and the first that resolves wins.
    pub fn from_initialize(params: &serde_json::Value) -> Self {
        let from_folders = params
            .get("workspaceFolders")
            .and_then(|f| f.as_array())
            .and_then(|folders| folders.first())
            .and_then(|folder| folder.get("uri"))
            .and_then(|uri| uri.as_str())
            .and_then(uri_to_path);
        let from_uri = params
            .get("rootUri")
            .and_then(|uri| uri.as_str())
            .and_then(uri_to_path);
        let from_path = params
            .get("rootPath")
            .and_then(|p| p.as_str())
            .map(PathBuf::from);

        match from_folders.or(from_uri).or(from_path) {
            Some(root) if root.is_dir() => Index::build(root),
            _ => Index::empty(),
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Re-reads the project from disk.
    pub fn refresh(&mut self) {
        let Some(root) = &self.root else {
            return;
        };
        self.files = sources(root)
            .into_iter()
            .filter_map(|path| {
                let text = fs::read_to_string(&path).ok()?;
                Some(Document::new(path_to_uri(&path), text, 0))
            })
            .collect();
    }

    /// The indexed files, minus the ones the editor has open.
    ///
    /// The open copy is what the author is looking at, and it is the store's
    /// job to answer for those. Yielding both would report every component in
    /// an open file twice.
    pub fn closed_files<'a>(
        &'a self,
        open: &'a HashSet<String>,
    ) -> impl Iterator<Item = &'a Document> {
        self.files
            .iter()
            .filter(move |doc| !open.contains(&doc.uri))
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

/// Lists the `.rsx` files under `root`, sorted, skipping build output.
///
/// `rsx_core::project::find_sources` does nearly this, and is not used because
/// it does not skip `node_modules`: a project that holds both templates and a
/// JavaScript toolchain would pay for the whole dependency tree on every
/// refresh.
fn sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(root, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || SKIP.contains(&name.as_ref()) {
                continue;
            }
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == EXTENSION) {
            out.push(path);
        }
    }
}
