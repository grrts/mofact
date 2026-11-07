//! Cloud saves (T62): the save slots follow the player to another device.
//!
//! A platform keeps the slots in its own cloud behind a [`Cloud`]: Play
//! Games saved games on Android, iCloud on iOS. Neither is wired up yet,
//! as both need the store accounts; until then there is no cloud and the
//! slots stay on the device. The game decides what goes where:
//!
//! - At start, a slot the cloud holds a further-played game for is
//!   brought down before anything loads.
//! - Going to the background sends the slot being played up.
//!
//! Nothing waits on the network (T64): a cloud answers from what it has
//! already, and sends when it can.

use crate::slots::Store;
use mofact_core::content::Content;
use mofact_sim::Sim;
use mofact_ui::SLOTS;

/// Where a platform's cloud keeps the save slots.
pub trait Cloud {
    /// The save the cloud holds for `slot`, as far as it knows right now.
    /// Never waits on the network.
    fn saved(&mut self, slot: u8) -> Option<Vec<u8>>;
    /// Send the save for `slot` up when the network allows.
    fn upload(&mut self, slot: u8, bytes: &[u8]);
}

/// Whether `remote` should replace `local`: it loads, and it has been
/// played further (more ticks). A damaged cloud save never wins, and on a
/// tie the device keeps its own.
pub fn newer(content: &Content, local: Option<&[u8]>, remote: &[u8]) -> bool {
    let ticks = |b: &[u8]| Sim::load(content, b).ok().map(|(s, _)| s.snapshot().tick);
    let Some(theirs) = ticks(remote) else {
        return false;
    };
    local.and_then(ticks).is_none_or(|ours| theirs > ours)
}

/// Bring down every slot the cloud has played further. Returns the slots
/// replaced.
pub fn pull(content: &Content, cloud: &mut dyn Cloud, store: &mut dyn Store) -> Vec<u8> {
    let mut replaced = Vec::new();
    for slot in 0..SLOTS {
        let Some(remote) = cloud.saved(slot) else {
            continue;
        };
        if newer(content, store.read(slot).as_deref(), &remote)
            && store.write(slot, &remote).is_ok()
        {
            replaced.push(slot);
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slots::Memory;
    use std::collections::HashMap;

    /// A cloud in memory, as another device left it.
    #[derive(Default)]
    struct Shelf(HashMap<u8, Vec<u8>>);

    impl Cloud for Shelf {
        fn saved(&mut self, slot: u8) -> Option<Vec<u8>> {
            self.0.get(&slot).cloned()
        }
        fn upload(&mut self, slot: u8, bytes: &[u8]) {
            self.0.insert(slot, bytes.to_vec());
        }
    }

    fn played(content: &Content, ticks: u32) -> Vec<u8> {
        let mut sim = Sim::with_content(content);
        for _ in 0..ticks {
            sim.step();
        }
        sim.save()
    }

    #[test]
    fn the_further_played_game_wins_and_a_damaged_one_never_does() {
        let content = crate::content().unwrap();
        let (early, late) = (played(&content, 10), played(&content, 50));
        let mut cloud = Shelf::default();
        cloud.upload(0, &late);
        cloud.upload(1, &early);
        cloud.upload(2, b"not a save");
        let mut store = Memory::default();
        store.slots[0] = Some(early.clone());
        store.slots[1] = Some(late.clone());
        assert_eq!(pull(&content, &mut cloud, &mut store), vec![0]);
        assert_eq!(store.slots[0].as_deref(), Some(&late[..]));
        assert_eq!(
            store.slots[1].as_deref(),
            Some(&late[..]),
            "the device's own was further"
        );
        assert_eq!(store.slots[2], None);
        // An empty slot takes whatever good save the cloud has.
        let mut store = Memory::default();
        assert_eq!(pull(&content, &mut cloud, &mut store), vec![0, 1]);
    }
}
