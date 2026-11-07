//! Turning snapshots into sprite instances: the CPU half of the renderer.
//!
//! Kept free of wgpu so it can be tested without a GPU.

use crate::atlas::{band_for, sprites, Atlas, Uv};
use crate::look::{self, Conveyor, Look, ITEM_TILES};
use crate::palette::{self, Rgba};
use mofact_artkit::belt::{self as belts, Bend, Joins};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId, RecipeId};
use mofact_core::snapshot::{BeltItemView, EntityView, MachineState, Snapshot, TrainView};
use std::collections::{HashMap, HashSet};
use std::f32::consts::TAU;

/// What the screen shows: the world point at its centre and how many screen
/// pixels one tile covers. Pan and pinch arrive with T15.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub center: [f32; 2],
    pub pixels_per_tile: f32,
}

impl Camera {
    /// Clip-space scale and offset for a `width` × `height` pixel target.
    pub fn transform(&self, width: u32, height: u32) -> [f32; 4] {
        let sx = 2.0 * self.pixels_per_tile / width as f32;
        let sy = -2.0 * self.pixels_per_tile / height as f32;
        [sx, sy, -self.center[0] * sx, -self.center[1] * sy]
    }

    /// The tiles on screen, as min and max corners, with a margin of one tile.
    pub fn visible(&self, width: u32, height: u32) -> ([f32; 2], [f32; 2]) {
        let hw = width as f32 / self.pixels_per_tile / 2.0 + 1.0;
        let hh = height as f32 / self.pixels_per_tile / 2.0 + 1.0;
        let c = self.center;
        ([c[0] - hw, c[1] - hh], [c[0] + hw, c[1] + hh])
    }
}

/// One sprite on screen. Matches the vertex layout in `shader.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    /// Top-left corner, in tiles.
    pub pos: [f32; 2],
    /// Size, in tiles.
    pub size: [f32; 2],
    pub uv: Uv,
    /// Multiplies the sprite's colour (sRGB bytes).
    pub tint: Rgba,
    /// Quarter turns clockwise from east.
    pub turns: u32,
}

/// The sprites for one frame, in draw order, and the band they come from.
#[derive(Default)]
pub struct Scene {
    pub band: usize,
    /// Ore on the ground, drawn first. Rebuilt by [`Scene::ore`].
    pub ore: Vec<Instance>,
    /// Belts, items and machines. Rebuilt by [`Scene::build`].
    pub instances: Vec<Instance>,
    /// What the ground is made of under the camera. Rebuilt by [`Scene::terrain`].
    pub terrain: TerrainMap,
    /// Seconds since the game started, for water ripples.
    pub time: f32,
    /// Low-power mode (T66): the ground keeps still and skips its fine
    /// close-up detail.
    /// How far into the night it is (S9): 0 by day, 1 at its darkest.
    pub night: f32,
    pub low_power: bool,
    /// Reduce motion: trees and water hold still, at full detail.
    pub still: bool,
    /// The tiles buildings stand on, from the last [`Scene::build`]: no
    /// decorations are drawn there.
    occupied: HashSet<TilePos>,
}

/// The ground under the camera, one RGBA texel per tile, for the ground
/// shader: the terrain number in red and its shade in green.
#[derive(Default)]
pub struct TerrainMap {
    /// The texture's top-left texel, counted in texels from tile (0, 0).
    pub origin: [i32; 2],
    /// Width and height in texels.
    pub size: [u32; 2],
    /// Tiles each texel covers, across and down: 1 up close, more on the
    /// world map so a far view stays a small texture.
    pub stride: i32,
    pub texels: Vec<u8>,
    /// Goes up whenever `texels` changes, so the GPU copy knows to refresh.
    pub version: u64,
    /// What the texels were made from: the rectangle and the map's version.
    made_from: Option<([i32; 4], u64, i32)>,
}

/// What a tile with no terrain yet shows: plain grass.
pub const UNKNOWN_TERRAIN: (u8, u8) = (3, 128);

/// Where the shadow of a sprite that is turned when drawn falls, in tiles:
/// always down and to the right on screen, whichever way the sprite faces.
/// Sprites painted per direction carry their own; these are painted facing
/// east with none, and cast it here from their outline.
const BELT_SHADOW: [f32; 2] = [0.03, 0.05];
const MACHINE_SHADOW: [f32; 2] = [0.07, 0.09];
const TRAIN_SHADOW: [f32; 2] = [0.06, 0.08];
const PLATFORM_SHADOW: [f32; 2] = [0.03, 0.03];

/// The shadow `of` casts `by` tiles away: its outline, dark and see-through.
fn shadow(of: &Instance, by: [f32; 2]) -> Instance {
    Instance {
        pos: [of.pos[0] + by[0], of.pos[1] + by[1]],
        tint: [0, 0, 0, palette::SHADOW[3]],
        ..*of
    }
}

fn turns(dir: Dir) -> u32 {
    match dir {
        Dir::East => 0,
        Dir::South => 1,
        Dir::West => 2,
        Dir::North => 3,
    }
}

/// What picks an entity's variant: its recipe, or the ore under a drill.
fn setting(e: &EntityView) -> Option<&str> {
    if e.exit {
        return Some("exit");
    }
    e.recipe
        .as_ref()
        .map(|r| r.as_str())
        .or(e.ore.as_ref().map(|o| o.as_str()))
}

/// Where a belt item is, in tiles, at `along` on its tile, which runs `dir`
/// and turns as `bend` says.
fn item_center(item: &BeltItemView, dir: Dir, bend: Bend) -> [f32; 2] {
    // As painted facing east, from the tile's centre; then turned.
    let (x, y) = match bend {
        Bend::Straight => (item.along - 0.5, 0.0),
        _ => belts::corner_point(bend, item.along),
    };
    let (x, y) = match dir {
        Dir::East => (x, y),
        Dir::South => (-y, x),
        Dir::West => (-x, -y),
        Dir::North => (y, -x),
    };
    [item.tile.x as f32 + 0.5 + x, item.tile.y as f32 + 0.5 + y]
}

