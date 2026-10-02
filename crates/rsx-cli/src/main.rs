//! The `rsx` command line interface.
//!
//! The proc macro is the usual way to compile templates, so this binary exists
//! for the cases the macro cannot serve: inspecting the generated Rust, wiring
//! `.rsx` into a build system that is not cargo, and checking templates in CI
//! without compiling the crate around them.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rsx_core::project::{self, CompiledFile};
use rsx_core::{Error, Options};

const USAGE: &str = "\
rsx — compile .rsx templates to Rust

Usage:
  rsx generate [paths...]   Write <name>.rs beside each <name>.rsx (default: .)
  rsx check [paths...]      Parse and generate without writing anything
  rsx print <path>          Write the generated Rust for one file to stdout
  rsx lsp                   Start the language server on stdin/stdout
  rsx help                  Show this message

Options:
  --out-dir <dir>   Write generated files under <dir>, mirroring the input tree
  --runtime <path>  Path to the runtime crate in generated code (default: ::rsx)
  --no-header       Omit the \"do not edit\" banner
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut options = Options::default();
    let mut out_dir: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out-dir" => {
                let value = iter.next().ok_or("--out-dir needs a directory")?;
                out_dir = Some(PathBuf::from(value));
            }
            "--runtime" => {
                options.runtime = iter.next().ok_or("--runtime needs a path")?.clone();
            }
            "--no-header" => options.header = false,
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(ExitCode::SUCCESS);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option `{other}`\n\n{USAGE}"));
            }
            other => positional.push(other.to_string()),
        }
    }

    let (command, paths) = match positional.split_first() {
        None => ("generate", Vec::new()),
        Some((first, rest)) => (first.as_str(), rest.to_vec()),
    };

    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };

    match command {
        "help" => {
            print!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        "lsp" => match rsx_lsp::run() {
            Ok(()) => Ok(ExitCode::SUCCESS),
            Err(e) => Err(format!("language server stopped: {e}")),
        },
        "generate" => compile(&roots, &options, out_dir.as_deref(), Action::Write),
        "check" => compile(&roots, &options, None, Action::Check),
        "print" => compile(&roots, &options, None, Action::Print),
        // With no subcommand, treat everything as a path to generate.
        other => {
            let mut roots = vec![PathBuf::from(other)];
            roots.extend(paths.iter().map(PathBuf::from));
            compile(&roots, &options, out_dir.as_deref(), Action::Write)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Write,
    Check,
    Print,
}

fn compile(
    roots: &[PathBuf],
    options: &Options,
    out_dir: Option<&Path>,
    action: Action,
) -> Result<ExitCode, String> {
    let mut failed = false;

    for root in roots {
        let compilation = match project::compile_dir(root, options) {
            Ok(c) => c,
            Err(error) => {
                report(&error);
                failed = true;
                continue;
            }
        };

        for file in &compilation.files {
            match action {
                Action::Check => {}
                Action::Print => print!("{}", file.code),
                Action::Write => {
                    let destination = destination(root, file, out_dir);
                    if let Some(parent) = destination.parent() {
                        if let Err(e) = fs::create_dir_all(parent) {
                            return Err(format!("{}: {e}", parent.display()));
                        }
                    }
                    if let Err(e) = fs::write(&destination, &file.code) {
                        return Err(format!("{}: {e}", destination.display()));
                    }
                    println!("{} -> {}", file.path.display(), destination.display());
                }
            }
        }

        if action == Action::Check {
            println!(
                "checked {} file{} ({} component{})",
                compilation.files.len(),
                plural(compilation.files.len()),
                compilation.registry.len(),
                plural(compilation.registry.len()),
            );
        }
    }

    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Where a generated file goes: beside its source, or mirrored under
/// `--out-dir`.
fn destination(root: &Path, file: &CompiledFile, out_dir: Option<&Path>) -> PathBuf {
    match out_dir {
        None => file.path.with_extension("rs"),
        Some(dir) => {
            let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
            dir.join(relative).with_extension("rs")
        }
    }
}

fn report(error: &Error) {
    let source = error
        .path
        .as_ref()
        .and_then(|p| fs::read_to_string(p).ok())
        .unwrap_or_default();
    let mut rendered = String::new();
    let _ = write!(rendered, "{}", error.render(&source));
    eprint!("{rendered}");
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}
