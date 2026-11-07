//! Save format v10 (T11, R54): a versioned binary file. v2 adds research
//! progress and labs (T28); v9 cleared trees, v10 rails and trains (T46).
//!
//! A save is the magic bytes `MOFACT`, a little-endian `u16` format version,
//! then the world encoded with postcard. Content is stored by id, never by
//! interned index, so a save survives items, recipes and buildings being
//! added or reordered; anything the current content no longer has is dropped
//! with a warning instead of failing the load.
//!
//! Each format change bumps [`VERSION`] and adds a step to [`migrate`], so
//! every save ever written keeps loading.

use crate::entities::Entity;
use crate::rails::{Phase, Train};
use crate::tables::{Role, Tables};
use crate::{Crafter, Lab, Sim, State, Tunnel};
use mofact_core::command::TrainStop;
use mofact_core::content::Content;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId, RecipeId, ResearchId};
use mofact_core::Tick;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAGIC: &[u8; 6] = b"MOFACT";
/// The format this build writes.
pub const VERSION: u16 = 14;

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    NotASave,
    /// Written by a newer build than this one.
    TooNew(u16),
    Corrupt(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotASave => write!(f, "not a Mofact save"),
            LoadError::TooNew(v) => {
                write!(f, "save format {v} is newer than this build ({VERSION})")
            }
            LoadError::Corrupt(e) => write!(f, "save is damaged: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Serialize, Deserialize)]
struct World {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    /// Entities by slot, so ids stay the same across a save and load.
    entities: Vec<(u32, Saved)>,
    /// Units done per research, and whether it is finished.
    research: Vec<(ResearchId, u32, bool)>,
    current: Option<ResearchId>,
    /// The goal the player is on, by id; none once all are met.
    goal: Option<String>,
    /// Tiles whose tree or boulder was cleared.
    cleared: Vec<TilePos>,
    /// How much ore each held tile has left, and the tiles mined out.
    ore_left: Vec<u32>,
    depleted: Vec<TilePos>,
    /// The player's pins on the world map.
    pins: Vec<TilePos>,
    /// The map's seed and preset (S4); none in saves from before them,
    /// which were all seed 42, standard.
    world: Option<(u64, u8)>,
}

/// Version 12: the same, before the map's seed was saved.
#[derive(Deserialize)]
struct WorldV12 {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    entities: Vec<(u32, Saved)>,
    research: Vec<(ResearchId, u32, bool)>,
    current: Option<ResearchId>,
    goal: Option<String>,
    cleared: Vec<TilePos>,
    ore_left: Vec<u32>,
    depleted: Vec<TilePos>,
    pins: Vec<TilePos>,
}

/// Version 11: the same, before pins.
#[derive(Deserialize)]
struct WorldV11 {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    entities: Vec<(u32, Saved)>,
    research: Vec<(ResearchId, u32, bool)>,
    current: Option<ResearchId>,
    goal: Option<String>,
    cleared: Vec<TilePos>,
    ore_left: Vec<u32>,
    depleted: Vec<TilePos>,
}

/// Versions 9 and 10: the same, before ore that runs out.
#[derive(Deserialize)]
struct WorldV10 {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    entities: Vec<(u32, Saved)>,
    research: Vec<(ResearchId, u32, bool)>,
    current: Option<ResearchId>,
    goal: Option<String>,
    cleared: Vec<TilePos>,
}

/// Versions 7 and 8: the same, before cleared trees.
#[derive(Deserialize)]
struct WorldV8 {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    entities: Vec<(u32, Saved)>,
    research: Vec<(ResearchId, u32, bool)>,
    current: Option<ResearchId>,
    goal: Option<String>,
}

/// Versions 2 to 6: the same, before goals.
#[derive(Deserialize)]
struct WorldV6 {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    entities: Vec<(u32, Saved)>,
    research: Vec<(ResearchId, u32, bool)>,
    current: Option<ResearchId>,
}

/// Version 1: the same, before research.
#[derive(Deserialize)]
struct WorldV1 {
    tick: Tick,
    stock: Vec<(ItemId, u32)>,
    delivered: Vec<(ItemId, u64)>,
    ore: Vec<(TilePos, ItemId)>,
    entities: Vec<(u32, Saved)>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    building: BuildingId,
    pos: TilePos,
    dir: Dir,
    state: SavedState,
}

#[derive(Serialize, Deserialize)]
enum SavedState {
    Hub,
    Belt {
        items: Vec<(ItemId, u32)>,
        last: Option<ItemId>,
    },
    Miner {
        prog: u32,
        out: Option<ItemId>,
        waiting: u32,
    },
    Crafter(SavedCrafter),
    // New variants go at the end, so older saves still decode.
    Lab {
        packs: Vec<(ItemId, u32)>,
        unit: Option<(ResearchId, u32)>,
        idle: u32,
    },
    Pole,
    Generator,
    Splitter {
        held: Option<ItemId>,
        right: bool,
    },
    Underground {
        exit: bool,
        partner: Option<u32>,
        items: Vec<(ItemId, u32)>,
    },
    /// A pipe or tank, and its share of its segment's fluid in milli-units.
    Pipe {
        content: Option<(ItemId, u64)>,
    },
    PipeTunnel {
        exit: bool,
        partner: Option<u32>,
        content: Option<(ItemId, u64)>,
    },
    Pump,
    Well,
    /// An engine and the share of its output it gave on its last tick.
    Engine {
        share: u32,
    },
    /// A crafter with more than one output; the first is in the crafter,
    /// the rest follow in recipe order.
    CrafterOuts(SavedCrafter, Vec<(ItemId, u32)>),
    Rail,
    Stop,
    Train(SavedTrain),
    /// The megaproject's stage and what that stage has taken, by item.
    Mega {
        stage: u32,
        have: Vec<(ItemId, u32)>,
    },
}

/// A train. Its route is found again after a load.
#[derive(Serialize, Deserialize)]
struct SavedTrain {
    cargo: Vec<(ItemId, u32)>,
    schedule: Vec<TrainStop>,
    next: u32,
    heading: Dir,
    to: Option<TilePos>,
    along: u32,
    phase: u8,
    wait: u32,
}

/// A crafter, with its first output. Encodes the same as the struct variant
/// that versions 1 to 7 wrote.
#[derive(Serialize, Deserialize)]
struct SavedCrafter {
    recipe: Option<RecipeId>,
    fixed: bool,
    inputs: Vec<u32>,
    out: u32,
    out_item: Option<ItemId>,
    prog: u32,
    working: bool,
    idle: u32,
    blocked: u32,
}

/// Bring an older payload up to [`VERSION`].
fn migrate(version: u16, payload: &[u8]) -> Result<World, LoadError> {
    let corrupt = |e: postcard::Error| LoadError::Corrupt(e.to_string());
    match version {
        1 => {
            let v1: WorldV1 = postcard::from_bytes(payload).map_err(corrupt)?;
            Ok(World {
                tick: v1.tick,
                stock: v1.stock,
                delivered: v1.delivered,
                ore: v1.ore,
                entities: v1.entities,
                research: Vec::new(),
                current: None,
                goal: Some(String::new()),
                cleared: Vec::new(),
                ore_left: Vec::new(),
                depleted: Vec::new(),
                pins: Vec::new(),
                world: None,
            })
        }
        // Versions 3 to 6 added poles and generators, then splitters and
        // underground belts, then fluids, then engines: new variants only,
        // so a version 2 payload reads as it is. Version 7 adds goals; an
        // older game starts at the first, and moves past what it has done.
        2..=6 => {
            let v6: WorldV6 = postcard::from_bytes(payload).map_err(corrupt)?;
            Ok(World {
                tick: v6.tick,
                stock: v6.stock,
                delivered: v6.delivered,
                ore: v6.ore,
                entities: v6.entities,
                research: v6.research,
                current: v6.current,
                goal: Some(String::new()),
                cleared: Vec::new(),
                ore_left: Vec::new(),
                depleted: Vec::new(),
                pins: Vec::new(),
                world: None,
            })
        }
        // Version 8 adds crafters with several outputs: a new variant only.
        // Version 9 adds cleared trees.
        7 | 8 => {
            let v8: WorldV8 = postcard::from_bytes(payload).map_err(corrupt)?;
            Ok(World {
                tick: v8.tick,
                stock: v8.stock,
                delivered: v8.delivered,
                ore: v8.ore,
                entities: v8.entities,
                research: v8.research,
                current: v8.current,
                goal: v8.goal,
                cleared: Vec::new(),
                ore_left: Vec::new(),
                depleted: Vec::new(),
                pins: Vec::new(),
                world: None,
            })
        }
        // Version 10 adds rails and trains: new variants only.
        // Version 11 adds ore that runs out.
        9 | 10 => {
            let v10: WorldV10 = postcard::from_bytes(payload).map_err(corrupt)?;
            Ok(World {
                tick: v10.tick,
                stock: v10.stock,
                delivered: v10.delivered,
                ore: v10.ore,
                entities: v10.entities,
                research: v10.research,
                current: v10.current,
                goal: v10.goal,
                cleared: v10.cleared,
                ore_left: Vec::new(),
                depleted: Vec::new(),
                pins: Vec::new(),
                world: None,
            })
        }
        // Version 12 adds pins on the world map.
        11 => {
            let v11: WorldV11 = postcard::from_bytes(payload).map_err(corrupt)?;
            Ok(World {
                tick: v11.tick,
                stock: v11.stock,
                delivered: v11.delivered,
                ore: v11.ore,
                entities: v11.entities,
                research: v11.research,
                current: v11.current,
                goal: v11.goal,
                cleared: v11.cleared,
                ore_left: v11.ore_left,
                depleted: v11.depleted,
                pins: Vec::new(),
                world: None,
            })
        }
        // Version 13 saves the map's seed and preset.
        12 => {
            let v: WorldV12 = postcard::from_bytes(payload).map_err(corrupt)?;
            Ok(World {
                tick: v.tick,
                stock: v.stock,
                delivered: v.delivered,
                ore: v.ore,
                entities: v.entities,
                research: v.research,
                current: v.current,
                goal: v.goal,
                cleared: v.cleared,
                ore_left: v.ore_left,
                depleted: v.depleted,
                pins: v.pins,
                world: None,
            })
        }
        // Version 14 adds the megaproject: a new variant only.
        13 | 14 => postcard::from_bytes(payload).map_err(corrupt),
        v => Err(LoadError::TooNew(v)),
    }
}

/// A saved crafter, with its outputs after the first. Anything the content
/// no longer has is dropped with a warning.
fn load_crafter(
    t: &Tables,
    c: SavedCrafter,
    rest: Vec<(ItemId, u32)>,
    warnings: &mut Vec<String>,
) -> State {
    let recipe = c.recipe.and_then(|r| {
        let found = t.recipe(&r);
        if found.is_none() {
            warnings.push(format!("recipe `{r}` no longer exists"));
        }
        found
    });
    let want = recipe.map_or(0, |r| t.recipes[r as usize].inputs.len());
    let inputs = if c.inputs.len() == want {
        c.inputs
    } else {
        vec![0; want]
    };
    let mut item = |id: &ItemId| {
        let i = t.item(id);
        if i.is_none() {
            warnings.push(format!("item `{id}` no longer exists"));
        }
        i
    };
    let first = c.out_item.as_ref().and_then(&mut item);
    let mut outs = Vec::new();
    if recipe.is_some() {
        if let Some(i) = first {
            outs.push((i, c.out));
        }
        for (id, n) in &rest {
            // A lost item keeps its slot, empty, so later outputs stay in order.
            outs.push(item(id).map_or((0, 0), |i| (i, *n)));
        }
    }
    State::Crafter(Crafter {
        recipe,
        fixed: c.fixed && recipe.is_some(),
        inputs,
        outs,
        prog: c.prog,
        working: c.working,
        idle: c.idle,
        blocked: c.blocked,
    })
}

impl Sim {
    /// The whole world as a save file.
    pub fn save(&self) -> Vec<u8> {
        let t = &self.tables;
        let item = |i: u16| t.items[i as usize].clone();
        let entities = self
            .entities
            .iter()
            .filter_map(|(id, e)| {
                let state = match self.states.get(id.0 as usize)?.as_ref()? {
                    State::Hub => SavedState::Hub,
                    State::Belt { items, last, .. } => SavedState::Belt {
                        items: items.iter().map(|&(i, p)| (item(i), p)).collect(),
                        last: last.map(item),
                    },
                    State::Miner { prog, out, waiting } => SavedState::Miner {
                        prog: *prog,
                        out: out.map(item),
                        waiting: *waiting,
                    },
                    State::Crafter(c) => {
                        let first = c.outs.first().copied();
                        let saved = SavedCrafter {
                            recipe: c.recipe.map(|r| t.recipes[r as usize].id.clone()),
                            fixed: c.fixed,
                            inputs: c.inputs.clone(),
                            out: first.map_or(0, |o| o.1),
                            out_item: first.map(|o| item(o.0)),
                            prog: c.prog,
                            working: c.working,
                            idle: c.idle,
                            blocked: c.blocked,
                        };
                        match c.outs.get(1..) {
                            Some(rest) if !rest.is_empty() => SavedState::CrafterOuts(
                                saved,
                                rest.iter().map(|&(i, n)| (item(i), n)).collect(),
                            ),
                            _ => SavedState::Crafter(saved),
                        }
                    }
                    State::Lab(lab) => SavedState::Lab {
                        packs: lab.packs.iter().map(|&(i, n)| (item(i), n)).collect(),
                        unit: lab
                            .unit
                            .map(|(r, p)| (t.research[r as usize].id.clone(), p)),
                        idle: lab.idle,
                    },
                    State::Pole => SavedState::Pole,
                    State::Generator => SavedState::Generator,
                    State::Splitter { held, right } => SavedState::Splitter {
                        held: held.map(item),
                        right: *right,
                    },
                    State::Tunnel(tu) => SavedState::Underground {
                        exit: tu.exit,
                        partner: tu.partner.map(|p| p.0),
                        items: tu.items.iter().map(|&(i, p)| (item(i), p)).collect(),
                    },
                    State::Pipe => SavedState::Pipe {
                        content: self.fluids.share(id.0 as usize).map(|(f, n)| (item(f), n)),
                    },
                    State::PipeTunnel(tu) => SavedState::PipeTunnel {
                        exit: tu.exit,
                        partner: tu.partner.map(|p| p.0),
                        content: self.fluids.share(id.0 as usize).map(|(f, n)| (item(f), n)),
                    },
                    State::Pump => SavedState::Pump,
                    State::Well => SavedState::Well,
                    State::Engine { share } => SavedState::Engine { share: *share },
                    State::Rail => SavedState::Rail,
                    State::Stop { .. } => SavedState::Stop,
                    State::Mega { stage, have } => {
                        let needs = match &t.buildings[e.kind as usize].role {
                            Role::Mega { stages, .. } => stages.get(*stage as usize).cloned(),
                            _ => None,
                        };
                        SavedState::Mega {
                            stage: *stage,
                            have: needs
                                .unwrap_or_default()
                                .iter()
                                .zip(have)
                                .map(|(&(i, _), &n)| (item(i), n))
                                .collect(),
                        }
                    }
                    State::Train(tr) => SavedState::Train(SavedTrain {
                        cargo: tr.cargo.iter().map(|&(i, n)| (item(i), n)).collect(),
                        schedule: tr.schedule.clone(),
                        next: tr.next as u32,
                        heading: tr.heading,
                        to: tr.to,
                        along: tr.along,
                        phase: tr.phase.code(),
                        wait: tr.wait,
                    }),
                };
                let saved = Saved {
                    building: t.buildings[e.kind as usize].id.clone(),
                    pos: e.pos,
                    dir: e.dir,
                    state,
                };
                Some((id.0, saved))
            })
            .collect();
        let mut held: Vec<_> = self.ore.iter().collect();
        held.sort_by_key(|(p, _)| (p.y, p.x)); // the same world always saves the same bytes
        let ore = held.iter().map(|(p, &(i, _))| (**p, item(i))).collect();
        let ore_left = held.iter().map(|(_, &(_, n))| n).collect();
        let world = World {
            tick: self.tick,
            stock: self
                .stock
                .iter()
                .enumerate()
                .map(|(i, &n)| (item(i as u16), n))
                .collect(),
            delivered: self
                .delivered
                .iter()
                .enumerate()
                .map(|(i, &n)| (item(i as u16), n))
                .collect(),
            ore,
            entities,
            research: t
                .research
                .iter()
                .enumerate()
                .map(|(r, def)| {
                    (
                        def.id.clone(),
                        self.research.units[r],
                        self.research.done[r],
                    )
                })
                .collect(),
            current: self
                .research
                .current
                .map(|r| t.research[r as usize].id.clone()),
            goal: self.goal_id(),
            cleared: self.cleared.to_vec(),
            ore_left,
            depleted: self.depleted.to_vec(),
            pins: self.pins.to_vec(),
            world: self.world.map(|w| (w.seed, w.preset as u8)),
        };
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend(postcard::to_allocvec(&world).expect("a world always encodes"));
        out
    }

    /// Load a save written by any build up to this one. Returns the world and
    /// a warning for each thing the current content no longer has.
    pub fn load(content: &Content, bytes: &[u8]) -> Result<(Sim, Vec<String>), LoadError> {
        let rest = bytes.strip_prefix(MAGIC).ok_or(LoadError::NotASave)?;
        let (version, payload) = rest.split_first_chunk::<2>().ok_or(LoadError::NotASave)?;
        let world = migrate(u16::from_le_bytes(*version), payload)?;

        let mut sim = Sim::with_content(content);
        let mut warnings = Vec::new();
        let t = sim.tables.clone();
        let item = |id: &ItemId, warnings: &mut Vec<String>| {
            let i = t.item(id);
            if i.is_none() {
                warnings.push(format!("item `{id}` no longer exists"));
            }
            i
        };
        sim.tick = world.tick;
        for (id, n) in &world.stock {
            if let Some(i) = item(id, &mut warnings) {
                sim.stock[i as usize] = *n;
            }
        }
        for (id, n) in &world.delivered {
            if let Some(i) = item(id, &mut warnings) {
                sim.delivered[i as usize] = *n;
            }
        }
        for (k, (pos, id)) in world.ore.iter().enumerate() {
            if let Some(i) = item(id, &mut warnings) {
                // Older saves kept no amounts: give them a placed tile's worth.
                let left = world.ore_left.get(k).copied().unwrap_or(crate::PLACED_ORE);
                sim.ore.insert(*pos, (i, left.max(1)));
            }
        }
        let research = |id: &ResearchId, w: &mut Vec<String>| {
            let r = t.research(id);
            if r.is_none() {
                w.push(format!("research `{id}` no longer exists"));
            }
            r
        };
        for (id, units, done) in &world.research {
            if let Some(r) = research(id, &mut warnings) {
                sim.research.units[r as usize] = *units;
                sim.research.done[r as usize] = *done;
            }
        }
        sim.research.current = world
            .current
            .as_ref()
            .and_then(|id| research(id, &mut warnings))
            .filter(|&r| !sim.research.done[r as usize]);
        // An empty id, from a save older than goals, starts at the first;
        // a goal the content no longer has starts over too, and the sim
        // moves past what's already done.
        sim.cleared = std::sync::Arc::new(world.cleared.clone());
        sim.depleted = std::sync::Arc::new(world.depleted.clone());
        let mut pins = world.pins.clone();
        pins.sort();
        pins.dedup();
        pins.truncate(crate::MAX_PINS);
        sim.pins = std::sync::Arc::new(pins);
        if let Some((seed, preset)) = world.world {
            sim.set_world_gen(seed, mofact_world::Preset::from_u8(preset));
        }
        sim.goal = match &world.goal {
            None => content.goals.len() as u16,
            Some(id) => content
                .goals
                .iter()
                .position(|g| &g.id == id)
                .unwrap_or_else(|| {
                    if !id.is_empty() {
                        warnings.push(format!("goal `{id}` is gone; goals start over"));
                    }
                    0
                }) as u16,
        };
        // What each pipe brought, to fill the segments once they are built.
        let mut fluid = std::collections::HashMap::new();
        for (slot, s) in world.entities {
            let Some(kind) = t.building(&s.building) else {
                warnings.push(format!("building `{}` no longer exists", s.building));
                continue;
            };
            let b = &t.buildings[kind as usize];
            let opt =
                |id: &Option<ItemId>, w: &mut Vec<String>| id.as_ref().and_then(|i| item(i, w));
            let state = match (s.state, &b.role) {
                (SavedState::Hub, Role::Hub) => State::Hub,
                (SavedState::Belt { items, last }, Role::Belt { per_tick }) => State::Belt {
                    items: items
                        .iter()
                        .filter_map(|(i, p)| Some((item(i, &mut warnings)?, *p)))
                        .collect(),
                    last: opt(&last, &mut warnings),
                    per_tick: *per_tick,
                },
                (SavedState::Miner { prog, out, waiting }, Role::Miner { .. }) => State::Miner {
                    prog,
                    out: opt(&out, &mut warnings),
                    waiting,
                },
                (SavedState::Crafter(c), Role::Crafter { .. }) => {
                    load_crafter(&t, c, Vec::new(), &mut warnings)
                }
                (SavedState::CrafterOuts(c, rest), Role::Crafter { .. }) => {
                    load_crafter(&t, c, rest, &mut warnings)
                }
                (SavedState::Lab { packs, unit, idle }, Role::Lab { .. }) => State::Lab(Lab {
                    packs: packs
                        .iter()
                        .filter_map(|(i, n)| Some((item(i, &mut warnings)?, *n)))
                        .collect(),
                    unit: unit.and_then(|(r, p)| Some((research(&r, &mut warnings)?, p))),
                    idle,
                }),
                (SavedState::Pole, Role::Pole { .. }) => State::Pole,
                (SavedState::Generator, Role::Generator { .. }) => State::Generator,
                (SavedState::Splitter { held, right }, Role::Splitter) => State::Splitter {
                    held: opt(&held, &mut warnings),
                    right,
                },
                (
                    SavedState::Underground {
                        exit,
                        partner,
                        items,
                    },
                    Role::Underground { .. },
                ) => State::Tunnel(Tunnel {
                    exit,
                    partner: partner.map(mofact_core::ids::EntityId),
                    items: items
                        .iter()
                        .filter_map(|(i, p)| Some((item(i, &mut warnings)?, *p)))
                        .collect(),
                }),
                (SavedState::Pipe { content }, Role::Pipe { .. } | Role::Tank { .. }) => {
                    if let Some((f, n)) = content {
                        if let Some(f) = item(&f, &mut warnings) {
                            fluid.insert(slot as usize, (f, n));
                        }
                    }
                    State::Pipe
                }
                (
                    SavedState::PipeTunnel {
                        exit,
                        partner,
                        content,
                    },
                    Role::UndergroundPipe { .. },
                ) => {
                    if let Some((f, n)) = content {
                        if let Some(f) = item(&f, &mut warnings) {
                            fluid.insert(slot as usize, (f, n));
                        }
                    }
                    State::PipeTunnel(Tunnel {
                        exit,
                        partner: partner.map(mofact_core::ids::EntityId),
                        items: Vec::new(),
                    })
                }
                (SavedState::Pump, Role::Pump { .. }) => State::Pump,
                (SavedState::Well, Role::Well { .. }) => State::Well,
                (SavedState::Engine { share }, Role::Engine { .. }) => State::Engine {
                    share: share.min(crate::power::FULL),
                },
                (SavedState::Rail, Role::Rail { .. }) => State::Rail,
                (SavedState::Stop, Role::Stop) => State::Stop { docked: None },
                (SavedState::Mega { stage, have }, Role::Mega { stages, .. }) => {
                    let stage = stage.min(stages.len() as u32);
                    let needs = stages.get(stage as usize).cloned().unwrap_or_default();
                    let mut held = vec![0; needs.len()];
                    for (i, n) in &have {
                        let Some(i) = item(i, &mut warnings) else {
                            continue;
                        };
                        if let Some(k) = needs.iter().position(|&(x, _)| x == i) {
                            held[k] = (*n).min(needs[k].1);
                        }
                    }
                    State::Mega { stage, have: held }
                }
                (SavedState::Train(tr), Role::Train { .. }) => State::Train(Train {
                    cargo: tr
                        .cargo
                        .iter()
                        .filter_map(|(i, n)| Some((item(i, &mut warnings)?, *n)))
                        .collect(),
                    next: tr.next as usize % tr.schedule.len().max(1),
                    schedule: tr.schedule,
                    at: s.pos,
                    heading: tr.heading,
                    to: tr.to,
                    along: if tr.to.is_some() { tr.along } else { 0 },
                    path: Vec::new(),
                    phase: Phase::from_code(tr.phase),
                    wait: tr.wait,
                }),
                _ => {
                    warnings.push(format!(
                        "building `{}` changed role; rebuilt empty",
                        s.building
                    ));
                    let mut state = sim.initial_state(kind, None);
                    if let State::Train(tr) = &mut state {
                        tr.at = s.pos;
                        tr.heading = s.dir;
                    }
                    state
                }
            };
            let train = matches!(state, State::Train(_));
            let entity = Entity {
                kind,
                pos: s.pos,
                // Trains take no tiles of their own.
                size: if train { (0, 0) } else { b.size },
                dir: s.dir,
            };
            if !sim
                .entities
                .insert_at(mofact_core::ids::EntityId(slot), entity)
            {
                warnings.push(format!(
                    "{} at {},{} overlaps another building",
                    s.building, s.pos.x, s.pos.y
                ));
                continue;
            }
            sim.states
                .resize(sim.states.len().max(slot as usize + 1), None);
            sim.states[slot as usize] = Some(state);
            if train {
                sim.rails.trains.push(mofact_core::ids::EntityId(slot));
            }
        }
        // An underground end whose partner didn't load stands alone.
        for slot in 0..sim.states.len() {
            let id = mofact_core::ids::EntityId(slot as u32);
            let partner = sim.tunnel(id).and_then(|t| t.partner);
            if partner.is_some_and(|p| sim.tunnel(p).and_then(|t| t.partner) != Some(id)) {
                if let Some(t) = sim.tunnel_mut(id) {
                    t.partner = None;
                }
            }
        }
        sim.rebuild_grids();
        sim.rebuild_blocks();
        sim.rails.trains.sort();
        sim.redock();
        sim.fluids = match sim.build_fluids(|slot| fluid.get(&slot).copied()) {
            Ok(f) => f,
            Err(_) => {
                warnings.push("pipes held two fluids in one line; emptied them".into());
                sim.build_fluids(|_| None).expect("empty pipes can't mix")
            }
        };
        Ok((sim, warnings))
    }
}
