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
    Cliff = 7,
    /// Desert sand.
    Dune = 8,
    /// Tundra.
    Snow = 9,
    /// Frozen lakes and rivers in the tundra: firm enough to build on.
    Ice = 10,
    /// The volcanic lands: ash plains, black rock and lava.
    Ash = 11,
    Basalt = 12,
    Lava = 13,
}

impl Terrain {
    /// Lakes and rivers.
    pub fn is_water(self) -> bool {
        matches!(self, Terrain::DeepWater | Terrain::Water)
    }

    /// Nothing can be built here: water, cliffs and lava.
    pub fn blocks(self) -> bool {
        self.is_water() || matches!(self, Terrain::Cliff | Terrain::Lava)
    }
}

/// The great regions of the map, in rings that get further out with each
/// tier: grassland around the start, then desert, tundra and volcanic land
/// (R12, R13). Each holds one ore found nowhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Biome {
    Grassland = 0,
    Desert = 1,
    Tundra = 2,
    Volcanic = 3,
}

impl Biome {
    /// The fewest tiles from the start each biome can begin.
    pub const RINGS: [(Biome, f64); 3] = [
        (Biome::Volcanic, 1_600.0),
        (Biome::Tundra, 1_000.0),
        (Biome::Desert, 450.0),
    ];

    /// The resource found only here: resources after the first four
    /// (iron, copper, stone, coal) belong to desert, tundra and volcanic
    /// land in that order (bauxite, titanium, rare earths).
    pub fn exclusive(self) -> Option<u8> {
        match self {
            Biome::Grassland => None,
            Biome::Desert => Some(4),
            Biome::Tundra => Some(5),
            Biome::Volcanic => Some(6),
        }
    }
}

