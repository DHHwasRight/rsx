# rsx

Compile-time HTML templates for Rust. A `.rsx` file is Rust — `use`
declarations, items, functions — except that a function returning `Element`
has a markup body. Templates are compiled into the binary when the crate is
built; nothing is read from disk at runtime, and nothing is interpreted.

```rust
use crate::models::User;

pub fn Card(title: &str, count: usize) -> Element {
    <div class="card">
        <h2>{title}</h2>
        <span>{count}</span>
    </div>
}

pub fn Page(user: &User) -> Element {
    let greeting = format!("Hi, {}", user.name);
    <Layout title="Home">
        <p>{greeting}</p>
        if user.verified {
            <span class="badge">Verified</span>
        }
        for post in &user.posts {
            <Card title={&post.title} count={post.views} />
        }
    </Layout>
}
```

## Getting started

```toml
[dependencies]
rsx = { path = "crates/rsx" }
```

```rust
// src/views.rs
rsx::load_dir!("src/views");        // every .rsx under the directory
// or
rsx::load!("src/views/card.rsx");   // one file
```

```rust
use rsx::Template;

let html = views::card::Card { title: "Hi", count: 2 }.render_to_string()?;
```

Paths are relative to the crate root (`CARGO_MANIFEST_DIR`). No `build.rs` is
involved: the macro compiles the templates during expansion and emits an
`include_bytes!` for every file it reads, which is what makes cargo rebuild
when a template changes.

## Syntax

### Components

A component is a function returning `Element`. Its name must start with an
uppercase letter, because that is how markup tells a component from an HTML
element. Visibility, generics, `where` clauses and `mut` parameters are
ordinary Rust.

```rust
pub fn Badge(label: &str) -> Element {
    <span class="badge">{label}</span>
}

pub(crate) fn List<T: std::fmt::Display>(items: &[T]) -> Element
where
    T: Clone,
{
    <ul>
        for item in items {
            <li>{item}</li>
        }
    </ul>
}
```

Anything that is not such a function — `use`, `struct`, `impl`, a plain `fn` —
passes straight through to the generated module.

### Children

A parameter of type `Element` receives markup:

```rust
pub fn Layout(title: &str, children: Element) -> Element {
    <html>
        <head><title>{title}</title></head>
        <body>{children}</body>
    </html>
}

pub fn Home() -> Element {
    <Layout title="Home">
        <p>content</p>
    </Layout>
}
```

A component with a children parameter that is called without a block renders
nothing there. A component with no such parameter rejects a children block at
compile time.

### Text and expressions

| Syntax | Renders |
| --- | --- |
| `plain text` | verbatim, with character references preserved |
| `{ expr }` | the value's `Display` output, HTML-escaped |
| `{! expr }` | the value's `Display` output, **not** escaped |

Any `Display` type works, and nothing is allocated to render one: formatting
goes straight into the output buffer through an escaping adapter.

`{! }` is how you render HTML you built elsewhere. Passing user input through
it is an XSS vulnerability.

Static text is escaped, but an author writing HTML means the entities they
type. A bare `&` becomes `&amp;`, while `&amp;`, `&nbsp;` and `&#8212;` are
left as the references they already are — so `AT&T` and `Tom &amp; Jerry` both
come out right.

Whitespace follows HTML: runs collapse to a single space, indentation around a
text run is dropped, and a space between two words or two inline elements is
kept. `<p>{n} items</p>` renders `3 items`,

```rust
<p>
    Hello
</p>
```

renders `<p>Hello</p>`, and `<b>a</b> <i>b</i>` keeps the space between them.

### Attributes

```rust
<input
    class="field"              // static
    value={user.name}          // expression, HTML-escaped
    required                   // bare
    checked?={is_checked}      // rendered only when the expression is true
    { ...extra }               // spread a collection of attributes
>
```

Attribute names are taken as written, so `data-id`, `aria-label`, `@click` and
`:class` all work, and so does `xlink:href` on an SVG element.

A spread takes anything iterable as key/value pairs — a `HashMap`, a
`BTreeMap`, a slice of tuples. A `bool` value renders as a bare attribute when
true and is skipped when false; anything else renders as `name="value"`.

On a component call, a spread is Rust's struct-update syntax instead:
`<Card title="a" { ...defaults } />`.

### Control flow

Markup elements are statements, and Rust's control flow contains them:

```rust
<ul class="feed">
    if user.verified {
        <span class="badge">Verified</span>
    } else if user.pending {
        <span>Pending</span>
    }

    for (i, item) in items.iter().enumerate() {
        <li data-index={i}>{item}</li>
    }

    match status {
        Status::Ok => <b>ok</b>,
        Status::Failed(code) => {
            <span>failed</span>
            <code>{code}</code>
        }
        _ => <i>unknown</i>,
    }
</ul>
```

`let`, `while`, `loop` and bare statements such as `n -= 1;` or
`buf.push(x);` work too. Nothing is collected into an intermediate tree: a
loop writes each iteration straight into the output.

#### How Rust is told from text

