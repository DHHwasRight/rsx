//! Compiling one file or a whole directory of them.
//!
//! A directory is compiled as a unit so every component is in the registry
//! before any of them is generated, which is what lets a call in one file be
//! checked against a declaration in another.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::codegen::{self, Options};
use crate::error::{Error, Result};
use crate::naming::to_module_name;
use crate::parser;
use crate::registry::Registry;
use crate::source_map::SourceMap;
use crate::span::Span;

/// The file extension this crate compiles.
pub const EXTENSION: &str = "rsx";

/// Contents that stand in for what is on disk.
///
/// An editor asks about the buffer in front of the user, which is usually not
/// what the file says: the edit that prompted the question has not been saved.
/// Compiling the disk copy would answer about different text, and every offset
/// after the first unsaved change would be wrong.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    files: HashMap<PathBuf, String>,
}

impl Overlay {
    pub fn new() -> Self {
        Overlay::default()
    }

    /// Makes `text` the contents of `path` for this compilation.
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: impl Into<String>) {
        self.files.insert(path.into(), text.into());
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// A value that changes whenever the overlaid text does.
    ///
    /// Lets a caller tell whether a compilation it already has is still
    /// current, without comparing the text itself.
    pub fn fingerprint(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut entries: Vec<_> = self.files.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let mut hasher = DefaultHasher::new();
        for (path, text) in entries {
            path.hash(&mut hasher);
            text.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The overlaid contents of `path`, or what is on disk.
    pub fn read(&self, path: &Path) -> Result<String> {
        if let Some(text) = self.files.get(path) {
            return Ok(text.clone());
        }
        // Paths reach here from a directory walk and from an editor, which
        // spell them differently often enough to be worth reconciling.
        if !self.files.is_empty() {
            if let Ok(canonical) = path.canonicalize() {
                for (candidate, text) in &self.files {
                    if candidate.canonicalize().is_ok_and(|c| c == canonical) {
                        return Ok(text.clone());
                    }
                }
            }
        }
        read(path)
    }
}

/// One generated file.
#[derive(Debug, Clone)]
pub struct CompiledFile {
    /// The `.rsx` file this came from.
    pub path: PathBuf,
    /// Module path relative to the compiled root, outermost first.
    pub module_path: Vec<String>,
    /// The generated Rust.
    pub code: String,
    /// Maps lines of `code` back to positions in `path`.
    pub map: SourceMap,
}

/// The result of compiling a directory.
#[derive(Debug, Clone, Default)]
pub struct Compilation {
    pub files: Vec<CompiledFile>,
    pub registry: Registry,
}

impl Compilation {
    /// The source files that were read, for build-system dependency tracking.
    pub fn source_paths(&self) -> impl Iterator<Item = &Path> {
        self.files.iter().map(|f| f.path.as_path())
    }

    /// Renders the compilation as nested `pub mod` declarations mirroring the
    /// directory layout.
    pub fn into_module_tree(self) -> String {
        self.into_module_tree_with_offsets().0
    }

    /// The module tree, and where each file's generated code starts in it.
    ///
    /// Module bodies are not re-indented. Indenting them would read better,
    /// but it would shift every byte of the generated code by an amount that
    /// varies per line, and the offsets in each file's [`SourceMap`] would no
    /// longer locate anything. Rust does not care about the indentation, and
    /// keeping it flat means a file's code appears verbatim at a known
    /// offset, so a position in a template still resolves to a position in
    /// the tree.
    pub fn into_module_tree_with_offsets(self) -> (String, BTreeMap<PathBuf, usize>) {
        let mut root = ModuleNode::default();
        for file in self.files {
            root.insert(&file.module_path, file.path, file.code);
        }
        let mut out = String::new();
        let mut offsets = BTreeMap::new();
        root.render(&mut out, &mut offsets);
        (out, offsets)
    }
}

#[derive(Default)]
struct ModuleNode {
    children: BTreeMap<String, ModuleNode>,
    code: Option<(PathBuf, String)>,
}

impl ModuleNode {
    fn insert(&mut self, path: &[String], source: PathBuf, code: String) {
        match path.split_first() {
            None => self.code = Some((source, code)),
            Some((head, rest)) => self
                .children
                .entry(head.clone())
                .or_default()
                .insert(rest, source, code),
        }
    }

    fn render(&self, out: &mut String, offsets: &mut BTreeMap<PathBuf, usize>) {
        if let Some((source, code)) = &self.code {
            offsets.insert(source.clone(), out.len());
            out.push_str(code);
            if !code.ends_with('\n') {
                out.push('\n');
            }
        }
        for (name, child) in &self.children {
            out.push_str("pub mod ");
            out.push_str(name);
            out.push_str(" {\n");
            child.render(out, offsets);
            out.push_str("}\n");
        }
    }
}

/// Compiles a single source string. Only components in this string are known
/// to the registry.
pub fn compile_source(source: &str, options: &Options) -> Result<String> {
    let file = parser::parse(source)?;
    let mut registry = Registry::new();
    registry.add_file(&file);
    codegen::generate(&file, &registry, options)
}

/// Compiles one `.rsx` file.
pub fn compile_file(path: &Path, options: &Options) -> Result<CompiledFile> {
    compile_file_with(path, options, &Overlay::new())
}

/// Compiles one `.rsx` file, preferring overlaid contents.
pub fn compile_file_with(
    path: &Path,
    options: &Options,
    overlay: &Overlay,
) -> Result<CompiledFile> {
    let source = overlay.read(path)?;
    let file = parser::parse(&source).map_err(|e| e.in_file(path))?;
    let mut registry = Registry::new();
    registry.add_file(&file);
    let (code, map) =
        codegen::generate_with_map(&file, &registry, options).map_err(|e| e.in_file(path))?;
    Ok(CompiledFile {
        path: path.to_path_buf(),
        module_path: vec![module_name_for(path)],
        code,
        map,
    })
}

/// Compiles every `.rsx` file under `root`, skipping the ones that fail.
///
/// A directory being compiled for an editor almost always contains one file
/// that does not parse — the one being typed in. Failing the whole directory
/// there would take Rust support away from every other template at exactly
/// the moment it is most wanted, so the broken file is left out and reported
/// separately.
pub fn compile_dir_lossy(root: &Path, options: &Options) -> (Compilation, Vec<Error>) {
    compile_dir_lossy_with(root, options, &Overlay::new())
}

/// The lenient compile, preferring overlaid contents over what is on disk.
pub fn compile_dir_lossy_with(
    root: &Path,
    options: &Options,
    overlay: &Overlay,
) -> (Compilation, Vec<Error>) {
    let mut errors = Vec::new();
    let paths = match find_sources(root) {
        Ok(paths) => paths,
        Err(error) => return (Compilation::default(), vec![error]),
    };

    let mut parsed = Vec::new();
    let mut registry = Registry::new();
    for path in paths {
        let source = match overlay.read(&path) {
            Ok(source) => source,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        match parser::parse(&source) {
            Ok(file) => {
                let module_path = module_path_for(root, &path);
                registry.add_file_at(&module_path, &file);
                parsed.push((path, module_path, file));
            }
            Err(error) => errors.push(error.in_file(&path)),
        }
    }

    let mut files = Vec::with_capacity(parsed.len());
    for (path, module_path, file) in parsed {
        match codegen::generate_at(&module_path, &file, &registry, options) {
            Ok((code, map)) => files.push(CompiledFile {
                module_path,
                path,
                code,
                map,
            }),
            Err(error) => errors.push(error.in_file(&path)),
        }
    }

    (Compilation { files, registry }, errors)
}

/// Compiles every `.rsx` file under `root`, sharing one registry.
pub fn compile_dir(root: &Path, options: &Options) -> Result<Compilation> {
    let paths = find_sources(root)?;
    if paths.is_empty() {
        return Err(Error::new(
            format!("no .{EXTENSION} files found under `{}`", root.display()),
            Span::default(),
        ));
    }

    // Pass one: parse everything and collect every component declaration, so
    // a call can be resolved against a file that has not been generated yet.
    let mut parsed = Vec::with_capacity(paths.len());
    let mut registry = Registry::new();
    for path in paths {
        let source = read(&path)?;
        let file = parser::parse(&source).map_err(|e| e.in_file(&path))?;
        let module_path = module_path_for(root, &path);
        registry.add_file_at(&module_path, &file);
        parsed.push((path, module_path, file));
    }

    // Pass two: generate. Each file resolves component names through its own
    // scope, so two files may each declare a `Card` without colliding.
    let mut files = Vec::with_capacity(parsed.len());
    for (path, module_path, file) in parsed {
        let (code, map) = codegen::generate_at(&module_path, &file, &registry, options)
            .map_err(|e| e.in_file(&path))?;
        files.push(CompiledFile {
            module_path,
            path,
            code,
            map,
        });
    }

    Ok(Compilation { files, registry })
}

/// Lists every `.rsx` file under `root`, sorted, so builds are reproducible.
pub fn find_sources(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if root.is_file() {
        out.push(root.to_path_buf());
        return Ok(out);
    }
    collect(root, &mut out).map_err(|e| io_error(root, e))?;
    out.sort();
    Ok(out)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            // Skip the places a build would never keep templates, so a stray
            // `target/` does not slow every build down.
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == "target" || name.starts_with('.') {
                continue;
            }
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == EXTENSION) {
            out.push(path);
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| io_error(path, e))
}

fn io_error(path: &Path, e: io::Error) -> Error {
    Error::new(format!("{}: {e}", path.display()), Span::default())
}

fn module_name_for(path: &Path) -> String {
    to_module_name(
        path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
            .as_str(),
    )
}

fn module_path_for(root: &Path, path: &Path) -> Vec<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut parts: Vec<String> = relative
        .parent()
        .map(|p| {
            p.components()
                .map(|c| to_module_name(&c.as_os_str().to_string_lossy()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    parts.push(module_name_for(path));
    parts
}
