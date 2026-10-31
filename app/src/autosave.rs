//! Autosave (T11): every 30 seconds while playing, and whenever the app goes
//! to the background, where a phone may kill it without warning. Killing the
//! app therefore loses at most 30 seconds of play.
//!
//! The simulation thread encodes the save between two ticks; the main thread
//! only writes the bytes, to a temporary file renamed over the old save, so a
//! crash mid-write never leaves a broken save behind.

use mofact_core::content::Content;
use mofact_sim::{Sim, SimHandle};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub const EVERY: Duration = Duration::from_secs(30);

pub struct Autosave {
    path: PathBuf,
    last: Instant,
    pending: Option<Receiver<Vec<u8>>>,
}

impl Autosave {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            last: Instant::now(),
            pending: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Call once a frame: asks for a save when one is due and writes it when
    /// it arrives. Never blocks.
    pub fn update(&mut self, sim: &SimHandle) -> Result<(), String> {
        if self.pending.is_none() && self.last.elapsed() >= EVERY {
            self.pending = Some(sim.save());
        }
        if let Some(bytes) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.pending = None;
            self.last = Instant::now();
            write(&self.path, &bytes)?;
        }
        Ok(())
    }

    /// Save right now and wait for it, as when going to the background.
    pub fn now(&mut self, sim: &SimHandle) -> Result<(), String> {
        let bytes = sim
            .save()
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "the simulation did not answer the save".to_string())?;
        self.pending = None;
        self.last = Instant::now();
        write(&self.path, &bytes)
    }
}

/// Load the save at `path`, if there is one. Problems are reported, not fatal:
/// a damaged save means starting fresh, never failing to start.
pub fn load(path: &Path, content: &Content) -> Option<Sim> {
    let bytes = std::fs::read(path).ok()?;
    match Sim::load(content, &bytes) {
        Ok((sim, warnings)) => {
            for w in warnings {
                eprintln!("mofact: {}: {w}", path.display());
            }
            Some(sim)
        }
        Err(e) => {
            eprintln!("mofact: {}: {e}; starting a new game", path.display());
            None
        }
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("saving to {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(fail)?;
    }
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(fail)?;
        f.write_all(bytes).map_err(fail)?;
        f.sync_all().map_err(fail)?;
    }
    std::fs::rename(&tmp, path).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_replace_the_old_save_whole() {
        let dir = std::env::temp_dir().join(format!("mofact-autosave-{}", std::process::id()));
        let path = dir.join("slot.mofact");
        write(&path, b"first").unwrap();
        write(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(!path.with_extension("tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
