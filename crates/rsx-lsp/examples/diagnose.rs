//! Prints how a template resolves to a Rust host file, and optionally asks
//! `rust-analyzer` what it knows at an offset.
//!
//! The bridge falls back silently by design — a missing analyzer must not
//! break an editor — which makes it awkward to find out *why* nothing came
//! back. This reports every step instead.
//!
//! ```text
//! cargo run -p rsx-lsp --example diagnose -- path/to/card.rsx 150
//! ```
use std::path::Path;

fn main() {
    let arg = std::env::args()
        .nth(1)
        .expect("usage: diagnose <file.rsx> [offset]");
    let path = Path::new(&arg);
    println!("template : {}", path.display());
    println!("crate    : {:?}", rsx_lsp::bridge::crate_root(path));

    let options = rsx_core::Options {
        header: false,
        ..Default::default()
    };
    match rsx_lsp::bridge::resolve(path, &options) {
        None => println!("host     : none"),
        Some(host) => {
            println!("host     : {}", host.path.display());
            println!("shift    : {}", host.shift);
            println!("fragments: {}", host.map.fragments().len());
            if let Some(offset) = std::env::args().nth(2).and_then(|s| s.parse().ok()) {
                println!("offset {offset} -> host {:?}", host.to_host_offset(offset));
            }
            if let Some(offset) = std::env::args().nth(2).and_then(|s| s.parse().ok()) {
                probe(path, offset);
            }
        }
    }
}

/// Runs the same steps the bridge does, but reports every error instead of
/// falling back, so a failure says what went wrong.
fn probe(path: &Path, offset: usize) {
    let options = rsx_core::Options {
        header: false,
        ..Default::default()
    };
    let host = rsx_lsp::bridge::resolve(path, &options).expect("no host");
    let host_offset = host.to_host_offset(offset).expect("offset does not map");
    let (line, character) = rsx_lsp::rust::offset_to_position(&host.text, host_offset);
    println!("host position: {line}:{character}");

    let root = rsx_lsp::bridge::crate_root(path).expect("no crate root");
    println!("spawning rust-analyzer in {}", root.display());
    let started = std::time::Instant::now();
    let mut analyzer = match rsx_lsp::rust_analyzer::RustAnalyzer::spawn("rust-analyzer", &root) {
        Ok(analyzer) => analyzer,
        Err(error) => {
            println!("spawn failed: {error}");
            return;
        }
    };
    println!("initialized in {:.1}s", started.elapsed().as_secs_f32());
    let primed = analyzer.wait_until_primed();
    println!(
        "primed: {primed} after {:.1}s",
        started.elapsed().as_secs_f32()
    );

    if let Err(error) = analyzer.overlay(&host.path, &host.text) {
        println!("overlay failed: {error}");
        return;
    }
    match analyzer.completion(&host.path, line, character) {
        Ok(result) => {
            let items = result
                .get("items")
                .and_then(|i| i.as_array().cloned())
                .or_else(|| result.as_array().cloned())
                .unwrap_or_default();
            println!("completion returned {} items", items.len());
            for item in items.iter().take(10) {
                println!("   {} | {}", item["label"], item["detail"]);
            }
        }
        Err(error) => println!("completion failed: {error}"),
    }
    analyzer.shutdown();
}
