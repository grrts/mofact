//! How each building and item looks: the catalogue of sprites the art kit
//! paints into the atlases, and which one draws a given entity.
//!
//! Machines are painted once per variant they can show: each direction they
//! can face (the output arrow and the exhaust move with it, the light does
//! not), each recipe they can be set to, and, for furnaces, lit or cold.
//! Belts are painted once, running east, and turned by the shader.

use crate::atlas::{Atlas, BANDS};
use crate::palette::{self, Rgba};
use mofact_artkit::belt::{self, Bend, Joins};
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
/// A soft white dot, a thin white ring and a white bar, tinted per use for
/// machine animations (see [`crate::Scene::animate`]).
pub const FX_DOT: &str = "fx:dot";
pub const FX_RING: &str = "fx:ring";
pub const FX_BAR: &str = "fx:bar";

/// How a machine shows it is working, beyond its sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Smoke from a chimney: furnaces and boilers; `big` for refineries.
    Smoke {
        big: bool,
    },
    /// White puffs: steam engines.
    Steam,
    /// Chips of ore flying from the bit: drills.
    Dust,
    /// Bubbles rising in the vat: chemical plants.
    Bubbles,
    /// Only the progress bar: assemblers and labs.
    Busy,
}
/// The fluid in a pipe joined on a set of sides, `pipe-fluid:<links>`.
const PIPE_FLUID: &str = "pipe-fluid";

