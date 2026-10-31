//! The ore the camera can see: generated chunks from the world lane, plus any
//! ore a scenario placed by hand.

use mofact_core::content::{Content, IconShape};
use mofact_core::geom::{ChunkPos, TilePos};
use mofact_core::ids::ItemId;
use mofact_core::CHUNK_SIZE;
use mofact_render::Camera;
use mofact_world::{Chunk, Generator, WorldGen};
use std::collections::HashMap;

pub struct OreMap {
    gen: WorldGen,
    worker: Option<Generator>,
    /// The ore items, in the order the world generator numbers resources.
    items: Vec<ItemId>,
    chunks: HashMap<ChunkPos, Vec<(TilePos, ItemId)>>,
    extra: Vec<(TilePos, ItemId)>,
}

impl OreMap {
    /// `extra` is ore placed by a scenario, drawn on top of the generated map.
    pub fn new(content: &Content, seed: u64, extra: Vec<(TilePos, ItemId)>) -> Self {
        let items: Vec<ItemId> = content
            .items
            .iter()
            .filter(|i| i.shape == IconShape::Ore)
            .map(|i| i.id.clone())
            .collect();
        Self {
            gen: WorldGen::new(seed, items.len() as u8),
            worker: None,
            items,
            chunks: HashMap::new(),
            extra,
        }
    }

    /// The chunks a camera sees on a `size` pixel target.
    fn visible(camera: &Camera, size: (u32, u32)) -> impl Iterator<Item = ChunkPos> {
        let (lo, hi) = camera.visible(size.0, size.1);
        let chunk = |v: f32| (v.floor() as i32).div_euclid(CHUNK_SIZE);
        let (x0, y0, x1, y1) = (chunk(lo[0]), chunk(lo[1]), chunk(hi[0]), chunk(hi[1]));
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| ChunkPos { x, y }))
    }

    fn insert(&mut self, chunk: Chunk) {
        let ores = chunk
            .ores()
            .filter_map(|(t, o)| Some((t, self.items.get(usize::from(o.resource))?.clone())))
            .collect();
        self.chunks.insert(chunk.pos, ores);
    }

    /// Ask the worker for chunks coming into view and take the ones it finished.
    /// Never waits, so a frame never stalls on generation.
    pub fn update(&mut self, camera: &Camera, size: (u32, u32)) {
        let gen = self.gen;
        let worker = self.worker.get_or_insert_with(|| Generator::spawn(gen));
        for pos in Self::visible(camera, size) {
            worker.request(pos);
        }
        let done: Vec<Chunk> = worker.ready().collect();
        for chunk in done {
            self.insert(chunk);
        }
    }

    /// Generate every chunk in view right now, on this thread. For screenshots.
    pub fn fill(&mut self, camera: &Camera, size: (u32, u32)) {
        for pos in Self::visible(camera, size) {
            if !self.chunks.contains_key(&pos) {
                let chunk = mofact_world::generate(&self.gen, pos);
                self.insert(chunk);
            }
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = (TilePos, &ItemId)> {
        self.chunks
            .values()
            .flatten()
            .chain(&self.extra)
            .map(|(t, i)| (*t, i))
    }
}
