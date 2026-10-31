//! `mofact-headless [SCENARIO_DIR] [DATA_DIR]`: run every scenario and exit
//! non-zero if any rate is off or any run is not deterministic. CI runs it on
//! every pull request.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "scenarios".into()));
    let data = PathBuf::from(args.next().unwrap_or_else(|| "data".into()));
    let content = match mofact_content::load_dir(&data) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut paths: Vec<_> = match std::fs::read_dir(&dir) {
        Ok(d) => d
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "ron"))
            .collect(),
        Err(e) => {
            eprintln!("{}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    };
    paths.sort();
    let mut failed = 0;
    for path in &paths {
        let scenario = match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| mofact_headless::parse(&t))
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                failed += 1;
                continue;
            }
        };
        let start = std::time::Instant::now();
        let (first, second) = (
            mofact_headless::run(&content, &scenario),
            mofact_headless::run(&content, &scenario),
        );
        println!("{} ({:.2?} for two runs)", scenario.name, start.elapsed());
        match first {
            Err(e) => {
                println!("  FAIL {e}");
                failed += 1;
            }
            Ok(results) => {
                for m in &results {
                    println!("  {m}");
                    failed += usize::from(!m.ok());
                }
                if second.as_ref().ok() != Some(&results) {
                    println!("  FAIL two runs gave different results");
                    failed += 1;
                }
            }
        }
    }
    if failed > 0 {
        println!("{failed} check(s) failed");
        ExitCode::FAILURE
    } else {
        println!("{} scenario(s) passed", paths.len());
        ExitCode::SUCCESS
    }
}