/// The direction on the left of `d`.
fn left_of(d: Dir) -> Dir {
    d.rotate_cw().opposite()
}

/// Everything that carries items, by tile, with the way it faces and, for
/// an underground belt, whether it is the exit: what belts join up with.
#[derive(Default)]
struct Carriers(HashMap<TilePos, (Conveyor, Dir, bool)>);

impl Carriers {
    /// Whether what is on `at` hands items on to its neighbour in `d`.
    fn gives(&self, at: TilePos, d: Dir) -> bool {
        match self.0.get(&at) {
            Some(&(Conveyor::Belt, dir, _)) => dir == d,
            Some(&(Conveyor::Underground, dir, exit)) => exit && dir == d,
            Some(&(Conveyor::Splitter, dir, _)) => d == left_of(dir) || d == dir.rotate_cw(),
            None => false,
        }
    }

    /// Whether what is on `at` takes items coming in going `d`.
    fn takes(&self, at: TilePos, d: Dir) -> bool {
        match self.0.get(&at) {
            Some(&(Conveyor::Belt, dir, _)) => dir != d.opposite(),
            Some(&(Conveyor::Underground, dir, exit)) => !exit && dir == d,
            Some(&(Conveyor::Splitter, dir, _)) => dir == d,
            None => false,
        }
    }

    /// How the belt on `at`, running `d`, joins its neighbours: a corner
    /// when the only thing feeding it comes in from one side, as in the
    /// simulation, otherwise straight, with its rails closed off at an end
    /// nothing joins and opened where a belt feeds in from the side.
    fn shape(&self, at: TilePos, d: Dir) -> (Bend, Joins) {
        let (l, r) = (left_of(d), d.rotate_cw());
        let behind = self.gives(at.step(d.opposite()), d);
        let left = self.gives(at.step(l), r);
        let right = self.gives(at.step(r), l);
        let end = !self.takes(at.step(d), d);
        let joins = Joins {
            start: !behind,
            end,
            left,
            right,
        };
        match (behind, left, right) {
            (false, true, false) => (
                Bend::FromLeft,
                Joins {
                    end,
                    ..Joins::default()
                },
            ),
            (false, false, true) => (
                Bend::FromRight,
                Joins {
                    end,
                    ..Joins::default()
                },
            ),
            _ => (Bend::Straight, joins),
        }
    }
}

