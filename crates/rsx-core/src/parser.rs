//! Turns `.rsx` source into a [`File`].
//!
//! The file is parsed as a sequence of Rust items. Anything that is not a `fn`
//! returning `Element` is copied through verbatim; a `fn` returning `Element`
//! has its body parsed as markup. Inside a markup body, Rust fragments —
//! conditions, loop headers, interpolated expressions — are captured as source
//! text and handed to `rustc` later.

use crate::ast::*;
use crate::error::{Error, Result};
use crate::scanner::Scanner;
use crate::span::{Pos, Span};

/// Parses a `.rsx` source file.
pub fn parse(source: &str) -> Result<File> {
    Parser::new(source).parse_file()
}

/// Where a run of nodes is being parsed, which decides how it ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    /// A `{ … }` body: ends at the closing brace.
    Block,
    /// An element's children: ends at `</`.
    Children,
}

/// What a keyword at the start of a line introduces.
enum Control {
    /// `if`, `for`, `while`, `match`, `loop` — a header then a braced body.
    Block(&'static str),
    /// `let`, `return`, `break`, `continue` — a statement ending in `;`.
    Stmt,
}

/// Where a run of Rust source ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// The `{` that opens a block, at depth zero.
    OpenBrace,
    /// The `}` that closes the construct we are inside.
    CloseBrace,
    /// A match arm's `=>`.
    FatArrow,
    /// A `,` or `)` ending a parameter, with generic arguments nested.
    ParamEnd,
}

impl Stop {
    fn tracks_angles(self) -> bool {
        matches!(self, Stop::ParamEnd)
    }
}

