//! What the generator knows about components other than the one it is
//! generating.
//!
//! A call like `<Card title={t} />` becomes a struct literal, so the generator
//! needs the callee's parameter names to catch typos and to fill a markup
//! parameter with `()` when no children were given.
//!
//! Resolution follows Rust's own rules rather than a flat name table. Two
//! files in one compilation may each declare a `Card`, and a call means the
//! one its file can actually see: the local declaration, or whatever a `use`
//! brought in. A name that cannot be resolved is left alone — the literal is
//! emitted as written and `rustc` has the final say — so a component defined
//! in handwritten Rust or another crate keeps working.

use std::collections::{BTreeSet, HashMap};

use crate::ast::{File, UseDecl};

/// One component's parameters.
#[derive(Debug, Clone)]
pub struct Signature {
    pub name: String,
    pub params: Vec<ParamInfo>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub name: String,
    /// Declared as `Element`, so it receives markup.
    pub is_children: bool,
}

impl Signature {
    pub fn has_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }

    /// The parameter a children block should fill.
    ///
    /// One named `children` wins; otherwise the first markup parameter that
    /// the call did not already set explicitly.
    pub fn children_param(&self, provided: &BTreeSet<String>) -> Option<&str> {
        self.params
            .iter()
            .find(|p| p.is_children && p.name == "children" && !provided.contains(&p.name))
            .or_else(|| {
                self.params
                    .iter()
                    .find(|p| p.is_children && !provided.contains(&p.name))
            })
            .map(|p| p.name.as_str())
    }

    /// Markup parameters the call left unset, which render nothing.
    pub fn unfilled_children(&self, provided: &BTreeSet<String>) -> Vec<&str> {
        self.params
            .iter()
            .filter(|p| p.is_children && !provided.contains(&p.name))
            .map(|p| p.name.as_str())
            .collect()
    }

    /// A help line listing what the component does accept.
    pub fn describe_params(&self) -> String {
        if self.params.is_empty() {
            return format!("`{}` takes no parameters", self.name);
        }
        let names: Vec<&str> = self.params.iter().map(|p| p.name.as_str()).collect();
        format!("`{}` accepts: {}", self.name, names.join(", "))
    }
}

/// A component declaration, with the module it lives in.
#[derive(Debug, Clone)]
pub struct Declaration {
    /// Module path of the declaring file, relative to the compiled root.
    pub module_path: Vec<String>,
    pub signature: Signature,
}

/// Every component in a compilation, indexed by name.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    by_name: HashMap<String, Vec<Declaration>>,
    count: usize,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// Adds every component declared in `file`, which lives at `module_path`.
    pub fn add_file_at(&mut self, module_path: &[String], file: &File) {
        for component in file.components() {
            let signature = Signature {
                name: component.name.clone(),
                params: component
                    .params
                    .iter()
                    .map(|p| ParamInfo {
                        name: p.name.clone(),
                        is_children: p.is_children(),
                    })
                    .collect(),
            };
            let declarations = self.by_name.entry(component.name.clone()).or_default();
            // A file compiled twice should not grow the table; `rustc` reports
            // a genuine duplicate better than this crate could.
            if declarations.iter().any(|d| d.module_path == module_path) {
                continue;
            }
            declarations.push(Declaration {
                module_path: module_path.to_vec(),
                signature,
            });
            self.count += 1;
        }
    }

    /// Adds a file that is the whole compilation, as when a single source
    /// string is compiled.
    pub fn add_file(&mut self, file: &File) {
        self.add_file_at(&[], file);
    }

    /// All declarations of `name`, in the order they were added.
    pub fn declarations(&self, name: &str) -> &[Declaration] {
        self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The signature of `name` when exactly one component in the compilation
    /// has it. Ambiguous names resolve through a [`Scope`] instead.
    pub fn get(&self, name: &str) -> Option<&Signature> {
        match self.declarations(name) {
            [only] => Some(&only.signature),
            _ => None,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(|s| s.as_str())
    }

    /// What one file can see: its own components plus its `use` declarations.
    pub fn scope<'a>(&'a self, module_path: &[String], file: &'a File) -> Scope<'a> {
        Scope {
            registry: self,
            module_path: module_path.to_vec(),
            uses: &file.uses,
        }
    }
}

/// Component name resolution from the point of view of one file.
pub struct Scope<'a> {
    registry: &'a Registry,
    module_path: Vec<String>,
    uses: &'a [UseDecl],
}

impl<'a> Scope<'a> {
    /// Resolves a component name the way the file that wrote it would.
    pub fn get(&self, name: &str) -> Option<&'a Signature> {
        let declarations = self.registry.declarations(name);
        if declarations.is_empty() {
            return None;
        }

        // A declaration in this very file wins, as it does in Rust.
        if let Some(local) = declarations
            .iter()
            .find(|d| d.module_path == self.module_path)
        {
            return Some(&local.signature);
        }

        // Then whatever a `use` brought in, by name or through a glob.
        for decl in self.uses {
            if !decl.is_glob() && decl.name != name {
                continue;
            }
            if let Some(found) = declarations
                .iter()
                .find(|d| module_matches(&decl.module, &d.module_path))
            {
                return Some(&found.signature);
            }
        }

        // With only one candidate in the whole compilation there is nothing to
        // get wrong, which is also the single-file case.
        match declarations {
            [only] => Some(&only.signature),
            _ => None,
        }
    }
}

/// Whether a `use` path refers to the file at `module_path`.
///
/// The `use` is written from somewhere inside the generated module tree, and
/// where that tree is mounted is not knowable here — `crate::views::shared::card`
/// and `super::shared::card` may both mean the same file. Matching the file's
/// path as a suffix of the `use` path handles every spelling, and `super`
/// segments are dropped because they only move the starting point.
fn module_matches(use_path: &[String], module_path: &[String]) -> bool {
    let used: Vec<&String> = use_path
        .iter()
        .filter(|segment| *segment != "super")
        .collect();
    if module_path.is_empty() {
        return used.is_empty();
    }
    if used.len() < module_path.len() {
        return false;
    }
    used[used.len() - module_path.len()..]
        .iter()
        .zip(module_path)
        .all(|(a, b)| *a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn matches_a_file_path_as_a_suffix_of_the_use_path() {
        assert!(module_matches(
            &path(&["crate", "views", "shared", "card"]),
            &path(&["shared", "card"])
        ));
        assert!(module_matches(
            &path(&["super", "shared", "card"]),
            &path(&["shared", "card"])
        ));
        assert!(module_matches(&path(&["card"]), &path(&["card"])));
    }

    #[test]
    fn rejects_a_path_that_does_not_end_at_the_file() {
        assert!(!module_matches(
            &path(&["shared", "button"]),
            &path(&["shared", "card"])
        ));
        assert!(!module_matches(
            &path(&["card"]),
            &path(&["shared", "card"])
        ));
    }
}
