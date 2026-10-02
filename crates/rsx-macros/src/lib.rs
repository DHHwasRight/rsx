//! Build-time macros that compile `.rsx` templates into Rust.
//!
//! ```ignore
//! rsx::load!("src/views/card.rsx");   // one file, items at this position
//! rsx::load_dir!("src/views");        // a tree of `pub mod`s mirroring it
//! ```
//!
//! There is deliberately no macro that takes template source inline. A proc
//! macro receives its input already tokenised, and tokenising discards the
//! exact spacing that markup text depends on: `<h1>Hello, {name}!</h1>` would
//! come back as `< h1 > Hello , { name } ! < / h1 >` and render with a space
//! before the comma. Templates therefore live in `.rsx` files, where the
//! compiler reads the bytes the author wrote.
//!
//! Paths are resolved against `CARGO_MANIFEST_DIR`, because a proc macro
//! cannot reliably learn the path of the file that invoked it on stable Rust.
//!
//! Every file that is read is also referenced by an `include_bytes!` in the
//! expansion. That is what makes cargo rebuild when a template changes:
//! `proc_macro::tracked_path`, the API meant for this, is still unstable, and
//! without the trick a macro-based template pipeline silently serves stale
//! HTML after an edit.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro::TokenStream;
use proc_macro2::{Literal, TokenStream as TokenStream2};
use rsx_core::project::{self, Compilation};
use rsx_core::Options;

/// Compiles one `.rsx` file, placing its items where the macro is written.
#[proc_macro]
pub fn load(input: TokenStream) -> TokenStream {
    expand(input, Mode::File)
}

/// Compiles every `.rsx` file under a directory into nested `pub mod`s.
#[proc_macro]
pub fn load_dir(input: TokenStream) -> TokenStream {
    expand(input, Mode::Dir)
}

enum Mode {
    File,
    Dir,
}

fn expand(input: TokenStream, mode: Mode) -> TokenStream {
    let raw = input.to_string();
    let path =
        match string_literal(&raw) {
            Some(p) => p,
            None => return compile_error(
                "expected a string literal with a path, as in `rsx::load!(\"src/views/card.rsx\")`",
            ),
        };

    let root = manifest_dir().join(&path);
    // No banner: the header carries a file-level inner attribute, which is
    // only legal at the top of a file or module, and macro expansion happens
    // wherever the caller wrote the invocation.
    let options = Options {
        header: false,
        ..Options::default()
    };

    let compilation = match mode {
        Mode::File => project::compile_file(&root, &options).map(|file| Compilation {
            files: vec![file],
            ..Compilation::default()
        }),
        Mode::Dir => project::compile_dir(&root, &options),
    };

    let compilation = match compilation {
        Ok(c) => c,
        Err(error) => {
            let source = error
                .path
                .as_ref()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .unwrap_or_default();
            return compile_error(&error.render(&source));
        }
    };

    let tracked: Vec<PathBuf> = compilation.source_paths().map(Path::to_path_buf).collect();
    let code = match mode {
        Mode::File => compilation
            .files
            .first()
            .map(|f| f.code.clone())
            .unwrap_or_default(),
        Mode::Dir => compilation.into_module_tree(),
    };

    parse_generated(&code, &tracked)
}

/// Turns generated Rust text into tokens, attaching the dependency tracking.
fn parse_generated(code: &str, tracked: &[PathBuf]) -> TokenStream {
    let mut stream = match TokenStream2::from_str(code) {
        Ok(stream) => stream,
        Err(error) => {
            // A parse failure here is a bug in the generator rather than in the
            // template, so show the generated source to make it reportable.
            return compile_error(&format!(
                "rsx generated Rust that does not parse: {error}\n\n{code}"
            ));
        }
    };

    stream.extend(tracking_tokens(tracked));
    stream.into()
}

/// Emits one `const _: &[u8] = include_bytes!("…");` per source file.
fn tracking_tokens(paths: &[PathBuf]) -> TokenStream2 {
    let mut out = String::new();
    for path in paths {
        let literal = Literal::string(&path.to_string_lossy());
        out.push_str(&format!("const _: &[u8] = include_bytes!({literal});\n"));
    }
    TokenStream2::from_str(&out).unwrap_or_default()
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
}

/// Pulls the contents out of a single string-literal argument.
fn string_literal(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches(',').trim();
    let inner = trimmed.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\\", "\\").replace("\\\"", "\""))
}

fn compile_error(message: &str) -> TokenStream {
    let literal = Literal::string(message);
    TokenStream2::from_str(&format!("::core::compile_error!({literal});"))
        .unwrap_or_default()
        .into()
}
