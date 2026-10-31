//! World lane: an infinite map in 64×64 chunks, generated from a seed (T14, R11).
//!
//! [`generate`] is a pure function of the seed and the chunk position, so two
//! players with the same seed get the same map on any platform: it uses only
//! integer hashing and basic float arithmetic, never platform maths functions.
//! [`Generator`] runs it on a worker thread so generation never touches the
//! simulation thread.
//!
//! Each tile has a [`Terrain`] picked from two noise fields, height and
//! moisture: lakes in the low ground with sand around them, grassland, dense
//! forest where it is wet, dry scrub where it is not, and bare rock up high.
//! Trees, bushes and stones ([`Decor`]) dress the land; they are only looks
//! and never block building. Ore comes in separate patches, richer in the
//! middle and further from the start.

pub use mofact_core as core;

use mofact_core::geom::{ChunkPos, TilePos};
use mofact_core::CHUNK_SIZE;
use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};

const TILES: usize = (CHUNK_SIZE * CHUNK_SIZE) as usize;

/// Ore patches sit one to a cell this many tiles across, at most.
const PATCH_CELL: i32 = 44;
/// Spawn always has an iron and a copper patch within this many tiles of (0, 0).
pub const SPAWN_RADIUS: i32 = 24;
/// The middle of the starter factory: the land around it is always dry and open.
pub const START: TilePos = TilePos { x: 26, y: 26 };
/// No trees or stones this close to [`START`], so the first factory has room.
const CLEARING: i32 = 18;

/// One tile's ore: which resource (an index into the generator's resource list) and how much is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ore {
    pub resource: u8,
    pub amount: u32,
}

/// What the ground is made of. The numbers are what the renderer's terrain
/// texture stores, so they stay fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Terrain {
    DeepWater = 0,
    Water = 1,
    Sand = 2,
    Grass = 3,
    Forest = 4,
    Scrub = 5,
    Rock = 6,
}

impl Terrain {
    /// Lakes: nothing can be built on them.
    pub fn is_water(self) -> bool {
        matches!(self, Terrain::DeepWater | Terrain::Water)
    }
}

/// Small things lying on the land, for looks only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Decor {
    Tree = 1,
    Pine = 2,
    Bush = 3,
    Stone = 4,
    Boulder = 5,
    Flowers = 6,
    Tuft = 7,
}

impl Decor {
    pub const ALL: [Decor; 7] = [
        Decor::Tree,
        Decor::Pine,
        Decor::Bush,
        Decor::Stone,
        Decor::Boulder,
        Decor::Flowers,
        Decor::Tuft,
    ];
}

/// Everything generated on one tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    pub terrain: Terrain,
    /// A gentle variation within the terrain (0 to 255), for shading: how
    /// deep the water is, how lush the grass.
    pub shade: u8,
    pub decor: Option<Decor>,
    pub ore: Option<Ore>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub pos: ChunkPos,
    tiles: Box<[Tile]>,
}

impl Chunk {
    fn index(&self, tile: TilePos) -> Option<usize> {
        let o = self.pos.origin();
        let (x, y) = (tile.x - o.x, tile.y - o.y);
        ((0..CHUNK_SIZE).contains(&x) && (0..CHUNK_SIZE).contains(&y))
            .then_some((y * CHUNK_SIZE + x) as usize)
    }

    /// Everything at a tile inside this chunk.
    pub fn tile(&self, tile: TilePos) -> Option<Tile> {
        self.index(tile).map(|i| self.tiles[i])
    }

    /// The ore at a tile inside this chunk.
    pub fn ore(&self, tile: TilePos) -> Option<Ore> {
        self.tile(tile)?.ore
    }

