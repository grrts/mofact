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

    /// Save to `path` from now on, as when another slot is played. A save
    /// still on its way belonged to the old game, so it is dropped.
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
        self.pending = None;
        self.last = Instant::now();
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

/// The save slots as files (T63): the first is the autosave file, the
/// others sit beside it, and a small file remembers which is played.
pub struct Files {
    first: PathBuf,
}

impl Files {
    pub fn new(first: PathBuf) -> Self {
        Self { first }
    }

    /// The file for `slot`.
    pub fn path(&self, slot: u8) -> PathBuf {
        if slot == 0 {
            self.first.clone()
        } else {
            self.first
                .with_file_name(format!("slot-{}.mofact", u32::from(slot) + 1))
        }
    }

    fn marker(&self) -> PathBuf {
        self.first.with_extension("slot")
    }

    /// The settings that outlive a game: low-power and colour-blind mode,
    /// as `(low_power, colour_blind)`. Missing or unreadable means off.
    pub fn settings(&self) -> (bool, bool) {
        let text = std::fs::read_to_string(self.first.with_file_name("settings.txt"));
        let text = text.unwrap_or_default();
        let on = |key: &str| text.lines().any(|l| l.trim() == format!("{key} = on"));
        (on("low-power"), on("colour-blind"))
    }

    /// Whether the full game was bought, as remembered on this device.
    pub fn full_game(&self) -> bool {
        self.first.with_file_name("full-game").exists()
    }

    /// Remember that the full game was bought.
    pub fn keep_full_game(&self) -> impl FnMut(bool) + 'static {
        let path = self.first.with_file_name("full-game");
        move |on| {
            let done = if on {
                write(&path, b"owned\n")
            } else {
                std::fs::remove_file(&path).map_err(|e| e.to_string())
            };
            if let Err(e) = done {
                eprintln!("mofact: couldn't remember the full game: {e}");
            }
        }
    }

    pub fn set_settings(&self, (low_power, colour_blind): (bool, bool)) -> Result<(), String> {
        let flag = |on: bool| if on { "on" } else { "off" };
        let text = format!(
            "low-power = {}\ncolour-blind = {}\n",
            flag(low_power),
            flag(colour_blind)
        );
        write(&self.first.with_file_name("settings.txt"), text.as_bytes())
    }
}

impl crate::slots::Store for Files {
    fn read(&self, slot: u8) -> Option<Vec<u8>> {
        std::fs::read(self.path(slot)).ok()
    }
    fn write(&mut self, slot: u8, bytes: &[u8]) -> Result<(), String> {
        write(&self.path(slot), bytes)
    }
    fn remove(&mut self, slot: u8) {
        let _ = std::fs::remove_file(self.path(slot));
    }
    fn current(&self) -> u8 {
        std::fs::read_to_string(self.marker())
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|n| crate::slots::valid(*n))
            .unwrap_or(0)
    }
    fn set_current(&mut self, slot: u8) {
        let _ = write(&self.marker(), slot.to_string().as_bytes());
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

    #[test]
    fn slots_are_files_beside_the_autosave_and_the_played_one_is_remembered() {
        use crate::slots::Store;
        let dir = std::env::temp_dir().join(format!("mofact-slots-{}", std::process::id()));
        let mut files = Files::new(dir.join("autosave.mofact"));
        assert_eq!(files.current(), 0);
        files.write(0, b"one").unwrap();
        files.write(2, b"three").unwrap();
        assert_eq!(files.path(2), dir.join("slot-3.mofact"));
        assert_eq!(files.read(0).as_deref(), Some(&b"one"[..]));
        assert_eq!(files.read(1), None);
        files.set_current(2);
        assert_eq!(Files::new(dir.join("autosave.mofact")).current(), 2);
        files.remove(2);
        assert_eq!(files.read(2), None);
        assert_eq!(files.settings(), (false, false));
        files.set_settings((false, true)).unwrap();
        assert_eq!(files.settings(), (false, true));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
