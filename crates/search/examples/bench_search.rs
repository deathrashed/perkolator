//! Measures index build time and per-query latency on synthetic documents.
//!
//! Run: `cargo run --release -p perkolator-search --example bench_search`
//! Numbers are measurements on the current machine, not guarantees.

use std::time::{Duration, Instant};

use perkolator_search::{Boosts, Query, SearchIndex, synthetic};

const SIZES: &[usize] = &[10_000, 100_000];
const RUNS_PER_QUERY: usize = 25;
const LIMIT: usize = 50;

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    let rank = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[rank]
}

fn main() {
    let boosts = Boosts::default();
    for &size in SIZES {
        let started = Instant::now();
        let index = SearchIndex::from_docs(synthetic::docs(size));
        println!("\n{size} docs: build {:.1} ms", millis(started.elapsed()));
        println!(
            "{:>22} {:>6} {:>9} {:>9}",
            "query", "hits", "p50 ms", "p95 ms"
        );
        let mut all = Vec::new();
        for input in synthetic::QUERIES {
            let query = Query::parse(input);
            let mut samples = Vec::with_capacity(RUNS_PER_QUERY);
            let mut hits = 0;
            for _ in 0..RUNS_PER_QUERY {
                let started = Instant::now();
                hits = index.search(&query, &boosts, LIMIT).len();
                samples.push(started.elapsed());
            }
            samples.sort_unstable();
            println!(
                "{:>22} {hits:>6} {:>9.3} {:>9.3}",
                format!("{input:?}"),
                millis(percentile(&samples, 0.5)),
                millis(percentile(&samples, 0.95))
            );
            all.extend(samples);
        }
        all.sort_unstable();
        println!(
            "{:>22} {:>6} {:>9.3} {:>9.3}",
            "ALL QUERIES",
            "",
            millis(percentile(&all, 0.5)),
            millis(percentile(&all, 0.95))
        );
    }
}
