//! Content validator and balance report: reachability, ratios, chain depth and
//! by-product sinks for data/*.ron (R1, R3, R6).
//!
//! For now it loads the content and runs the loader's referential checks, so CI
//! fails on broken content. The balance report arrives in T25.
//!
//! Usage: `mofact-balance [DATA_DIR]` (default `data`).

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "data".into()));
    match mofact_content::load_dir(&dir) {
        Ok(c) => {
            println!(
                "{}: {} items, {} recipes, {} buildings, no problems",
                dir.display(),
                c.items.len(),
                c.recipes.len(),
                c.buildings.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