impl Scene {
    /// Build the frame `alpha` of the way from `prev` to `latest` (0.0 to 1.0).
    ///
    /// Tick smoothing: items on a belt tile are matched to the same tile's
    /// items in `prev` by order and drawn between the two positions, so motion
    /// stays smooth on displays faster than the 60 Hz simulation.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &mut self,
        look: &Look,
        atlases: &[Atlas],
        camera: &Camera,
        (width, height): (u32, u32),
        prev: &Snapshot,
        latest: &Snapshot,
        alpha: f32,
    ) {
        self.band = band_for(camera.pixels_per_tile).min(atlases.len().saturating_sub(1));
        self.instances.clear();
        let atlas = &atlases[self.band];
        let uv = |name: &str| atlas.get(name).unwrap_or([0.0; 4]);
        let (lo, hi) = camera.visible(width, height);
        let seen = |x: f32, y: f32, w: f32, h: f32| {
            x + w >= lo[0] && y + h >= lo[1] && x <= hi[0] && y <= hi[1]
        };
        // A big factory has far more outside the view than in it (T65): a
        // cheap test on the tile skips the rest, with room for the biggest
        // building and the neighbours a belt joins to.
        let reach = f32::from(look.largest()) + 1.0;
        let near = |t: TilePos| {
            let (x, y) = (t.x as f32, t.y as f32);
            x + reach >= lo[0] && y + reach >= lo[1] && x - 1.0 <= hi[0] && y - 1.0 <= hi[1]
        };

        let mut carriers = Carriers::default();
        for e in latest.entities.iter().filter(|e| near(e.pos)) {
            if let Some(c) = look.conveyor(&e.building) {
                carriers.0.insert(e.pos, (c, e.dir, e.exit));
            }
        }
        // The belts' surfaces move on with the items: the simulation's time,
        // smoothed as the items are, in half-tile cycles.
        let ticks =
            prev.tick as f64 + f64::from(alpha) * latest.tick.saturating_sub(prev.tick) as f64;
        let seconds = ticks * f64::from(mofact_core::TICK_SECONDS);
        let mut bends = HashMap::new();
        let mut belt_dirs = HashMap::new();
        let mut machines = Vec::new();
        // Shadows go under everything of their layer: belts' under belts,
        // machines' over belts and items but under machines.
        let mut belt_shadows = Vec::new();
        let mut machine_shadows = Vec::new();
        self.occupied.clear();
        let mut badges = Vec::new();
        // Badges keep a readable size on screen when zoomed out, as in the prototype.
        let badge = (7.0 / camera.pixels_per_tile).max(0.2) * 2.0;
        for e in latest.entities.iter().filter(|e| near(e.pos)) {
            let Some((w, h)) = look.size(&e.building) else {
                continue;
            };
            let belt = look.is_belt(&e.building);
            let turned = look.turns(&e.building);
            let (x, y, w, h) = (e.pos.x as f32, e.pos.y as f32, f32::from(w), f32::from(h));
            if turned {
                belt_dirs.insert(e.pos, e.dir);
            }
            if !seen(x, y, w, h) {
                continue;
            }
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    self.occupied
                        .insert(TilePos::new(e.pos.x + dx, e.pos.y + dy));
                }
            }
            let working = e.state == MachineState::Working;
            let Some(sprite) = look.linked(&e.building, e.dir, setting(e), working, e.links) else {
                continue;
            };
            // A turbine on a grid turns; one no pole reaches stands still.
            let sprite = match e.grid {
                Some(_) => {
                    let lag = f64::from(e.id.0 % 7) * 0.13;
                    look.spinning(&e.building, seconds + lag, working)
                        .unwrap_or(sprite)
                }
                None => sprite,
            };
            let instance = Instance {
                pos: [x, y],
                size: [w, h],
                uv: uv(sprite),
                tint: palette::WHITE,
                turns: if turned { turns(e.dir) } else { 0 },
            };
            if belt {
                let (bend, joins) = carriers.shape(e.pos, e.dir);
                let speed = look.belt_speed(&e.building).unwrap_or(1.0);
                let phase = (seconds * 2.0 * f64::from(speed)).fract() as f32;
                let (surface, frame, turns) =
                    look.belt_layers(&e.building, bend, joins, phase, instance.turns);
                let instance = Instance { turns, ..instance };
                let surface = Instance {
                    uv: uv(surface),
                    ..instance
                };
                belt_shadows.push(shadow(&surface, BELT_SHADOW));
                self.instances.push(surface);
                self.instances.push(Instance {
                    uv: uv(frame),
                    ..instance
                });
                if bend != Bend::Straight {
                    bends.insert(e.pos, bend);
                }
                continue;
            }
            if turned {
                machine_shadows.push(shadow(&instance, MACHINE_SHADOW));
            }
            machines.push(instance);
            for l in look.layers(&e.building, e.recipe.as_ref().map(|r| r.as_str())) {
                machines.push(Instance {
                    pos: [x + l.offset[0], y + l.offset[1]],
                    size: l.size,
                    uv: uv(l.sprite),
                    tint: l.tint,
                    turns: if l.turned { turns(e.dir) } else { 0 },
                });
            }
            if let Some(platform) = look.platform(&e.building) {
                let platform = Instance {
                    uv: uv(platform),
                    turns: turns(e.dir),
                    ..instance
                };
                machine_shadows.push(shadow(&platform, PLATFORM_SHADOW));
                machines.push(platform);
            }
            // What a pipe or tank holds shows in its channel or window: the
            // fluid's colour, fading in a pipe and shrinking in a tank as it
            // runs low. An empty one shows dark, so a starved line stands out.
            if let (Some((sprite, shrinks)), Some(f)) =
                (look.fluid(&e.building, e.exit, e.links), &e.fluid)
            {
                let fill = f.fill.clamp(0.0, 1.0);
                if fill > 0.0 {
                    let color = look.item_color(&f.item).unwrap_or(palette::WHITE);
                    let (k, a) = if shrinks {
                        (fill.sqrt().max(0.15), 1.0)
                    } else {
                        (1.0, 0.4 + 0.6 * fill)
                    };
                    machines.push(Instance {
                        pos: [x + w * (1.0 - k) / 2.0, y + h * (1.0 - k) / 2.0],
                        size: [w * k, h * k],
                        uv: uv(sprite),
                        tint: [color[0], color[1], color[2], (255.0 * a) as u8],
                        turns: instance.turns,
                    });
                }
            }
            let status = match e.state {
                MachineState::Starved | MachineState::NoPower => Some(look::STARVED),
                MachineState::Blocked => Some(look::BLOCKED),
                _ => None,
            };
            if let Some(status) = status.filter(|_| look.has_badge(&e.building)) {
                // On the top-right corner, a little inside.
                let r = badge / 2.0;
                badges.push(Instance {
                    pos: [x + w - r - r * 0.3, y - r + r * 0.3],
                    size: [badge, badge],
                    uv: uv(status),
                    tint: palette::WHITE,
                    turns: 0,
                });
            }
        }

        let mut before: HashMap<_, Vec<f32>> = HashMap::new();
        for item in prev.belt_items.iter().filter(|i| near(i.tile)) {
            before.entry(item.tile).or_default().push(item.along);
        }
        let mut index: HashMap<_, usize> = HashMap::new();
        let counts =
            latest
                .belt_items
                .iter()
                .filter(|i| near(i.tile))
                .fold(HashMap::new(), |mut m, i| {
                    *m.entry(i.tile).or_insert(0usize) += 1;
                    m
                });
        let size = ITEM_TILES;
        for item in latest.belt_items.iter().filter(|i| near(i.tile)) {
            let n = index.entry(item.tile).or_insert(0);
            let k = *n;
            *n += 1;
            let (Some(&dir), Some((sprite, tint))) =
                (belt_dirs.get(&item.tile), look.item(&item.item))
            else {
                continue;
            };
            let mut along = item.along;
            if let Some(old) = before.get(&item.tile) {
                if old.len() == counts[&item.tile] && old[k] <= along {
                    along = old[k] + (along - old[k]) * alpha;
                }
            }
            let c = item_center(
                &BeltItemView {
                    along,
                    ..item.clone()
                },
                dir,
                bends.get(&item.tile).copied().unwrap_or_default(),
            );
            if !seen(c[0] - size / 2.0, c[1] - size / 2.0, size, size) {
                continue;
            }
            self.instances.push(Instance {
                pos: [c[0] - size / 2.0, c[1] - size / 2.0],
                size: [size, size],
                uv: uv(sprite),
                tint,
                turns: 0,
            });
        }
        self.instances.splice(0..0, belt_shadows);
        self.instances.append(&mut machine_shadows);
        self.instances.append(&mut machines);
        // Trains ride over the track, smoothed between ticks as items are.
        let at = |t: &TrainView| {
            let (dx, dy) = t.dir.offset();
            let k = t.along.clamp(0.0, 1.0);
            [
                t.pos.x as f32 + dx as f32 * k,
                t.pos.y as f32 + dy as f32 * k,
            ]
        };
        for t in &latest.trains {
            let Some(sprite) = look.sprite(&t.building, t.dir, None, false) else {
                continue;
            };
            let mut p = at(t);
            if let Some(old) = prev.trains.iter().find(|o| o.id == t.id) {
                let q = at(old);
                // A train only moves a tile or so a tick; anything further is a jump.
                if (p[0] - q[0]).abs() + (p[1] - q[1]).abs() < 2.0 {
                    p = [q[0] + (p[0] - q[0]) * alpha, q[1] + (p[1] - q[1]) * alpha];
                }
            }
            if !seen(p[0], p[1], 1.0, 1.0) {
                continue;
            }
            let train = Instance {
                pos: p,
                size: [1.0, 1.0],
                uv: uv(sprite),
                tint: palette::WHITE,
                turns: turns(t.dir),
            };
            self.instances.push(shadow(&train, TRAIN_SHADOW));
            self.instances.push(train);
        }
        self.instances.append(&mut badges);
    }
}