Inside markup, a keyword is read as Rust only at the start of a line or as the
first thing in a block, and only when the construct it opens actually follows.
So this is text:

```rust
<p>
    if you ask me, this is prose
</p>
```

because no `{` follows. The same check keeps `Bash (the shell); see below`
from reading as a function call — a statement's `(` has to be flush against
its name, the way Rust is written.

One case needs care. `if a<b {` has a `<` followed by a letter, which reads as
a tag. Write `if a < b {`, with spaces, and it parses as the comparison it is.

### Fragments, comments, doctype

```rust
pub fn Head(title: &str) -> Element {
    <!DOCTYPE html>
    <>
        <!-- passed through -->
        <meta name="title" content={title}>
        <title>{title}</title>
    </>
}
```

### `<script>` and `<style>`

Their content is character data, so `<` does not open a tag, `{` does not
interpolate, and nothing is escaped — a CSS rule or a JavaScript block
survives intact:

```rust
<style>body { color: red; }</style>
<script>if (a < b) { go("x"); }</script>
```

`{! expr }` is still honoured there, as a deliberate escape hatch.
Interpolating untrusted data into a script is an injection vulnerability; HTML
escaping does not make JavaScript safe.

## What gets generated

Each component produces four items. For `pub fn Card(title: &str, count: usize)`:

```rust
// The primitive: writes into a buffer, with every run of static HTML
// batched into one call.
pub fn card(__w: &mut rsx::Writer<'_>, title: &str, count: usize) -> rsx::Result {
    __w.raw("<div class=\"card\"><h2>")?;
    __w.text(&(title))?;
    __w.raw("</h2><span>")?;
    __w.text(&(count))?;
    __w.raw("</span></div>")?;
    Ok(())
}

// The value form, for composing.
pub struct Card<'a> { pub title: &'a str, pub count: usize }
impl<'a> rsx::Template for Card<'a> { /* calls card() */ }
pub fn Card<'a>(title: &'a str, count: usize) -> Card<'a> { /* … */ }
```

Both call styles are available: `Card { title: "Hi", count: 2 }` and
`Card("Hi", 2)`. Markup calls go through the struct, which is why importing a
component is a plain `use path::to::Card;` — the snake_case function never has
to be in scope.

A braced struct lives only in the type namespace and a function only in the
value namespace, so the struct and the constructor can share a name.

Interpolation borrows rather than moves, so `{row.name}` works when `name` is
a `String` reached through a shared reference.

### Resolving component names

A call is checked against the component it would actually reach: the one
declared in the same file, or the one a `use` brought in. Two files may each
declare a `Card` without colliding. A name that cannot be resolved — a
component written by hand in Rust, or one from another crate — is passed
through for `rustc` to check.

## Runtime

```rust
pub trait Template {
    fn render(self, w: &mut Writer<'_>) -> Result;
    fn render_to_string(self) -> Result<String>;
    fn render_to(self, sink: &mut dyn io::Write) -> Result;
}
```

`render` takes `self` by value, so a component can own its parameters without
requiring `Copy`. For a heterogeneous collection there is `DynTemplate`, which
is object safe: `Vec<Box<dyn DynTemplate>>`.

`Writer` always appends to an owned `Vec<u8>`, which keeps `raw` and `text`
free of virtual dispatch. Built with `Writer::streaming(sink)` it hands the
buffer to an `io::Write` once it passes a chunk size, so a large page streams
instead of being held whole.

`Template` is implemented for `()` (renders nothing), `Option<T>` and `Vec<T>`,
and `rsx::from_fn` wraps a closure as one.

Rendering a 200-row table that needs escaping runs at roughly 1.5–2 GB/s,
whether into a fresh `String` or a reused buffer — about 13–17 µs for a 25 KB
page (`cargo run --release -p rsx --example bench`).

## Editor support

`rsx lsp` is a language server for `.rsx` files, and `editors/vscode` is a
VS Code extension that starts it.

- diagnostics as you type, including the ones only code generation finds, such
  as a prop a component does not declare
- hover for components, parameters, elements and expressions
- completion after `<` for components and HTML elements, and inside a tag for
  props and attributes
- go to definition on a component call
- document symbols for the components a file declares
- **field and method completion inside expressions**, and hover with real
  types — `{ user.| }` offers `name: String` and `posts: Vec<String>`

### Rust inside templates

The last one is not this server's own work. A cursor in `{ user.posts.| }` is
asking a Rust question, and the honest answer is that `rust-analyzer` already
knows it. So the question is forwarded.

That works because every Rust fragment in a template is copied verbatim into
the generated code, and the source map records the byte range each one landed
in. A position in a template therefore has an exact counterpart in real Rust —
not merely the same line, but the same character, which is what completion
after a `.` needs.

The file put to `rust-analyzer` is one that genuinely belongs to your crate,
with its contents overlaid in memory, so the surrounding types, imports and
dependencies are real:

| How your templates reach the crate | The file asked about |
| --- | --- |
| `rsx generate` wrote `card.rs` beside `card.rsx` | that generated file |
| `rsx::load_dir!("src/views")` in `src/views.rs` | `src/views.rs`, with the macro call replaced by its expansion |

