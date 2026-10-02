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
rsx::load_dir!("src/views");   // every .rsx under the directory
// or
rsx::load!("src/views/card.rsx");   // one file
```

```rust
use rsx::Template;

let html = views::card::Card { title: "Hi", count: 2 }.render_to_string()?;
```

Paths are relative to the crate root (`CARGO_MANIFEST_DIR`). No `build.rs` is
involved: the macro compiles the templates during expansion and emits an
`include_bytes!` for every file it read, which is what makes cargo rebuild
when a template changes.

## Syntax

### Components

A component is a function returning `Element`. Its name must start with an
uppercase letter, because that is how markup tells a component from an HTML
element. Visibility, generics and `where` clauses are ordinary Rust.

```rust
pub fn Badge(label: &str) -> Element {
    <span class="badge">{label}</span>
}

pub(crate) fn List<T: std::fmt::Display>(items: &[T]) -> Element where T: Clone {
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
| `plain text` | verbatim, including HTML entities you write yourself |
| `{ expr }` | the value's `Display` output, HTML-escaped |
| `{! expr }` | the value's `Display` output, **not** escaped |

Any `Display` type works, and nothing is allocated to render one: formatting
goes straight into the output buffer through an escaping adapter.

`{! }` is how you render HTML you built elsewhere. Passing user input through
it is an XSS vulnerability.

Whitespace follows HTML: runs collapse to a single space, indentation around a
text run is dropped, and a space between two words or two inline elements is
kept. `<p>{n} items</p>` renders `3 items`, and

```rust
<p>
    Hello
</p>
```

renders `<p>Hello</p>`.

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
`:class` all work.

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

`let`, `while`, `loop` and bare statements work too. Nothing is collected into
an intermediate tree: a loop writes each iteration straight into the output.

A keyword is only treated as Rust at the start of a line (or at the start of a
block), and only when the construct it opens is actually there. So

```rust
<p>
    if you ask me, this is text
</p>
```

is text, because no `{` follows. One case needs care: `if a<b {` has a `<`
followed by a letter, which reads as a tag. Write `if a < b {` — with spaces —
and it parses as the comparison it is.

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

Their content is character data, so `<` does not open a tag and `{` does not
interpolate — a CSS rule or a JavaScript block survives intact:

```rust
<style>body { color: red; }</style>
<script>if (a < b) { go("x"); }</script>
```

`{! expr }` is still honoured there, as a deliberate escape hatch. Interpolating
untrusted data into a script is an injection vulnerability; HTML escaping does
not make JavaScript safe.

## What gets generated

Each component produces four items. For `pub fn Card(title: &str, count: usize)`:

```rust
// The primitive: writes into a buffer, with every run of static HTML
// batched into one call.
pub fn card(__w: &mut rsx::Writer<'_>, title: &str, count: usize) -> rsx::Result {
    __w.raw("<div class=\"card\"><h2>")?;
    __w.text(title)?;
    __w.raw("</h2><span>")?;
    __w.text(count)?;
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

## CLI

The macro is the usual path. The `rsx` binary exists for inspecting the
generated Rust, checking templates in CI, and build systems that are not cargo.

```
rsx generate [paths...]   Write <name>.rs beside each <name>.rsx
rsx check [paths...]      Parse and generate without writing
rsx print <path>          Write the generated Rust to stdout
```

## Crates

| Crate | What it is |
| --- | --- |
| `rsx` | the runtime, and the macros re-exported |
| `rsx-core` | parser, syntax tree and code generator |
| `rsx-macros` | `load!` and `load_dir!` |
| `rsx-cli` | the `rsx` binary |

`rsx-core` has no dependencies and no opinion on how it is driven:
`compile_source`, `compile_file` and `compile_dir` are the entry points for
any other integration.

## Limitations

- Parameter types that elide a lifetime inside a path — `Foo<'_>` or a `Foo`
  that has a lifetime parameter — need it written out, because the generated
  struct cannot elide it. Plain `&T` and `&mut T` are handled.
- `impl Trait` in parameter position is rejected with a suggestion to use a
  named generic, since it cannot be a struct field.
- Parameters have no default values. Rust has none either; use `Option<T>`.
- Diagnostics for template syntax point into the `.rsx` file. Diagnostics for
  the Rust *inside* a template come from `rustc` and point at the macro call
  site, because a proc macro cannot attach spans to a file it read. When one
  is unclear, `rsx print` shows the generated Rust.
