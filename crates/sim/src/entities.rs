//! Flat entity storage with a per-chunk spatial index (T06).
//!
//! Entities live in one array indexed by [`EntityId`], so lookups are a
//! bounds check and iteration is in id order, which keeps the simulation
//! deterministic. Ids are never reused within a save. Each chunk keeps a
//! 64×64 grid of which entity covers each tile, so "what is at this tile"
//! never searches.

use mofact_core::geom::{ChunkPos, Dir, TilePos};
use mofact_core::ids::EntityId;
use mofact_core::CHUNK_SIZE;
use std::collections::HashMap;

/// Index of a building type in the loaded content.
pub type Kind = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entity {
    pub kind: Kind,
    /// Top-left tile of the footprint.
    pub pos: TilePos,
    pub size: (u8, u8),
    pub dir: Dir,
}

impl Entity {
    /// Every tile the entity covers.
    pub fn tiles(&self) -> impl Iterator<Item = TilePos> + '_ {
        let (w, h) = (i32::from(self.size.0), i32::from(self.size.1));
        (0..h).flat_map(move |dy| {
            (0..w).map(move |dx| TilePos::new(self.pos.x + dx, self.pos.y + dy))
        })
    }
}

const CHUNK_TILES: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

/// Which entity covers each tile of one chunk; 0 means empty, otherwise id + 1.
struct ChunkGrid {
    tiles: Box<[u32; CHUNK_TILES]>,
    occupied: u32,
}

impl ChunkGrid {
    fn new() -> Self {
        Self {
            tiles: Box::new([0; CHUNK_TILES]),
            occupied: 0,
        }
    }
}

fn tile_index(pos: TilePos) -> usize {
    let o = pos.chunk().origin();
    ((pos.y - o.y) * CHUNK_SIZE + (pos.x - o.x)) as usize
}

#[derive(Default)]
pub struct Entities {
    slots: Vec<Option<Entity>>,
    live: usize,
    chunks: HashMap<ChunkPos, ChunkGrid>,
}

impl Entities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.slots.get(id.0 as usize)?.as_ref()
    }

    pub fn get_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        self.slots.get_mut(id.0 as usize)?.as_mut()
    }

    /// The entity covering `pos`, if any.
    pub fn at(&self, pos: TilePos) -> Option<EntityId> {
        let raw = self.chunks.get(&pos.chunk())?.tiles[tile_index(pos)];
        raw.checked_sub(1).map(EntityId)
    }

    /// Whether every tile of a footprint is free.
    pub fn is_free(&self, entity: &Entity) -> bool {
        entity.tiles().all(|t| self.at(t).is_none())
    }

    /// Place an entity. Returns `None`, changing nothing, if any tile is taken.
    pub fn insert(&mut self, entity: Entity) -> Option<EntityId> {
        if !self.is_free(&entity) {
            return None;
        }
        let id = EntityId(u32::try_from(self.slots.len()).expect("fewer than 4 billion entities"));
        for t in entity.tiles() {
            let chunk = self.chunks.entry(t.chunk()).or_insert_with(ChunkGrid::new);
            chunk.tiles[tile_index(t)] = id.0 + 1;
            chunk.occupied += 1;
        }
        self.slots.push(Some(entity));
        self.live += 1;
        Some(id)
    }

    /// Remove an entity, returning it.
    pub fn remove(&mut self, id: EntityId) -> Option<Entity> {
        let entity = self.slots.get_mut(id.0 as usize)?.take()?;
        for t in entity.tiles() {
            let c = t.chunk();
            if let Some(chunk) = self.chunks.get_mut(&c) {
                chunk.tiles[tile_index(t)] = 0;
                chunk.occupied -= 1;
                if chunk.occupied == 0 {
                    self.chunks.remove(&c);
                }
            }
        }
        self.live -= 1;
        Some(entity)
    }

    /// Live entities in id order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|e| (EntityId(i as u32), e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(x: i32, y: i32, w: u8, h: u8) -> Entity {
        Entity {
            kind: 0,
            pos: TilePos::new(x, y),
            size: (w, h),
            dir: Dir::East,
        }
    }

    #[test]
    fn footprints_block_and_free_their_tiles() {
        let mut e = Entities::new();
        let hub = e.insert(ent(62, 62, 3, 3)).unwrap(); // straddles four chunks
        assert_eq!(e.at(TilePos::new(64, 64)), Some(hub));
        assert_eq!(e.at(TilePos::new(62, 62)), Some(hub));
        assert_eq!(e.at(TilePos::new(65, 64)), None);
        assert!(e.insert(ent(64, 63, 1, 1)).is_none(), "overlap refused");
        assert_eq!(e.len(), 1);
        assert_eq!(e.remove(hub).map(|h| h.size), Some((3, 3)));
        assert_eq!(e.at(TilePos::new(64, 64)), None);
        assert!(e.chunks.is_empty(), "empty chunks are dropped");
        assert!(e.remove(hub).is_none(), "removing twice is a no-op");
    }

    #[test]
    fn negative_coordinates_work() {
        let mut e = Entities::new();
        let id = e.insert(ent(-1, -65, 1, 1)).unwrap();
        assert_eq!(e.at(TilePos::new(-1, -65)), Some(id));
        assert_eq!(e.at(TilePos::new(-1, -64)), None);
    }

    #[test]
    fn ids_are_not_reused_and_iterate_in_order() {
        let mut e = Entities::new();
        let a = e.insert(ent(0, 0, 1, 1)).unwrap();
        let b = e.insert(ent(1, 0, 1, 1)).unwrap();
        e.remove(a);
        let c = e.insert(ent(0, 0, 1, 1)).unwrap();
        assert!(c > b);
        let ids: Vec<_> = e.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![b, c]);
    }

    #[test]
    fn hundred_thousand_inserts_are_fast() {
        let start = std::time::Instant::now();
        let mut e = Entities::new();
        for i in 0..100_000 {
            e.insert(ent(i % 400, i / 400, 1, 1)).unwrap();
        }
        let took = start.elapsed();
        assert_eq!(e.len(), 100_000);
        // The plan's target is 50 ms in release builds; debug test builds get 4x headroom.
        let limit = if cfg!(debug_assertions) { 200 } else { 50 };
        assert!(took.as_millis() < limit, "100k inserts took {took:?}");
    }
}