    /// Every tile in the chunk, row by row from the top left.
    pub fn tiles(&self) -> impl Iterator<Item = (TilePos, Tile)> + '_ {
        let o = self.pos.origin();
        self.tiles.iter().enumerate().map(move |(i, t)| {
            let i = i as i32;
            (TilePos::new(o.x + i % CHUNK_SIZE, o.y + i / CHUNK_SIZE), *t)
        })
    }

    /// Every ore tile in the chunk.
    pub fn ores(&self) -> impl Iterator<Item = (TilePos, Ore)> + '_ {
        self.tiles().filter_map(|(p, t)| Some((p, t.ore?)))
    }

    /// A stable fingerprint of the chunk's contents, for seed snapshot tests.
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv::default();
        for t in self.tiles.iter() {
            h.write(&[t.terrain as u8, t.shade, t.decor.map_or(0, |d| d as u8)]);
            match t.ore {
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
    let o = pos.origin();
    let tiles = (0..TILES as i32)
        .map(|i| {
            tile_at(
                gen,
                TilePos::new(o.x + i % CHUNK_SIZE, o.y + i / CHUNK_SIZE),
            )
        })
        .collect();
    Chunk { pos, tiles }
}

/// Everything generated at one tile.
pub fn tile_at(gen: &WorldGen, t: TilePos) -> Tile {
    let (terrain, shade) = terrain_at(gen, t);
    let ore = if terrain.is_water() {
        None
    } else {
        ore_on_land(gen, t)
    };
    let decor = match ore {
        Some(_) => None,
        None => decor_at(gen, t, terrain),
    };
    Tile {
        terrain,
        shade,
        decor,
        ore,
    }
}

/// The ore generated at one tile.
pub fn ore_at(gen: &WorldGen, t: TilePos) -> Option<Ore> {
    if terrain_at(gen, t).0.is_water() {
        return None;
    }
    ore_on_land(gen, t)
}

/// Whether a tile is lake, where nothing can be built.
pub fn is_water(gen: &WorldGen, t: TilePos) -> bool {
    terrain_at(gen, t).0.is_water()
}

/// How far `t` is from [`START`], in tiles.
fn from_start(t: TilePos) -> f64 {
    let (dx, dy) = (f64::from(t.x - START.x), f64::from(t.y - START.y));
    sqrt(dx * dx + dy * dy)
}

/// How strongly a tile is kept dry (0 to 1): fully near the start and the
/// starter ore, fading out over a few tiles.
fn keep_dry(t: TilePos) -> f64 {
    let mut keep = smooth((44.0 - from_start(t)) / 20.0);
    for (cx, cy, _, _) in SPAWN_PATCHES {
        let (dx, dy) = (f64::from(t.x - cx), f64::from(t.y - cy));
        keep = keep.max(smooth((14.0 - sqrt(dx * dx + dy * dy)) / 7.0));
    }
    keep
}

/// The terrain at a tile and its shade.
pub fn terrain_at(gen: &WorldGen, t: TilePos) -> (Terrain, u8) {
    let s = gen.seed;
    // Bend the noise a little so lake shores and forest edges wander.
    let wx = t.x + ((value_noise(s ^ 0x3a17, t.x, t.y, 9) - 0.5) * 10.0) as i32;
    let wy = t.y + ((value_noise(s ^ 0x91c3, t.x, t.y, 9) - 0.5) * 10.0) as i32;
    let mut height = 0.5 * value_noise(s ^ 0xe1e7, wx, wy, 96)
        + 0.32 * value_noise(s ^ 0x2b0b, wx, wy, 36)
        + 0.18 * value_noise(s ^ 0x77aa, wx, wy, 12);
    let mut wet = 0.6 * value_noise(s ^ 0x3017, wx, wy, 120)
        + 0.3 * value_noise(s ^ 0x0d0d, wx, wy, 40)
        + 0.1 * value_noise(s ^ 0xbeef, wx, wy, 10);
    // Around the start and the starter patches: dry land, never lake or bare
    // rock. Further out the world is left as it falls.
    let keep = keep_dry(t);
    height += (height.clamp(0.4, 0.64) - height) * keep;
    let near = smooth((40.0 - from_start(t)) / 22.0);
    wet += (wet.clamp(0.36, 0.6) - wet) * near;

    let terrain = if height < 0.29 {
        Terrain::DeepWater
    } else if height < 0.335 {
        Terrain::Water
    } else if height < 0.355 || (wet < 0.34 && height < 0.62) {
        Terrain::Sand
    } else if height > 0.7 {
        Terrain::Rock
    } else if wet > 0.6 {
        Terrain::Forest
    } else if wet < 0.44 {
        Terrain::Scrub
    } else {
        Terrain::Grass
    };
    let shade = match terrain {
        Terrain::DeepWater | Terrain::Water => height / 0.335,
        _ => 0.5 * value_noise(s ^ 0x5ade, t.x, t.y, 6) + 0.5 * wet,
    };
    (terrain, (shade.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Trees, bushes and stones: how thick they lie on each terrain, in 1/1000ths.
fn decor_at(gen: &WorldGen, t: TilePos, terrain: Terrain) -> Option<Decor> {
    use Decor::*;
    let table: &[(Decor, u64)] = match terrain {
        Terrain::Forest => &[(Tree, 260), (Pine, 160), (Bush, 70), (Flowers, 20)],
        Terrain::Grass => &[(Tree, 30), (Bush, 45), (Flowers, 45), (Tuft, 90)],
        Terrain::Scrub => &[(Bush, 40), (Stone, 35), (Tuft, 80), (Pine, 10)],
        Terrain::Sand => &[(Stone, 20), (Tuft, 25)],
        Terrain::Rock => &[(Boulder, 90), (Stone, 110), (Pine, 15)],
        Terrain::Water | Terrain::DeepWater => &[],
    };
    // Groves: trees bunch together rather than lying evenly.
    let grove = value_noise(gen.seed ^ 0x6a0e, t.x, t.y, 7);
    let near = from_start(t);
    let mut roll = hash(gen.seed ^ 0xdec0, t.x, t.y) % 1000;
    for &(decor, per_mille) in table {
        let tall = matches!(decor, Tree | Pine | Boulder);
        if tall && near < f64::from(CLEARING) {
            continue;
        }
        let n = if tall {
            (per_mille as f64 * (0.3 + 1.4 * grove)) as u64
        } else {
            per_mille
        };
        if roll < n {
            return Some(decor);
        }
        roll -= n.min(roll);
    }
    None
}

/// Ore on a land tile: a starter patch, or one of the patches dotted about.
fn ore_on_land(gen: &WorldGen, t: TilePos) -> Option<Ore> {
    if let Some(ore) = spawn_patch(gen, t) {
        return Some(ore);
    }
    // Keep the spawn clearing free of random patches so the starter patches read clearly.
    let d2 = i64::from(t.x).pow(2) + i64::from(t.y).pow(2);
    if d2 < i64::from(SPAWN_RADIUS * 2).pow(2) {
        return None;
    }
    let (cx, cy) = (t.x.div_euclid(PATCH_CELL), t.y.div_euclid(PATCH_CELL));
    let h = hash(gen.seed ^ 0x0e0e, cx, cy);
    // About two cells in three hold a patch.
    if h % 3 == 0 {
        return None;
    }
    // Its middle, well inside the cell so it never runs into the next one.
    let px = cx * PATCH_CELL + 13 + ((h >> 8) % 18) as i32;
    let py = cy * PATCH_CELL + 13 + ((h >> 16) % 18) as i32;
    let far = sqrt(f64::from(px) * f64::from(px) + f64::from(py) * f64::from(py));
    // Bigger further out, and ragged at the edge.
    let radius = 4.0 + ((h >> 24) % 5) as f64 + (far / 400.0).min(3.0);
    let edge = 0.7 + 0.6 * value_noise(gen.seed ^ 0xed6e, t.x, t.y, 4);
    let (dx, dy) = (f64::from(t.x - px), f64::from(t.y - py));
    let d = sqrt(dx * dx + dy * dy) / (radius * edge);
    if d >= 1.0 {
        return None;
    }
    let resource = pick_resource(gen.resources, (h >> 32) % 100);
    // Richest in the middle, and richer the further you go.
    let rich = 1.0 + ((h >> 40) % 100) as f64 / 100.0 + far / 600.0;
    let amount = (300.0 + 900.0 * rich * (1.0 - d * d)) as u32;
    Some(Ore { resource, amount })
}

/// Which ore a patch holds: iron and copper are commonest, then coal and stone.
fn pick_resource(resources: u8, roll: u64) -> u8 {
    const WEIGHTS: [u64; 4] = [34, 30, 18, 18];
    if usize::from(resources) > WEIGHTS.len() {
        return (roll % u64::from(resources)) as u8;
    }
    let weights = &WEIGHTS[..usize::from(resources)];
    let total: u64 = weights.iter().sum();
    let mut roll = roll * total / 100;
    for (i, &w) in weights.iter().enumerate() {
        if roll < w {
            return i as u8;
        }
        roll -= w;
    }
    resources - 1
}

/// The starter patches: centre, radius and resource.
const SPAWN_PATCHES: [(i32, i32, i32, u8); 4] =
    [(-10, 1, 4, 0), (11, 8, 3, 1), (4, 44, 3, 2), (52, 6, 3, 3)];

/// Iron (resource 0) west of spawn and copper (resource 1) south-east, as in
/// the prototype; stone (2) and coal (3) a short walk from the factory.
fn spawn_patch(gen: &WorldGen, t: TilePos) -> Option<Ore> {
    for (cx, cy, r, resource) in SPAWN_PATCHES {
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

/// Square root by Newton's method, so every platform gets the same bits.
fn sqrt(v: f64) -> f64 {
    if v <= 0.0 {
        return 0.0;
    }
    let mut x = if v > 1.0 { v / 2.0 } else { 1.0 };
    for _ in 0..40 {
        let next = 0.5 * (x + v / x);
        if (next - x).abs() < 1e-9 {
            return next;
        }
        x = next;
    }
    x
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
///
/// The browser has no threads to give it, so there each chunk is made as it
/// is asked for, and is ready at once.
pub struct Generator {
    tx: Option<Sender<ChunkPos>>,
    rx: Receiver<Chunk>,
    asked: HashSet<ChunkPos>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// Where chunks go when they are made inline, and how to make them.
    #[cfg(target_arch = "wasm32")]
    inline: (Sender<Chunk>, WorldGen),
}

impl Generator {
    #[cfg(target_arch = "wasm32")]
    pub fn spawn(gen: WorldGen) -> Self {
        let (done, rx) = mpsc::channel();
        Self {
            tx: None,
            rx,
            asked: HashSet::new(),
            worker: None,
            inline: (done, gen),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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
            #[cfg(target_arch = "wasm32")]
            let _ = self.inline.0.send(generate(&self.inline.1, pos));
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

    const SNAPSHOT_SEED_42_CHUNK_0_0: u64 = 13_658_882_497_565_849_921;

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
    use super::*;

    /// `cargo test -p mofact-world print_map -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_map() {
        let g = WorldGen::new(42, 4);
        let mut counts = std::collections::BTreeMap::new();
        for y in (-80..120).step_by(3) {
            let row: String = (-100..160)
                .step_by(2)
                .map(|x| {
                    let t = tile_at(&g, TilePos::new(x, y));
                    *counts.entry(format!("{:?}", t.terrain)).or_insert(0) += 1;
                    if x == START.x && y == START.y {
                        return '@';
                    }
                    if let Some(o) = t.ore {
                        return ['I', 'C', 'S', 'K'][usize::from(o.resource)];
                    }
                    match t.terrain {
                        Terrain::DeepWater => '#',
                        Terrain::Water => '~',
                        Terrain::Sand => ':',
                        Terrain::Grass => '.',
                        Terrain::Forest => 'f',
                        Terrain::Scrub => ',',
                        Terrain::Rock => '^',
                    }
                })
                .collect();
            println!("{row}");
        }
        println!("{counts:?}");
    }
}
