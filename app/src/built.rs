//! What stands where, from the latest snapshot, for the build tools.

use mofact_core::content::{BuildingRole, Content};
use mofact_core::geom::TilePos;
use mofact_core::ids::BuildingId;
use mofact_core::snapshot::Snapshot;
use mofact_input::{Ground, Occupant};
use std::collections::HashMap;

pub struct Built {
    /// Footprint size and whether it's a belt, the hub, per building.
    kinds: HashMap<BuildingId, ((u8, u8), bool, bool)>,
    tiles: HashMap<TilePos, Occupant>,
    tick: Option<u64>,
}

impl Built {
    pub fn new(content: &Content) -> Self {
        let kinds = content
            .buildings
            .iter()
            .map(|b| {
                let belt = matches!(b.role, BuildingRole::Belt { .. });
                let hub = matches!(b.role, BuildingRole::Hub);
                (b.id.clone(), (b.size, belt, hub))
            })
            .collect();
        Self {
            kinds,
            tiles: HashMap::new(),
            tick: None,
        }
    }

    /// Catch up with `snap`; cheap when it hasn't changed.
    pub fn update(&mut self, snap: &Snapshot) {
        if self.tick == Some(snap.tick) {
            return;
        }
        self.tick = Some(snap.tick);
        self.tiles.clear();
        for e in &snap.entities {
            let Some(&((w, h), belt, hub)) = self.kinds.get(&e.building) else {
                continue;
            };
            let occupant = Occupant {
                id: e.id,
                belt,
                dir: e.dir,
                removable: !hub,
            };
            for dy in 0..i32::from(h) {
                for dx in 0..i32::from(w) {
                    self.tiles
                        .insert(TilePos::new(e.pos.x + dx, e.pos.y + dy), occupant);
                }
            }
        }
    }
}

impl Ground for Built {
    fn at(&self, tile: TilePos) -> Option<Occupant> {
        self.tiles.get(&tile).copied()
    }
}