Everything about this degrades quietly. No `rust-analyzer` on `PATH`, a
template outside any crate, a position in markup rather than Rust — each just
falls back to what the template server knows on its own. The first question
that needs Rust waits for the crate to finish indexing, which takes a second
or two; later ones return immediately. A sibling template that does not parse
is skipped rather than taking the feature down with it.

| Setting | Default | Meaning |
| --- | --- | --- |
| `rsx.server.path` | `rsx` | The binary providing the language server |
| `rsx.server.enabled` | `true` | Set to `false` for highlighting only |
| `rsx.rustAnalyzer.enabled` | `true` | Consult `rust-analyzer` for Rust expressions |
| `rsx.rustAnalyzer.path` | `rust-analyzer` | Which analyzer to run |

## CLI

The macro is the usual path. The `rsx` binary exists for inspecting the
generated Rust, checking templates in CI, build systems that are not cargo,
and running the language server.

```
rsx generate [paths...]   Write <name>.rs beside each <name>.rsx
rsx check [paths...]      Parse and generate without writing
rsx print <path>          Write the generated Rust to stdout
rsx lsp                   Start the language server on stdin/stdout
```

## Crates

| Crate | What it is |
| --- | --- |
| `rsx` | the runtime, and the macros re-exported |
| `rsx-core` | parser, syntax tree, code generator and source map |
| `rsx-macros` | `load!` and `load_dir!` |
| `rsx-cli` | the `rsx` binary |
| `rsx-lsp` | the language server, and the bridge to `rust-analyzer` |

`rsx-core` and `rsx` have no dependencies; `rsx-lsp` has `serde`. `rsx-core` has no opinion on how it
is driven: `compile_source`, `compile_file` and `compile_dir` are the entry
points for any other integration, and `generate_with_map` additionally returns
a `SourceMap` from generated lines back to `.rsx` positions.

## Tests

```sh
cargo test --workspace            # the compiler, runtime and language server
cd editors/vscode && npm test     # the extension, inside a real VS Code
mise run check                    # build, clippy with -D warnings, test
```

The extension tests launch an actual VS Code with `@vscode/test-electron` and
drive it through the editor API, which is the only way to cover activation,
the language client and the stdio transport. They caught a bug no unit test
could: language clients append `--stdio` to the command they spawn, and the
CLI used to reject it, so the server failed to start in every editor.

The test that asks for field completion after a dot runs against a real
`rust-analyzer` over a real fixture crate. It is skipped, with a message,
where the binary is not installed.

`crates/rsx-core/testdata` holds `.rsx` fixtures with their generated `.rs`
committed beside them, so a change in output shows up as a reviewable diff:

```sh
UPDATE_GOLDEN=1 cargo test -p rsx-core --test golden
```

One test asserts the fixtures still exercise every kind of node, so the set
cannot quietly stop covering a feature.

## Limitations

- Parameter types that elide a lifetime inside a path — `Foo<'_>`, or a `Foo`
  that has a lifetime parameter — need it written out, because the generated
  struct cannot elide it. Plain `&T` and `&mut T` are handled.
- `impl Trait` in parameter position is rejected with a suggestion to use a
  named generic, since it cannot be a struct field.
- Parameters have no default values. Rust has none either; use `Option<T>`.
- There is no macro that takes template source inline. A proc macro receives
  its input already tokenised, and tokenising discards the spacing markup
  depends on: `<h1>Hello, {name}!</h1>` would render with a space before the
  comma.
- Diagnostics for template syntax point into the `.rsx` file. Diagnostics for
  the Rust *inside* a template come from `rustc` and point at the macro call
  site, because a proc macro cannot attach spans to a file it read. When one
  is unclear, `rsx print` shows the generated Rust, and `rsx generate` writes
  it to a file `rustc` can point into directly.

## Lineage

This is a Rust descendant of [`crx`](../../crystal/templ), a Crystal shard,
which was itself a descendant of Go's `templ`. The feature set is the same one
that suite covers — control flow, children, fragments, raw output, spreads,
entity-aware escaping, source maps, a language server — with the syntax moved
from Crystal to Rust and nudged toward JSX.

Three things changed deliberately along the way:

- **Whitespace.** `crx` strips the leading whitespace of every text run, which
  silently glues `{n}` to `items` in `<p>{n} items</p>`. Edge whitespace now
  survives unless its run spans a newline.
- **Bare statements.** `crx` treats a statement with no keyword as text, so it
  ends up in the page. Here `n -= 1;` is recognised by shape and emitted as
  Rust.
- **Name resolution.** `crx` resolves components through `require`; this
  resolves them through `use`, per file, so two files may each declare a
  `Card`.

The Crystal LSP also indexes Crystal types, so it can complete members after
a dot. This reaches the same place by a different route: rather than build a
second type checker, it maps the cursor into the generated Rust and asks
`rust-analyzer`, which already knows the answer and keeps knowing it as Rust
changes.
