//! Crash reports with no analytics: when the game panics, on any thread,
//! the report is written next to the saves (in local storage on the web)
//! and nothing is sent anywhere. Next start, the game says so, and the
//! game menu can copy the report for the player to paste into an email or
//! a bug report if they choose.

use std::fmt::Write as _;

/// Where a report is kept, by the platform.
pub trait Keep: Send + Sync {
    fn write(&self, report: &str);
}

/// The words of a report: what panicked where, on what build.
pub fn report(message: &str, location: Option<String>, backtrace: &str) -> String {
    let mut r = String::new();
    let _ = writeln!(r, "Mofact {} crash report", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        r,
        "Platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(
        r,
        "Thread: {}",
        std::thread::current().name().unwrap_or("unnamed")
    );
    let _ = writeln!(r, "Panic: {message}");
    if let Some(at) = location {
        let _ = writeln!(r, "At: {at}");
    }
    if !backtrace.trim().is_empty() {
        let _ = writeln!(r, "\n{backtrace}");
    }
    r
}

/// The panic's message, whichever type it was raised with.
fn message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into())
}

/// Write a report through `keep` on every panic, after the usual message.
pub fn install(keep: Box<dyn Keep>) {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        before(info);
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        keep.write(&report(&message(info.payload()), at, &trace));
    }));
}

/// A report kept in a file.
pub struct File(pub std::path::PathBuf);

impl Keep for File {
    fn write(&self, report: &str) {
        let _ = std::fs::write(&self.0, report);
    }
}

impl File {
    /// The report from last time, if the game crashed, and forget it.
    pub fn take(&self) -> Option<String> {
        let text = std::fs::read_to_string(&self.0).ok()?;
        let _ = std::fs::remove_file(&self.0);
        (!text.trim().is_empty()).then_some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct Memory(Arc<Mutex<Vec<String>>>);

    impl Keep for Memory {
        fn write(&self, report: &str) {
            self.0.lock().unwrap().push(report.to_owned());
        }
    }

    #[test]
    fn a_panic_on_any_thread_leaves_a_report_and_nothing_else() {
        let kept = Arc::new(Mutex::new(Vec::new()));
        install(Box::new(Memory(kept.clone())));
        let died = std::thread::Builder::new()
            .name("sim".into())
            .spawn(|| panic!("belt {} jammed", 7))
            .unwrap()
            .join();
        let _ = std::panic::take_hook();
        assert!(died.is_err());
        let kept = kept.lock().unwrap();
        let r = kept.last().expect("a report");
        assert!(r.starts_with("Mofact "), "{r}");
        assert!(r.contains("Thread: sim"), "{r}");
        assert!(r.contains("Panic: belt 7 jammed"), "{r}");
        assert!(r.contains("crash.rs"), "where it happened: {r}");
    }

    #[test]
    fn a_report_is_shown_once() {
        let dir = std::env::temp_dir().join(format!("mofact-crash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = File(dir.join("crash.txt"));
        assert_eq!(f.take(), None);
        f.write("Mofact crash report\nPanic: x\n");
        assert!(f.take().unwrap().contains("Panic: x"));
        assert_eq!(f.take(), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
