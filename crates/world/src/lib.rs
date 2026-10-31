//! World lane: an infinite map in 64×64 chunks, generated from a seed (T14, R11).
//!
//! [`generate`] is a pure function of the seed and the chunk position, so two
//! players with the same seed get the same map on any platform: it uses only
//! integer hashing and basic float arithmetic, never platform maths functions.
//! [`Generator`] runs it on a worker thread so generation never touches the
//! simulation thread.
//!
//! For now chunks hold ore patches. Biomes (T42) and terrain (T43) come later.

pub use mofact_core as core;

use mofact_core::geom::{ChunkPos, TilePos};
use mofact_core::CHUNK_SIZE;
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};

const TILES: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

/// Ore patches are this many tiles across on average.
const PATCH_SCALE: i32 = 11;
/// Noise above this is ore.
const THRESHOLD: f64 = 0.74;
/// Spawn always has an iron and a copper patch within this many tiles of (0, 0).
pub const SPAWN_RADIUS: i32 = 24;

/// One tile's ore: which resource (an index into the generator's resource list) and how much is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ore {
    pub resource: u8,
    pub amount: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub pos: ChunkPos,
    tiles: Box<[Option<Ore>; TILES]>,
}

impl Chunk {
    /// The ore at a tile inside this chunk.
    pub fn ore(&self, tile: TilePos) -> Option<Ore> {
        let o = self.pos.origin();
        let (x, y) = (tile.x - o.x, tile.y - o.y);
        if !(0..CHUNK_SIZE).contains(&x) || !(0..CHUNK_SIZE).contains(&y) {
            return None;
        }
        self.tiles[(y * CHUNK_SIZE + x) as usize]
    }

    /// Every ore tile in the chunk.
    pub fn ores(&self) -> impl Iterator<Item = (TilePos, Ore)> + '_ {
        let o = self.pos.origin();
        self.tiles.iter().enumerate().filter_map(move |(i, t)| {
            let i = i as i32;
            t.map(|ore| {
                (
                    TilePos::new(o.x + i % CHUNK_SIZE, o.y + i / CHUNK_SIZE),
                    ore,
                )
            })
        })
    }

    /// A stable fingerprint of the chunk's contents, for seed snapshot tests.
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv::default();
        for t in self.tiles.iter() {
            match t {
                None => h.write(&[0]),
                Some(o) => {
                    h.write(&[1, o.resource]);
                    h.write(&o.amount.to_le_bytes());
                }
            }
        }
        h.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldGen {
    pub seed: u64,
    /// How many kinds of ore to place; resource `0` is the first.
    pub resources: u8,
}

impl WorldGen {
    pub fn new(seed: u64, resources: u8) -> Self {
        Self {
            seed,
            resources: resources.max(1),
        }
    }
}

/// Generate one chunk. Pure: same inputs, same chunk.
pub fn generate(gen: &WorldGen, pos: ChunkPos) -> Chunk {
    let mut tiles = Box::new([None; TILES]);
    let o = pos.origin();
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let t = TilePos::new(o.x + x, o.y + y);
            tiles[(y * CHUNK_SIZE + x) as usize] = ore_at(gen, t);
        }
    }
    Chunk { pos, tiles }
}

/// The ore generated at one tile.
pub fn ore_at(gen: &WorldGen, t: TilePos) -> Option<Ore> {
    if let Some(ore) = spawn_patch(gen, t) {
        return Some(ore);
    }
    // Keep the spawn clearing free of random patches so the starter patches read clearly.
    let d2 = i64::from(t.x).pow(2) + i64::from(t.y).pow(2);
    if d2 < i64::from(SPAWN_RADIUS * 2).pow(2) {
        return None;
    }
    // Two octaves, so patches have ragged edges rather than the noise grid's squares.
    let n = 0.7 * value_noise(gen.seed, t.x, t.y, PATCH_SCALE)
        + 0.3 * value_noise(gen.seed ^ 0xf00d, t.x, t.y, 4);
    if n < THRESHOLD {
        return None;
    }
    // Which resource varies slowly, so a patch is almost always one kind.
    let kind = value_noise(gen.seed ^ 0x5eed, t.x, t.y, PATCH_SCALE * 4);
    let resource = ((kind * f64::from(gen.resources)) as u8).min(gen.resources - 1);
    // Richer towards the middle of a patch and further from spawn.
    let distance = (d2 as f64).sqrt() as u32;
    let amount = ((n - THRESHOLD) * 10_000.0) as u32 + 300 + distance / 4;
    Some(Ore { resource, amount })
}

