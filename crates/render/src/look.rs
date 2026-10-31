//! How each building and item looks: the catalogue of sprites the art kit
//! paints into the atlases, and which one draws a given entity.
//!
//! Machines are painted once per variant they can show: each direction they
//! can face (the output arrow and the exhaust move with it, the light does
//! not), each recipe they can be set to, and, for furnaces, lit or cold.
//! Belts are painted once, running east, and turned by the shader.

use crate::atlas::{Atlas, BANDS};
use crate::palette::{self, Rgba};
use mofact_artkit::draw::{self, Pose, Shape};
use mofact_artkit::Canvas;
use mofact_core::content::{BuildingRole, Content, IconShape};
use mofact_core::geom::Dir;
use mofact_core::ids::{BuildingId, ItemId};
use std::collections::HashMap;

/// At this many pixels a tile and up, sprites carry the prototype's close-zoom detail.
const FINE_PX: u32 = 26;

pub const BELT: &str = "belt";
/// Ore lying on a tile, painted white and tinted per ore.
pub const ORE: &str = "ore-tile";
/// The badges over a machine that is waiting for input or can't get rid of its output.
pub const STARVED: &str = "badge:starved";
pub const BLOCKED: &str = "badge:blocked";

/// What a sprite in the catalogue is a picture of.
#[derive(Clone, Debug, PartialEq)]
enum Art {
    Belt,
    Hub,
    Drill {
        ore: Rgba,
        pose: Pose,
    },
    Furnace {
        ring: Option<Rgba>,
        pose: Pose,
    },
    Assembler {
        product: Option<(Shape, Rgba)>,
        pose: Pose,
    },
    Item {
        shape: Shape,
        color: Rgba,
    },
    Badge {
        starved: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Belt,
    Hub,
    Drill,
    Furnace,
    Assembler,
}

#[derive(Clone, Debug)]
struct BuildingLook {
    kind: Kind,
    size: (u8, u8),
    /// What tells its variants apart, in order after "none": the recipes a
    /// crafter can be set to, or the ores a drill can stand on.
    settings: Vec<String>,
    /// Its first sprite in the catalogue.
    base: usize,
}

/// One sprite: its name, its size in tiles, and what it shows.
#[derive(Clone, Debug)]
struct Sprite {
    name: String,
    tiles: f32,
    tiles_high: f32,
    art: Art,
}

/// How each building and item looks, worked out once from the content.
#[derive(Default)]
pub struct Look {
    sprites: Vec<Sprite>,
    buildings: HashMap<BuildingId, BuildingLook>,
    items: HashMap<ItemId, (usize, Rgba)>,
}

fn shape(s: IconShape) -> Shape {
    match s {
        IconShape::Ore => Shape::Ore,
        IconShape::Plate => Shape::Plate,
        IconShape::Gear => Shape::Gear,
        IconShape::Wire => Shape::Wire,
        IconShape::Circuit => Shape::Circuit,
        IconShape::Droplet => Shape::Droplet,
        IconShape::Pack => Shape::Pack,
    }
}

fn dir_index(d: Dir) -> usize {
    Dir::ALL.iter().position(|&x| x == d).unwrap_or(0)
}

impl Look {
    pub fn new(content: &Content) -> Self {
        let mut look = Look::default();
        let item_look = |id: &ItemId| {
            content
                .items
                .iter()
                .find(|i| &i.id == id)
                .map(|i| (shape(i.shape), palette::item(&i.color)))
        };
        let push = |look: &mut Look, name: String, (w, h): (u8, u8), art| {
            look.sprites.push(Sprite {
                name,
                tiles: f32::from(w),
                tiles_high: f32::from(h),
                art,
            });
        };
        push(&mut look, BELT.into(), (1, 1), Art::Belt);
        push(
            &mut look,
            STARVED.into(),
            (1, 1),
            Art::Badge { starved: true },
        );
        push(
            &mut look,
            BLOCKED.into(),
            (1, 1),
            Art::Badge { starved: false },
        );
        for i in &content.items {
            look.items
                .insert(i.id.clone(), (look.sprites.len(), palette::item(&i.color)));
            let art = Art::Item {
                shape: shape(i.shape),
                color: palette::item(&i.color),
            };
            push(&mut look, format!("item:{}", i.id.as_str()), (1, 1), art);
        }
        for b in &content.buildings {
            let (kind, recipes) = match &b.role {
                BuildingRole::Belt { .. } => (Kind::Belt, Vec::new()),
                BuildingRole::Hub => (Kind::Hub, Vec::new()),
                BuildingRole::Miner { .. } => (
                    Kind::Drill,
                    content
                        .items
                        .iter()
                        .filter(|i| i.shape == IconShape::Ore)
                        .map(|i| i.id.as_str().to_owned())
                        .collect(),
                ),
                BuildingRole::Crafter { categories, .. } => {
                    let recipes = content
                        .recipes
                        .iter()
                        .filter(|r| categories.contains(&r.category))
                        .map(|r| r.id.as_str().to_owned())
                        .collect();
                    let smelts = categories.iter().any(|c| c == "smelting");
                    (
                        if smelts {
                            Kind::Furnace
                        } else {
                            Kind::Assembler
                        },
                        recipes,
                    )
                }
            };
            let base = look.sprites.len();
            let id = b.id.as_str();
            match kind {
                Kind::Belt => {}
                Kind::Hub => push(&mut look, id.to_owned(), b.size, Art::Hub),
                _ => {
                    let none = std::iter::once(None);
                    let choices: Vec<Option<&str>> = none
                        .chain(recipes.iter().map(|r| Some(r.as_str())))
                        .collect();
                    for setting in &choices {
                        let product = setting
                            .and_then(|r| content.recipes.iter().find(|d| d.id.as_str() == r))
                            .and_then(|d| d.outputs.first())
                            .and_then(|s| item_look(&s.item));
                        for working in [false, true] {
                            for (d, _) in Dir::ALL.iter().enumerate() {
                                let pose = Pose {
                                    dir: d as u8,
                                    working,
                                    // A steady flame, as the prototype shows at rest.
                                    t: 1.0 / 11.0,
                                    progress: 0.0,
                                };
                                let art = match kind {
                                    Kind::Drill => Art::Drill {
                                        ore: setting
                                            .and_then(|o| {
                                                content.items.iter().find(|i| i.id.as_str() == o)
                                            })
                                            .map_or(palette::item("iron-ore"), |i| {
                                                palette::item(&i.color)
                                            }),
                                        pose: Pose {
                                            working: false,
                                            ..pose
                                        },
                                    },
                                    Kind::Furnace => Art::Furnace {
                                        ring: product.map(|p| p.1),
                                        pose,
                                    },
                                    _ => Art::Assembler {
                                        product,
                                        pose: Pose {
                                            working: false,
                                            ..pose
                                        },
                                    },
                                };
                                let r = setting.unwrap_or("none");
                                let lit = if working { "lit" } else { "cold" };
                                push(&mut look, format!("{id}:{r}:{lit}:{d}"), b.size, art);
                            }
                        }
                    }
                }
            }
            look.buildings.insert(
                b.id.clone(),
                BuildingLook {
                    kind,
                    size: b.size,
                    settings: recipes,
                    base,
                },
            );
        }
        look
    }