impl Scene {
    /// Add see-through `ghosts` (building, tile, facing) of what a gesture
    /// would build, on top of everything. Call after [`Scene::build`].
    pub fn ghosts<'a>(
        &mut self,
        look: &Look,
        atlases: &[Atlas],
        ghosts: impl IntoIterator<Item = (&'a BuildingId, TilePos, Dir, Option<&'a RecipeId>)>,
    ) {
        let atlas = &atlases[self.band];
        for (building, tile, dir, recipe) in ghosts {
            let (Some((w, h)), Some(sprite)) = (
                look.size(building),
                look.sprite(building, dir, recipe.map(|r| r.as_str()), false),
            ) else {
                continue;
            };
            self.instances.push(Instance {
                pos: [tile.x as f32, tile.y as f32],
                size: [f32::from(w), f32::from(h)],
                uv: atlas.get(sprite).unwrap_or([0.0; 4]),
                tint: [255, 255, 255, 140],
                turns: if look.turns(building) { turns(dir) } else { 0 },
            });
        }
    }

    /// Lay `ore` (tile and ore item) on the ground, keeping what the camera sees.
    /// Call after [`Scene::build`], which picks the band.
    pub fn ore<'a>(
        &mut self,
        look: &Look,
        atlases: &[Atlas],
        camera: &Camera,
        (width, height): (u32, u32),
        ore: impl IntoIterator<Item = (TilePos, &'a ItemId)>,
    ) {
        self.ore.clear();
        // On the world map the patches show as icons instead.
        if map_stride(camera.pixels_per_tile) > 1 {
            return;
        }
        let uvs: Vec<Uv> = (0..sprites::ORE_VARIANTS)
            .map(|k| {
                atlases[self.band]
                    .get(&sprites::ore_name(k))
                    .unwrap_or([0.0; 4])
            })
            .collect();
        let (lo, hi) = camera.visible(width, height);
        for (tile, item) in ore {
            let (x, y) = (tile.x as f32, tile.y as f32);
            if x + 1.0 < lo[0] || y + 1.0 < lo[1] || x > hi[0] || y > hi[1] {
                continue;
            }
            if let Some(tint) = look.item_color(item) {
                // The same tile always gets the same variant.
                let h = (tile.x as u32).wrapping_mul(0x9e37_79b1)
                    ^ (tile.y as u32).wrapping_mul(0x85eb_ca77);
                let uv = uvs[((h >> 13) % sprites::ORE_VARIANTS) as usize];
                self.ore.push(Instance {
                    pos: [x, y],
                    size: [1.0, 1.0],
                    uv,
                    tint,
                    turns: 0,
                });
            }
        }
    }
}

impl Scene {
    /// Lay trees, bushes and stones (`kind` numbered from 1, as the world
    /// numbers them) on the ground, over the ore and under everything built.
    /// Call after [`Scene::ore`]; none are drawn under a building.
    pub fn decor(
        &mut self,
        atlases: &[Atlas],
        camera: &Camera,
        (width, height): (u32, u32),
        decor: impl IntoIterator<Item = (TilePos, u8)>,
    ) {
        // Too small to read from far out, and there would be thousands.
        if camera.pixels_per_tile < 7.0 {
            return;
        }
        let atlas = &atlases[self.band];
        let uvs: Vec<Uv> = (1..=sprites::DECOR_KINDS)
            .map(|k| atlas.get(&sprites::decor_name(k)).unwrap_or([0.0; 4]))
            .collect();
        let (lo, hi) = camera.visible(width, height);
        let mut seen: Vec<(TilePos, u8)> = decor
            .into_iter()
            .filter(|(t, k)| {
                let (x, y) = (t.x as f32, t.y as f32);
                (1..=sprites::DECOR_KINDS).contains(k)
                    && x + 2.0 >= lo[0]
                    && y + 2.0 >= lo[1]
                    && x - 1.0 <= hi[0]
                    && y - 1.0 <= hi[1]
                    && !self.occupied.contains(t)
            })
            .collect();
        // Back to front, so a tree further down overlaps the one above it.
        seen.sort_by_key(|(t, _)| (t.y, t.x));
        for (tile, kind) in seen {
            let h = (tile.x as u32).wrapping_mul(0x27d4_eb2d)
                ^ (tile.y as u32).wrapping_mul(0x1656_67b1);
            let h = h ^ (h >> 15);
            let jitter = |shift: u32| ((h >> shift) & 0xff) as f32 / 255.0 - 0.5;
            // Trees and boulders spread past their tile; the rest stay inside it.
            let base = match kind {
                1 | 2 => 1.45,
                5 => 1.15,
                _ => 0.9,
            };
            let size = base * (1.0 + 0.25 * jitter(0));
            // Trees and bushes sway in the wind (S8): a slow lean and a
            // breath of the canopy, each on its own beat, the wind's gusts
            // rolling across the land. Still in low-power mode.
            let sway = if self.low_power || self.still || !matches!(kind, 1..=3) {
                0.0
            } else {
                let gust = 0.6 + 0.4 * (self.time * 0.35 - tile.x as f32 * 0.07).sin();
                gust * (self.time * 1.3 + jitter(24) * TAU).sin()
            };
            let size = size * (1.0 + 0.02 * sway);
            let (x, y) = (
                tile.x as f32 + 0.5 + 0.3 * jitter(8) - size / 2.0 + 0.035 * sway,
                tile.y as f32 + 0.5 + 0.3 * jitter(16) - size / 2.0,
            );
            self.ore.push(Instance {
                pos: [x, y],
                size: [size, size],
                uv: uvs[usize::from(kind - 1)],
                tint: palette::WHITE,
                turns: 0,
            });
        }
    }