/// Iron (resource 0) west of spawn and copper (resource 1) south-east, as in the prototype.
fn spawn_patch(gen: &WorldGen, t: TilePos) -> Option<Ore> {
    const PATCHES: [(i32, i32, i32, u8); 2] = [(-10, 1, 4, 0), (11, 8, 3, 1)];
    for (cx, cy, r, resource) in PATCHES {
        if resource >= gen.resources {
            continue;
        }
        let (dx, dy) = (i64::from(t.x - cx), i64::from(t.y - cy));
        let wobble = (hash(gen.seed, t.x, t.y) % 3) as i64 - 1;
        if dx * dx + dy * dy <= (i64::from(r) + wobble).max(1).pow(2) {
            return Some(Ore {
                resource,
                amount: 600,
            });
        }
    }
    None
}

fn hash(seed: u64, x: i32, y: i32) -> u64 {
    let mut h = seed ^ 0x9e37_79b9_7f4a_7c15;
    for v in [x as u32 as u64, y as u32 as u64] {
        h ^= v;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^= h >> 33;
    }
    h
}

/// Smooth value noise in 0..1 with features about `scale` tiles across.
fn value_noise(seed: u64, x: i32, y: i32, scale: i32) -> f64 {
    let (gx, gy) = (x.div_euclid(scale), y.div_euclid(scale));
    let (fx, fy) = (
        f64::from(x.rem_euclid(scale)) / f64::from(scale),
        f64::from(y.rem_euclid(scale)) / f64::from(scale),
    );
    let corner = |cx, cy| (hash(seed, cx, cy) >> 11) as f64 / (1u64 << 53) as f64;
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(fx), smooth(fy));
    let top = corner(gx, gy) + (corner(gx + 1, gy) - corner(gx, gy)) * sx;
    let bottom = corner(gx, gy + 1) + (corner(gx + 1, gy + 1) - corner(gx, gy + 1)) * sx;
    top + (bottom - top) * sy
}

#[derive(Default)]
struct Fnv(u64);

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        if self.0 == 0 {
            self.0 = 0xcbf2_9ce4_8422_2325;
        }
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }
}

