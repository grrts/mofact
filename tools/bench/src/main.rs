//! Benchmark harness: ticks the starter, R44 budget and 2× stress bases and
//! reports tick time, snapshot time, save size and memory (R44).
//!
//! Usage: `mofact-bench [--ticks N] [--only NAME] [--summary FILE] [--write DIR] [--data DIR]`
//!
//! `--summary` appends a Markdown table (CI points it at the job summary, so
//! each runner's numbers show on the run). `--write` saves the bases as
//! `.mofact` files for the device runs.

use mofact_bench::{build, time, BASES};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// The phase 1 exit bar: the budget base ticks in under 4 ms on the reference phone.
const BUDGET_TICK: Duration = Duration::from_millis(4);

fn resident_mb() -> Option<f64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096.0 / 1e6)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> ExitCode {
    let mut ticks = 600u32;
    let mut only = None;
    let mut summary = None;
    let mut write = None;
    let mut data = PathBuf::from("data");
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_default();
        match a.as_str() {
            "--ticks" => ticks = value().parse().unwrap_or(ticks),
            "--only" => only = Some(value()),
            "--summary" => summary = Some(PathBuf::from(value())),
            "--write" => write = Some(PathBuf::from(value())),
            "--data" => data = PathBuf::from(value()),
            _ => {
                eprintln!("usage: mofact-bench [--ticks N] [--only NAME] [--summary FILE] [--write DIR] [--data DIR]");
                return ExitCode::FAILURE;
            }
        }
    }
    let content = match mofact_content::load_dir(&data) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let mut table = String::from(
        "| base | belts | machines | tick mean | tick p99 | snapshot | save | memory |\n|---|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for (name, cells) in BASES {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let mut sim = match build(&content, name, cells) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        if let Some(dir) = &write {
            let path = dir.join(format!("{name}.mofact"));
            if let Err(e) =
                std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, sim.save()))
            {
                eprintln!("{}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
        // Two minutes of warm-up, so every chain is full and working.
        let t = time(&mut sim, 60 * 120, ticks);
        let mem = resident_mb().map_or("n/a".into(), |m| format!("{m:.0} MB"));
        let row = format!(
            "| {name} | {} | {} | {:.3} ms | {:.3} ms | {:.3} ms | {} KB | {mem} |\n",
            t.belts,
            t.machines,
            ms(t.mean()),
            ms(t.quantile(0.99)),
            ms(t.snapshot),
            t.save_bytes / 1024,
        );
        print!("{name}: {row}");
        table.push_str(&row);
        if name == "budget" {
            let verdict = if t.mean() < BUDGET_TICK {
                "under"
            } else {
                "OVER"
            };
            println!(
                "budget: mean tick {:.3} ms, {verdict} the 4 ms phone target on this machine",
                ms(t.mean())
            );
        }
    }
    println!("\n{table}");
    if let Some(path) = summary {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let text = format!("### Benchmarks ({os}, {arch})\n\n{table}\n");
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = written {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