    /// The footprint of `building`, in tiles.
    pub fn size(&self, building: &BuildingId) -> Option<(u8, u8)> {
        self.buildings.get(building).map(|b| b.size)
    }

    pub fn is_belt(&self, building: &BuildingId) -> bool {
        self.buildings
            .get(building)
            .is_some_and(|b| b.kind == Kind::Belt)
    }

    /// Whether `building` shows a status badge when it stalls.
    pub fn has_badge(&self, building: &BuildingId) -> bool {
        self.buildings
            .get(building)
            .is_some_and(|b| !matches!(b.kind, Kind::Belt | Kind::Hub))
    }

    /// The sprite that draws `building` facing `dir`, set to `setting` (a
    /// crafter's recipe or the ore under a drill), and
    /// working or not. Belts face east; turn them by `dir` when drawing.
    pub fn sprite(
        &self,
        building: &BuildingId,
        dir: Dir,
        setting: Option<&str>,
        working: bool,
    ) -> Option<&str> {
        let b = self.buildings.get(building)?;
        let at = match b.kind {
            Kind::Belt => return Some(BELT),
            Kind::Hub => b.base,
            _ => {
                let r = setting
                    .and_then(|r| b.settings.iter().position(|x| x == r))
                    .map_or(0, |i| i + 1);
                // Only furnaces look different while working.
                let lit = usize::from(working && b.kind == Kind::Furnace);
                b.base + (r * 2 + lit) * 4 + dir_index(dir)
            }
        };
        self.sprites.get(at).map(|s| s.name.as_str())
    }

    /// The sprite and tint that draw `item`, for icons and belts.
    pub fn item(&self, item: &ItemId) -> Option<(&str, Rgba)> {
        self.items
            .get(item)
            .map(|&(i, _)| (self.sprites[i].name.as_str(), palette::WHITE))
    }

    /// An item's colour, for flat marks such as ore on the ground.
    pub fn item_color(&self, item: &ItemId) -> Option<Rgba> {
        self.items.get(item).map(|&(_, c)| c)
    }

    /// The sprite and tint that draw `building` as an icon: facing east, idle.
    pub fn building(&self, building: &BuildingId) -> Option<(&str, Rgba)> {
        self.sprite(building, Dir::East, None, false)
            .map(|s| (s, palette::WHITE))
    }

    /// Paint every sprite at `px` pixels a tile into `atlas`. Returns false if
    /// the atlas ran out of room.
    pub fn paint(&self, atlas: &mut Atlas, px: u32) -> bool {
        let mut ok = crate::atlas::sprites::paint_ore(atlas, px);
        let fine = px >= FINE_PX;
        for s in &self.sprites {
            let (w, h, canvas) = render(&s.art, s.tiles, s.tiles_high, px, fine);
            let rgba = canvas.rgba();
            ok &= atlas
                .add(&s.name, w, h, |x, y| {
                    let i = ((y * w + x) * 4) as usize;
                    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
                })
                .is_some();
        }
        ok
    }

