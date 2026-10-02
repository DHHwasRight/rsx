//! What the generator knows about components other than the one it is
//! generating.
//!
//! A call like `<Card title={t} />` becomes a struct literal, so the generator
//! needs the callee's parameter names to catch typos and to fill the children
//! parameter with `()` when no children were given. Components the registry
//! has never seen still generate — the literal is emitted as written and
//! `rustc` has the final say — so a component defined in handwritten Rust or
//! another crate keeps working.

use std::collections::{BTreeSet, HashMap};

use crate::ast::File;

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

/// Every component visible to a compilation.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    components: HashMap<String, Signature>,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// Adds every component declared in `file`.
    ///
    /// A name declared twice keeps the first, since the generator only uses
    /// the registry for diagnostics and defaults — `rustc` reports the real
    /// conflict with a better message than this crate could.
    pub fn add_file(&mut self, file: &File) {
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
            self.components
                .entry(component.name.clone())
                .or_insert(signature);
        }
    }

    pub fn get(&self, name: &str) -> Option<&Signature> {
        self.components.get(name)
    }

    pub fn len(&self) -> usize {
        self.components.len()
    }

    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.components.keys().map(|s| s.as_str())
    }
}