/// Which biome a tile lies in. Each ring starts at its distance and pushes
/// further out in places, so its edge wanders but is never nearer.
pub fn biome_at(gen: &WorldGen, t: TilePos) -> Biome {
    let s = gen.seed;
    let reach = from_start(t) - 260.0 * value_noise(s ^ 0xb10e, t.x, t.y, 180);
    let region = value_noise(s ^ 0x4e61, t.x, t.y, 240);
    let other = value_noise(s ^ 0x0717, t.x, t.y, 320);
    for (biome, ring) in Biome::RINGS {
        let share = match biome {
            Biome::Volcanic => region > 0.55,
            Biome::Tundra => other > 0.45,
            _ => region < 0.6,
        };
        if reach > ring && share {
            return biome;
        }
    }
    Biome::Grassland
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
    /// Trees and boulders stand in the way of building until cleared; the
    /// small things are built over.
    pub fn blocks(self) -> bool {
        matches!(self, Decor::Tree | Decor::Pine | Decor::Boulder)
    }

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
    let ore = if terrain.blocks() {
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
    if terrain_at(gen, t).0.blocks() {
        return None;
    }
    ore_on_land(gen, t)
}

/// What stops building on a tile, if anything: water, a cliff, or a tree
/// or boulder until it is cleared.
pub fn obstacle(gen: &WorldGen, t: TilePos) -> Option<Obstacle> {
    let tile = tile_at(gen, t);
    if tile.terrain.blocks() {
        Some(Obstacle::Terrain)
    } else if tile.decor.is_some_and(Decor::blocks) {
        Some(Obstacle::Tree)
    } else {
        None
    }
}

/// Why a tile can't be built on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Obstacle {
    /// Water or a cliff: never.
    Terrain,
    /// A tree or a boulder: once it is cleared.
    Tree,
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

/// Height and moisture at a tile (0 to 1), and how strongly it is kept dry.
fn fields(gen: &WorldGen, t: TilePos) -> (f64, f64, f64) {
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

    (height, wet, keep)
}

/// The broad shape of the land, without the wobble and fine detail, for
/// cliffs to follow in long clean lines.
fn ridge(gen: &WorldGen, t: TilePos) -> f64 {
    let s = gen.seed;
    0.6 * value_noise(s ^ 0xe1e7, t.x, t.y, 96) + 0.4 * value_noise(s ^ 0x2b0b, t.x, t.y, 36)
}

/// Highland contour steps: a cliff runs wherever a tile and its neighbour
/// fall on different steps.
fn contour(height: f64) -> i32 {
    ((height - 0.56) / 0.05).floor() as i32
}

/// The terrain at a tile and its shade.
pub fn terrain_at(gen: &WorldGen, t: TilePos) -> (Terrain, u8) {
    let s = gen.seed;
    let (height, wet, keep) = fields(gen, t);
    // Rivers: where a slow, winding noise crosses its middle, through the
    // lowland between the lakes and the hills.
    let bend =
        value_noise(s ^ 0x41fe, t.x, t.y, 150) + 0.25 * value_noise(s ^ 0x7e77, t.x, t.y, 40);
    let river = (bend - 0.625).abs();
    let lowland = (0.34..0.64).contains(&height) && keep < 0.2;
    if lowland && (river < 0.009 || river < 0.016) {
        let biome = biome_at(gen, t);
        let terrain = if river < 0.009 {
            in_biome(biome, Terrain::Water, wet)
        } else {
            in_biome(biome, Terrain::Sand, wet)
        };
        return (terrain, if river < 0.009 { 200 } else { 140 });
    }
    // Cliffs: ragged lines along the highland contours, with gaps to pass through.
    if height > 0.56 && keep < 0.2 && value_noise(s ^ 0xc11f, t.x, t.y, 14) > 0.4 {
        let step = contour(ridge(gen, t));
        let east = contour(ridge(gen, TilePos::new(t.x + 1, t.y)));
        let south = contour(ridge(gen, TilePos::new(t.x, t.y + 1)));
        if step >= 0 && (step != east || step != south) {
            return (Terrain::Cliff, (height.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
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
    let terrain = in_biome(biome_at(gen, t), terrain, wet);
    (terrain, (shade.clamp(0.0, 1.0) * 255.0) as u8)
}

/// What a grassland terrain becomes in another biome.
fn in_biome(biome: Biome, terrain: Terrain, wet: f64) -> Terrain {
    use Terrain::*;
    match (biome, terrain) {
        (Biome::Grassland, t) | (_, t @ (Cliff | Rock)) => t,
        // Desert: dunes, with an oasis of grass where it is wettest.
        (Biome::Desert, Forest) if wet > 0.7 => Grass,
        (Biome::Desert, Water | DeepWater) => terrain,
        (Biome::Desert, Forest) => Scrub,
        (Biome::Desert, _) => Dune,
        // Tundra: snow, with frozen lakes; forest stays as dark taiga.
        (Biome::Tundra, Water | DeepWater) => Ice,
        (Biome::Tundra, Forest) => Forest,
        (Biome::Tundra, _) => Snow,
        // Volcanic: lava where the lakes would be, ash and black rock.
        (Biome::Volcanic, Water | DeepWater) => Lava,
        (Biome::Volcanic, Forest | Grass) if wet > 0.5 => Basalt,
        (Biome::Volcanic, _) => Ash,
    }
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
        Terrain::Dune => &[(Stone, 15), (Tuft, 18), (Boulder, 8)],
        Terrain::Snow => &[(Pine, 50), (Stone, 20), (Boulder, 15)],
        Terrain::Ash => &[(Stone, 40), (Boulder, 25)],
        Terrain::Basalt => &[(Boulder, 50), (Stone, 60)],
        Terrain::Water | Terrain::DeepWater | Terrain::Cliff | Terrain::Ice | Terrain::Lava => &[],
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
    let cell = patch_cell(gen, cx, cy)?;
    let edge = 0.7 + 0.6 * value_noise(gen.seed ^ 0xed6e, t.x, t.y, 4);
    let (dx, dy) = (f64::from(t.x - cell.mid.x), f64::from(t.y - cell.mid.y));
    let d = sqrt(dx * dx + dy * dy) / (cell.radius * edge);
    if d >= 1.0 {
        return None;
    }
    let resource = cell_resource(gen, &cell);
    // A far biome's own ore stops at the biome's edge.
    if resource >= 4 && biome_at(gen, t).exclusive() != Some(resource) {
        return None;
    }
    let (h, far) = (cell.hash, cell.far);
    // Richest in the middle, and richer the further you go.
    let rich = 1.0 + ((h >> 40) % 100) as f64 / 100.0 + far / 600.0;
    let amount = (1200.0 + 3600.0 * rich * (1.0 - d * d)) as u32;
    Some(Ore { resource, amount })
}

/// The patch a 44-tile cell holds, if any: its middle and size.
struct PatchCell {
    mid: TilePos,
    radius: f64,
    hash: u64,
    /// How far its middle is from the origin, in tiles.
    far: f64,
}

fn patch_cell(gen: &WorldGen, cx: i32, cy: i32) -> Option<PatchCell> {
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
    Some(PatchCell {
        mid: TilePos::new(px, py),
        radius,
        hash: h,
        far,
    })
}

fn cell_resource(gen: &WorldGen, cell: &PatchCell) -> u8 {
    let h = cell.hash;
    match biome_at(gen, cell.mid).exclusive() {
        // Two patches in five in a far biome hold its own ore.
        Some(own) if own < gen.resources && (h >> 48) % 5 < 2 => own,
        _ => pick_resource(gen.resources.min(4), (h >> 32) % 100),
    }
}

/// One ore patch, for the world map: its middle, its ore and about how far
/// it reaches, in tiles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub center: TilePos,
    pub resource: u8,
    pub radius: f32,
}

/// Every ore patch whose middle lies between `lo` and `hi` (inclusive),
/// starter patches included.
pub fn patches(gen: &WorldGen, lo: TilePos, hi: TilePos) -> Vec<Patch> {
    let inside = |t: TilePos| (lo.x..=hi.x).contains(&t.x) && (lo.y..=hi.y).contains(&t.y);
    let mut out: Vec<Patch> = SPAWN_PATCHES
        .iter()
        .filter(|p| p.3 < gen.resources)
        .map(|&(x, y, r, resource)| Patch {
            center: TilePos::new(x, y),
            resource,
            radius: r as f32,
        })
        .filter(|p| inside(p.center))
        .collect();
    let keep_out = i64::from(SPAWN_RADIUS * 2).pow(2);
    for cy in lo.y.div_euclid(PATCH_CELL)..=hi.y.div_euclid(PATCH_CELL) {
        for cx in lo.x.div_euclid(PATCH_CELL)..=hi.x.div_euclid(PATCH_CELL) {
            let Some(cell) = patch_cell(gen, cx, cy) else {
                continue;
            };
            let m = cell.mid;
            if !inside(m) || i64::from(m.x).pow(2) + i64::from(m.y).pow(2) < keep_out {
                continue;
            }
            // Lakes and lava can swallow a whole patch.
            if ore_at(gen, m).is_none() {
                continue;
            }
            out.push(Patch {
                center: m,
                resource: cell_resource(gen, &cell),
                radius: cell.radius as f32,
            });
        }
    }
    out
}

/// A far-out view of the map: every `stride`-th tile's terrain and shade,
/// in blocks the size of a chunk. Block `pos` covers tiles from
/// `pos.origin() * stride`, sampling the middle of each `stride` square.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coarse {
    pub stride: i32,
    pub pos: ChunkPos,
    /// Terrain number and shade, two bytes a sample, row by row.
    pub ground: Box<[u8]>,
}

pub fn coarse(gen: &WorldGen, stride: i32, pos: ChunkPos) -> Coarse {
    let stride = stride.max(1);
    let o = pos.origin();
    let mut ground = Vec::with_capacity(TILES * 2);
    for y in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE {
            let t = TilePos::new(
                (o.x + x) * stride + stride / 2,
                (o.y + y) * stride + stride / 2,
            );
            let (terrain, shade) = terrain_at(gen, t);
            ground.extend_from_slice(&[terrain as u8, shade]);
        }
    }
    Coarse {
        stride,
        pos,
        ground: ground.into(),
    }
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

/// Ore on each tile of a starter patch: a starter iron patch worked by a
/// handful of drills runs out in about five hours (R14).
pub const SPAWN_ORE: u32 = 2_500;

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
                amount: SPAWN_ORE,
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
    tx: Option<Sender<Job>>,
    rx: Receiver<Made>,
    asked: HashSet<Job>,
    worker: Option<std::thread::JoinHandle<()>>,
    /// On the web there are no threads: jobs wait here, and a few are done
    /// each frame so the page never stalls.
    #[cfg(target_arch = "wasm32")]
    queue: std::cell::RefCell<(std::collections::VecDeque<Job>, WorldGen)>,
}

/// What the generator is asked for: a chunk, or a far-out block of the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Job {
    Chunk(ChunkPos),
    Coarse(i32, ChunkPos),
}

/// What the generator made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Made {
    Chunk(Chunk),
    Coarse(Coarse),
}