/// What a sprite in the catalogue is a picture of.
#[derive(Clone, Debug, PartialEq)]
enum Art {
    Belt,
    /// The moving layer of a belt tile at one phase of [`belt::FRAMES`].
    BeltSurface {
        bend: Bend,
        frame: u8,
    },
    /// The still layer of a belt tile: rails, caps and side openings.
    BeltFrame {
        bend: Bend,
        joins: Joins,
    },
    Hub,
    Pole,
    Turbine,
    Splitter,
    Underground {
        exit: bool,
    },
    /// A pipe joined on the sides in `links` (bits east 1, south 2, west 4, north 8).
    Pipe {
        links: u8,
    },
    /// The fluid inside a pipe, in white for tinting.
    PipeFluid {
        links: u8,
    },
    Tank,
    TankFluid,
    Pump,
    UndergroundPipe {
        exit: bool,
    },
    Well {
        fluid: Rgba,
    },
    Boiler {
        pose: Pose,
    },
    Engine,
    Refinery {
        pose: Pose,
        light: Rgba,
        tar: Rgba,
    },
    ChemicalPlant {
        product: Option<Rgba>,
        pose: Pose,
    },
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
    /// 0 a dot, 1 a ring, 2 a bar.
    Fx(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Belt,
    Hub,
    Drill,
    Furnace,
    Assembler,
    /// Drawn as an assembler showing a research pack until labs get their own art.
    Lab,
    Pole,
    Generator,
    /// Splitters and underground belts face east and are turned when drawn,
    /// as belts are.
    Splitter,
    Underground,
    /// Pipes are painted once per set of joined sides and never turned.
    Pipe,
    Tank,
    /// Pumps and underground pipes face east and are turned, as belts are.
    Pump,
    UndergroundPipe,
    Well,
    /// Boilers are crafters, painted per direction and lit or cold.
    Boiler,
    Engine,
    /// Refineries and chemical plants are crafters too, lit while working.
    Refinery,
    ChemicalPlant,
}

impl Kind {
    /// Whether it looks different while working: a fire or a stir.
    fn lights(self) -> bool {
        matches!(
            self,
            Kind::Furnace | Kind::Boiler | Kind::Refinery | Kind::ChemicalPlant
        )
    }
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
    /// The first of the sixteen pipe-fluid sprites, one per set of sides.
    pipe_fluid: usize,
    /// The first belt surface sprite: [`belt::FRAMES`] per bend.
    belt_surface: usize,
    /// The first belt frame sprite: sixteen straight, then two per corner.
    belt_frame: usize,
    /// How fast each belt runs, in tiles a second, to move its surface.
    belt_speed: HashMap<BuildingId, f32>,
}

/// What carries items from tile to tile, for joining belts up when drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conveyor {
    Belt,
    /// Takes from behind, gives out left and right.
    Splitter,
    /// An underground belt end: entrances take from behind, exits give ahead.
    Underground,
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
        look.belt_surface = look.sprites.len();
        for (b, bend) in Bend::ALL.into_iter().enumerate() {
            for frame in 0..belt::FRAMES {
                let art = Art::BeltSurface { bend, frame };
                push(&mut look, format!("belt-surface:{b}:{frame}"), (1, 1), art);
            }
        }
        look.belt_frame = look.sprites.len();
        for bits in 0..16u8 {
            let art = Art::BeltFrame {
                bend: Bend::Straight,
                joins: Joins::from_bits(bits),
            };
            push(&mut look, format!("belt-frame:0:{bits}"), (1, 1), art);
        }
        for (b, bend) in [(1, Bend::FromLeft), (2, Bend::FromRight)] {
            for end in [false, true] {
                let joins = Joins {
                    end,
                    ..Joins::default()
                };
                let art = Art::BeltFrame { bend, joins };
                let bits = joins.bits();
                push(&mut look, format!("belt-frame:{b}:{bits}"), (1, 1), art);
            }
        }
        for (k, name) in [FX_DOT, FX_RING, FX_BAR].into_iter().enumerate() {
            push(&mut look, name.into(), (1, 1), Art::Fx(k as u8));
        }
        look.pipe_fluid = look.sprites.len();
        for links in 0..16u8 {
            let art = Art::PipeFluid { links };
            push(&mut look, format!("{PIPE_FLUID}:{links}"), (1, 1), art);
        }
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
                BuildingRole::Belt { tiles_per_second } => {
                    look.belt_speed.insert(b.id.clone(), *tiles_per_second);
                    (Kind::Belt, Vec::new())
                }
                BuildingRole::Hub => (Kind::Hub, Vec::new()),
                BuildingRole::Lab { .. } => (Kind::Lab, Vec::new()),
                BuildingRole::Pole { .. } => (Kind::Pole, Vec::new()),
                BuildingRole::Generator { .. } => (Kind::Generator, Vec::new()),
                BuildingRole::Splitter => (Kind::Splitter, Vec::new()),
                BuildingRole::Underground { .. } => (Kind::Underground, Vec::new()),
                BuildingRole::Pipe { .. } => (Kind::Pipe, Vec::new()),
                BuildingRole::Tank { .. } => (Kind::Tank, Vec::new()),
                BuildingRole::Pump { .. } => (Kind::Pump, Vec::new()),
                BuildingRole::UndergroundPipe { .. } => (Kind::UndergroundPipe, Vec::new()),
                BuildingRole::Well { .. } => (Kind::Well, Vec::new()),
                BuildingRole::Engine { .. } => (Kind::Engine, Vec::new()),
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
                    let has = |k: &str| categories.iter().any(|c| c == k);
                    (
                        if smelts {
                            Kind::Furnace
                        } else if has("boiling") {
                            Kind::Boiler
                        } else if has("refining") {
                            Kind::Refinery
                        } else if has("chemistry") {
                            Kind::ChemicalPlant
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
                Kind::Pole => push(&mut look, id.to_owned(), b.size, Art::Pole),
                Kind::Generator => push(&mut look, id.to_owned(), b.size, Art::Turbine),
                Kind::Splitter => push(&mut look, id.to_owned(), b.size, Art::Splitter),
                Kind::Underground | Kind::UndergroundPipe => {
                    for exit in [false, true] {
                        let end = if exit { "exit" } else { "entrance" };
                        let art = if kind == Kind::Underground {
                            Art::Underground { exit }
                        } else {
                            Art::UndergroundPipe { exit }
                        };
                        push(&mut look, format!("{id}:{end}"), b.size, art);
                    }
                }
                Kind::Pipe => {
                    for links in 0..16u8 {
                        push(
                            &mut look,
                            format!("{id}:{links}"),
                            b.size,
                            Art::Pipe { links },
                        );
                    }
                }
                Kind::Tank => {
                    push(&mut look, id.to_owned(), b.size, Art::Tank);
                    push(&mut look, format!("{id}:fluid"), b.size, Art::TankFluid);
                }
                Kind::Pump => push(&mut look, id.to_owned(), b.size, Art::Pump),
                Kind::Engine => push(&mut look, id.to_owned(), b.size, Art::Engine),
                Kind::Well => {
                    let fluid = match &b.role {
                        BuildingRole::Well { fluid, .. } => item_look(fluid).map(|l| l.1),
                        _ => None,
                    };
                    let fluid = fluid.unwrap_or(palette::item("water"));
                    push(&mut look, id.to_owned(), b.size, Art::Well { fluid });
                }
                _ => {
                    let none = std::iter::once(None);
                    let choices: Vec<Option<&str>> = none
                        .chain(recipes.iter().map(|r| Some(r.as_str())))
                        .collect();
                    for setting in &choices {
                        let def = setting
                            .and_then(|r| content.recipes.iter().find(|d| d.id.as_str() == r));
                        let output = |k: usize| {
                            def.and_then(|d| d.outputs.get(k))
                                .and_then(|s| item_look(&s.item))
                        };
                        let product = output(0);
                        let states: &[bool] = if kind.lights() {
                            &[false, true]
                        } else {
                            &[false]
                        };
                        for &working in states {
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
                                    Kind::Boiler => Art::Boiler { pose },
                                    Kind::Refinery => Art::Refinery {
                                        pose,
                                        light: output(0)
                                            .map_or(palette::item("light-oil"), |p| p.1),
                                        tar: output(1).map_or(palette::item("tar"), |p| p.1),
                                    },
                                    Kind::ChemicalPlant => Art::ChemicalPlant {
                                        product: product.map(|p| p.1),
                                        pose,
                                    },
                                    Kind::Lab => Art::Assembler {
                                        product: Some((Shape::Pack, palette::item("pack-survey"))),
                                        pose: Pose {
                                            working: false,
                                            ..pose
                                        },
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

    /// How `building` carries items on, if it does.
    pub fn conveyor(&self, building: &BuildingId) -> Option<Conveyor> {
        match self.buildings.get(building)?.kind {
            Kind::Belt => Some(Conveyor::Belt),
            Kind::Splitter => Some(Conveyor::Splitter),
            Kind::Underground => Some(Conveyor::Underground),
            _ => None,
        }
    }

    /// How fast belt `building` runs, in tiles a second.
    pub fn belt_speed(&self, building: &BuildingId) -> Option<f32> {
        self.belt_speed.get(building).copied()
    }

    /// The two layers that draw a belt tile, both facing east and turned
    /// with it: its surface at `phase` (0 to 1) of its cycle, then its frame.
    pub fn belt_layers(&self, bend: Bend, joins: Joins, phase: f32) -> (&str, &str) {
        let frames = usize::from(belt::FRAMES);
        let frame = ((phase.rem_euclid(1.0) * frames as f32) as usize).min(frames - 1);
        let b = Bend::ALL.iter().position(|&x| x == bend).unwrap_or(0);
        let surface = self.belt_surface + b * frames + frame;
        let framed = match bend {
            Bend::Straight => usize::from(joins.bits()),
            _ => 16 + (b - 1) * 2 + usize::from(joins.end),
        };
        let name = |i: usize| self.sprites.get(i).map_or(BELT, |s| s.name.as_str());
        (name(surface), name(self.belt_frame + framed))
    }

    /// Whether `building` is drawn facing east and turned by its direction:
    /// belts, splitters and underground belts. Items ride across these.
    pub fn turns(&self, building: &BuildingId) -> bool {
        self.buildings.get(building).is_some_and(|b| {
            matches!(
                b.kind,
                Kind::Belt
                    | Kind::Splitter
                    | Kind::Underground
                    | Kind::Pump
                    | Kind::UndergroundPipe
            )
        })
    }

    /// How `building` shows it is working.
    pub fn effect(&self, building: &BuildingId) -> Effect {
        match self.buildings.get(building).map(|b| b.kind) {
            Some(Kind::Furnace | Kind::Boiler) => Effect::Smoke { big: false },
            Some(Kind::Refinery) => Effect::Smoke { big: true },
            Some(Kind::Engine) => Effect::Steam,
            Some(Kind::Drill) => Effect::Dust,
            Some(Kind::ChemicalPlant) => Effect::Bubbles,
            Some(Kind::Assembler | Kind::Lab) => Effect::Busy,
            _ => Effect::None,
        }
    }

    /// Whether `building` shows a status badge when it stalls.
    pub fn has_badge(&self, building: &BuildingId) -> bool {
        self.buildings.get(building).is_some_and(|b| {
            !matches!(
                b.kind,
                Kind::Belt
                    | Kind::Hub
                    | Kind::Pole
                    | Kind::Generator
                    | Kind::Splitter
                    | Kind::Underground
                    | Kind::Pipe
                    | Kind::Tank
                    | Kind::UndergroundPipe
            )
        })
    }

    /// The sprite that draws `building` joined to pipes on `links` (bits
    /// east 1, south 2, west 4, north 8); for anything but a pipe, the same
    /// as [`Look::sprite`].
    pub fn linked(
        &self,
        building: &BuildingId,
        dir: Dir,
        setting: Option<&str>,
        working: bool,
        links: u8,
    ) -> Option<&str> {
        let b = self.buildings.get(building)?;
        if b.kind != Kind::Pipe {
            return self.sprite(building, dir, setting, working);
        }
        let at = b.base + usize::from(links & 15);
        self.sprites.get(at).map(|s| s.name.as_str())
    }

    /// The white sprite that shows the fluid inside `building`, drawn over
    /// it and turned with it, and whether it shrinks as it empties (a tank's
    /// window) rather than fading (a pipe's channel).
    pub fn fluid(&self, building: &BuildingId, exit: bool, links: u8) -> Option<(&str, bool)> {
        let b = self.buildings.get(building)?;
        let links = match b.kind {
            Kind::Pipe if links & 15 == 0 => 5,
            Kind::Pipe => links & 15,
            // An underground pipe faces east and is turned: its pipe is on
            // the west for an entrance, the east for an exit.
            Kind::UndergroundPipe => {
                if exit {
                    1
                } else {
                    4
                }
            }
            Kind::Tank => {
                let at = b.base + 1;
                return self.sprites.get(at).map(|s| (s.name.as_str(), true));
            }
            _ => return None,
        };
        self.sprites
            .get(self.pipe_fluid + usize::from(links))
            .map(|s| (s.name.as_str(), false))
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
            Kind::Hub | Kind::Pole | Kind::Generator | Kind::Splitter => b.base,
            Kind::Tank | Kind::Pump | Kind::Well | Kind::Engine => b.base,
            // A pipe on its own, as an icon or a ghost, runs east to west.
            Kind::Pipe => b.base + 5,
            Kind::Underground | Kind::UndergroundPipe => {
                b.base + usize::from(setting == Some("exit"))
            }
            _ => {
                let r = setting
                    .and_then(|r| b.settings.iter().position(|x| x == r))
                    .map_or(0, |i| i + 1);
                // Assemblers, drills and labs look the same while working,
                // so they have no lit sprites.
                let (states, lit) = if b.kind.lights() {
                    (2, usize::from(working))
                } else {
                    (1, 0)
                };
                b.base + (r * states + lit) * 4 + dir_index(dir)
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
                let mut atlas = Atlas::new(atlas_size(px));
                let painted = self.paint(&mut atlas, px);
                debug_assert!(painted, "band {px} has room for every sprite");
                atlas
            })
            .collect()
    }
}

/// Each band's atlas is this many tiles on a side, padding included: 3,168
/// pixels at the closest zoom, inside what every target GPU takes. Machines
/// have a sprite per recipe, state and direction, so this grows with content;
/// when tier 3-4 fills it, turn machine bodies on the GPU instead.
pub const ATLAS_TILES: u32 = 24;

/// The side of the atlas for the band of `px` pixels a tile. Each sprite
/// carries its padding, which is most of a tile's room in the smallest band,
/// so it is counted in: every band then fits the same set of sprites.
pub fn atlas_size(px: u32) -> u32 {
    (px + 2 * crate::atlas::PAD) * ATLAS_TILES
}

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
        Art::BeltSurface { bend, frame } => {
            let phase = f32::from(frame) / f32::from(belt::FRAMES);
            belt::surface(&mut c, z, fine, bend, phase);
        }
        Art::BeltFrame { bend, joins } => belt::frame(&mut c, z, fine, bend, joins),
        Art::Hub => draw::hub(&mut c, z, fine, 0.0),
        Art::Pole => draw::pole(&mut c, z, fine),
        Art::Turbine => draw::turbine(&mut c, z, fine, 0.3),
        Art::Splitter => draw::splitter(&mut c, z, fine),
        Art::Underground { exit } => draw::underground(&mut c, z, fine, exit),
        Art::Pipe { links } => draw::pipe(&mut c, z, fine, links),
        Art::PipeFluid { links } => draw::pipe_fluid(&mut c, z, links),
        Art::Tank => draw::tank(&mut c, z, fine),
        Art::TankFluid => draw::tank_fluid(&mut c, z),
        Art::Pump => draw::pump(&mut c, z, fine),
        Art::UndergroundPipe { exit } => draw::underground_pipe(&mut c, z, fine, exit),
        Art::Well { fluid } => draw::well(&mut c, z, fine, fluid),
        Art::Boiler { ref pose } => draw::boiler(&mut c, z, fine, pose),
        Art::Engine => draw::engine(&mut c, z, fine, 0.3),
        Art::Refinery {
            ref pose,
            light,
            tar,
        } => draw::refinery(&mut c, z, fine, pose, light, tar),
        Art::ChemicalPlant { product, ref pose } => {
            draw::chemical_plant(&mut c, z, fine, pose, product)
        }
        Art::Drill { ore, ref pose } => draw::drill(&mut c, z, ore, fine, pose),
        Art::Furnace { ring, ref pose } => draw::furnace(&mut c, z, ring, fine, pose),
        Art::Assembler { product, ref pose } => draw::assembler(&mut c, z, product, fine, pose),
        Art::Item { shape, color } => {
            // On a belt, the prototype draws items 0.32 of a tile across.
            let s = w as f32;
            draw::item(&mut c, shape, color, s / 2.0, s / 2.0, s * 0.64, !fine);
        }
        Art::Badge { starved } => draw::badge(&mut c, w as f32 / 2.0, starved),
        Art::Fx(0) => draw::fx_dot(&mut c, w as f32),
        Art::Fx(1) => draw::fx_ring(&mut c, w as f32),
        Art::Fx(_) => draw::fx_bar(&mut c, w as f32, h as f32),
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
            goals: Vec::new(),
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
            let mut atlas = Atlas::new(atlas_size(px));
            assert!(look.paint(&mut atlas, px), "band {px} is full");
            for s in &look.sprites {
                assert!(atlas.get(&s.name).is_some(), "{} at {px}", s.name);
            }
        }
    }
}
