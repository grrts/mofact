//! Content validator and balance report: reachability, ratios, chain depth and
//! by-product sinks for data/*.ron (R1, R3, R6). See the library for what
//! each check means. Exits with failure on any problem, so CI catches broken
//! content.
//!
//! Usage: `mofact-balance [DATA_DIR]` (default `data`).

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "data".into()));
    let content = match mofact_content::load_dir(&dir) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "{}: {} items, {} recipes, {} buildings\n",
        dir.display(),
        content.items.len(),
        content.recipes.len(),
        content.buildings.len()
    );
    let report = mofact_balance::analyse(&content);
    print!("{}", report.text());
    if report.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