struct Parser<'a> {
    sc: Scanner<'a>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Parser {
            sc: Scanner::new(source),
        }
    }

    fn err<T>(&self, message: impl Into<String>, span: Span) -> Result<T> {
        Err(Error::new(message, span))
    }

    // ----------------------------------------------------------------- file

    fn parse_file(mut self) -> Result<File> {
        let start = self.sc.pos();
        let mut items = Vec::new();

        loop {
            let chunk_start = self.sc.offset();
            let chunk_start_pos = self.sc.pos();
            self.sc.skip_ws();
            if self.sc.eof() {
                self.push_rust(&mut items, chunk_start, chunk_start_pos);
                break;
            }

            // Attributes and doc comments attach to whatever follows, so they
            // are collected before deciding what that is.
            let attrs_start = self.sc.pos();
            let attrs = self.scan_outer_attrs();

            let before_fn = self.sc.pos();
            match self.try_parse_component(attrs)? {
                Some(component) => {
                    // Flush the plain Rust that preceded the attributes.
                    let upto = attrs_start.offset;
                    if self.sc.slice(chunk_start, upto).trim().is_empty() {
                        // Only whitespace: keep it so formatting survives.
                        let text = self.sc.slice(chunk_start, upto);
                        if !text.is_empty() {
                            items.push(Item::Rust(RustItem {
                                span: Span::new(chunk_start_pos, attrs_start),
                                source: text.to_string(),
                            }));
                        }
                    } else {
                        items.push(Item::Rust(RustItem {
                            span: Span::new(chunk_start_pos, attrs_start),
                            source: self.sc.slice(chunk_start, upto).to_string(),
                        }));
                    }
                    items.push(Item::Component(Box::new(component)));
                }
                None => {
                    self.sc.restore(before_fn);
                    self.sc.restore(attrs_start);
                    self.scan_rust_item();
                    self.push_rust(&mut items, chunk_start, chunk_start_pos);
                }
            }
        }

        let uses = items
            .iter()
            .filter_map(|item| match item {
                Item::Rust(rust) => Some(rust),
                Item::Component(_) => None,
            })
            .flat_map(|rust| parse_use_decls(&rust.source, rust.span))
            .collect();

        Ok(File {
            span: Span::new(start, self.sc.pos()),
            uses,
            items,
        })
    }

    fn push_rust(&self, items: &mut Vec<Item>, from: usize, from_pos: Pos) {
        let text = self.sc.slice(from, self.sc.offset());
        if text.is_empty() {
            return;
        }
        items.push(Item::Rust(RustItem {
            span: Span::new(from_pos, self.sc.pos()),
            source: text.to_string(),
        }));
    }

    /// Collects `#[…]` attributes and comments, returning them verbatim.
    fn scan_outer_attrs(&mut self) -> String {
        let start = self.sc.offset();
        loop {
            self.sc.skip_ws();
            if self.sc.starts_with("#[") || self.sc.starts_with("#![") {
                self.sc.eat("#");
                self.sc.eat("!");
                self.skip_balanced(b'[', b']');
            } else if self.sc.starts_with("//") {
                while let Some(b) = self.sc.bump() {
                    if b == b'\n' {
                        break;
                    }
                }
            } else if self.sc.starts_with("/*") {
                self.sc.skip_block_comment();
            } else {
                break;
            }
        }
        self.sc.slice(start, self.sc.offset()).to_string()
    }

    /// Consumes one complete Rust item, verbatim.
    ///
    /// An item ends either at a top-level `;` or at the `}` closing its first
    /// top-level block, which covers `use`, `const`, `type`, `struct`, `enum`,
    /// `impl`, `mod` and plain `fn` alike.
    fn scan_rust_item(&mut self) {
        self.scan_outer_attrs();
        loop {
            if self.sc.eof() {
                return;
            }
            if self.sc.starts_with("//") {
                while let Some(b) = self.sc.bump() {
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }
            if self.sc.starts_with("/*") {
                self.sc.skip_block_comment();
                continue;
            }
            if self.sc.skip_literal() {
                continue;
            }
            match self.sc.peek() {
                Some(b';') => {
                    self.sc.bump();
                    return;
                }
                Some(b'{') => {
                    self.skip_balanced(b'{', b'}');
                    return;
                }
                Some(b'(') => {
                    self.skip_balanced(b'(', b')');
                }
                Some(b'[') => {
                    self.skip_balanced(b'[', b']');
                }
                Some(_) => {
                    self.sc.bump();
                }
                None => return,
            }
        }
    }

    /// Consumes a balanced `open`…`close` pair starting at the cursor.
    fn skip_balanced(&mut self, open: u8, close: u8) {
        if self.sc.peek() != Some(open) {
            return;
        }
        self.sc.bump();
        let mut depth = 1usize;
        while depth > 0 && !self.sc.eof() {
            if self.sc.starts_with("//") {
                while let Some(b) = self.sc.bump() {
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }
            if self.sc.starts_with("/*") {
                self.sc.skip_block_comment();
                continue;
            }
            if self.sc.skip_literal() {
                continue;
            }
            match self.sc.bump() {
                Some(b) if b == open => depth += 1,
                Some(b) if b == close => depth -= 1,
                Some(_) => {}
                None => break,
            }
        }
    }

    // ------------------------------------------------------------ component

    /// Attempts to parse a component declaration, rewinding if the item turns
    /// out to be something else.
    fn try_parse_component(&mut self, attrs: String) -> Result<Option<Component>> {
        let save = self.sc.pos();
        let start = self.sc.pos();

        let vis = self.parse_visibility();
        self.sc.skip_ws();
        if !self.sc.eat_keyword("fn") {
            self.sc.restore(save);
            return Ok(None);
        }
        self.sc.skip_ws();

        let name_start = self.sc.pos();
        let name = self.sc.read_ident();
        if name.is_empty() {
            self.sc.restore(save);
            return Ok(None);
        }
        let name_span = self.sc.span_from(name_start);

        self.sc.skip_ws();
        let generics = if self.sc.peek() == Some(b'<') {
            let from = self.sc.offset();
            self.skip_balanced(b'<', b'>');
            let raw = self.sc.slice(from, self.sc.offset());
            raw.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .trim()
                .to_string()
        } else {
            String::new()
        };

        self.sc.skip_ws();
        if self.sc.peek() != Some(b'(') {
            self.sc.restore(save);
            return Ok(None);
        }

        // The parameter list is only skipped for now. Parsing it properly
        // before the return type is known would turn a malformed parameter in
        // an ordinary `fn` into a hard error, when that `fn` should simply
        // pass through; it is parsed for real once `-> Element` confirms this
        // is a component, so its errors can be reported.
        let params_start = self.sc.pos();
        self.skip_balanced(b'(', b')');

        self.sc.skip_trivia();
        if !self.sc.eat("->") {
            self.sc.restore(save);
            return Ok(None);
        }
        self.sc.skip_trivia();

        let ret_start = self.sc.pos();
        let ret = self.read_rust(Stop::OpenBrace);
        let ret_trimmed = ret.trim();
        let (ret_ty, where_clause) = match ret_trimmed.find("where") {
            Some(idx)
                if ret_trimmed[..idx].trim_end().ends_with(|c: char| {
                    c.is_alphanumeric() || c == '>' || c == ']' || c == ')' || c == '_'
                }) =>
            {
                (
                    ret_trimmed[..idx].trim().to_string(),
                    ret_trimmed[idx + "where".len()..].trim().to_string(),
                )
            }
            _ => (ret_trimmed.to_string(), String::new()),
        };

        if ret_ty != "Element" {
            self.sc.restore(save);
            return Ok(None);
        }

        if self.sc.peek() != Some(b'{') {
            return self.err(
                format!("component `{name}` has no body"),
                Span::new(ret_start, self.sc.pos()),
            );
        }

        // Confirmed a component: now the parameters are worth real errors.
        let body_brace = self.sc.pos();
        self.sc.restore(params_start);
        let params = self.parse_params()?;
        self.sc.restore(body_brace);

        self.sc.bump();

        let body_start = self.sc.pos();
        let body = self.parse_nodes(Ctx::Block)?;
        let body_span = self.sc.span_from(body_start);

        if self.sc.peek() != Some(b'}') {
            return self.err(
                format!("component `{name}` is missing its closing `}}`"),
                Span::at(self.sc.pos()),
            );
        }
        self.sc.bump();

        for param in &params {
            if param.ty.contains("impl ") {
                return Err(Error::new(
                    format!(
                        "parameter `{}` uses `impl Trait`, which cannot be stored in the generated component struct",
                        param.name
                    ),
                    param.ty_span,
                )
                .with_help("declare a named generic parameter instead, as in `fn Card<T: Display>(value: T) -> Element`"));
            }
        }

        Ok(Some(Component {
            span: Span::new(start, self.sc.pos()),
            attrs,
            vis,
            name,
            name_span,
            generics,
            where_clause,
            params,
            body,
            body_span,
        }))
    }

    fn parse_visibility(&mut self) -> String {
        let start = self.sc.offset();
        if !self.sc.eat_keyword("pub") {
            return String::new();
        }
        let save = self.sc.pos();
        self.sc.skip_ws();
        if self.sc.peek() == Some(b'(') {
            self.skip_balanced(b'(', b')');
        } else {
            self.sc.restore(save);
        }
        self.sc.slice(start, self.sc.offset()).trim().to_string()
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        self.sc.bump(); // (
        let mut params = Vec::new();
        loop {
            self.sc.skip_trivia();
            match self.sc.peek() {
                Some(b')') => {
                    self.sc.bump();
                    return Ok(params);
                }
                None => {
                    return self.err("unterminated parameter list", Span::at(self.sc.pos()));
                }
                _ => {}
            }

            let from = self.sc.pos();
            let mutable = self.sc.eat_keyword("mut");
            if mutable {
                self.sc.skip_trivia();
            }
            let name = self.sc.read_ident();
            if name.is_empty() {
                return self.err("expected a parameter name", Span::new(from, self.sc.pos()));
            }
            self.sc.skip_trivia();
            if self.sc.peek() != Some(b':') {
                return self.err(
                    format!("parameter `{name}` needs a type, as in `{name}: &str`"),
                    Span::new(from, self.sc.pos()),
                );
            }
            self.sc.bump();
            self.sc.skip_trivia();

            let ty_start = self.sc.pos();
            let ty = self.read_rust(Stop::ParamEnd).trim().to_string();
            let ty_span = self.sc.span_from(ty_start);
            if ty.is_empty() {
                return self.err(
                    format!("parameter `{name}` needs a type"),
                    Span::new(from, self.sc.pos()),
                );
            }

            params.push(Param {
                span: Span::new(from, self.sc.pos()),
                mutable,
                name,
                ty,
                ty_span,
            });

            self.sc.skip_trivia();
            if self.sc.peek() == Some(b',') {
                self.sc.bump();
            }
        }
    }

    // ---------------------------------------------------------------- nodes

    fn parse_nodes(&mut self, ctx: Ctx) -> Result<Vec<Node>> {
        let mut nodes = Vec::new();
        // The first thing in a block may be Rust even when it shares a line
        // with the opening brace, as in `-> Element { match x { … } }`. After
        // that, a keyword only counts at the start of a line, so prose like
        // "if you like" inside an element stays text.
        //
        // Leading whitespace in a block is layout rather than content, so it
        // can be skipped to put the cursor on the keyword. In an element's
        // children the same whitespace may be a word separator, so it is only
        // skipped if a control structure really does follow it.
        let mut first = true;
        if ctx == Ctx::Block {
            self.sc.skip_ws();
        }
        loop {
            if self.sc.eof() {
                return Ok(nodes);
            }
            if ctx == Ctx::Block && self.sc.peek() == Some(b'}') {
                return Ok(nodes);
            }
            if ctx == Ctx::Children && self.sc.starts_with("</") {
                return Ok(nodes);
            }

            let before = self.sc.offset();

            // Rust comments are only recognised on their own line; `//` inside
            // a line of prose stays text.
            if self.sc.at_line_start() && (self.sc.starts_with("//") || self.sc.starts_with("/*")) {
                self.sc.skip_trivia();
                if self.sc.offset() != before {
                    continue;
                }
            }

            if self.sc.peek() == Some(b'<') {
                if let Some(node) = self.parse_markup()? {
                    nodes.push(node);
                    first = false;
                    continue;
                }
                // A `<` that starts nothing is ordinary text.
            } else if self.sc.peek() == Some(b'{') {
                nodes.push(self.parse_interpolation()?);
                first = false;
                continue;
            } else if self.sc.at_line_start() || first {
                // `<ul> for item in items { … }` keeps the loop on the line
                // that opened the element. Without this the loop would be
                // emitted into the page as text, which is a silent wrong
                // answer rather than an error.
                let resume = self.sc.pos();
                if first && !self.sc.at_line_start() {
                    self.sc.skip_ws();
                }
                let control = self.control_ahead();
                if control.is_none() {
                    self.sc.restore(resume);
                }
                if let Some(control) = control {
                    match control {
                        Control::Block(kw) => {
                            nodes.push(self.parse_control_block(kw)?);
                            first = false;
                            continue;
                        }
                        Control::Stmt => {
                            nodes.push(self.parse_stmt()?);
                            first = false;
                            continue;
                        }
                    }
                }
            }

            match self.parse_text(ctx)? {
                Some(text) => {
                    nodes.push(Node::Text(text));
                    first = false;
                }
                None => {
                    if self.sc.offset() == before {
                        // Nothing consumed: avoid spinning on an unparseable byte.
                        return self.err(
                            format!(
                                "unexpected `{}`",
                                self.sc.rest().chars().next().unwrap_or(' ')
                            ),
                            Span::at(self.sc.pos()),
                        );
                    }
                }
            }
        }
    }

    /// Decides whether a line-leading keyword begins Rust rather than text.
    ///
    /// A bare `if` in prose — "if you like" — must stay text, so a block
    /// keyword only counts when a `{` actually follows on the same construct,
    /// and a statement keyword only when a `;` does.
    fn control_ahead(&self) -> Option<Control> {
        const BLOCKS: &[&str] = &["if", "for", "while", "match", "loop", "unsafe"];
        const STMTS: &[&str] = &["let", "return", "break", "continue"];

        for kw in STMTS {
            if self.sc.at_keyword(kw) {
                return self.terminator_ahead(b';').then_some(Control::Stmt);
            }
        }
        for kw in BLOCKS {
            if self.sc.at_keyword(kw) {
                return self.terminator_ahead(b'{').then_some(Control::Block(kw));
            }
        }

        // A bare statement — `n -= 1;`, `buf.push(x);`, `log!("hi");` — has no
        // keyword to recognise it by, so it is identified by shape instead:
        // a path, then something only Rust would write after one, then a `;`
        // before any markup. Prose does not look like that, and the
        // alternative is emitting the statement into the page as text.
        (self.statement_ahead() && self.terminator_ahead(b';')).then_some(Control::Stmt)
    }

    /// Whether the cursor is on something shaped like a Rust expression
    /// statement rather than on prose.
    fn statement_ahead(&self) -> bool {
        let mut sc = self.sc.clone();

        // An assignment through a dereference: `*slot = x;`.
        while matches!(sc.peek(), Some(b'*') | Some(b'&')) {
            sc.bump();
            sc.skip_ws();
        }

        if sc.read_ident().is_empty() {
            return false;
        }
        while sc.starts_with("::") {
            sc.advance(2);
            if sc.read_ident().is_empty() {
                return false;
            }
        }
        // Whether anything separates the path from what follows it. Nobody
        // writes `buf .push(x)`, so requiring the call or field access to be
        // flush against the path keeps prose such as "Bash (the shell); see
        // below" from reading as a function call.
        let before = sc.offset();
        sc.skip_ws();
        let spaced = sc.offset() != before;

        // What follows a path in a statement, but not in a sentence.
        const COMPOUND: &[&str] = &["+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="];
        if COMPOUND.iter().any(|op| sc.starts_with(op)) {
            return true;
        }
        match sc.peek() {
            // `=` alone is an assignment; `==` is a comparison, which is not a
            // statement anybody means to write.
            Some(b'=') => sc.peek_at(1) != Some(b'='),
            Some(b'.') | Some(b'(') | Some(b'[') | Some(b'!') | Some(b'?') => !spaced,
            _ => false,
        }
    }

    /// Scans ahead for `terminator` at nesting depth zero, giving up at the
    /// first thing that looks like a tag.
    fn terminator_ahead(&self, terminator: u8) -> bool {
        let mut sc = self.sc.clone();
        let mut paren = 0i32;
        let mut bracket = 0i32;
        while !sc.eof() {
            if sc.starts_with("//") || sc.starts_with("/*") {
                sc.skip_trivia();
                continue;
            }
            if sc.skip_literal() {
                continue;
            }
            let b = match sc.peek() {
                Some(b) => b,
                None => break,
            };
            if b == b'<'
                && matches!(sc.peek_at(1), Some(n) if n.is_ascii_alphabetic() || n == b'/' || n == b'>' || n == b'!')
            {
                return false;
            }
            if b == terminator && paren == 0 && bracket == 0 {
                return true;
            }
            // A `;` before the opening brace means this was a statement, not a
            // block, and vice versa.
            if terminator == b'{' && b == b';' && paren == 0 && bracket == 0 {
                return false;
            }
            if terminator == b';' && b == b'}' && paren == 0 && bracket == 0 {
                return false;
            }
            match b {
                b'(' => paren += 1,
                b')' => paren -= 1,
                b'[' => bracket += 1,
                b']' => bracket -= 1,
                _ => {}
            }
            sc.bump();
        }
        false
    }

    // ----------------------------------------------------------------- text

    fn parse_text(&mut self, ctx: Ctx) -> Result<Option<Text>> {
        let from = self.sc.pos();
        let start = self.sc.offset();

        while !self.sc.eof() {
            let b = self.sc.peek().unwrap();
            if b == b'<' || b == b'{' {
                break;
            }
            if ctx == Ctx::Block && b == b'}' {
                break;
            }
            if self.sc.at_line_start() && self.sc.offset() > start {
                if self.control_ahead().is_some() {
                    break;
                }
                if self.sc.starts_with("//") || self.sc.starts_with("/*") {
                    break;
                }
            }
            self.sc.bump();
        }

        let raw = self.sc.slice(start, self.sc.offset());
        Ok(normalize_text(raw, ctx == Ctx::Children).map(|value| Text {
            span: Span::new(from, self.sc.pos()),
            value,
        }))
    }

    // --------------------------------------------------------- interpolation

    fn parse_interpolation(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.bump(); // {
        let raw = self.sc.peek() == Some(b'!');
        if raw {
            self.sc.bump();
        }
        let source = self.read_rust(Stop::CloseBrace).trim().to_string();
        if self.sc.peek() != Some(b'}') {
            return self.err(
                "unterminated `{` expression",
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        let span = self.sc.span_from(from);

        if source.is_empty() {
            return Err(Error::new("empty `{}` expression", span)
                .with_help("write the value to render between the braces, as in `{ user.name }`"));
        }

        let expr = Expr { span, source };
        Ok(if raw {
            Node::RawExpr(expr)
        } else {
            Node::Expr(expr)
        })
    }

    // --------------------------------------------------------------- markup

    /// Parses whatever a `<` introduces, or returns `None` when it introduces
    /// nothing and should be treated as text.
    fn parse_markup(&mut self) -> Result<Option<Node>> {
        if self.sc.starts_with("<!--") {
            return Ok(Some(self.parse_html_comment()?));
        }
        if self.sc.starts_with("<!") {
            return Ok(Some(self.parse_doctype()?));
        }
        if self.sc.starts_with("<>") {
            return Ok(Some(self.parse_fragment()?));
        }
        match self.sc.peek_at(1) {
            Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
            _ => return Ok(None),
        }
        Ok(Some(self.parse_element()?))
    }

    fn parse_html_comment(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.advance(4);
        let start = self.sc.offset();
        while !self.sc.eof() && !self.sc.starts_with("-->") {
            self.sc.bump();
        }
        let value = self.sc.slice(start, self.sc.offset()).to_string();
        if !self.sc.eat("-->") {
            return self.err(
                "unterminated `<!--` comment",
                Span::new(from, self.sc.pos()),
            );
        }
        Ok(Node::Comment(Text {
            span: self.sc.span_from(from),
            value,
        }))
    }

    fn parse_doctype(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        let start = self.sc.offset();
        while !self.sc.eof() && self.sc.peek() != Some(b'>') {
            self.sc.bump();
        }
        if self.sc.peek() != Some(b'>') {
            return self.err(
                "unterminated `<!` declaration",
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        Ok(Node::Doctype(Text {
            span: self.sc.span_from(from),
            value: self.sc.slice(start, self.sc.offset()).to_string(),
        }))
    }

    fn parse_fragment(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.advance(2); // <>
        let children = self.parse_nodes(Ctx::Children)?;
        if !self.sc.eat("</>") {
            return Err(
                Error::new("unclosed fragment", Span::new(from, self.sc.pos()))
                    .with_help("a fragment opened with `<>` is closed with `</>`"),
            );
        }
        Ok(Node::Fragment(Fragment {
            span: self.sc.span_from(from),
            children,
        }))
    }

    fn parse_element(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.bump(); // <
        let tag_start = self.sc.pos();
        let tag = self.read_tag_name();
        if tag.is_empty() {
            return self.err("expected a tag name after `<`", Span::at(tag_start));
        }

        let is_component = tag
            .rsplit("::")
            .next()
            .and_then(|s| s.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase())
            || tag.contains("::");

        let (attrs, self_closing) = self.parse_attributes(&tag)?;

        if is_component {
            let children = if self_closing {
                Vec::new()
            } else {
                let children = self.parse_nodes(Ctx::Children)?;
                self.expect_closing_tag(&tag, from)?;
                children
            };
            return Ok(Node::Call(Call {
                span: self.sc.span_from(from),
                path: tag,
                attrs,
                children,
                has_children_block: !self_closing,
            }));
        }

        let void = is_void_element(&tag);
        let raw_text = is_raw_text_element(&tag);

        let children = if self_closing || void {
            Vec::new()
        } else if raw_text {
            let children = self.parse_raw_text(&tag)?;
            self.expect_closing_tag(&tag, from)?;
            children
        } else {
            let children = self.parse_nodes(Ctx::Children)?;
            self.expect_closing_tag(&tag, from)?;
            children
        };

        Ok(Node::Element(Element {
            span: self.sc.span_from(from),
            tag,
            attrs,
            children,
            self_closing,
            void,
            raw_text,
        }))
    }

    fn expect_closing_tag(&mut self, tag: &str, open_from: Pos) -> Result<()> {
        if !self.sc.starts_with("</") {
            return Err(Error::new(
                format!("unclosed `<{tag}>`"),
                Span::new(open_from, self.sc.pos()),
            )
            .with_help(format!("add `</{tag}>`")));
        }
        let close_from = self.sc.pos();
        self.sc.advance(2);
        let name = self.read_tag_name();
        self.sc.skip_ws();
        if self.sc.peek() != Some(b'>') {
            return self.err(
                format!("unterminated closing tag `</{name}`"),
                Span::new(close_from, self.sc.pos()),
            );
        }
        self.sc.bump();
        if name != tag {
            return Err(Error::new(
                format!("closing tag `</{name}>` does not match `<{tag}>`"),
                Span::new(close_from, self.sc.pos()),
            )
            .with_help(format!("`<{tag}>` was opened at {open_from}")));
        }
        Ok(())
    }

    fn read_tag_name(&mut self) -> String {
        let start = self.sc.offset();
        while let Some(b) = self.sc.peek() {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                self.sc.bump();
            } else if b == b':' && self.sc.peek_at(1) == Some(b':') {
                self.sc.advance(2);
            } else if b == b':' {
                // A namespaced HTML name such as `xlink:href`.
                self.sc.bump();
            } else {
                break;
            }
        }
        self.sc.slice(start, self.sc.offset()).to_string()
    }

    /// Parses attributes up to `>` or `/>`, reporting whether the tag closed
    /// itself.
    fn parse_attributes(&mut self, tag: &str) -> Result<(Vec<Attribute>, bool)> {
        let mut attrs = Vec::new();
        loop {
            self.sc.skip_ws();
            if self.sc.eof() {
                return self.err(
                    format!("unterminated `<{tag}` tag"),
                    Span::at(self.sc.pos()),
                );
            }
            if self.sc.starts_with("/>") {
                self.sc.advance(2);
                return Ok((attrs, true));
            }
            if self.sc.peek() == Some(b'>') {
                self.sc.bump();
                return Ok((attrs, false));
            }
            if self.sc.peek() == Some(b'{') {
                attrs.push(self.parse_spread()?);
                continue;
            }

            let from = self.sc.pos();
            let name = self.read_attr_name();
            if name.is_empty() {
                return self.err(
                    format!(
                        "unexpected `{}` in `<{tag}>`",
                        self.sc.rest().chars().next().unwrap_or(' ')
                    ),
                    Span::at(self.sc.pos()),
                );
            }

            if self.sc.eat("?=") {
                let expr = self.read_attr_expr(&name, from)?;
                attrs.push(Attribute::Conditional {
                    span: self.sc.span_from(from),
                    name,
                    expr,
                });
            } else if self.sc.peek() == Some(b'=') {
                self.sc.bump();
                match self.sc.peek() {
                    Some(b'{') => {
                        let expr = self.read_attr_expr(&name, from)?;
                        attrs.push(Attribute::Expr {
                            span: self.sc.span_from(from),
                            name,
                            expr,
                        });
                    }
                    Some(q @ (b'"' | b'\'')) => {
                        self.sc.bump();
                        let start = self.sc.offset();
                        while !self.sc.eof() && self.sc.peek() != Some(q) {
                            self.sc.bump();
                        }
                        let value = self.sc.slice(start, self.sc.offset()).to_string();
                        if self.sc.peek() != Some(q) {
                            return self.err(
                                format!("unterminated value for attribute `{name}`"),
                                Span::new(from, self.sc.pos()),
                            );
                        }
                        self.sc.bump();
                        attrs.push(Attribute::Static {
                            span: self.sc.span_from(from),
                            name,
                            value,
                        });
                    }
                    _ => {
                        // An unquoted value, as HTML allows.
                        let start = self.sc.offset();
                        while let Some(b) = self.sc.peek() {
                            if b.is_ascii_whitespace() || b == b'>' || b == b'/' {
                                break;
                            }
                            self.sc.bump();
                        }
                        let value = self.sc.slice(start, self.sc.offset()).to_string();
                        if value.is_empty() {
                            return self.err(
                                format!("attribute `{name}` has no value after `=`"),
                                Span::new(from, self.sc.pos()),
                            );
                        }
                        attrs.push(Attribute::Static {
                            span: self.sc.span_from(from),
                            name,
                            value,
                        });
                    }
                }
            } else {
                attrs.push(Attribute::Bare {
                    span: self.sc.span_from(from),
                    name,
                });
            }
        }
    }

    fn read_attr_expr(&mut self, name: &str, from: Pos) -> Result<String> {
        if self.sc.peek() != Some(b'{') {
            return self.err(
                format!("expected `{{` after `{name}=`"),
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        let expr = self.read_rust(Stop::CloseBrace).trim().to_string();
        if self.sc.peek() != Some(b'}') {
            return self.err(
                format!("unterminated expression for attribute `{name}`"),
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        if expr.is_empty() {
            return self.err(
                format!("attribute `{name}` has an empty expression"),
                Span::new(from, self.sc.pos()),
            );
        }
        Ok(expr)
    }

    fn parse_spread(&mut self) -> Result<Attribute> {
        let from = self.sc.pos();
        self.sc.bump(); // {
        self.sc.skip_ws();
        if !self.sc.eat("...") {
            return Err(Error::new(
                "expected `...` in a spread attribute",
                Span::new(from, self.sc.pos()),
            )
            .with_help("spread a collection of attributes with `{ ...attrs }`"));
        }
        let expr = self.read_rust(Stop::CloseBrace).trim().to_string();
        if self.sc.peek() != Some(b'}') {
            return self.err(
                "unterminated spread attribute",
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        if expr.is_empty() {
            return self.err("empty spread attribute", Span::new(from, self.sc.pos()));
        }
        Ok(Attribute::Spread {
            span: self.sc.span_from(from),
            expr,
        })
    }

    fn read_attr_name(&mut self) -> String {
        let start = self.sc.offset();
        while let Some(b) = self.sc.peek() {
            match b {
                b'=' | b'/' | b'>' | b'<' | b'"' | b'\'' | b'{' | b'}' => break,
                b if b.is_ascii_whitespace() => break,
                b if b == b'?' && self.sc.peek_at(1) == Some(b'=') => break,
                _ => {
                    self.sc.bump();
                }
            }
        }
        self.sc.slice(start, self.sc.offset()).to_string()
    }

    /// Reads the body of a `<script>` or `<style>` element.
    ///
    /// The content is character data, so `<` does not open a tag and `{` does
    /// not interpolate — otherwise a CSS rule or a JavaScript block would be
    /// misread. `{! expr }` is still honoured, as a deliberate escape hatch
    /// that is vanishingly unlikely to occur in real CSS or JavaScript.
    fn parse_raw_text(&mut self, tag: &str) -> Result<Vec<Node>> {
        let closing = format!("</{tag}");
        let mut nodes = Vec::new();
        let mut from = self.sc.pos();
        let mut start = self.sc.offset();

        loop {
            if self.sc.eof() {
                break;
            }
            if self.sc.rest().len() >= closing.len()
                && self.sc.rest()[..closing.len()].eq_ignore_ascii_case(&closing)
            {
                break;
            }
            if self.sc.starts_with("{!") {
                let text = self.sc.slice(start, self.sc.offset());
                if !text.is_empty() {
                    nodes.push(Node::Text(Text {
                        span: Span::new(from, self.sc.pos()),
                        value: text.to_string(),
                    }));
                }
                nodes.push(self.parse_interpolation()?);
                from = self.sc.pos();
                start = self.sc.offset();
                continue;
            }
            self.sc.bump();
        }

        let text = self.sc.slice(start, self.sc.offset());
        if !text.is_empty() {
            nodes.push(Node::Text(Text {
                span: Span::new(from, self.sc.pos()),
                value: text.to_string(),
            }));
        }
        Ok(nodes)
    }

    // --------------------------------------------------------- control flow

    fn parse_control_block(&mut self, kw: &str) -> Result<Node> {
        match kw {
            "if" => self.parse_if(),
            "for" => self.parse_for(),
            "while" => self.parse_while(),
            "match" => self.parse_match(),
            "loop" | "unsafe" => self.parse_bare_block(kw),
            _ => unreachable!("unhandled control keyword `{kw}`"),
        }
    }

    fn parse_if(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.eat_keyword("if");
        let cond = self.read_rust(Stop::OpenBrace).trim().to_string();
        let then_body = self.parse_braced_body("if", from)?;

        let mut else_ifs = Vec::new();
        let mut else_body = None;

        loop {
            let save = self.sc.pos();
            self.sc.skip_trivia();
            if !self.sc.eat_keyword("else") {
                self.sc.restore(save);
                break;
            }
            self.sc.skip_trivia();
            if self.sc.eat_keyword("if") {
                let branch_from = save;
                let cond = self.read_rust(Stop::OpenBrace).trim().to_string();
                let body = self.parse_braced_body("else if", branch_from)?;
                else_ifs.push(ElseIf {
                    span: self.sc.span_from(branch_from),
                    cond,
                    body,
                });
            } else {
                else_body = Some(self.parse_braced_body("else", save)?);
                break;
            }
        }

        Ok(Node::If(If {
            span: self.sc.span_from(from),
            cond,
            then_body,
            else_ifs,
            else_body,
        }))
    }

    fn parse_for(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.eat_keyword("for");
        let header = self.read_rust(Stop::OpenBrace).trim().to_string();
        let body = self.parse_braced_body("for", from)?;
        Ok(Node::For(For {
            span: self.sc.span_from(from),
            header,
            body,
        }))
    }

    fn parse_while(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.eat_keyword("while");
        let header = self.read_rust(Stop::OpenBrace).trim().to_string();
        let body = self.parse_braced_body("while", from)?;
        Ok(Node::While(While {
            span: self.sc.span_from(from),
            header,
            body,
        }))
    }

    /// `loop { … }` and `unsafe { … }`, which have no header.
    fn parse_bare_block(&mut self, kw: &str) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.eat_keyword(kw);
        let header = self.read_rust(Stop::OpenBrace).trim().to_string();
        if !header.is_empty() {
            return self.err(
                format!("unexpected `{header}` after `{kw}`"),
                Span::new(from, self.sc.pos()),
            );
        }
        let body = self.parse_braced_body(kw, from)?;
        Ok(Node::While(While {
            span: self.sc.span_from(from),
            header: if kw == "loop" {
                "loop".to_string()
            } else {
                "unsafe".to_string()
            },
            body,
        }))
    }

    fn parse_braced_body(&mut self, what: &str, from: Pos) -> Result<Vec<Node>> {
        if self.sc.peek() != Some(b'{') {
            return self.err(
                format!("expected `{{` to open the `{what}` body"),
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        let body = self.parse_nodes(Ctx::Block)?;
        if self.sc.peek() != Some(b'}') {
            return self.err(
                format!("unclosed `{what}` body"),
                Span::new(from, self.sc.pos()),
            );
        }
        self.sc.bump();
        Ok(body)
    }

    fn parse_match(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        self.sc.eat_keyword("match");
        let subject = self.read_rust(Stop::OpenBrace).trim().to_string();
        if self.sc.peek() != Some(b'{') {
            return self.err(
                "expected `{` to open the `match` body",
                Span::at(self.sc.pos()),
            );
        }
        self.sc.bump();

        let mut arms = Vec::new();
        loop {
            self.sc.skip_trivia();
            if self.sc.peek() == Some(b'}') {
                self.sc.bump();
                break;
            }
            if self.sc.eof() {
                return self.err("unclosed `match` body", Span::new(from, self.sc.pos()));
            }

            let arm_from = self.sc.pos();
            let pattern = self.read_rust(Stop::FatArrow).trim().to_string();
            if !self.sc.eat("=>") {
                return self.err(
                    "expected `=>` after the match pattern",
                    Span::new(arm_from, self.sc.pos()),
                );
            }
            self.sc.skip_trivia();

            let body = if self.sc.peek() == Some(b'{') {
                self.parse_braced_body("match arm", arm_from)?
            } else {
                match self.parse_arm_expr_body(arm_from)? {
                    Some(node) => vec![node],
                    None => Vec::new(),
                }
            };

            self.sc.skip_trivia();
            if self.sc.peek() == Some(b',') {
                self.sc.bump();
            }

            arms.push(MatchArm {
                span: self.sc.span_from(arm_from),
                pattern,
                body,
            });
        }

        Ok(Node::Match(Match {
            span: self.sc.span_from(from),
            subject,
            arms,
        }))
    }

    /// A match arm written without braces, as in `Role::Admin => <b>Admin</b>,`.
    fn parse_arm_expr_body(&mut self, arm_from: Pos) -> Result<Option<Node>> {
        match self.sc.peek() {
            Some(b'<') => self.parse_markup(),
            Some(b'{') => self.parse_interpolation().map(Some),
            _ => Err(Error::new(
                "a match arm body must be markup or a `{ }` expression",
                Span::new(arm_from, self.sc.pos()),
            )
            .with_help("wrap several nodes in braces: `pattern => { <p>one</p> <p>two</p> }`")),
        }
    }

    fn parse_stmt(&mut self) -> Result<Node> {
        let from = self.sc.pos();
        let start = self.sc.offset();
        self.read_rust(Stop::CloseBrace);
        // `read_rust` with CloseBrace stops at `;` too; consume it.
        if self.sc.peek() == Some(b';') {
            self.sc.bump();
        }
        let source = self.sc.slice(start, self.sc.offset()).trim().to_string();
        Ok(Node::Stmt(Stmt {
            span: self.sc.span_from(from),
            source,
        }))
    }

    // ------------------------------------------------------- rust fragments

    /// Reads Rust source up to `stop`, at nesting depth zero.
    fn read_rust(&mut self, stop: Stop) -> String {
        let start = self.sc.offset();
        let mut paren = 0i32;
        let mut bracket = 0i32;
        let mut brace = 0i32;
        let mut angle = 0i32;

        loop {
            if self.sc.eof() {
                break;
            }
            if self.sc.starts_with("//") {
                while let Some(b) = self.sc.bump() {
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }
            if self.sc.starts_with("/*") {
                self.sc.skip_block_comment();
                continue;
            }
            if self.sc.skip_literal() {
                continue;
            }

            // `->` and `=>` are not angle brackets.
            if (self.sc.starts_with("->") || self.sc.starts_with("=>")) && stop != Stop::FatArrow {
                self.sc.advance(2);
                continue;
            }

            let b = self.sc.peek().unwrap();
            let flat =
                paren == 0 && bracket == 0 && brace == 0 && (!stop.tracks_angles() || angle == 0);

            if flat {
                match stop {
                    Stop::OpenBrace if b == b'{' => break,
                    Stop::CloseBrace if b == b'}' || b == b';' => break,
                    Stop::FatArrow if b == b'=' && self.sc.peek_at(1) == Some(b'>') => break,
                    Stop::ParamEnd if b == b',' || b == b')' => break,
                    _ => {}
                }
            }

            match b {
                b'(' => paren += 1,
                b')' => paren -= 1,
                b'[' => bracket += 1,
                b']' => bracket -= 1,
                b'{' => brace += 1,
                b'}' => brace -= 1,
                b'<' if stop.tracks_angles() => angle += 1,
                b'>' if stop.tracks_angles() => angle -= 1,
                _ => {}
            }
            self.sc.bump();
        }

        self.sc.slice(start, self.sc.offset()).to_string()
    }
}

/// Extracts the names a chunk of Rust brings into scope.
///
/// Only `use` declarations matter, and only enough of them to resolve a
/// component name to the file that declares it. Anything this does not
/// understand simply yields no names, which costs a cross-file check rather
/// than producing a wrong one.
fn parse_use_decls(source: &str, span: Span) -> Vec<UseDecl> {
    let mut out = Vec::new();
    for statement in source.split(';') {
        let statement = strip_comments(statement);
        let trimmed = statement.trim();
        let Some(rest) = trimmed.strip_prefix("use ") else {
            continue;
        };
        // `pub use` re-exports a name without bringing it into scope here, but
        // treating it as an import is harmless and often what was meant.
        let rest = rest.trim();
        collect_use_tree(rest, &[], span, &mut out);
    }
    out
}

/// Walks one `use` tree, which may branch at a `{…}` group.
fn collect_use_tree(path: &str, prefix: &[String], span: Span, out: &mut Vec<UseDecl>) {
    let path = path.trim();
    if path.is_empty() {
        return;
    }

    if let Some(open) = path.find('{') {
        let Some(close) = path.rfind('}') else {
            return;
        };
        let head = path[..open].trim().trim_end_matches("::");
        let mut nested = prefix.to_vec();
        nested.extend(split_path(head));
        for branch in split_top_level_commas(&path[open + 1..close]) {
            collect_use_tree(&branch, &nested, span, out);
        }
        return;
    }

    let mut segments = prefix.to_vec();
    segments.extend(split_path(path));
    let Some(last) = segments.pop() else {
        return;
    };

    // `self` inside a group means the group's own module: `a::{self, B}`.
    let (name, module) = if last == "self" {
        match segments.last() {
            Some(name) => (name.clone(), segments.clone()),
            None => return,
        }
    } else if let Some((name, alias)) = last.split_once(" as ") {
        let _ = name;
        (alias.trim().to_string(), segments.clone())
    } else {
        (last, segments.clone())
    };

    let module = module
        .into_iter()
        .filter(|segment| segment != "crate" && segment != "self")
        .collect();

    out.push(UseDecl { span, name, module });
}

fn split_path(path: &str) -> Vec<String> {
    path.split("::")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Splits a `use` group on commas that are not inside a nested group.
fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Removes line comments, so a commented-out `use` is not indexed.
fn strip_comments(s: &str) -> String {
    s.lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Normalises an HTML text run, returning `None` when nothing survives.
///
/// Runs of whitespace collapse to a single space, as HTML itself would render
/// them. Whether an edge run survives depends on whether it spans a line:
///
/// * `<p>\n    Hello\n</p>` — the edge runs are indentation, and go.
/// * `<p>{n} items</p>` — the space before `items` is between words, and stays.
/// * `<a>x</a> <a>y</a>` — a whitespace-only run on one line is a real space,
///   but only between elements (`inline`); around a block's braces it is not.
///
/// Dropping every edge run, which is the simpler rule, silently glues `{n}` to
/// `items`; keeping every edge run leaves indentation in the output.
fn normalize_text(raw: &str, inline: bool) -> Option<String> {
    if raw.is_empty() {
        return None;
    }

    let lead_end = raw.len() - raw.trim_start().len();
    let leading = &raw[..lead_end];
    let core = raw.trim();
    let trailing = &raw[lead_end + core.len()..];

    let keep_leading = !leading.is_empty() && !leading.contains('\n');
    let keep_trailing = !trailing.is_empty() && !trailing.contains('\n');

    if core.is_empty() {
        // The whole run is whitespace. Between two elements on one line that
        // is a real word separator; in a block body — around the braces of a
        // component or an `if` — it is only layout.
        return (inline && keep_leading).then(|| " ".to_string());
    }

    let mut out = String::with_capacity(raw.len());
    if keep_leading {
        out.push(' ');
    }
    let mut in_ws = false;
    for ch in core.chars() {
        if ch.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws {
                out.push(' ');
            }
            in_ws = false;
            out.push(ch);
        }
    }
    if keep_trailing {
        out.push(' ');
    }
    Some(out)
}
