//! A rough throughput check for the generated rendering path.
//!
//! Run with `cargo run --release --example bench`.

use std::time::Instant;

use rsx::Template;

rsx::load!("examples/bench.rsx");

fn main() {
    let rows: Vec<Row> = (0..200)
        .map(|id| Row {
            id,
            name: format!("row number {id}"),
            // Something that actually needs escaping, so the escape path is
            // measured rather than skipped.
            note: format!("a & b <{id}> \"quoted\""),
        })
        .collect();

    let size = Table { rows: &rows }.render_to_string().unwrap().len();
    let iterations = 2_000;

    // Fresh allocation each time: what a request handler would do.
    let start = Instant::now();
    for _ in 0..iterations {
        let html = Table { rows: &rows }.render_to_string().unwrap();
        std::hint::black_box(&html);
    }
    let fresh = start.elapsed();

    // Reused buffer: what a server with a pool would do.
    let mut buffer = Vec::with_capacity(size);
    let start = Instant::now();
    for _ in 0..iterations {
        buffer.clear();
        let mut w = rsx::Writer::streaming(&mut buffer);
        Table { rows: &rows }.render(&mut w).unwrap();
        w.finish().unwrap();
        std::hint::black_box(&buffer);
    }
    let reused = start.elapsed();

    println!("{} rows -> {size} bytes", rows.len());
    println!(
        "fresh String   {:>8.1} us/render   {:>6.1} MB/s",
        fresh.as_secs_f64() * 1e6 / iterations as f64,
        (size * iterations) as f64 / fresh.as_secs_f64() / 1e6,
    );
    println!(
        "reused buffer  {:>8.1} us/render   {:>6.1} MB/s",
        reused.as_secs_f64() * 1e6 / iterations as f64,
        (size * iterations) as f64 / reused.as_secs_f64() / 1e6,
    );
}
