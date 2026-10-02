//! Compiling one file or a whole directory of them.
//!
//! A directory is compiled as a unit so every component is in the registry
//! before any of them is generated, which is what lets a call in one file be
//! checked against a declaration in another.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::codegen::{self, Options};
use crate::error::{Error, Result};
use crate::naming::to_module_name;
use crate::parser;
use crate::registry::Registry;
use crate::span::Span;

/// The file extension this crate compiles.
pub const EXTENSION: &str = "rsx";

/// One generated file.
#[derive(Debug, Clone)]
pub struct CompiledFile {
    /// The `.rsx` file this came from.
    pub path: PathBuf,
    /// Module path relative to the compiled root, outermost first.
    pub module_path: Vec<String>,
    /// The generated Rust.
    pub code: String,
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
        let mut root = ModuleNode::default();
        for file in self.files {
            root.insert(&file.module_path, file.code);
        }
        let mut out = String::new();
        root.render(&mut out, 0);
        out
    }
}

#[derive(Default)]
struct ModuleNode {
    children: BTreeMap<String, ModuleNode>,
    code: Option<String>,
}

impl ModuleNode {
    fn insert(&mut self, path: &[String], code: String) {
        match path.split_first() {
            None => self.code = Some(code),
            Some((head, rest)) => self
                .children
                .entry(head.clone())
                .or_default()
                .insert(rest, code),
        }
    }

    fn render(&self, out: &mut String, depth: usize) {
        let pad = "    ".repeat(depth);
        if let Some(code) = &self.code {
            for line in code.lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(&pad);
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        for (name, child) in &self.children {
            out.push_str(&format!("{pad}pub mod {name} {{\n"));
            child.render(out, depth + 1);
            out.push_str(&format!("{pad}}}\n"));
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
    let source = read(path)?;
    let file = parser::parse(&source).map_err(|e| e.in_file(path))?;
    let mut registry = Registry::new();
    registry.add_file(&file);
    let code = codegen::generate(&file, &registry, options).map_err(|e| e.in_file(path))?;
    Ok(CompiledFile {
        path: path.to_path_buf(),
        module_path: vec![module_name_for(path)],
        code,
    })
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
        registry.add_file(&file);
        parsed.push((path, file));
    }

    // Pass two: generate.
    let mut files = Vec::with_capacity(parsed.len());
    for (path, file) in parsed {
        let code = codegen::generate(&file, &registry, options).map_err(|e| e.in_file(&path))?;
        files.push(CompiledFile {
            module_path: module_path_for(root, &path),
            path,
            code,
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
