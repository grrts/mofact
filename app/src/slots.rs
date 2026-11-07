//! Save slots (T63): three games side by side. A platform keeps the bytes
//! (files on desktop and phones, local storage in the browser) behind a
//! [`Store`]; the game decides what goes where.

use mofact_core::content::Content;
use mofact_sim::Sim;
use mofact_ui::{tr, trf, SlotView, SLOTS};
use mofact_world::Preset;

/// Where the slots live.
pub trait Store {
    /// The save in `slot`, if there is one.
    fn read(&self, slot: u8) -> Option<Vec<u8>>;
    fn write(&mut self, slot: u8, bytes: &[u8]) -> Result<(), String>;
    fn remove(&mut self, slot: u8);
    /// The slot being played, remembered between runs.
    fn current(&self) -> u8;
    fn set_current(&mut self, slot: u8);
}

/// What the game menu asked of the slots, for [`crate::game::Game::serve`].
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Save, then list the slots afresh.
    Refresh,
    Load(u8),
    Delete(u8),
    New {
        slot: u8,
        seed: u64,
        preset: u8,
    },
}

/// A slot as the menu lists it: what map it is on and how far it has got.
pub fn summary(content: &Content, bytes: Option<&[u8]>, current: bool) -> SlotView {
    let Some(sim) = bytes
        .and_then(|b| Sim::load(content, b).ok())
        .map(|(s, _)| s)
    else {
        return SlotView {
            current,
            ..SlotView::default()
        };
    };
    describe(&sim, current)
}

/// The words for a game: its map, then its play time and size.
pub fn describe(sim: &Sim, current: bool) -> SlotView {
    let (seed, preset) = sim.world_seed().unwrap_or((42, Preset::Standard));
    let snap = sim.snapshot();
    let seconds = snap.tick / u64::from(mofact_core::TICKS_PER_SECOND);
    let played = match (seconds / 3600, seconds / 60 % 60) {
        (0, 0) => tr("slot.just_started").to_string(),
        (0, m) => trf("slot.minutes", &[&m]),
        (h, m) => trf("slot.hours", &[&h, &m]),
    };
    let built = snap.entities.len();
    SlotView {
        used: true,
        current,
        title: trf(
            "slot.title",
            &[&mofact_ui::preset_name(preset as u8), &seed],
        ),
        detail: trf("slot.detail", &[&played, &built]),
    }
}

/// A slot number the store can hold.
pub fn valid(slot: u8) -> bool {
    slot < SLOTS
}

/// A store in memory, for tests and screenshots.
#[derive(Default)]
pub struct Memory {
    pub slots: [Option<Vec<u8>>; SLOTS as usize],
    pub current: u8,
}

impl Store for Memory {
    fn read(&self, slot: u8) -> Option<Vec<u8>> {
        self.slots.get(usize::from(slot))?.clone()
    }
    fn write(&mut self, slot: u8, bytes: &[u8]) -> Result<(), String> {
        let s = self
            .slots
            .get_mut(usize::from(slot))
            .ok_or("no such slot")?;
        *s = Some(bytes.to_vec());
        Ok(())
    }
    fn remove(&mut self, slot: u8) {
        if let Some(s) = self.slots.get_mut(usize::from(slot)) {
            *s = None;
        }
    }
    fn current(&self) -> u8 {
        self.current
    }
    fn set_current(&mut self, slot: u8) {
        self.current = slot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_says_what_map_it_is_on_and_how_far_it_has_got() {
        let content = crate::content().unwrap();
        let (mut sim, _) = crate::game::new_game(&content, 707).unwrap();
        sim.set_world_gen(707, Preset::Lakes);
        for _ in 0..90 * mofact_core::TICKS_PER_SECOND {
            sim.step();
        }
        let mut store = Memory::default();
        store.write(1, &sim.save()).unwrap();
        let shown = summary(&content, store.read(1).as_deref(), false);
        assert!(shown.used && !shown.current);
        assert_eq!(shown.title, "Lakes · seed 707");
        assert!(
            shown.detail.starts_with("1 min played · "),
            "{}",
            shown.detail
        );
        // An empty or damaged slot shows as empty.
        assert!(!summary(&content, store.read(2).as_deref(), false).used);
        assert!(!summary(&content, Some(b"junk"), true).used);
        assert!(!valid(SLOTS));
    }
}
