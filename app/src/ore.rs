//! The map the camera can see: terrain, decorations and ore from the world
//! lane's generated chunks, plus any ore a scenario placed by hand.

use mofact_core::content::{Content, IconShape};
use mofact_core::geom::{ChunkPos, TilePos};
use mofact_core::ids::ItemId;
use mofact_core::CHUNK_SIZE;
use mofact_render::Camera;
use mofact_world::{Chunk, Coarse, Generator, Made, Patch, WorldGen};
use std::collections::{HashMap, HashSet};

/// The world map caches ore patches in squares this many tiles across.
const PATCH_BLOCK: i32 = 352;

pub struct OreMap {
    gen: WorldGen,
    worker: Option<Generator>,
    /// The ore items, in the order the world generator numbers resources.
    items: Vec<ItemId>,
    chunks: HashMap<ChunkPos, Vec<(TilePos, ItemId)>>,
    /// Each chunk's terrain number and shade, two bytes a tile, row by row.
    ground: HashMap<ChunkPos, Box<[u8]>>,
    decor: HashMap<ChunkPos, Vec<(TilePos, u8)>>,
    /// Far-out blocks of the map, by stride and block.
    coarse: HashMap<(i32, ChunkPos), Box<[u8]>>,
    /// Ore patches for the world map, by block of [`PATCH_BLOCK`] tiles.
    patches: std::cell::RefCell<HashMap<(i32, i32), Vec<Patch>>>,
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
            coarse: HashMap::new(),
            patches: Default::default(),
            extra,
            version: 0,
        }
    }

    /// The generator the map comes from.
    pub fn gen(&self) -> WorldGen {
        self.gen
    }

    /// The seed the map is generated from.
    pub fn seed(&self) -> u64 {
        self.gen.seed
    }

    /// The chunks a camera sees on a `size` pixel target.
    fn visible(camera: &Camera, size: (u32, u32)) -> impl Iterator<Item = ChunkPos> {
        Self::blocks(camera, size, 1)
    }

    /// The blocks of `stride`-tile texels a camera sees: chunks at stride 1.
    fn blocks(camera: &Camera, size: (u32, u32), stride: i32) -> impl Iterator<Item = ChunkPos> {
        let (lo, hi) = camera.visible(size.0, size.1);
        let span = (CHUNK_SIZE * stride) as f32;
        let chunk = |v: f32| (v / span).floor() as i32;
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

    /// The terrain number and shade at texel `t` of `stride` tiles, once
    /// something covering it is generated. Falls back on the nearest
    /// stride there is, so zooming never flashes blank ground.
    pub fn terrain(&self, t: TilePos, stride: i32) -> Option<(u8, u8)> {
        let look = |s: i32| -> Option<(u8, u8)> {
            // The texel of stride `s` that holds the middle of texel `t`.
            let mid = |v: i32| (v * stride + stride / 2).div_euclid(s);
            let t = TilePos::new(mid(t.x), mid(t.y));
            let pos = ChunkPos {
                x: t.x.div_euclid(CHUNK_SIZE),
                y: t.y.div_euclid(CHUNK_SIZE),
            };
            let o = pos.origin();
            let i = (((t.y - o.y) * CHUNK_SIZE + t.x - o.x) * 2) as usize;
            let g = if s == 1 {
                self.ground.get(&pos)
            } else {
                self.coarse.get(&(s, pos))
            }?;
            Some((g[i], g[i + 1]))
        };
        let mut finer = stride;
        while finer >= 1 {
            if let Some(v) = look(finer) {
                return Some(v);
            }
            finer /= 2;
        }
        let mut coarser = stride * 2;
        while coarser <= 64 {
            if let Some(v) = look(coarser) {
                return Some(v);
            }
            coarser *= 2;
        }
        None
    }

    /// The ore patches whose middles lie in view, for the world map.
    pub fn patches(&self, camera: &Camera, size: (u32, u32)) -> Vec<(Patch, ItemId)> {
        let (lo, hi) = camera.visible(size.0, size.1);
        let block = |v: f32| (v.floor() as i32 - 12).div_euclid(PATCH_BLOCK);
        let (x0, y0) = (block(lo[0]), block(lo[1]));
        let (x1, y1) = (block(hi[0] + 24.0), block(hi[1] + 24.0));
        let mut cache = self.patches.borrow_mut();
        let mut out = Vec::new();
        for by in y0..=y1 {
            for bx in x0..=x1 {
                let found = cache.entry((bx, by)).or_insert_with(|| {
                    let lo = TilePos::new(bx * PATCH_BLOCK, by * PATCH_BLOCK);
                    let hi = TilePos::new(lo.x + PATCH_BLOCK - 1, lo.y + PATCH_BLOCK - 1);
                    mofact_world::patches(&self.gen, lo, hi)
                });
                for p in found.iter() {
                    if let Some(item) = self.items.get(usize::from(p.resource)) {
                        out.push((*p, item.clone()));
                    }
                }
            }
        }
        out
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
    /// Up close that is whole chunks; zoomed out it is blocks of the far
    /// map, which are much cheaper.
    pub fn update(&mut self, camera: &Camera, size: (u32, u32)) {
        let gen = self.gen;
        let worker = self.worker.get_or_insert_with(|| Generator::spawn(gen));
        let stride = mofact_render::scene::map_stride(camera.pixels_per_tile);
        if stride == 1 {
            for pos in Self::visible(camera, size) {
                worker.request(pos);
            }
        } else {
            for pos in Self::blocks(camera, size, stride) {
                worker.request_coarse(stride, pos);
            }
        }
        let done: Vec<Made> = worker.ready().collect();
        for made in done {
            match made {
                Made::Chunk(chunk) => self.insert(chunk),
                Made::Coarse(c) => self.insert_coarse(c),
            }
        }
    }

    fn insert_coarse(&mut self, c: Coarse) {
        self.coarse.insert((c.stride, c.pos), c.ground);
        self.version += 1;
    }

    /// Generate everything in view right now, on this thread. For screenshots.
    pub fn fill(&mut self, camera: &Camera, size: (u32, u32)) {
        let stride = mofact_render::scene::map_stride(camera.pixels_per_tile);
        if stride > 1 {
            for pos in Self::blocks(camera, size, stride) {
                if !self.coarse.contains_key(&(stride, pos)) {
                    self.insert_coarse(mofact_world::coarse(&self.gen, stride, pos));
                }
            }
            return;
        }
        for pos in Self::visible(camera, size) {
            if !self.chunks.contains_key(&pos) {
                let chunk = mofact_world::generate(&self.gen, pos);
                self.insert(chunk);
            }
        }
    }

    /// The ore at `t`, once its chunk is generated.
    pub fn ore_at(&self, t: TilePos) -> Option<&ItemId> {
        if let Some((_, i)) = self.extra.iter().find(|(e, _)| *e == t) {
            return Some(i);
        }
        let pos = ChunkPos {
            x: t.x.div_euclid(CHUNK_SIZE),
            y: t.y.div_euclid(CHUNK_SIZE),
        };
        self.chunks
            .get(&pos)?
            .iter()
            .find(|(e, _)| *e == t)
            .map(|(_, i)| i)
    }

    pub fn tiles(&self) -> impl Iterator<Item = (TilePos, &ItemId)> {
        self.chunks
            .values()
            .flatten()
            .chain(&self.extra)
            .map(|(t, i)| (*t, i))
    }
}
