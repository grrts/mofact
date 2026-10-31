//! The map the camera can see: terrain, decorations and ore from the world
//! lane's generated chunks, plus any ore a scenario placed by hand.

use mofact_core::content::{Content, IconShape};
use mofact_core::geom::{ChunkPos, TilePos};
use mofact_core::ids::ItemId;
use mofact_core::CHUNK_SIZE;
use mofact_render::Camera;
use mofact_world::{Chunk, Generator, WorldGen};
use std::collections::{HashMap, HashSet};

pub struct OreMap {
    gen: WorldGen,
    worker: Option<Generator>,
    /// The ore items, in the order the world generator numbers resources.
    items: Vec<ItemId>,
    chunks: HashMap<ChunkPos, Vec<(TilePos, ItemId)>>,
    /// Each chunk's terrain number and shade, two bytes a tile, row by row.
    ground: HashMap<ChunkPos, Box<[u8]>>,
    decor: HashMap<ChunkPos, Vec<(TilePos, u8)>>,
    extra: Vec<(TilePos, ItemId)>,
    /// Goes up with every chunk added.
    version: u64,
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
            ground: HashMap::new(),
            decor: HashMap::new(),
            extra,
            version: 0,
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
        let placed: HashSet<TilePos> = self.extra.iter().map(|(t, _)| *t).collect();
        let mut ground = Vec::with_capacity((CHUNK_SIZE * CHUNK_SIZE * 2) as usize);
        let mut decor = Vec::new();
        for (t, tile) in chunk.tiles() {
            ground.extend_from_slice(&[tile.terrain as u8, tile.shade]);
            // Scenario ore lies on clear ground.
            if let Some(d) = tile.decor.filter(|_| !placed.contains(&t)) {
                decor.push((t, d as u8));
            }
        }
        self.ground.insert(chunk.pos, ground.into());
        self.decor.insert(chunk.pos, decor);
        self.version += 1;
    }

    /// Goes up whenever more of the map is generated.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The terrain number and shade at `t`, once its chunk is generated.
    pub fn terrain(&self, t: TilePos) -> Option<(u8, u8)> {
        let pos = ChunkPos {
            x: t.x.div_euclid(CHUNK_SIZE),
            y: t.y.div_euclid(CHUNK_SIZE),
        };
        let o = pos.origin();
        let i = (((t.y - o.y) * CHUNK_SIZE + t.x - o.x) * 2) as usize;
        self.ground.get(&pos).map(|g| (g[i], g[i + 1]))
    }

    /// The trees, bushes and stones in the chunks a camera sees, by kind.
    pub fn decor(
        &self,
        camera: &Camera,
        size: (u32, u32),
    ) -> impl Iterator<Item = (TilePos, u8)> + '_ {
        Self::visible(camera, size)
            .filter_map(|pos| self.decor.get(&pos))
            .flatten()
            .copied()
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