/// Generates chunks on a worker thread. Ask for chunks near the player or a
/// building with [`request`](Self::request) and collect them with [`ready`](Self::ready).
pub struct Generator {
    tx: Option<Sender<ChunkPos>>,
    rx: Receiver<Chunk>,
    asked: HashSet<ChunkPos>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Generator {
    pub fn spawn(gen: WorldGen) -> Self {
        let (tx, jobs) = mpsc::channel::<ChunkPos>();
        let (done, rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("mofact-worldgen".into())
            .spawn(move || {
                for pos in jobs {
                    if done.send(generate(&gen, pos)).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn world generator");
        Self {
            tx: Some(tx),
            rx,
            asked: HashSet::new(),
            worker: Some(worker),
        }
    }

    /// Queue a chunk unless it was already asked for.
    pub fn request(&mut self, pos: ChunkPos) {
        if self.asked.insert(pos) {
            if let Some(tx) = &self.tx {
                let _ = tx.send(pos);
            }
        }
    }

    /// Chunks finished since the last call.
    pub fn ready(&self) -> impl Iterator<Item = Chunk> + '_ {
        self.rx.try_iter()
    }

    /// Wait for the next finished chunk.
    pub fn wait(&self) -> Option<Chunk> {
        self.rx.recv().ok()
    }
}

impl Drop for Generator {
    fn drop(&mut self) {
        self.tx = None; // ends the worker's job loop
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEN: WorldGen = WorldGen {
        seed: 42,
        resources: 2,
    };

    #[test]
    fn same_seed_same_chunk_and_snapshot_is_stable() {
        let a = generate(&GEN, ChunkPos { x: 0, y: 0 });
        let b = generate(&GEN, ChunkPos { x: 0, y: 0 });
        assert_eq!(a, b);
        // Seed snapshot: if this changes, every saved world changes. Update it only on purpose.
        assert_eq!(
            a.fingerprint(),
            SNAPSHOT_SEED_42_CHUNK_0_0,
            "seed 42 chunk (0,0) changed"
        );
        assert_ne!(
            generate(&WorldGen::new(43, 2), ChunkPos { x: 3, y: 3 }),
            generate(&GEN, ChunkPos { x: 3, y: 3 })
        );
    }

    const SNAPSHOT_SEED_42_CHUNK_0_0: u64 = 7_317_724_568_364_547_354;

    #[test]
    fn spawn_has_iron_and_copper_close_by() {
        for seed in 0..50 {
            let gen = WorldGen::new(seed, 2);
            let mut found = [false; 2];
            for y in -SPAWN_RADIUS..=SPAWN_RADIUS {
                for x in -SPAWN_RADIUS..=SPAWN_RADIUS {
                    if let Some(o) = ore_at(&gen, TilePos::new(x, y)) {
                        found[o.resource as usize] = true;
                    }
                }
            }
            assert_eq!(found, [true, true], "seed {seed}");
            assert_eq!(
                ore_at(&gen, TilePos::new(0, 0)),
                None,
                "spawn itself is clear"
            );
        }
    }

    #[test]
    fn world_goes_on_five_thousand_tiles_out() {
        // No edge: chunks far from spawn generate and hold ore of every kind.
        let gen = WorldGen::new(7, 3);
        let mut kinds = HashSet::new();
        let mut ore_tiles = 0;
        for cy in -2..2 {
            for cx in 76..84 {
                let c = generate(&gen, ChunkPos { x: cx, y: cy }); // ~5,000 tiles east
                for (_, o) in c.ores() {
                    kinds.insert(o.resource);
                    ore_tiles += 1;
                }
            }
        }
        assert_eq!(kinds.len(), 3, "all resources appear");
        assert!(ore_tiles > 200, "{ore_tiles} ore tiles in 32 chunks");
        let far = generate(
            &gen,
            ChunkPos {
                x: -10_000,
                y: 10_000,
            },
        );
        assert_eq!(far.pos.origin(), TilePos::new(-640_000, 640_000));
    }

    #[test]
    fn worker_generates_requested_chunks_once() {
        let mut g = Generator::spawn(GEN);
        for x in 0..4 {
            g.request(ChunkPos { x, y: 0 });
            g.request(ChunkPos { x, y: 0 });
        }
        let mut got: Vec<_> = (0..4).map(|_| g.wait().unwrap()).collect();
        got.sort_by_key(|c| c.pos.x);
        assert_eq!(got[2], generate(&GEN, ChunkPos { x: 2, y: 0 }));
        assert_eq!(g.ready().count(), 0, "duplicates were not generated");
    }
}
#[cfg(test)]
mod map_print {
    #[test]
    #[ignore]
    fn print_map() {
        let g = super::WorldGen::new(42, 2);
        for y in (-60..60).step_by(2) {
            let row: String = (-100..100)
                .step_by(1)
                .map(|x| match super::ore_at(&g, super::TilePos::new(x, y)) {
                    None => {
                        if x == 0 && y == 0 {
                            '@'
                        } else {
                            '.'
                        }
                    }
                    Some(o) => {
                        if o.resource == 0 {
                            'I'
                        } else {
                            'c'
                        }
                    }
                })
                .collect();
            println!("{row}");
        }
    }
}
