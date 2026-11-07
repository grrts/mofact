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
use mofact_artkit::rail;
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
    /// A belt as an icon, its rails in its tier's colour past the first.
    Belt {
        trim: Option<Rgba>,
    },
    /// The moving layer of a belt tile at one phase of [`belt::FRAMES`].
    BeltSurface {
        bend: Bend,
        frame: u8,
    },
    /// The still layer of a belt tile: rails, caps and side openings. A
    /// corner is lit for the quarter `turns` it is drawn at.
    BeltFrame {
        bend: Bend,
        joins: Joins,
        trim: Option<Rgba>,
        turns: u8,
    },
    Hub,
    Pole,
    Turbine,
    /// The turbine with its blades turned to frame `0` of [`ROTOR_FRAMES`].
    TurbineSpin(u8),
    /// A generator bigger than a tile: a solar panel.
    Solar,
    /// A radar (S3).
    Radar,
    /// A drone port (T49).
    DronePort,
    /// A logistic chest; `requester` tells its band's colour.
    Chest {
        requester: bool,
    },
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
    /// A well for anything but water: a pumpjack.
    Pumpjack {
        fluid: Rgba,
    },
    Lab {
        pack: Rgba,
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
        /// A faster tier's colour: a deep drill.
        trim: Option<Rgba>,
    },
    Furnace {
        ring: Option<Rgba>,
        pose: Pose,
        /// A faster tier's colour: an electric furnace.
        trim: Option<Rgba>,
    },
    /// A furnace's ring in white, to tint with what it smelts.
    Ring {
        electric: bool,
    },
    /// The output arrow, facing east.
    Notch,
    /// A chemical plant's vat in white, and its paddle facing east.
    Vat,
    Paddle,
    /// An assembler's stripe in white, to tint with its product's colour:
    /// an octagon's band for the faster kind.
    Stripe {
        octagon: bool,
    },
    Assembler {
        product: Option<(Shape, Rgba)>,
        pose: Pose,
        /// A faster tier's colour.
        trim: Option<Rgba>,
        /// Bigger than a tile: a precision assembler.
        big: bool,
    },
    Item {
        shape: Shape,
        color: Rgba,
        /// A sign that tells it from items of the same shape (T58), 0 for none.
        mark: u8,
    },
    Badge {
        starved: bool,
        /// Painted in the colour-blind set.
        safe: bool,
    },
    /// Track joined on the sides in `links`, as pipes are.
    Rail {
        links: u8,
    },
    Signal {
        links: u8,
    },
    Stop {
        links: u8,
    },
    StopIcon,
    /// A stop's platform, facing east and turned.
    Platform,
    Locomotive,
    /// A tank train (S5).
    TankTrain,
    /// The megaproject's anchor.
    Skyhook,
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
    /// Track is painted once per set of joined sides, as pipes are; a
    /// stop's platform and trains face east and are turned.
    Rail,
    Signal,
    Stop,
    Train,
    Mega,
    Radar,
    Port,
    Chest,
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
    /// What is drawn over it for its recipe, if it is not baked in.
    layers: Option<Layers>,
}

/// How a machine shows its recipe, drawn over one body per direction: an
/// assembler's stripe tinted the product's colour and the product's icon
/// over its window, or a furnace's ring in the colour of what it smelts.
/// Baking a sprite per recipe would fill the atlas many times over.
#[derive(Clone, Copy, Debug)]
struct Layers {
    /// The white sprite tinted the product's colour, if it has one.
    stripe: Option<usize>,
    /// The output arrow, facing east and turned, to keep it over the stripe.
    notch: Option<usize>,
    /// Where the icon's centre is, in tiles from the top-left, and the item
    /// sprite's side in tiles, if it shows one.
    icon: Option<([f32; 2], f32)>,
}

/// A sprite drawn over a machine: where, how big, and its tint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer<'a> {
    pub sprite: &'a str,
    /// From the machine's top-left corner, in tiles.
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub tint: Rgba,
    /// Turned to the way the machine faces.
    pub turned: bool,
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
    /// What each recipe makes first, for the icon over an assembler.
    products: HashMap<String, ItemId>,
    /// The first of the sixteen pipe-fluid sprites, one per set of sides.
    pipe_fluid: usize,
    /// The first belt surface sprite: [`belt::FRAMES`] per bend.
    belt_surface: usize,
    /// The first belt frame sprite of each tier: sixteen straight, then two
    /// per corner.
    belt_frame: Vec<usize>,
    /// Each belt's tier, 0 for the slowest.
    belt_tier: HashMap<BuildingId, usize>,
    /// How fast each belt runs, in tiles a second, to move its surface.
    belt_speed: HashMap<BuildingId, f32>,
    /// Colour-blind mode (T58): safer colours, and marks on look-alike items.
    colour_blind: bool,
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
        IconShape::Powder => Shape::Powder,
        IconShape::Cell => Shape::Cell,
        IconShape::Wafer => Shape::Wafer,
        IconShape::Chip => Shape::Chip,
        IconShape::Frame => Shape::Frame,
        IconShape::Magnet => Shape::Magnet,
        IconShape::Satellite => Shape::Satellite,
        IconShape::Nozzle => Shape::Nozzle,
    }
}

