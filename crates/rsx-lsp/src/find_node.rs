//! Locating what the cursor is on.
//!
//! Every feature that answers a question about a position starts here, so the
//! rules about which of several overlapping nodes wins live in one place: the
//! innermost node containing the offset, with a component's own name and
//! parameters taking precedence over its body.

use rsx_core::ast::{Attribute, Call, Component, Element, Expr, File, Node, Param};
use rsx_core::span::Span;

/// What the cursor is on.
#[derive(Debug, Clone, Copy)]
pub enum Found<'a> {
    /// The name in a component declaration.
    ComponentName(&'a Component),
    /// A parameter in a component declaration.
    Param(&'a Component, &'a Param),
    /// A call to another component.
    Call(&'a Call),
    /// An HTML element.
    Element(&'a Element),
    /// An attribute, with the element or call carrying it.
    Attribute(&'a Attribute),
    /// An interpolated expression.
    Expr(&'a Expr),
    /// Inside a component, but not on anything more specific.
    Component(&'a Component),
}

impl Found<'_> {
    pub fn span(&self) -> Span {
        match self {
            Found::ComponentName(c) => c.name_span,
            Found::Param(_, p) => p.span,
            Found::Call(c) => c.span,
            Found::Element(e) => e.span,
            Found::Attribute(a) => a.span(),
            Found::Expr(e) => e.span,
            Found::Component(c) => c.span,
        }
    }
}

fn contains(span: Span, offset: usize) -> bool {
    offset >= span.from.offset && offset < span.to.offset
}

/// Finds the innermost node at `offset`, or `None` outside any component.
pub fn at(file: &File, offset: usize) -> Option<Found<'_>> {
    let component = file.components().find(|c| contains(c.span, offset))?;

    if contains(component.name_span, offset) {
        return Some(Found::ComponentName(component));
    }
    if let Some(param) = component.params.iter().find(|p| contains(p.span, offset)) {
        return Some(Found::Param(component, param));
    }
    if let Some(found) = in_nodes(&component.body, offset) {
        return Some(found);
    }
    Some(Found::Component(component))
}

fn in_nodes(nodes: &[Node], offset: usize) -> Option<Found<'_>> {
    nodes.iter().find_map(|node| in_node(node, offset))
}

fn in_node(node: &Node, offset: usize) -> Option<Found<'_>> {
    if !contains(node.span(), offset) {
        return None;
    }

    match node {
        Node::Element(element) => Some(
            in_attributes(&element.attrs, offset)
                .or_else(|| in_nodes(&element.children, offset))
                .unwrap_or(Found::Element(element)),
        ),
        Node::Call(call) => Some(
            in_attributes(&call.attrs, offset)
                .or_else(|| in_nodes(&call.children, offset))
                .unwrap_or(Found::Call(call)),
        ),
        Node::Fragment(fragment) => in_nodes(&fragment.children, offset),
        Node::Expr(expr) | Node::RawExpr(expr) => Some(Found::Expr(expr)),
        Node::If(node) => in_nodes(&node.then_body, offset)
            .or_else(|| {
                node.else_ifs
                    .iter()
                    .find_map(|branch| in_nodes(&branch.body, offset))
            })
            .or_else(|| {
                node.else_body
                    .as_ref()
                    .and_then(|body| in_nodes(body, offset))
            }),
        Node::For(node) => in_nodes(&node.body, offset),
        Node::While(node) => in_nodes(&node.body, offset),
        Node::Match(node) => node.arms.iter().find_map(|arm| in_nodes(&arm.body, offset)),
        Node::Text(_) | Node::Doctype(_) | Node::Comment(_) | Node::Stmt(_) => None,
    }
}

fn in_attributes(attrs: &[Attribute], offset: usize) -> Option<Found<'_>> {
    attrs
        .iter()
        .find(|attr| contains(attr.span(), offset))
        .map(Found::Attribute)
}
