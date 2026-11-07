//! `mofact-bot [DATA_DIR] [HOURS]`: play a new game through to the
//! megaproject, giving up after HOURS of game time (default 120). Exits
//! non-zero if the bot can't finish, or finishes in under 80 hours: a full
//! playthrough is meant to take about 100. The nightly workflow runs it.
//!
//! `mofact-bot --estimate [DATA_DIR]`: work out how long that playthrough
//! takes from the content alone, step by step, without playing it, and
//! exit non-zero outside 80 to 120 hours. CI runs it on every change.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

/// A full playthrough takes about 100 hours of game time; these are the
/// bounds the checks hold it to.
const SHORTEST: f64 = 80.0;
const LONGEST: u64 = 120;

fn clock(seconds: f64) -> String {
    let s = seconds as u64;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let estimate = args.first().is_some_and(|a| a == "--estimate");
    if estimate {
        args.remove(0);
    }
    let mut args = args.into_iter();
    let data = PathBuf::from(args.next().unwrap_or_else(|| "data".into()));
    let hours = args.next().and_then(|h| h.parse().ok()).unwrap_or(LONGEST);
    let content = match mofact_content::load_dir(&data) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let start = Instant::now();
    let mut bot = match mofact_bot::bot::Bot::new(&content, hours) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    if estimate {
        let plan = match bot.goal() {
            Ok(goal) => bot.plan(&goal),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        };
        return match mofact_bot::estimate::estimate(&content, &plan) {
            Ok(steps) => {
                let mut total = 0.0;
                for s in steps {
                    total += s.seconds;
                    println!(
                        "[{}] {:<34} {:>9} limited by {}",
                        clock(total),
                        s.what,
                        clock(s.seconds),
                        s.limit
                    );
                }
                println!("{} in all", clock(total));
                let hours = total / 3600.0;
                if (SHORTEST..=LONGEST as f64).contains(&hours) {
                    ExitCode::SUCCESS
                } else {
                    println!("FAIL a playthrough should take {SHORTEST} to {LONGEST} hours");
                    ExitCode::FAILURE
                }
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let result = bot.play();
    println!(
        "{} of game time, {} entities, {:.1?} to run",
        bot.clock(),
        bot.entities(),
        start.elapsed()
    );
    let hours = bot.sim.tick() as f64 / f64::from(mofact_core::TICKS_PER_SECOND) / 3600.0;
    match result {
        Ok(()) if hours < SHORTEST => {
            println!("FAIL the game was won in under {SHORTEST} hours");
            ExitCode::FAILURE
        }
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("FAIL {e}");
            ExitCode::FAILURE
        }
    }
}