fn dir_index(d: Dir) -> usize {
    Dir::ALL.iter().position(|&x| x == d).unwrap_or(0)
}

impl Look {
    pub fn new(content: &Content) -> Self {
        Self::with_colour_blind(content, false)
    }

    /// The look for `content`, in colour-blind mode (T58) if `on`.
    pub fn with_colour_blind(content: &Content, on: bool) -> Self {
        let mut look = Look {
            colour_blind: on,
            ..Look::default()
        };
        let item_color = |key: &str| {
            if on {
                palette::item_safe(key)
            } else {
                palette::item(key)
            }
        };
        for r in &content.recipes {
            if let Some(out) = r.outputs.first() {
                look.products
                    .insert(r.id.as_str().to_owned(), out.item.clone());
            }
        }
        let item_look = |id: &ItemId| {
            content
                .items
                .iter()
                .find(|i| &i.id == id)
                .map(|i| (shape(i.shape), item_color(&i.color)))
        };
        let push = |look: &mut Look, name: String, (w, h): (u8, u8), art| {
            look.sprites.push(Sprite {
                name,
                tiles: f32::from(w),
                tiles_high: f32::from(h),
                art,
            });
        };
        push(&mut look, BELT.into(), (1, 1), Art::Belt { trim: None });
        push(
            &mut look,
            STARVED.into(),
            (1, 1),
            Art::Badge {
                starved: true,
                safe: on,
            },
        );
        push(
            &mut look,
            BLOCKED.into(),
            (1, 1),
            Art::Badge {
                starved: false,
                safe: on,
            },
        );
        look.belt_surface = look.sprites.len();
        for (b, bend) in Bend::ALL.into_iter().enumerate() {
            for frame in 0..belt::FRAMES {
                let art = Art::BeltSurface { bend, frame };
                push(&mut look, format!("belt-surface:{b}:{frame}"), (1, 1), art);
            }
        }
        // Belts are told apart by speed: the slowest has steel rails, each
        // faster one its tier's colour.
        let mut speeds: Vec<f32> = content
            .buildings
            .iter()
            .filter_map(|b| match b.role {
                BuildingRole::Belt { tiles_per_second } => Some(tiles_per_second),
                _ => None,
            })
            .collect();
        speeds.sort_by(f32::total_cmp);
        speeds.dedup();
        let tiers = speeds.len().clamp(1, 1 + palette::TIER.len());
        for tier in 0..tiers {
            look.belt_frame.push(look.sprites.len());
            let trim = tier_trim(tier);
            for bits in 0..16u8 {
                let art = Art::BeltFrame {
                    bend: Bend::Straight,
                    joins: Joins::from_bits(bits),
                    trim,
                    turns: 0,
                };
                push(
                    &mut look,
                    format!("belt-frame:{tier}:0:{bits}"),
                    (1, 1),
                    art,
                );
            }
            for (b, bend) in [(1, Bend::FromLeft), (2, Bend::FromRight)] {
                for turns in 0..4u8 {
                    for end in [false, true] {
                        let joins = Joins {
                            end,
                            ..Joins::default()
                        };
                        let art = Art::BeltFrame {
                            bend,
                            joins,
                            trim,
                            turns,
                        };
                        let bits = joins.bits();
                        push(
                            &mut look,
                            format!("belt-frame:{tier}:{b}:{turns}:{bits}"),
                            (1, 1),
                            art,
                        );
                    }
                }
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
        // In colour-blind mode, each item after the first of its shape
        // carries a mark of its own.
        let mut same_shape: Vec<(IconShape, u8)> = Vec::new();
        for i in &content.items {
            let color = item_color(&i.color);
            look.items.insert(i.id.clone(), (look.sprites.len(), color));
            let at = match same_shape.iter().position(|(k, _)| *k == i.shape) {
                Some(at) => at,
                None => {
                    same_shape.push((i.shape, 0));
                    same_shape.len() - 1
                }
            };
            let seen = &mut same_shape[at].1;
            let mark = match *seen {
                0 => 0,
                n => (n - 1) % draw::MARKS + 1,
            };
            *seen = seen.saturating_add(1);
            let art = Art::Item {
                shape: shape(i.shape),
                color,
                mark: if on { mark } else { 0 },
            };
            push(&mut look, format!("item:{}", i.id.as_str()), (1, 1), art);
        }
        for b in &content.buildings {
            let (kind, recipes) = match &b.role {
                BuildingRole::Belt { tiles_per_second } => {
                    look.belt_speed.insert(b.id.clone(), *tiles_per_second);
                    let tier = speeds
                        .iter()
                        .position(|&v| v == *tiles_per_second)
                        .unwrap_or(0)
                        .min(tiers - 1);
                    look.belt_tier.insert(b.id.clone(), tier);
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
                BuildingRole::Rail => (Kind::Rail, Vec::new()),
                BuildingRole::RailSignal => (Kind::Signal, Vec::new()),
                BuildingRole::TrainStop => (Kind::Stop, Vec::new()),
                BuildingRole::Locomotive { .. } => (Kind::Train, Vec::new()),
                BuildingRole::Megaproject { .. } => (Kind::Mega, Vec::new()),
                BuildingRole::Radar { .. } => (Kind::Radar, Vec::new()),
                BuildingRole::DronePort { .. } => (Kind::Port, Vec::new()),
                BuildingRole::ProviderChest { .. } | BuildingRole::RequesterChest { .. } => {
                    (Kind::Chest, Vec::new())
                }
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
            let mut layers = None;
            // A crafter faster than the first kind wears its tier's colour.
            let trim = match b.role {
                BuildingRole::Crafter { speed, .. } if speed > 1.0 => {
                    tier_trim(if speed >= 3.0 { 2 } else { 1 })
                }
                _ => None,
            };
            // So does a drill faster than the first, by how much faster.
            let drill_trim = match b.role {
                BuildingRole::Miner { seconds_per_ore } => {
                    let first = content
                        .buildings
                        .iter()
                        .find_map(|d| match d.role {
                            BuildingRole::Miner { seconds_per_ore } => Some(seconds_per_ore),
                            _ => None,
                        })
                        .unwrap_or(seconds_per_ore);
                    let fast = first / seconds_per_ore.max(0.01);
                    if fast >= 2.9 {
                        tier_trim(2)
                    } else if fast > 1.0 {
                        tier_trim(1)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            match kind {
                Kind::Belt => {
                    let tier = look.belt_tier.get(&b.id).copied().unwrap_or(0);
                    if tier > 0 {
                        let art = Art::Belt {
                            trim: tier_trim(tier),
                        };
                        push(&mut look, format!("{id}:icon"), b.size, art);
                    }
                }
                Kind::Hub => push(&mut look, id.to_owned(), b.size, Art::Hub),
                Kind::Pole => push(&mut look, id.to_owned(), b.size, Art::Pole),
                Kind::Generator => {
                    if b.size.0 > 1 {
                        push(&mut look, id.to_owned(), b.size, Art::Solar);
                    } else {
                        push(&mut look, id.to_owned(), b.size, Art::Turbine);
                        for k in 0..ROTOR_FRAMES {
                            push(
                                &mut look,
                                format!("{id}:spin:{k}"),
                                b.size,
                                Art::TurbineSpin(k),
                            );
                        }
                    }
                }
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
                Kind::Rail | Kind::Signal | Kind::Stop => {
                    for links in 0..16u8 {
                        let art = match kind {
                            Kind::Rail => Art::Rail { links },
                            Kind::Signal => Art::Signal { links },
                            _ => Art::Stop { links },
                        };
                        push(&mut look, format!("{id}:{links}"), b.size, art);
                    }
                    if kind == Kind::Stop {
                        push(&mut look, format!("{id}:icon"), b.size, Art::StopIcon);
                        push(&mut look, format!("{id}:platform"), b.size, Art::Platform);
                    }
                }
                Kind::Train => {
                    let art = match b.role {
                        BuildingRole::Locomotive { tank: true, .. } => Art::TankTrain,
                        _ => Art::Locomotive,
                    };
                    push(&mut look, id.to_owned(), b.size, art)
                }
                Kind::Mega => push(&mut look, id.to_owned(), b.size, Art::Skyhook),
                Kind::Radar => push(&mut look, id.to_owned(), b.size, Art::Radar),
                Kind::Port => push(&mut look, id.to_owned(), b.size, Art::DronePort),
                Kind::Chest => {
                    let requester = matches!(b.role, BuildingRole::RequesterChest { .. });
                    push(&mut look, id.to_owned(), b.size, Art::Chest { requester });
                }
                Kind::Engine => push(&mut look, id.to_owned(), b.size, Art::Engine),
                Kind::Well => {
                    let fluid = match &b.role {
                        BuildingRole::Well { fluid, .. } => item_look(fluid).map(|l| l.1),
                        _ => None,
                    };
                    let water = matches!(&b.role, BuildingRole::Well { fluid, .. } if fluid.as_str() == "water");
                    let fluid = fluid.unwrap_or(palette::item("water"));
                    let art = if water {
                        Art::Well { fluid }
                    } else {
                        Art::Pumpjack { fluid }
                    };
                    push(&mut look, id.to_owned(), b.size, art);
                }
                _ => {
                    let none = std::iter::once(None);
                    // Assemblers and furnaces draw their recipe as layers
                    // over the body.
                    let baked =
                        if matches!(kind, Kind::Assembler | Kind::Furnace | Kind::ChemicalPlant) {
                            &[][..]
                        } else {
                            &recipes[..]
                        };
                    let choices: Vec<Option<&str>> =
                        none.chain(baked.iter().map(|r| Some(r.as_str()))).collect();
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
                                        trim: drill_trim,
                                    },
                                    Kind::Furnace => Art::Furnace {
                                        ring: product.map(|p| p.1),
                                        pose,
                                        trim,
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
                                    Kind::Lab => Art::Lab {
                                        pack: palette::item("pack-survey"),
                                    },
                                    _ => Art::Assembler {
                                        product,
                                        pose: Pose {
                                            working: false,
                                            ..pose
                                        },
                                        trim,
                                        big: b.size.0 > 1,
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
            if kind == Kind::Assembler {
                let big = b.size.0 > 1;
                let stripe = (!big).then(|| {
                    let at = look.sprites.len();
                    let art = Art::Stripe {
                        octagon: trim.is_some(),
                    };
                    push(&mut look, format!("{id}:stripe"), b.size, art);
                    at
                });
                // Where each body draws its product, and how big: the item
                // sprite is drawn 0.64 of its side across.
                let (centre, across) = match (big, trim.is_some()) {
                    (true, _) => ([1.0, 1.12], 0.32),
                    (false, true) => ([0.5, 0.54], 0.26),
                    (false, false) => ([0.5, 0.52], 0.3),
                };
                layers = Some(Layers {
                    stripe,
                    notch: None,
                    icon: Some((centre, across / 0.64)),
                });
            }
            if kind == Kind::ChemicalPlant {
                // The vat in the product's colour, and the paddle over it.
                let vat = look.sprites.len();
                push(&mut look, format!("{id}:vat"), b.size, Art::Vat);
                push(&mut look, format!("{id}:paddle"), b.size, Art::Paddle);
                layers = Some(Layers {
                    stripe: Some(vat),
                    notch: Some(vat + 1),
                    icon: None,
                });
            }
            if kind == Kind::Furnace {
                let ring = look.sprites.len();
                let electric = trim.is_some();
                push(
                    &mut look,
                    format!("{id}:ring"),
                    b.size,
                    Art::Ring { electric },
                );
                // The fire furnace's ring crosses its arrow.
                let notch = (!electric).then(|| {
                    push(&mut look, format!("{id}:notch"), b.size, Art::Notch);
                    ring + 1
                });
                layers = Some(Layers {
                    stripe: Some(ring),
                    notch,
                    icon: None,
                });
            }
            look.buildings.insert(
                b.id.clone(),
                BuildingLook {
                    kind,
                    size: b.size,
                    settings: recipes,
                    base,
                    layers,
                },
            );
        }
        look
    }

    /// The footprint of `building`, in tiles.
    /// The most tiles across or down any building is.
    pub fn largest(&self) -> u8 {
        self.buildings
            .values()
            .map(|b| b.size.0.max(b.size.1))
            .max()
            .unwrap_or(1)
    }

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
    ///
    /// Drawn at `turns` quarter turns clockwise, a straight tile's rails would
    /// show their lit edge on the south or east for half the directions, so
    /// those are mirrored across the belt (`MIRROR` in the turns the shader
    /// takes) and use the frame with its side openings swapped. Corners are
    /// painted lit for each turn instead. Returns the turns to draw with.
    pub fn belt_layers(
        &self,
        building: &BuildingId,
        bend: Bend,
        joins: Joins,
        phase: f32,
        turns: u32,
    ) -> (&str, &str, u32) {
        let tier = self.belt_tier.get(building).copied().unwrap_or(0);
        let frames_at = self.belt_frame.get(tier).copied().unwrap_or(0);
        let frames = usize::from(belt::FRAMES);
        let frame = ((phase.rem_euclid(1.0) * frames as f32) as usize).min(frames - 1);
        let b = Bend::ALL.iter().position(|&x| x == bend).unwrap_or(0);
        let surface = self.belt_surface + b * frames + frame;
        let turns = turns % 4;
        let mirror = bend == Bend::Straight && matches!(turns, 1 | 2);
        let (framed, turns) = match bend {
            Bend::Straight if mirror => {
                let swapped = Joins {
                    left: joins.right,
                    right: joins.left,
                    ..joins
                };
                (usize::from(swapped.bits()), turns | MIRROR)
            }
            Bend::Straight => (usize::from(joins.bits()), turns),
            _ => {
                let lit = ((b - 1) * 4 + turns as usize) * 2;
                (16 + lit + usize::from(joins.end), turns)
            }
        };
        let name = |i: usize| self.sprites.get(i).map_or(BELT, |s| s.name.as_str());
        (name(surface), name(frames_at + framed), turns)
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
                    | Kind::Train
            )
        })
    }

    /// Whether `building` is a train, which rides on track and is drawn
    /// from the snapshot's trains.
    pub fn is_train(&self, building: &BuildingId) -> bool {
        self.buildings
            .get(building)
            .is_some_and(|b| b.kind == Kind::Train)
    }

    /// The platform drawn over a train stop, facing east and turned the
    /// way the stop faces: the side it unloads out of.
    pub fn platform(&self, building: &BuildingId) -> Option<&str> {
        let b = self.buildings.get(building)?;
        if b.kind != Kind::Stop {
            return None;
        }
        self.sprites.get(b.base + 17).map(|s| s.name.as_str())
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
                    | Kind::Rail
                    | Kind::Signal
                    | Kind::Stop
                    | Kind::Train
                    | Kind::Mega
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
        if !matches!(b.kind, Kind::Pipe | Kind::Rail | Kind::Signal | Kind::Stop) {
            return self.sprite(building, dir, setting, working);
        }
        let at = b.base + usize::from(links & 15);
        self.sprites.get(at).map(|s| s.name.as_str())
    }

    /// For a wind turbine on a grid, its sprite with the blades turned for
    /// `t` seconds; `None` for anything that doesn't spin.
    pub fn spinning(&self, building: &BuildingId, t: f64) -> Option<&str> {
        let b = self.buildings.get(building)?;
        if b.kind != Kind::Generator || b.size.0 > 1 {
            return None;
        }
        let frame = ((t / ROTOR_SECONDS).fract() * f64::from(ROTOR_FRAMES)) as usize;
        let at = b.base + 1 + frame.min(usize::from(ROTOR_FRAMES) - 1);
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
            Kind::Belt if self.belt_tier.get(building).is_some_and(|&t| t > 0) => b.base,
            Kind::Belt => return Some(BELT),
            Kind::Hub | Kind::Pole | Kind::Generator | Kind::Splitter => b.base,
            Kind::Tank | Kind::Pump | Kind::Well | Kind::Engine => b.base,
            // A pipe or rail on its own, as an icon or a ghost, runs east to west.
            Kind::Pipe | Kind::Rail | Kind::Signal => b.base + 5,
            Kind::Stop => b.base + 16,
            Kind::Train | Kind::Mega | Kind::Radar | Kind::Port | Kind::Chest => b.base,
            Kind::Underground | Kind::UndergroundPipe => {
                b.base + usize::from(setting == Some("exit"))
            }
            Kind::Assembler => b.base + dir_index(dir),
            Kind::Furnace | Kind::ChemicalPlant => {
                b.base + usize::from(working) * 4 + dir_index(dir)
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

    /// What is drawn over `building` to show its `recipe`, in order.
    pub fn layers(&self, building: &BuildingId, recipe: Option<&str>) -> Vec<Layer<'_>> {
        let (Some(b), Some(item)) = (
            self.buildings.get(building),
            recipe.and_then(|r| self.products.get(r)),
        ) else {
            return Vec::new();
        };
        let (Some(l), Some(&(icon, color))) = (b.layers, self.items.get(item)) else {
            return Vec::new();
        };
        let size = [f32::from(b.size.0), f32::from(b.size.1)];
        let whole = |sprite: usize, tint, turned| Layer {
            sprite: &self.sprites[sprite].name,
            offset: [0.0, 0.0],
            size,
            tint,
            turned,
        };
        let mut out = Vec::new();
        out.extend(l.stripe.map(|s| whole(s, color, false)));
        out.extend(l.notch.map(|s| whole(s, palette::WHITE, true)));
        if let Some((centre, side)) = l.icon {
            out.push(Layer {
                sprite: &self.sprites[icon].name,
                offset: [centre[0] - side / 2.0, centre[1] - side / 2.0],
                size: [side, side],
                tint: palette::WHITE,
                turned: false,
            });
        }
        out
    }

    /// The sprite and tint that draw `item`, for icons and belts.
    pub fn item(&self, item: &ItemId) -> Option<(&str, Rgba)> {
        self.items
            .get(item)
            .map(|&(i, _)| (self.sprites[i].name.as_str(), palette::WHITE))
    }

    /// Whether this is the colour-blind look (T58).
    pub fn colour_blind(&self) -> bool {
        self.colour_blind
    }

    /// The colour that says `status`, in this look's set.
    pub fn status(&self, status: palette::Status) -> Rgba {
        palette::status(status, self.colour_blind)
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

    /// `building` drawn as its icon at `px` pixels a tile, as RGBA rows:
    /// its width, height and pixels. For the silhouette sheet and tests.
    pub fn picture(&self, building: &BuildingId, px: u32) -> Option<(u32, u32, Vec<u8>)> {
        let (name, _) = self.building(building)?;
        let s = self.sprites.iter().find(|s| s.name == name)?;
        let (w, h, canvas) = render(&s.art, s.tiles, s.tiles_high, px, px >= FINE_PX);
        Some((w, h, canvas.rgba()))
    }

    /// Paint every sprite at `px` pixels a tile into `atlas`. Returns false if
    /// the atlas ran out of room.
    pub fn paint(&self, atlas: &mut Atlas, px: u32) -> bool {
        let mut ok = crate::atlas::sprites::paint_ore(atlas, px);
        ok &= crate::atlas::sprites::paint_decor(atlas, px);
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

/// Each band's atlas is this many tiles on a side, padding included: 3,564
/// pixels at the closest zoom, inside the 4,096 every target GPU takes. Machines
/// have a sprite per recipe, state and direction, so this grows with content;
/// when tier 3-4 fills it, turn machine bodies on the GPU instead.
pub const ATLAS_TILES: u32 = 27;

/// Frames of a wind turbine's blades over a third of a turn, and the
/// seconds that third takes.
const ROTOR_FRAMES: u8 = 12;
const ROTOR_SECONDS: f64 = 1.4;

/// The side of the atlas for the band of `px` pixels a tile. Each sprite
/// carries its padding, which is most of a tile's room in the smallest band,
/// so it is counted in: every band then fits the same set of sprites.
pub fn atlas_size(px: u32) -> u32 {
    (px + 2 * crate::atlas::PAD) * ATLAS_TILES
}

/// Set in an instance's turns to mirror its sprite across the way it runs
/// (top to bottom as painted) before it is turned.
pub const MIRROR: u32 = 4;

/// In an instance's turns: a light, which shines as bright by night (S9).
pub const LIT: u32 = 8;

/// Items and badges are drawn at this fraction of a tile.
pub const ITEM_TILES: f32 = 0.5;

/// The trim colour of tier `tier`, counted from 0 for the first kind.
fn tier_trim(tier: usize) -> Option<Rgba> {
    tier.checked_sub(1)
        .map(|k| palette::TIER[k.min(palette::TIER.len() - 1)])
}

fn render(art: &Art, tiles: f32, tiles_high: f32, px: u32, fine: bool) -> (u32, u32, Canvas) {
    let z = px as f32;
    let small = ((z * ITEM_TILES).round() as u32).max(4);
    let (w, h) = match art {
        Art::Item { .. } | Art::Badge { .. } => (small, small),
        _ => ((z * tiles) as u32, (z * tiles_high) as u32),
    };
    let mut c = Canvas::new(w, h);
    match *art {
        Art::Belt { trim } => draw::belt_icon(&mut c, z, fine, trim),
        Art::BeltSurface { bend, frame } => {
            let phase = f32::from(frame) / f32::from(belt::FRAMES);
            belt::surface(&mut c, z, fine, bend, phase);
        }
        Art::BeltFrame {
            bend,
            joins,
            trim,
            turns,
        } => belt::trimmed(&mut c, z, fine, bend, joins, trim, turns),
        Art::Hub => draw::hub(&mut c, z, fine, 0.0),
        Art::Pole => draw::pole(&mut c, z, fine),
        Art::Turbine => draw::turbine(&mut c, z, fine, 0.3),
        // Three blades repeat every third of a turn; `turbine` turns them
        // by twice its `t` in radians.
        Art::TurbineSpin(k) => {
            let turn = f32::from(k) / f32::from(ROTOR_FRAMES) * std::f32::consts::TAU / 3.0;
            draw::turbine(&mut c, z, fine, 0.3 + turn / 2.0)
        }
        Art::Solar => draw::solar(&mut c, z, fine),
        Art::Radar => draw::radar(&mut c, z, fine),
        Art::DronePort => draw::drone_port(&mut c, z, fine),
        Art::Chest { requester } => {
            let trim = if requester {
                mofact_artkit::palette::GLASS
            } else {
                mofact_artkit::palette::ACCENT
            };
            draw::logistic_chest(&mut c, z, fine, trim, requester)
        }
        Art::Stripe { octagon } => draw::stripe(&mut c, z, fine, octagon),
        Art::Ring { electric } => draw::furnace_ring(&mut c, z, fine, electric),
        Art::Notch => draw::notch_east(&mut c, z),
        Art::Vat => draw::chemical_vat(&mut c, z),
        Art::Paddle => draw::chemical_paddle(&mut c, z),
        Art::Splitter => draw::splitter(&mut c, z, fine),
        Art::Underground { exit } => draw::underground(&mut c, z, fine, exit),
        Art::Pipe { links } => draw::pipe(&mut c, z, fine, links),
        Art::PipeFluid { links } => draw::pipe_fluid(&mut c, z, links),
        Art::Tank => draw::tank(&mut c, z, fine),
        Art::TankFluid => draw::tank_fluid(&mut c, z),
        Art::Pump => draw::pump(&mut c, z, fine),
        Art::UndergroundPipe { exit } => draw::underground_pipe(&mut c, z, fine, exit),
        Art::Well { fluid } => draw::water_well(&mut c, z, fine, fluid),
        Art::Pumpjack { fluid } => draw::pumpjack(&mut c, z, fine, fluid, 0.4),
        Art::Lab { pack } => draw::lab(&mut c, z, fine, pack),
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
        Art::Drill {
            ore,
            ref pose,
            trim: None,
        } => draw::drill(&mut c, z, ore, fine, pose),
        Art::Drill {
            ore,
            ref pose,
            trim: Some(trim),
        } => draw::deep_drill(&mut c, z, ore, fine, pose, trim),
        Art::Furnace {
            ring,
            ref pose,
            trim: None,
        } => draw::furnace(&mut c, z, ring, fine, pose),
        Art::Furnace {
            ring,
            ref pose,
            trim: Some(trim),
        } => draw::electric_furnace(&mut c, z, ring, fine, pose, trim),
        Art::Assembler {
            product,
            ref pose,
            trim: None,
            ..
        } => draw::assembler(&mut c, z, product, fine, pose),
        Art::Assembler {
            product,
            ref pose,
            trim: Some(trim),
            big: false,
        } => draw::fast_assembler(&mut c, z, product, fine, pose, trim),
        Art::Assembler {
            product,
            ref pose,
            trim: Some(trim),
            big: true,
        } => draw::precision_assembler(&mut c, z, product, fine, pose, trim),
        Art::Item { shape, color, mark } => {
            // On a belt, the prototype draws items 0.32 of a tile across.
            let s = w as f32;
            draw::item(&mut c, shape, color, s / 2.0, s / 2.0, s * 0.64, !fine);
            draw::mark(&mut c, mark, s / 2.0, s / 2.0, s * 0.8);
        }
        Art::Badge { starved, safe } => draw::badge(&mut c, w as f32 / 2.0, starved, safe),
        Art::Rail { links } => rail::rail(&mut c, z, fine, links),
        Art::Signal { links } => rail::signal(&mut c, z, fine, links),
        Art::Stop { links } => rail::stop(&mut c, z, fine, links),
        Art::StopIcon => rail::stop_icon(&mut c, z, fine),
        Art::Platform => rail::platform(&mut c, z, fine),
        Art::Locomotive => rail::locomotive(&mut c, z, fine),
        Art::TankTrain => rail::tank_train(&mut c, z, fine),
        Art::Skyhook => draw::skyhook(&mut c, z, fine, tiles),
        Art::Fx(0) => draw::fx_dot(&mut c, w as f32),
        Art::Fx(1) => draw::fx_ring(&mut c, w as f32),
        Art::Fx(_) => draw::fx_bar(&mut c, w as f32, h as f32),
    }
    (w, h, c)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn in_colour_blind_mode_no_two_items_share_both_shape_and_mark() {
        let content = content();
        let signs = |look: &Look| -> Vec<(String, (Shape, u8), Rgba)> {
            content
                .items
                .iter()
                .map(|i| {
                    let (at, color) = look.items[&i.id];
                    match look.sprites[at].art {
                        Art::Item { shape, mark, .. } => {
                            (i.id.as_str().to_owned(), (shape, mark), color)
                        }
                        _ => panic!("{} is not drawn as an item", i.id.as_str()),
                    }
                })
                .collect()
        };
        let safe = signs(&Look::with_colour_blind(&content, true));
        for (n, (a, sa, _)) in safe.iter().enumerate() {
            for (b, sb, _) in &safe[n + 1..] {
                assert_ne!(sa, sb, "{a} and {b} look alike without colour");
            }
        }
        // The usual look has no marks, and takes the usual colours.
        let plain = signs(&Look::new(&content));
        assert!(plain.iter().all(|(_, (_, m), _)| *m == 0));
        let ore = |v: &[(String, (Shape, u8), Rgba)]| {
            v.iter().find(|(id, ..)| id == "copper-ore").unwrap().2
        };
        assert_ne!(ore(&safe), ore(&plain));
        assert!(Look::with_colour_blind(&content, true).colour_blind());
    }

    #[test]
    fn in_colour_blind_mode_every_ore_on_the_ground_stays_apart() {
        // Ore lies on the ground as nothing but its colour, so every ore,
        // including any added later, needs a colour-blind colour of its own.
        let content = content();
        let look = Look::with_colour_blind(&content, true);
        let ores: Vec<_> = content
            .items
            .iter()
            .filter(|i| i.shape == IconShape::Ore)
            .map(|i| (i.id.as_str(), look.item_color(&i.id).unwrap()))
            .collect();
        for cvd in [None].into_iter().chain(palette::CVDS.map(Some)) {
            for (n, (a, ca)) in ores.iter().enumerate() {
                for (b, cb) in &ores[n + 1..] {
                    let d = palette::difference(*ca, *cb, cvd);
                    assert!(d >= 20.0, "{a} and {b} are {d:.1} apart under {cvd:?}");
                }
            }
        }
    }

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
        assert_eq!(cold, "furnace:none:cold:0");
        assert_eq!(lit, "furnace:none:lit:0");
        assert_eq!(south, "furnace:none:lit:1");
        let ring = look.layers(&furnace, Some(iron));
        assert_eq!(ring.len(), 2, "a ring and the arrow over it");
        assert_eq!(ring[0].sprite, "furnace:ring");
        assert_eq!(ring[0].tint, palette::item("iron"));
        assert!(ring[1].turned);
        let gear = "gear";
        let asm = BuildingId::from("assembler");
        assert_eq!(
            look.sprite(&asm, Dir::North, Some(gear), true),
            Some("assembler:none:cold:3"),
            "assemblers look the same working, and draw their recipe on top"
        );
        let layers = look.layers(&asm, Some(gear));
        assert_eq!(layers.len(), 2, "a stripe and an icon");
        assert_eq!(layers[0].sprite, "assembler:stripe");
        assert_eq!(layers[0].tint, palette::item("iron"));
        assert_eq!(layers[1].sprite, "item:gear");
        assert!(look.layers(&asm, None).is_empty());
        let vat = look.layers(&"chemical-plant".into(), Some("plastic"));
        assert_eq!(vat[0].sprite, "chemical-plant:vat");
        assert_eq!(vat[0].tint, palette::item("plastic"));
        assert!(vat[1].turned, "the paddle turns with the plant");
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

    /// `rgba`, a `z`-pixel square sprite, as the shader lays it on screen at
    /// `turns` (with [`MIRROR`]): the pixel each screen pixel shows.
    fn on_screen(rgba: &[u8], z: u32, turns: u32) -> Vec<[u8; 4]> {
        let mut out = vec![[0; 4]; (z * z) as usize];
        for y in 0..z {
            for x in 0..z {
                let mut t = ((x as f32 + 0.5) / z as f32, (y as f32 + 0.5) / z as f32);
                for _ in 0..turns % 4 {
                    t = (t.1, 1.0 - t.0);
                }
                if turns & MIRROR != 0 {
                    t.1 = 1.0 - t.1;
                }
                let (px, py) = ((t.0 * z as f32) as u32, (t.1 * z as f32) as u32);
                let i = ((py * z + px) * 4) as usize;
                out[(y * z + x) as usize] = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
            }
        }
        out
    }

    /// Belt rails are lit from the top left of the screen whichever way the
    /// belt runs: where a rail crosses a tile's edge, the lit side of it is
    /// its north (or west) side, on straight runs and corners alike.
    #[test]
    fn belt_rails_are_lit_from_the_same_side_whichever_way_they_run() {
        let look = Look::new(&content());
        let belt: BuildingId = "belt".into();
        let z = 64u32;
        let bright = |p: [u8; 4]| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]);
        for bend in Bend::ALL {
            for (turns, dir) in Dir::ALL.into_iter().enumerate() {
                let (_, frame, drawn) =
                    look.belt_layers(&belt, bend, Joins::default(), 0.0, turns as u32);
                let s = look.sprites.iter().find(|s| s.name == frame).unwrap();
                let (_, _, c) = render(&s.art, 1.0, 1.0, z, true);
                let px = on_screen(&c.rgba(), z, drawn);
                // Each edge, one pixel in, read across the rails.
                let lines: [Box<dyn Fn(u32) -> u32>; 4] = [
                    Box::new(|i| i * z + 1),
                    Box::new(|i| i * z + z - 2),
                    Box::new(|i| z + i),
                    Box::new(|i| (z - 2) * z + i),
                ];
                let mut crossings = 0;
                for line in &lines {
                    let at = |i: u32| px[line(i) as usize];
                    let mut i = 0;
                    while i < z {
                        // A rail: a run of opaque pixels brighter than the belt.
                        let rail = |p: [u8; 4]| p[3] > 200 && bright(p) > 180;
                        if rail(at(i)) {
                            let start = i;
                            while i < z && rail(at(i)) {
                                i += 1;
                            }
                            let (first, last) = (at(start), at(i - 1));
                            if i - start >= 4 {
                                crossings += 1;
                                assert!(
                                    bright(first) > bright(last),
                                    "{bend:?} running {dir:?}: a rail is lit on its far side"
                                );
                            }
                        }
                        i += 1;
                    }
                }
                assert!(
                    crossings >= 4,
                    "{bend:?} running {dir:?}: {crossings} rails found"
                );
            }
        }
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
