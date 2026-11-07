//! `mofact-bot [DATA_DIR] [HOURS]`: play a new game through to the
//! megaproject, giving up after HOURS of game time (default 48). Exits
//! non-zero if the bot can't finish. The nightly workflow runs it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let data = PathBuf::from(args.next().unwrap_or_else(|| "data".into()));
    let hours = args.next().and_then(|h| h.parse().ok()).unwrap_or(48);
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
    let result = bot.play();
    println!(
        "{} of game time, {} entities, {:.1?} to run",
        bot.clock(),
        bot.entities(),
        start.elapsed()
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("FAIL {e}");
            ExitCode::FAILURE
        }
    }
}