impl Job {
    fn run(self, gen: &WorldGen) -> Made {
        match self {
            Job::Chunk(pos) => Made::Chunk(generate(gen, pos)),
            Job::Coarse(stride, pos) => Made::Coarse(coarse(gen, stride, pos)),
        }
    }
}

/// Jobs the web build does per [`Generator::ready`] call.
#[cfg(target_arch = "wasm32")]
const JOBS_PER_FRAME: usize = 3;

impl Generator {
    #[cfg(target_arch = "wasm32")]
    pub fn spawn(gen: WorldGen) -> Self {
        let (_, rx) = mpsc::channel();
        Self {
            tx: None,
            rx,
            asked: HashSet::new(),
            worker: None,
            queue: std::cell::RefCell::new((std::collections::VecDeque::new(), gen)),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn spawn(gen: WorldGen) -> Self {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (done, rx) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("mofact-worldgen".into())
            .spawn(move || {
                for job in jobs {
                    if done.send(job.run(&gen)).is_err() {
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
        self.ask(Job::Chunk(pos));
    }

    /// Queue a far-out block of the map unless it was already asked for.
    pub fn request_coarse(&mut self, stride: i32, pos: ChunkPos) {
        self.ask(Job::Coarse(stride, pos));
    }

    fn ask(&mut self, job: Job) {
        if self.asked.insert(job) {
            #[cfg(target_arch = "wasm32")]
            self.queue.borrow_mut().0.push_back(job);
            if let Some(tx) = &self.tx {
                let _ = tx.send(job);
            }
        }
    }

    /// Whether `job` was asked for already.
    pub fn asked(&self, job: Job) -> bool {
        self.asked.contains(&job)
    }

    /// What was finished since the last call.
    pub fn ready(&self) -> impl Iterator<Item = Made> + '_ {
        #[cfg(target_arch = "wasm32")]
        let inline: Vec<Made> = {
            let mut q = self.queue.borrow_mut();
            let gen = q.1;
            let n = q.0.len().min(JOBS_PER_FRAME);
            q.0.drain(..n).map(|j| j.run(&gen)).collect()
        };
        #[cfg(not(target_arch = "wasm32"))]
        let inline: Vec<Made> = Vec::new();
        inline.into_iter().chain(self.rx.try_iter())
    }

    /// Wait for the next finished job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&self) -> Option<Made> {
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

    const SNAPSHOT_SEED_42_CHUNK_0_0: u64 = 12_243_604_811_049_948_823;

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
    fn biomes_ring_out_and_keep_their_own_ore() {
        // Sample a coarse grid out to 2,600 tiles on a few seeds.
        for seed in 0..6 {
            let gen = WorldGen::new(seed, 7);
            let mut nearest = [f64::MAX; 4];
            let mut own_ore = [false; 3];
            for y in (-2600..2600).step_by(13) {
                for x in (-2600..2600).step_by(13) {
                    let t = TilePos::new(x, y);
                    let b = biome_at(&gen, t);
                    let d = from_start(t);
                    nearest[b as usize] = nearest[b as usize].min(d);
                    if let Some(o) = ore_at(&gen, t) {
                        if o.resource >= 4 {
                            assert_eq!(b.exclusive(), Some(o.resource), "seed {seed} {t:?}");
                            own_ore[usize::from(o.resource - 4)] = true;
                        }
                    }
                }
            }
            assert_eq!(nearest[0], 0.0, "the start is grassland");
            assert!(nearest[1] >= 450.0, "desert at {}", nearest[1]);
            assert!(nearest[2] >= 1_000.0, "tundra at {} (R13)", nearest[2]);
            assert!(nearest[3] >= 1_600.0, "volcanic at {}", nearest[3]);
            assert!(
                nearest[1] < 1_200.0 && nearest[2] < 2_000.0,
                "seed {seed}: {nearest:?}"
            );
            assert_eq!(
                own_ore, [true; 3],
                "seed {seed}: every far biome has its ore"
            );
        }
    }

    #[test]
    fn far_patches_are_bigger_and_richer() {
        let gen = WorldGen::new(3, 4);
        let patch = |cx: i32, cy: i32| {
            let mut total = 0u64;
            for y in cy - 300..cy + 300 {
                for x in cx - 300..cx + 300 {
                    total += ore_at(&gen, TilePos::new(x, y)).map_or(0, |o| u64::from(o.amount));
                }
            }
            total
        };
        assert!(
            patch(2_000, 0) > patch(300, 0),
            "ore near 2,000 tiles out beats ore near the start"
        );
    }

    #[test]
    fn worker_generates_requested_chunks_once() {
        let mut g = Generator::spawn(GEN);
        for x in 0..4 {
            g.request(ChunkPos { x, y: 0 });
            g.request(ChunkPos { x, y: 0 });
        }
        let mut got: Vec<Chunk> = (0..4)
            .map(|_| match g.wait().unwrap() {
                Made::Chunk(c) => c,
                Made::Coarse(_) => panic!("only chunks were asked for"),
            })
            .collect();
        got.sort_by_key(|c| c.pos.x);
        assert_eq!(got[2], generate(&GEN, ChunkPos { x: 2, y: 0 }));
        assert_eq!(g.ready().count(), 0, "duplicates were not generated");
        g.request_coarse(4, ChunkPos { x: 0, y: 0 });
        let Some(Made::Coarse(c)) = g.wait() else {
            panic!("a far-out block")
        };
        assert_eq!(c, coarse(&GEN, 4, ChunkPos { x: 0, y: 0 }));
    }

    #[test]
    fn coarse_blocks_sample_the_real_terrain() {
        let c = coarse(&GEN, 8, ChunkPos { x: -1, y: 2 });
        for (i, pair) in c.ground.chunks(2).enumerate().step_by(97) {
            let (x, y) = ((i as i32) % CHUNK_SIZE, (i as i32) / CHUNK_SIZE);
            let t = TilePos::new((-CHUNK_SIZE + x) * 8 + 4, (2 * CHUNK_SIZE + y) * 8 + 4);
            let (terrain, shade) = terrain_at(&GEN, t);
            assert_eq!(pair, [terrain as u8, shade], "{t:?}");
        }
    }

    #[test]
    fn patches_list_where_the_ore_is() {
        let gen = WorldGen::new(42, 7);
        let found = patches(&gen, TilePos::new(-600, -600), TilePos::new(600, 600));
        assert!(found.len() > 100, "{} patches", found.len());
        for p in &found {
            let ore = ore_at(&gen, p.center).expect("ore at a patch's middle");
            assert_eq!(ore.resource, p.resource, "{p:?}");
        }
        assert!(
            found.iter().any(|p| p.resource == 4),
            "the desert's own ore shows"
        );
    }
}
#[cfg(test)]
mod map_print {
    use super::*;

    /// `cargo test -p mofact-world print_map -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_map() {
        let g = WorldGen::new(42, 7);
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
                        return ['I', 'C', 'S', 'K', 'B', 'T', 'R'][usize::from(o.resource)];
                    }
                    match t.terrain {
                        Terrain::DeepWater => '#',
                        Terrain::Water => '~',
                        Terrain::Sand => ':',
                        Terrain::Grass => '.',
                        Terrain::Forest => 'f',
                        Terrain::Scrub => ',',
                        Terrain::Rock => '^',
                        Terrain::Cliff => '%',
                        Terrain::Dune => 'd',
                        Terrain::Snow => '*',
                        Terrain::Ice => '=',
                        Terrain::Ash => 'a',
                        Terrain::Basalt => 'b',
                        Terrain::Lava => '!',
                    }
                })
                .collect();
            println!("{row}");
        }
        println!("{counts:?}");
    }
}