    /// Fill [`Scene::terrain`] for what the camera sees, from `at` (terrain
    /// number and shade, or `None` where nothing is generated yet). `at` is
    /// asked for texels: a texel position and how many tiles it spans. Up
    /// close that is one tile; zoomed far out it is a few, see
    /// [`map_stride`]. `version` is the map's: it is only read again when
    /// that or the view changes.
    /// Forget the terrain drawn so far, as when another map is loaded:
    /// the next [`terrain`](Self::terrain) call makes it afresh.
    pub fn forget_terrain(&mut self) {
        self.terrain.made_from = None;
    }

    pub fn terrain(
        &mut self,
        camera: &Camera,
        (width, height): (u32, u32),
        version: u64,
        at: impl Fn(TilePos, i32) -> Option<(u8, u8)>,
    ) {
        let stride = map_stride(camera.pixels_per_tile);
        let (lo, hi) = camera.visible(width, height);
        let s = stride as f32;
        // A texel's edges blend with its neighbours, so take one more all round.
        let rect = [
            (lo[0] / s).floor() as i32 - 2,
            (lo[1] / s).floor() as i32 - 2,
            (hi[0] / s).ceil() as i32 + 2,
            (hi[1] / s).ceil() as i32 + 2,
        ];
        let map = &mut self.terrain;
        if map.made_from == Some((rect, version, stride)) {
            return;
        }
        map.made_from = Some((rect, version, stride));
        map.origin = [rect[0], rect[1]];
        map.size = [(rect[2] - rect[0]) as u32, (rect[3] - rect[1]) as u32];
        map.stride = stride;
        map.texels.clear();
        for y in rect[1]..rect[3] {
            for x in rect[0]..rect[2] {
                let (kind, shade) = at(TilePos::new(x, y), stride).unwrap_or(UNKNOWN_TERRAIN);
                map.texels.extend_from_slice(&[kind, shade, 0, 255]);
            }
        }
        map.version += 1;
    }
}

/// Tiles per terrain texel at `pixels_per_tile`: one while a tile is at
/// least 3 px, then doubling, so a texel never drops below 3 px.
pub fn map_stride(pixels_per_tile: f32) -> i32 {
    let mut stride = 1;
    while (stride as f32) * pixels_per_tile < 3.0 && stride < 64 {
        stride *= 2;
    }
    stride
}

