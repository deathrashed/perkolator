//! Read-only smoke run: `cargo run -p perkolator-registry --example scan_home [HOME] [PROJECT..]`.
//! Prints counts, timing and problem paths only (no file contents or secrets).

use std::path::PathBuf;
use std::time::Instant;

use perkolator_core::{CancelToken, InstallState};
use perkolator_registry::{Registry, ScanConfig, scan};

const MAX_PROBLEMS_SHOWN: usize = 15;

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let Some(home) = args.next().or_else(dirs::home_dir) else {
        eprintln!("no home directory");
        return;
    };
    let projects: Vec<PathBuf> = args.collect();
    let config = ScanConfig::well_known(&home, &projects);
    println!("roots: {}", config.roots.len());
    for root in &config.roots {
        println!("  {:<12} {}", root.provider, root.path.display());
    }
    let started = Instant::now();
    let report = scan(&config, &CancelToken::new());
    let elapsed = started.elapsed();
    let registry = Registry::from_report(&report);
    println!("scanned in {elapsed:?}: {} records", registry.len());
    for (kind, count) in registry.count_by_kind() {
        println!("  {:<12} {count}", kind.plural_label());
    }
    let linked = registry
        .iter()
        .flat_map(|record| record.deployments())
        .filter(|location| location.state == InstallState::Linked)
        .count();
    let with_errors = registry.iter().filter(|record| record.has_errors()).count();
    println!("linked deployments: {linked}, records with errors: {with_errors}");
    println!(
        "problems: {}, broken links: {}",
        report.problems.len(),
        report.broken_links.len()
    );
    for problem in report.problems.iter().take(MAX_PROBLEMS_SHOWN) {
        println!("  {}: {}", problem.path.display(), problem.message);
    }
}