    /// One atlas per zoom band, with every sprite painted.
    pub fn atlases(&self) -> Vec<Atlas> {
        BANDS
            .iter()
            .map(|&px| {
                let mut atlas = Atlas::new(px * ATLAS_TILES);
                let painted = self.paint(&mut atlas, px);
                debug_assert!(painted, "band {px} has room for every sprite");
                atlas
            })
            .collect()
    }
}

/// Each band's atlas is this many tiles on a side: 1,536 pixels at the
/// closest zoom, well inside what every target GPU takes. Machines have a
/// sprite per recipe, state and direction, so this grows with content; when
/// tier 3-4 fills it, turn machine bodies on the GPU instead.
pub const ATLAS_TILES: u32 = 24;

/// Items and badges are drawn at this fraction of a tile.
pub const ITEM_TILES: f32 = 0.5;

fn render(art: &Art, tiles: f32, tiles_high: f32, px: u32, fine: bool) -> (u32, u32, Canvas) {
    let z = px as f32;
    let small = ((z * ITEM_TILES).round() as u32).max(4);
    let (w, h) = match art {
        Art::Item { .. } | Art::Badge { .. } => (small, small),
        _ => ((z * tiles) as u32, (z * tiles_high) as u32),
    };
    let mut c = Canvas::new(w, h);
    match *art {
        Art::Belt => draw::belt(&mut c, z, fine, 0.0, 2.0),
        Art::Hub => draw::hub(&mut c, z, fine, 0.0),
        Art::Drill { ore, ref pose } => draw::drill(&mut c, z, ore, fine, pose),
        Art::Furnace { ring, ref pose } => draw::furnace(&mut c, z, ring, fine, pose),
        Art::Assembler { product, ref pose } => draw::assembler(&mut c, z, product, fine, pose),
        Art::Item { shape, color } => {
            // On a belt, the prototype draws items 0.32 of a tile across.
            let s = w as f32;
            draw::item(&mut c, shape, color, s / 2.0, s / 2.0, s * 0.64, !fine);
        }
        Art::Badge { starved } => draw::badge(&mut c, w as f32 / 2.0, starved),
    }
    (w, h, c)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn content() -> Content {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        Content {
            items: ron::from_str(&read("items.ron")).unwrap(),
            recipes: ron::from_str(&read("recipes.ron")).unwrap(),
            buildings: ron::from_str(&read("buildings.ron")).unwrap(),
            research: Vec::new(),
        }
    }

    #[test]
    fn every_variant_has_its_own_sprite() {
        let look = Look::new(&content());
        let furnace = BuildingId::from("furnace");
        let iron = "iron-plate";
        let cold = look.sprite(&furnace, Dir::East, Some(iron), false).unwrap();
        let lit = look.sprite(&furnace, Dir::East, Some(iron), true).unwrap();
        let south = look.sprite(&furnace, Dir::South, Some(iron), true).unwrap();
        assert_eq!(cold, "furnace:iron-plate:cold:0");
        assert_eq!(lit, "furnace:iron-plate:lit:0");
        assert_eq!(south, "furnace:iron-plate:lit:1");
        let gear = "gear";
        let asm = BuildingId::from("assembler");
        assert_eq!(
            look.sprite(&asm, Dir::North, Some(gear), true),
            Some("assembler:gear:cold:3"),
            "assemblers look the same working"
        );
        assert_eq!(
            look.sprite(&asm, Dir::West, None, false),
            Some("assembler:none:cold:2")
        );
        assert_eq!(
            look.sprite(&"hub".into(), Dir::West, None, false),
            Some("hub")
        );
        assert_eq!(
            look.sprite(&"belt".into(), Dir::West, None, false),
            Some(BELT)
        );
        // A recipe it can't make draws as unset.
        assert_eq!(
            look.sprite(&asm, Dir::East, Some(iron), false),
            Some("assembler:none:cold:0")
        );
        // Drills show the ore they stand on.
        let drill = BuildingId::from("drill");
        assert_eq!(
            look.sprite(&drill, Dir::South, Some("copper-ore"), true),
            Some("drill:copper-ore:cold:1")
        );
        assert_eq!(
            look.sprite(&drill, Dir::East, None, false),
            Some("drill:none:cold:0")
        );
    }

    #[test]
    fn every_band_fits_every_sprite() {
        let look = Look::new(&content());
        for px in BANDS {
            let mut atlas = Atlas::new(px * ATLAS_TILES);
            assert!(look.paint(&mut atlas, px), "band {px} is full");
            for s in &look.sprites {
                assert!(atlas.get(&s.name).is_some(), "{} at {px}", s.name);
            }
        }
    }
}