/// The atlases for every band, painted with the placeholder sprites.
/// How far into the night it is at `tick` (S9): a day lasts ten minutes,
/// dusk and dawn ease in and out, and the dark part is about a fifth of it.
/// Drawing only: nothing in the factory runs differently at night.
pub fn night_at(tick: u64) -> f32 {
    const DAY: u64 = 10 * 60 * mofact_core::TICKS_PER_SECOND as u64;
    // Start at mid-morning, so a new game opens in daylight.
    let phase = ((tick + DAY / 8) % DAY) as f32 / DAY as f32;
    let sun = (phase * std::f32::consts::TAU).cos();
    // Below the horizon from a quarter of the way down.
    ((-sun - 0.3) / 0.5).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofact_core::content::Content;
    use mofact_core::ids::EntityId;
    use mofact_core::snapshot::EntityView;

    fn content() -> Content {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        Content {
            items: ron::from_str(&read("items.ron")).unwrap(),
            recipes: ron::from_str(&read("recipes.ron")).unwrap(),
            buildings: ron::from_str(&read("buildings.ron")).unwrap(),
            research: Vec::new(),
            goals: Vec::new(),
        }
    }

    #[test]
    fn ground_shader_uses_the_palette_terrain_colours() {
        // The shader spells the land colours out; keep them in step with the
        // palette, which the map and the tests read.
        let shader = include_str!("shader.wgsl");
        for kind in 2..=12 {
            let [r, g, b, _] = palette::TERRAIN[kind];
            let f = |v: u8| format!("{:.3}", f32::from(v) / 255.0);
            let want = format!("vec3<f32>({}, {}, {})", f(r), f(g), f(b));
            assert!(
                shader.contains(&want),
                "terrain {kind}: {want} not in the shader"
            );
        }
    }

    fn belt(x: i32) -> EntityView {
        EntityView {
            request: None,
            id: EntityId(x as u32),
            building: "belt".into(),
            pos: TilePos::new(x, 0),
            dir: Dir::East,
            recipe: None,
            ore: None,
            grid: None,
            exit: false,
            fluid: None,
            links: 0,
            state: MachineState::Idle,
            progress: 0.0,
        }
    }

    fn snap(along: f32) -> Snapshot {
        Snapshot {
            entities: (0..4).map(belt).collect(),
            belt_items: vec![BeltItemView {
                item: "iron-plate".into(),
                tile: TilePos::new(1, 0),
                along,
            }],
            ..Default::default()
        }
    }

    const CAMERA: Camera = Camera {
        center: [2.0, 0.5],
        pixels_per_tile: 32.0,
    };

    #[test]
    fn items_move_smoothly_between_ticks() {
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        let x_at = |scene: &mut Scene, alpha| {
            scene.build(
                &look,
                &atlases,
                &CAMERA,
                (640, 360),
                &snap(0.2),
                &snap(0.6),
                alpha,
            );
            assert_eq!(
                scene.instances.len(),
                13,
                "four belts in two layers over their shadows, and an item"
            );
            scene.instances[12].pos[0] + ITEM_TILES / 2.0
        };
        let xs: Vec<f32> = [0.0, 0.5, 1.0]
            .iter()
            .map(|&a| x_at(&mut scene, a))
            .collect();
        for (x, want) in xs.iter().zip([1.2, 1.4, 1.6]) {
            assert!((x - want).abs() < 1e-5, "{xs:?}");
        }
    }

    #[test]
    fn far_away_things_change_nothing_on_screen_but_edge_ones_still_draw() {
        // Culling by tile before anything else (T65) must draw the same: a
        // big assembler poking in from just off the left edge still shows,
        // and a thousand belts and items far away add nothing.
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        let (lo, _) = CAMERA.visible(640, 360);
        let mut near = snap(0.5);
        let mut edge = belt(0);
        edge.building = "hub".into();
        edge.id = EntityId(500);
        edge.pos = TilePos::new(lo[0].floor() as i32 - 2, 0);
        near.entities.push(edge);
        scene.build(&look, &atlases, &CAMERA, (640, 360), &near, &near, 1.0);
        let alone = scene.instances.clone();
        assert!(
            alone.iter().any(|i| i.pos[0] < lo[0]),
            "the assembler on the edge is drawn"
        );
        let mut crowded = near.clone();
        for k in 0..1000 {
            let mut b = belt(0);
            b.id = EntityId(1000 + k);
            b.pos = TilePos::new(300 + k as i32 % 40, 300 + k as i32 / 40);
            crowded.belt_items.push(BeltItemView {
                item: "iron-plate".into(),
                tile: b.pos,
                along: 0.5,
            });
            crowded.entities.push(b);
        }
        scene.build(
            &look,
            &atlases,
            &CAMERA,
            (640, 360),
            &crowded,
            &crowded,
            1.0,
        );
        assert_eq!(scene.instances, alone);
    }

    #[test]
    fn trees_sway_in_the_wind_but_stones_low_power_and_reduce_motion_stay_still() {
        let look = Look::new(&content());
        let atlases = look.atlases();
        let at_with = |time: f32, low_power: bool, still: bool| {
            let mut scene = Scene {
                time,
                low_power,
                still,
                ..Scene::default()
            };
            let decor = [(TilePos::new(1, 0), 1u8), (TilePos::new(3, 0), 4u8)];
            scene.decor(&atlases, &CAMERA, (640, 360), decor);
            scene
                .ore
                .iter()
                .map(|i| (i.pos, i.size))
                .collect::<Vec<_>>()
        };
        let at = |time: f32, low_power: bool| at_with(time, low_power, false);
        let (a, b) = (at(0.0, false), at(0.7, false));
        assert_ne!(a[0], b[0], "the tree moves");
        assert_eq!(a[1], b[1], "the stone does not");
        assert_eq!(
            at(0.0, true),
            at(0.7, true),
            "nothing moves in low-power mode"
        );
        assert_eq!(
            at_with(0.0, false, true),
            at_with(0.7, false, true),
            "nor with reduce motion"
        );
        // A sway, not a walk: it stays within a few hundredths of a tile.
        assert!((a[0].0[0] - b[0].0[0]).abs() < 0.1);
    }

    #[test]
    fn off_screen_sprites_are_culled_and_bands_follow_zoom() {
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        let far = Camera {
            center: [100.0, 100.0],
            ..CAMERA
        };
        scene.build(
            &look,
            &atlases,
            &far,
            (640, 360),
            &snap(0.5),
            &snap(0.5),
            1.0,
        );
        assert!(scene.instances.is_empty());
        let close = Camera {
            pixels_per_tile: 64.0,
            ..CAMERA
        };
        scene.build(
            &look,
            &atlases,
            &close,
            (640, 360),
            &snap(0.5),
            &snap(0.5),
            1.0,
        );
        assert_eq!(scene.band, 2);
    }

    #[test]
    fn machines_draw_their_variant_and_a_badge_when_stalled() {
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        let furnace = |state| EntityView {
            request: None,
            id: EntityId(9),
            building: "furnace".into(),
            pos: TilePos::new(2, 0),
            dir: Dir::South,
            recipe: Some("copper-plate".into()),
            ore: None,
            grid: None,
            exit: false,
            fluid: None,
            links: 0,
            state,
            progress: 0.0,
        };
        let mut frame = |state| {
            let snap = Snapshot {
                entities: vec![furnace(state)],
                ..Default::default()
            };
            scene.build(&look, &atlases, &CAMERA, (640, 360), &snap, &snap, 1.0);
            scene.instances.clone()
        };
        let uv = |name: &str| atlases[1].get(name).unwrap();
        let working = frame(MachineState::Working);
        // The body, then its ring in copper and the arrow over it.
        assert_eq!(working.len(), 3, "no badge while working");
        assert_eq!(working[0].uv, uv("furnace:none:lit:1"));
        assert_eq!(working[1].uv, uv("furnace:ring"));
        assert_eq!(working[1].tint, palette::item("copper"));
        assert_eq!(working[2].turns, 1, "the arrow faces south");
        let starved = frame(MachineState::Starved);
        assert_eq!(starved.len(), 4);
        assert_eq!(starved[0].uv, uv("furnace:none:cold:1"));
        assert_eq!(starved[3].uv, uv(look::STARVED));
        assert!(starved[3].pos[0] > 2.5, "on the machine's right");
        assert_eq!(frame(MachineState::Blocked)[3].uv, uv(look::BLOCKED));
    }

    #[test]
    fn pipes_show_what_they_hold_and_an_empty_one_shows_dark() {
        use mofact_core::snapshot::FluidView;
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        let pipe = |fill: Option<f32>| EntityView {
            request: None,
            id: EntityId(3),
            building: "pipe".into(),
            pos: TilePos::new(1, 0),
            dir: Dir::East,
            recipe: None,
            ore: None,
            grid: None,
            exit: false,
            fluid: fill.map(|fill| FluidView {
                item: "water".into(),
                fill,
            }),
            links: 1 | 4 | 8,
            state: MachineState::Working,
            progress: 0.0,
        };
        let mut frame = |fill| {
            let snap = Snapshot {
                entities: vec![pipe(fill)],
                ..Default::default()
            };
            scene.build(&look, &atlases, &CAMERA, (640, 360), &snap, &snap, 1.0);
            scene.instances.clone()
        };
        let uv = |name: &str| atlases[1].get(name).unwrap();
        let empty = frame(None);
        assert_eq!(empty.len(), 1, "an empty pipe draws its dark channel only");
        assert_eq!(empty[0].uv, uv("pipe:13"), "joined east, west and north");
        let full = frame(Some(1.0));
        assert_eq!(full.len(), 2);
        assert_eq!(full[1].uv, uv("pipe-fluid:13"));
        let water = palette::item("water");
        assert_eq!(full[1].tint, [water[0], water[1], water[2], 255]);
        let low = frame(Some(0.1));
        assert!(low[1].tint[3] < 140, "a pipe running low fades");
    }

    #[test]
    fn camera_maps_its_center_to_the_middle_of_the_screen() {
        let [sx, sy, ox, oy] = CAMERA.transform(640, 360);
        assert!((CAMERA.center[0] * sx + ox).abs() < 1e-6);
        assert!((CAMERA.center[1] * sy + oy).abs() < 1e-6);
        assert!(
            ((CAMERA.center[0] + 10.0) * sx + ox - 1.0).abs() < 1e-6,
            "10 tiles is 320 px, the right edge"
        );
    }

    /// Needs a GPU; CI runs it on Linux with Mesa's software Vulkan.
    #[test]
    #[ignore = "needs a GPU"]
    fn sprites_reach_the_screen() {
        let gpu = crate::Gpu::headless().expect("a GPU");
        let mut renderer = crate::Renderer::new(&gpu, Look::new(&content()).atlases());
        let look = Look::new(&content());
        let mut scene = Scene::default();
        let size = (128, 64);
        let camera = Camera {
            center: [2.0, 0.5],
            pixels_per_tile: 32.0,
        };
        scene.build(
            &look,
            renderer.atlases(),
            &camera,
            size,
            &snap(0.5),
            &snap(0.5),
            1.0,
        );
        let px = renderer.capture(&gpu, size, &camera, &scene, None, crate::FLOOR);
        let at = |x: u32, y: u32| {
            let i = ((y * size.0 + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2]]
        };
        let close = |a: [u8; 3], b: [u8; 4]| a.iter().zip(b).all(|(&a, b)| a.abs_diff(b) <= 3);
        // The ground shader, with no terrain map: grass, give or take its mottling.
        let g = at(2, 2);
        let grass = palette::TERRAIN[3];
        let near = (0..3).all(|i| g[i].abs_diff(grass[i]) <= 12);
        assert!(near, "ground: {g:?}, grass {grass:?}");
        // Between a roller and the rail, clear of the chevrons.
        assert!(
            close(at(20, 24), palette::BELT),
            "belt body: {:?}",
            at(20, 24)
        );
        // The plate sits in the middle of tile 1 (x = 48 px, y = 32 px); just
        // above its middle groove it is plain plate colour.
        assert!(
            close(at(48, 30), palette::item("iron")),
            "plate: {:?}",
            at(48, 30)
        );
    }

    #[test]
    fn ore_in_view_is_laid_under_everything() {
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        scene.build(
            &look,
            &atlases,
            &CAMERA,
            (640, 360),
            &snap(0.5),
            &snap(0.5),
            1.0,
        );
        let iron = ItemId::from("iron-ore");
        let near = TilePos::new(3, 2);
        let far = TilePos::new(300, 2);
        scene.ore(
            &look,
            &atlases,
            &CAMERA,
            (640, 360),
            [(near, &iron), (far, &iron)],
        );
        assert_eq!(scene.ore.len(), 1, "the far tile is culled");
        assert_eq!(scene.ore[0].pos, [3.0, 2.0]);
        assert_eq!(scene.ore[0].tint, palette::item("iron-ore"));
    }

    /// Needs a GPU.
    #[test]
    #[ignore = "needs a GPU"]
    fn the_overlay_draws_over_the_world_in_pixels() {
        let gpu = crate::Gpu::headless().expect("a GPU");
        let mut renderer = crate::Renderer::new(&gpu, Look::new(&content()).atlases());
        let mut overlay = crate::Overlay::new(&Look::new(&content()));
        overlay.panel([10.0, 10.0], [60.0, 30.0], 6.0, palette::ACCENT);
        let w = overlay.text("Mofact", [80.0, 36.0], 24.0, palette::WHITE);
        let scene = Scene::default();
        let size = (160, 48);
        let px = renderer.capture(&gpu, size, &CAMERA, &scene, Some(&overlay), crate::FLOOR);
        let at = |x: u32, y: u32| {
            let i = ((y * size.0 + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2]]
        };
        assert_eq!(at(40, 25), [0xff, 0xd1, 0x66], "inside the panel");
        assert_ne!(
            at(11, 11),
            [0xff, 0xd1, 0x66],
            "the rounded corner lets the ground through"
        );
        let lit = (80..80 + w as u32)
            .flat_map(|x| (12..36).map(move |y| (x, y)))
            .filter(|&(x, y)| at(x, y)[0] > 200)
            .count();
        assert!(lit > 100, "the text drew {lit} bright pixels");
    }

    #[test]
    fn belts_join_up_with_corners_side_loads_and_ends() {
        use Dir::*;
        let mut c = Carriers::default();
        let t = TilePos::new;
        // A run east that turns south, then west, and a belt fed from both sides.
        for (x, y, d) in [(0, 0, East), (1, 0, East), (2, 0, South), (2, 1, West)] {
            c.0.insert(t(x, y), (Conveyor::Belt, d, false));
        }
        for (x, y, d) in [(5, 0, South), (5, 1, South), (4, 1, East), (6, 1, West)] {
            c.0.insert(t(x, y), (Conveyor::Belt, d, false));
        }
        let start = Joins {
            start: true,
            ..Joins::default()
        };
        assert_eq!(c.shape(t(0, 0), East), (Bend::Straight, start));
        assert_eq!(c.shape(t(1, 0), East), (Bend::Straight, Joins::default()));
        // Going south, the run comes in from the west: the right-hand side.
        assert_eq!(c.shape(t(2, 0), South), (Bend::FromRight, Joins::default()));
        let end = Joins {
            end: true,
            ..Joins::default()
        };
        assert_eq!(c.shape(t(2, 1), West), (Bend::FromRight, end));
        // Fed from behind and both sides: straight, open on both sides.
        let both = Joins {
            end: true,
            left: true,
            right: true,
            ..Joins::default()
        };
        assert_eq!(c.shape(t(5, 1), South), (Bend::Straight, both));
        // Side-feeders end at the belt they feed, with no cap.
        assert_eq!(c.shape(t(4, 1), East), (Bend::Straight, start));
        // A belt facing into another head-on is not taken: it ends there.
        c.0.insert(t(9, 0), (Conveyor::Belt, East, false));
        c.0.insert(t(10, 0), (Conveyor::Belt, West, false));
        assert!(c.shape(t(9, 0), East).1.end);
        // A splitter gives out sideways and takes from behind.
        c.0.insert(t(0, 5), (Conveyor::Splitter, North, false));
        c.0.insert(t(1, 5), (Conveyor::Belt, East, false));
        c.0.insert(t(0, 6), (Conveyor::Belt, North, false));
        assert!(!c.shape(t(1, 5), East).1.start, "fed by the splitter");
        assert!(!c.shape(t(0, 6), North).1.end, "feeds the splitter");
    }

    #[test]
    fn items_round_a_corner_on_the_arc() {
        let item = |along| BeltItemView {
            item: "iron-plate".into(),
            tile: TilePos::new(0, 0),
            along,
        };
        // Facing south, in from the west: from the west edge to the south edge.
        let a = item_center(&item(0.5), Dir::South, Bend::FromRight);
        let b = item_center(&item(1.0), Dir::South, Bend::FromRight);
        assert!(
            (a[0] - 0.0).abs() < 1e-4 && (a[1] - 0.5).abs() < 1e-4,
            "{a:?}"
        );
        assert!(
            (b[0] - 0.5).abs() < 1e-4 && (b[1] - 1.0).abs() < 1e-4,
            "{b:?}"
        );
    }

    /// A belt or splitter turned any way casts its shadow the same way on
    /// screen: down and to the right, not turned with it.
    #[test]
    fn shadows_fall_the_same_way_whichever_way_a_sprite_faces() {
        let look = Look::new(&content());
        let atlases = look.atlases();
        for building in ["belt", "splitter"] {
            let mut snapshot = Snapshot::default();
            for (i, dir) in Dir::ALL.into_iter().enumerate() {
                let mut e = belt(i as i32 * 3);
                e.building = building.into();
                e.dir = dir;
                snapshot.entities.push(e);
            }
            let mut scene = Scene::default();
            scene.build(
                &look,
                &atlases,
                &CAMERA,
                (640, 360),
                &snapshot,
                &snapshot,
                0.0,
            );
            let dark = |i: &&Instance| i.tint[..3] == [0, 0, 0];
            let shadows: Vec<_> = scene.instances.iter().filter(dark).collect();
            assert_eq!(shadows.len(), 4, "{building}: one shadow each");
            let by = if building == "belt" {
                BELT_SHADOW
            } else {
                MACHINE_SHADOW
            };
            for s in shadows {
                let body = scene
                    .instances
                    .iter()
                    .find(|i| !dark(i) && i.uv == s.uv && i.turns == s.turns)
                    .expect("the sprite that casts it");
                let off = [s.pos[0] - body.pos[0], s.pos[1] - body.pos[1]];
                assert!(
                    (off[0] - by[0]).abs() < 1e-6 && (off[1] - by[1]).abs() < 1e-6,
                    "{building} turned {}: shadow off by {off:?}",
                    s.turns
                );
            }
        }
    }

    /// The CPU side of 20,000 sprites (5,000 belts in two layers over a shadow, each with
    /// an item); prints the time per frame.
    #[test]
    fn ten_thousand_sprites() {
        let look = Look::new(&content());
        let atlases = look.atlases();
        let mut snapshot = Snapshot::default();
        for i in 0..5_000 {
            let mut b = belt(i % 100);
            b.pos.y = i / 100;
            snapshot.entities.push(b.clone());
            snapshot.belt_items.push(BeltItemView {
                item: "gear".into(),
                tile: b.pos,
                along: 0.5,
            });
        }
        let camera = Camera {
            center: [50.0, 25.0],
            pixels_per_tile: 10.0,
        };
        let mut scene = Scene::default();
        let start = std::time::Instant::now();
        for _ in 0..10 {
            scene.build(
                &look,
                &atlases,
                &camera,
                (1100, 600),
                &snapshot,
                &snapshot,
                0.5,
            );
        }
        assert_eq!(scene.instances.len(), 20_000);
        println!("20k sprites: {:?} per frame", start.elapsed() / 10);
    }
}
