//! Turning snapshots into sprite instances: the CPU half of the renderer.
//!
//! Kept free of wgpu so it can be tested without a GPU.

use crate::atlas::{band_for, sprites, Atlas, Uv, BANDS};
use crate::palette::{self, Rgba};
use mofact_core::content::{BuildingRole, Content, IconShape};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId};
use mofact_core::snapshot::{BeltItemView, MachineState, Snapshot};
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

/// How each building and item looks, looked up once from the content.
#[derive(Default)]
pub struct Look {
    buildings: HashMap<BuildingId, (&'static str, (u8, u8), Rgba)>,
    items: HashMap<ItemId, (usize, Rgba)>,
}

impl Look {
    pub fn new(content: &Content) -> Self {
        let buildings = content
            .buildings
            .iter()
            .map(|b| {
                let sprite = match b.role {
                    BuildingRole::Belt { .. } => sprites::BELT,
                    BuildingRole::Hub => sprites::HUB,
                    _ => sprites::MACHINE,
                };
                (
                    b.id.clone(),
                    (sprite, b.size, palette::building(b.id.as_str())),
                )
            })
            .collect();
        let items = content
            .items
            .iter()
            .map(|i| {
                let shape = match i.shape {
                    IconShape::Ore => 0,
                    IconShape::Plate => 1,
                    IconShape::Gear => 2,
                    IconShape::Wire => 3,
                    IconShape::Circuit => 4,
                    IconShape::Droplet => 5,
                };
                (i.id.clone(), (shape, palette::item(&i.color)))
            })
            .collect();
        Self { buildings, items }
    }
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

/// Where a belt item is, in tiles, at `along` on its tile.
fn item_center(item: &BeltItemView, dir: Dir) -> [f32; 2] {
    let (dx, dy) = dir.offset();
    let t = item.along - 0.5;
    [
        item.tile.x as f32 + 0.5 + dx as f32 * t,
        item.tile.y as f32 + 0.5 + dy as f32 * t,
    ]
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
        self.band = band_for(camera.pixels_per_tile);
        self.instances.clear();
        let atlas = &atlases[self.band];
        let uv = |name: &str| atlas.get(name).unwrap_or([0.0; 4]);
        let (lo, hi) = camera.visible(width, height);
        let seen = |x: f32, y: f32, w: f32, h: f32| {
            x + w >= lo[0] && y + h >= lo[1] && x <= hi[0] && y <= hi[1]
        };

        let mut belt_dirs = HashMap::new();
        let mut machines = Vec::new();
        for e in &latest.entities {
            let Some(&(sprite, (w, h), tint)) = look.buildings.get(&e.building) else {
                continue;
            };
            let (x, y, w, h) = (e.pos.x as f32, e.pos.y as f32, f32::from(w), f32::from(h));
            if sprite == sprites::BELT {
                belt_dirs.insert(e.pos, e.dir);
            }
            if !seen(x, y, w, h) {
                continue;
            }
            let state = match e.state {
                MachineState::Starved | MachineState::NoPower => palette::STARVE,
                _ => tint,
            };
            let instance = Instance {
                pos: [x, y],
                size: [w, h],
                uv: uv(sprite),
                tint: if sprite == sprites::BELT { tint } else { state },
                turns: if sprite == sprites::BELT {
                    turns(e.dir)
                } else {
                    0
                },
            };
            if sprite == sprites::BELT {
                self.instances.push(instance);
            } else {
                machines.push(instance);
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
        let size = 0.42;
        for item in &latest.belt_items {
            let n = index.entry(item.tile).or_insert(0);
            let k = *n;
            *n += 1;
            let (Some(&dir), Some(&(shape, tint))) =
                (belt_dirs.get(&item.tile), look.items.get(&item.item))
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
            );
            if !seen(c[0] - size / 2.0, c[1] - size / 2.0, size, size) {
                continue;
            }
            self.instances.push(Instance {
                pos: [c[0] - size / 2.0, c[1] - size / 2.0],
                size: [size, size],
                uv: uv(sprites::SHAPES[shape]),
                tint,
                turns: 0,
            });
        }
        self.instances.append(&mut machines);
    }
}

impl Scene {
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
        let uv = atlases[self.band].get(sprites::ORE).unwrap_or([0.0; 4]);
        let (lo, hi) = camera.visible(width, height);
        for (tile, item) in ore {
            let (x, y) = (tile.x as f32, tile.y as f32);
            if x + 1.0 < lo[0] || y + 1.0 < lo[1] || x > hi[0] || y > hi[1] {
                continue;
            }
            if let Some(&(_, tint)) = look.items.get(item) {
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
pub fn atlases() -> Vec<Atlas> {
    BANDS
        .iter()
        .map(|&px| {
            let mut atlas = Atlas::new(px * 8);
            sprites::paint_all(&mut atlas, px);
            atlas
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofact_core::ids::EntityId;
    use mofact_core::snapshot::EntityView;

    fn content() -> Content {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        Content {
            items: ron::from_str(&read("items.ron")).unwrap(),
            recipes: ron::from_str(&read("recipes.ron")).unwrap(),
            buildings: ron::from_str(&read("buildings.ron")).unwrap(),
        }
    }

    fn belt(x: i32) -> EntityView {
        EntityView {
            id: EntityId(x as u32),
            building: "belt".into(),
            pos: TilePos::new(x, 0),
            dir: Dir::East,
            recipe: None,
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
        let (look, atlases, mut scene) = (Look::new(&content()), atlases(), Scene::default());
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
            assert_eq!(scene.instances.len(), 5, "four belts and an item");
            scene.instances[4].pos[0] + 0.21
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
        let (look, atlases, mut scene) = (Look::new(&content()), atlases(), Scene::default());
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
        let mut renderer = crate::Renderer::new(&gpu, atlases());
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
        let px = renderer.capture(&gpu, size, &camera, &scene, crate::FLOOR);
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
        assert!(
            close(at(16, 32), palette::BELT),
            "belt body: {:?}",
            at(16, 32)
        );
        // The plate sits in the middle of tile 1: x = 48 px, y = 32 px.
        assert!(
            close(at(48, 32), palette::item("iron")),
            "plate: {:?}",
            at(48, 32)
        );
    }

    #[test]
    fn ore_in_view_is_laid_under_everything() {
        let (look, atlases, mut scene) = (Look::new(&content()), atlases(), Scene::default());
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

    /// The CPU side of 10,000 sprites; prints the time per frame.
    #[test]
    fn ten_thousand_sprites() {
        let look = Look::new(&content());
        let atlases = atlases();
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
        assert_eq!(scene.instances.len(), 10_000);
        println!("10k sprites: {:?} per frame", start.elapsed() / 10);
    }
}
