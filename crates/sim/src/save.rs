//! Save format v5 (T11, R54): a versioned binary file. v2 adds research
//! progress and labs (T28).
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
use crate::tables::Role;
use crate::{Crafter, Lab, Sim, State, Tunnel};
use mofact_core::content::Content;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId, RecipeId, ResearchId};
use mofact_core::Tick;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAGIC: &[u8; 6] = b"MOFACT";
/// The format this build writes.
pub const VERSION: u16 = 5;

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
    Crafter {
        recipe: Option<RecipeId>,
        fixed: bool,
        inputs: Vec<u32>,
        out: u32,
        out_item: Option<ItemId>,
        prog: u32,
        working: bool,
        idle: u32,
        blocked: u32,
    },
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
            })
        }
        // Versions 3 to 5 added poles and generators, then splitters and
        // underground belts, then fluids: new variants only, so a version 2
        // payload reads as it is.
        2..=5 => postcard::from_bytes(payload).map_err(corrupt),
        v => Err(LoadError::TooNew(v)),
    }
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
                    State::Crafter(c) => SavedState::Crafter {
                        recipe: c.recipe.map(|r| t.recipes[r as usize].id.clone()),
                        fixed: c.fixed,
                        inputs: c.inputs.clone(),
                        out: c.out,
                        out_item: c.out_item.map(item),
                        prog: c.prog,
                        working: c.working,
                        idle: c.idle,
                        blocked: c.blocked,
                    },
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
        let mut ore: Vec<_> = self.ore.iter().map(|(p, &i)| (*p, item(i))).collect();
        ore.sort_by_key(|(p, _)| (p.y, p.x)); // the same world always saves the same bytes
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
        for (pos, id) in &world.ore {
            if let Some(i) = item(id, &mut warnings) {
                sim.ore.insert(*pos, i);
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
                (
                    SavedState::Crafter {
                        recipe,
                        fixed,
                        inputs,
                        out,
                        out_item,
                        prog,
                        working,
                        idle,
                        blocked,
                    },
                    Role::Crafter { .. },
                ) => {
                    let recipe = recipe.and_then(|r| {
                        let found = t.recipe(&r);
                        if found.is_none() {
                            warnings.push(format!("recipe `{r}` no longer exists"));
                        }
                        found
                    });
                    let want = recipe.map_or(0, |r| t.recipes[r as usize].inputs.len());
                    let inputs = if inputs.len() == want {
                        inputs
                    } else {
                        vec![0; want]
                    };
                    State::Crafter(Crafter {
                        recipe,
                        fixed: fixed && recipe.is_some(),
                        inputs,
                        out: if recipe.is_some() { out } else { 0 },
                        out_item: opt(&out_item, &mut warnings),
                        prog,
                        working,
                        idle,
                        blocked,
                    })
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
                _ => {
                    warnings.push(format!(
                        "building `{}` changed role; rebuilt empty",
                        s.building
                    ));
                    sim.initial_state(kind, None)
                }
            };
            let entity = Entity {
                kind,
                pos: s.pos,
                size: b.size,
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
