//! The syntax tree for a `.rsx` file.
//!
//! Rust fragments — expressions, types, statements — are kept as source text
//! rather than parsed into a Rust AST. The generator copies them through
//! verbatim and lets `rustc` be the authority on whether they are valid, which
//! keeps this crate free of a Rust parser and keeps error messages about Rust
//! coming from the compiler that actually knows the rules.

use crate::span::Span;

/// A parsed `.rsx` file.
#[derive(Debug, Clone, Default)]
pub struct File {
    pub items: Vec<Item>,
    pub span: Span,
}

impl File {
    /// Every component declared in this file, in source order.
    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.items.iter().filter_map(|item| match item {
            Item::Component(c) => Some(&**c),
            Item::Rust(_) => None,
        })
    }
}

/// A top-level item.
#[derive(Debug, Clone)]
pub enum Item {
    /// Ordinary Rust — `use`, `struct`, `impl`, a non-component `fn` — copied
    /// through to the generated file untouched.
    Rust(RustItem),
    /// A `fn` whose return type is `Element`.
    ///
    /// Boxed because a `Component` is an order of magnitude larger than a
    /// `RustItem`, and a file is mostly the latter.
    Component(Box<Component>),
}

#[derive(Debug, Clone)]
pub struct RustItem {
    pub span: Span,
    pub source: String,
}

/// A component: `pub fn Card(title: &str) -> Element { <markup/> }`.
#[derive(Debug, Clone)]
pub struct Component {
    pub span: Span,
    /// Outer attributes and doc comments written above the `fn`, verbatim.
    pub attrs: String,
    /// Visibility as written — `""`, `"pub"`, `"pub(crate)"`.
    pub vis: String,
    pub name: String,
    pub name_span: Span,
    /// Generic parameters as written, without the angle brackets.
    pub generics: String,
    /// The `where` clause as written, without the `where` keyword.
    pub where_clause: String,
    pub params: Vec<Param>,
    pub body: Vec<Node>,
    /// Span of the body, between the braces.
    pub body_span: Span,
}

impl Component {
    /// The parameters that carry child markup rather than a plain value.
    pub fn children_params(&self) -> impl Iterator<Item = &Param> {
        self.params.iter().filter(|p| p.is_children())
    }

    pub fn param(&self, name: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.name == name)
    }
}

/// A component parameter.
#[derive(Debug, Clone)]
pub struct Param {
    pub span: Span,
    pub name: String,
    /// The type as written. `Element` marks a children parameter.
    pub ty: String,
    pub ty_span: Span,
}

impl Param {
    /// Whether this parameter receives markup rather than a value.
    pub fn is_children(&self) -> bool {
        self.ty.trim() == "Element"
    }
}

/// A node in a component body.
#[derive(Debug, Clone)]
pub enum Node {
    Text(Text),
    Element(Element),
    Fragment(Fragment),
    Call(Call),
    /// `{ expr }` — the value's `Display` output, HTML-escaped.
    Expr(Expr),
    /// `{! expr }` — the value's `Display` output, not escaped.
    RawExpr(Expr),
    /// `<!DOCTYPE html>`, passed through.
    Doctype(Text),
    /// `<!-- … -->`, passed through.
    Comment(Text),
    If(If),
    For(For),
    While(While),
    Match(Match),
    /// A Rust statement — `let`, `return`, a method call — copied verbatim.
    Stmt(Stmt),
}

impl Node {
    pub fn span(&self) -> Span {
        match self {
            Node::Text(n) | Node::Doctype(n) | Node::Comment(n) => n.span,
            Node::Element(n) => n.span,
            Node::Fragment(n) => n.span,
            Node::Call(n) => n.span,
            Node::Expr(n) | Node::RawExpr(n) => n.span,
            Node::If(n) => n.span,
            Node::For(n) => n.span,
            Node::While(n) => n.span,
            Node::Match(n) => n.span,
            Node::Stmt(n) => n.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Text {
    pub span: Span,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub span: Span,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub span: Span,
    pub source: String,
}

/// An HTML element.
#[derive(Debug, Clone)]
pub struct Element {
    pub span: Span,
    pub tag: String,
    pub attrs: Vec<Attribute>,
    pub children: Vec<Node>,
    /// Written as `<tag />`.
    pub self_closing: bool,
    /// One of the HTML void elements, which never gets a closing tag.
    pub void: bool,
    /// `<script>` or `<style>`, whose body is not parsed as markup.
    pub raw_text: bool,
}

/// `<>…</>` — children with no wrapper element.
#[derive(Debug, Clone)]
pub struct Fragment {
    pub span: Span,
    pub children: Vec<Node>,
}

/// A call to another component: `<Card title={t}>…</Card>`.
#[derive(Debug, Clone)]
pub struct Call {
    pub span: Span,
    /// The path as written, which may be qualified: `shared::Card`.
    pub path: String,
    pub attrs: Vec<Attribute>,
    pub children: Vec<Node>,
    /// Whether the call was written with a children block at all.
    pub has_children_block: bool,
}

impl Call {
    /// The final segment of the path, used for registry lookups.
    pub fn name(&self) -> &str {
        self.path.rsplit("::").next().unwrap_or(&self.path)
    }
}

/// An attribute on an element or a prop on a component call.
#[derive(Debug, Clone)]
pub enum Attribute {
    /// `class="card"`
    Static {
        span: Span,
        name: String,
        value: String,
    },
    /// `href={ url }`
    Expr {
        span: Span,
        name: String,
        expr: String,
    },
    /// `disabled` — present with no value.
    Bare { span: Span, name: String },
    /// `checked?={ is_checked }` — rendered only when the expression is true.
    Conditional {
        span: Span,
        name: String,
        expr: String,
    },
    /// `{ ...attrs }`
    Spread { span: Span, expr: String },
}

impl Attribute {
    pub fn span(&self) -> Span {
        match self {
            Attribute::Static { span, .. }
            | Attribute::Expr { span, .. }
            | Attribute::Bare { span, .. }
            | Attribute::Conditional { span, .. }
            | Attribute::Spread { span, .. } => *span,
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            Attribute::Static { name, .. }
            | Attribute::Expr { name, .. }
            | Attribute::Bare { name, .. }
            | Attribute::Conditional { name, .. } => Some(name),
            Attribute::Spread { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct If {
    pub span: Span,
    /// The condition as written, which may be an `if let` pattern.
    pub cond: String,
    pub then_body: Vec<Node>,
    pub else_ifs: Vec<ElseIf>,
    pub else_body: Option<Vec<Node>>,
}

#[derive(Debug, Clone)]
pub struct ElseIf {
    pub span: Span,
    pub cond: String,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct For {
    pub span: Span,
    /// Everything between `for` and the opening brace: `item in &items`.
    pub header: String,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct While {
    pub span: Span,
    /// Everything between `while` and the opening brace.
    pub header: String,
    pub body: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Match {
    pub span: Span,
    pub subject: String,
    pub arms: Vec<MatchArm>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub span: Span,
    /// The pattern including any `if` guard, as written.
    pub pattern: String,
    pub body: Vec<Node>,
}

/// HTML elements that are always empty and never take a closing tag.
pub const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Elements whose content is character data rather than markup.
pub const RAW_TEXT_ELEMENTS: &[&str] = &["script", "style"];

pub fn is_void_element(tag: &str) -> bool {
    VOID_ELEMENTS.contains(&tag)
}

pub fn is_raw_text_element(tag: &str) -> bool {
    RAW_TEXT_ELEMENTS.contains(&tag)
}
