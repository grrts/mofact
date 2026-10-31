//! Turning snapshots into sprite instances: the CPU half of the renderer.
//!
//! Kept free of wgpu so it can be tested without a GPU.

use crate::atlas::{band_for, sprites, Atlas, Uv};
use crate::look::{self, Conveyor, Look, ITEM_TILES};
use crate::palette::{self, Rgba};
use mofact_artkit::belt::{self as belts, Bend, Joins};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId, RecipeId};
use mofact_core::snapshot::{BeltItemView, EntityView, MachineState, Snapshot};
use std::collections::HashMap;

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

        let mut carriers = Carriers::default();
        for e in &latest.entities {
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
        let mut badges = Vec::new();
        // Badges keep a readable size on screen when zoomed out, as in the prototype.
        let badge = (7.0 / camera.pixels_per_tile).max(0.2) * 2.0;
        for e in &latest.entities {
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
            let working = e.state == MachineState::Working;
            let Some(sprite) = look.linked(&e.building, e.dir, setting(e), working, e.links) else {
                continue;
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
                let (surface, frame) = look.belt_layers(bend, joins, phase);
                self.instances.push(Instance {
                    uv: uv(surface),
                    ..instance
                });
                self.instances.push(Instance {
                    uv: uv(frame),
                    ..instance
                });
                if bend != Bend::Straight {
                    bends.insert(e.pos, bend);
                }
                continue;
            }
            machines.push(instance);
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
        for item in &prev.belt_items {
            before.entry(item.tile).or_default().push(item.along);
        }
        let mut index: HashMap<_, usize> = HashMap::new();
        let counts = latest.belt_items.iter().fold(HashMap::new(), |mut m, i| {
            *m.entry(i.tile).or_insert(0usize) += 1;
            m
        });
        let size = ITEM_TILES;
        for item in &latest.belt_items {
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
        self.instances.append(&mut machines);
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

/// The atlases for every band, painted with the placeholder sprites.
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

    fn belt(x: i32) -> EntityView {
        EntityView {
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
                9,
                "four belts in two layers, and an item"
            );
            scene.instances[8].pos[0] + ITEM_TILES / 2.0
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
        assert_eq!(working.len(), 1, "no badge while working");
        assert_eq!(working[0].uv, uv("furnace:copper-plate:lit:1"));
        let starved = frame(MachineState::Starved);
        assert_eq!(starved.len(), 2);
        assert_eq!(starved[0].uv, uv("furnace:copper-plate:cold:1"));
        assert_eq!(starved[1].uv, uv(look::STARVED));
        assert!(starved[1].pos[0] > 2.5, "on the machine's right");
        assert_eq!(frame(MachineState::Blocked)[1].uv, uv(look::BLOCKED));
    }

    #[test]
    fn pipes_show_what_they_hold_and_an_empty_one_shows_dark() {
        use mofact_core::snapshot::FluidView;
        let look = Look::new(&content());
        let (atlases, mut scene) = (look.atlases(), Scene::default());
        let pipe = |fill: Option<f32>| EntityView {
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
        // The ground shader: between the two ground tones.
        let g = at(2, 2);
        let between = (0..3).all(|i| {
            (palette::GROUND[i].saturating_sub(3)..=palette::GROUND2[i] + 3).contains(&g[i])
        });
        assert!(between, "ground: {g:?}");
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

    /// The CPU side of 15,000 sprites (5,000 belts in two layers, each with
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
        assert_eq!(scene.instances.len(), 15_000);
        println!("15k sprites: {:?} per frame", start.elapsed() / 10);
    }
}
