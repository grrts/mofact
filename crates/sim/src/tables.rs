//! Content interned into dense indices for the simulation (R1).

use mofact_core::content::{BuildingRole, Content, IconShape, ItemKind, Stack};
use mofact_core::ids::{BuildingId, ItemId, RecipeId, ResearchId};
use mofact_core::TICKS_PER_SECOND;
use std::collections::HashMap;

pub type Item = u16;
pub type Recipe = u16;

/// Belt positions are fixed point: one tile is this many units. 60 × 64, so
/// common belt speeds move a whole number of units per tick.
pub const UNIT: u32 = 3840;

#[derive(Clone, Debug)]
pub enum Role {
    Hub,
    /// Moves `per_tick` position units each tick.
    Belt {
        per_tick: u32,
    },
    Miner {
        ticks: u32,
    },
    Crafter {
        recipes: Vec<Recipe>,
        speed: f32,
    },
    Lab {
        speed: f32,
    },
    Pole {
        wire: u8,
        supply: u8,
    },
    Generator {
        watts: u32,
    },
    Splitter,
    Underground {
        per_tick: u32,
        reach: u8,
    },
    /// Pipes and tanks hold `capacity` milli-units of fluid (see [`FLUID`]).
    Pipe {
        capacity: u64,
    },
    Tank {
        capacity: u64,
    },
    UndergroundPipe {
        reach: u8,
        capacity: u64,
    },
    /// Moves `per_tick` milli-units a tick.
    Pump {
        per_tick: u64,
    },
    Well {
        fluid: Item,
        per_tick: u64,
    },
    /// Gives up to `watts` while it burns `per_tick` milli-units of `fluid` a tick.
    Engine {
        watts: u32,
        fluid: Item,
        per_tick: u64,
    },
}

/// What a goal wants, interned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// This many delivered to the hub, all told.
    Deliver(Item, u64),
    /// This many of a building standing.
    Build(u16, u32),
    Research(u16),
    /// Names something the content doesn't have.
    Never,
}

/// Fluid amounts are fixed point: one unit of fluid is this many milli-units.
pub const FLUID: u64 = 1000;
/// What one end of an underground pipe holds, in fluid units.
const UNDERGROUND_PIPE_UNITS: u64 = 100;

/// One research, interned.
#[derive(Clone, Debug)]
pub struct ResearchDef {
    pub id: ResearchId,
    pub requires: Vec<u16>,
    /// Packs per unit.
    pub cost: Vec<(Item, u32)>,
    pub units: u32,
    pub seconds: f32,
}

#[derive(Clone, Debug)]
pub struct Building {
    pub id: BuildingId,
    pub size: (u8, u8),
    pub role: Role,
    pub cost: Vec<(Item, u32)>,
    /// Watts drawn while working; zero needs no power.
    pub power: u32,
}

#[derive(Clone, Debug)]
pub struct RecipeDef {
    pub id: RecipeId,
    pub inputs: Vec<(Item, u32)>,
    pub outputs: Vec<(Item, u32)>,
    pub seconds: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Tables {
    pub items: Vec<ItemId>,
    /// Whether each item is a fluid.
    pub fluid: Vec<bool>,
    /// The ore items, in content order: the world generator's resource numbering.
    pub ores: Vec<Item>,
    pub recipes: Vec<RecipeDef>,
    pub buildings: Vec<Building>,
    pub research: Vec<ResearchDef>,
    /// The research that unlocks each recipe and building, if any.
    pub recipe_lock: Vec<Option<u16>>,
    pub building_lock: Vec<Option<u16>>,
    /// Items any research consumes: what labs take from belts.
    pub packs: Vec<Item>,
    research_index: HashMap<ResearchId, u16>,
    /// The goals in order, and what each gives when met.
    pub goals: Vec<(Goal, Vec<(Item, u32)>)>,
    pub goal_ids: Vec<String>,
    item_index: HashMap<ItemId, Item>,
    recipe_index: HashMap<RecipeId, Recipe>,
    building_index: HashMap<BuildingId, u16>,
}

/// Belt speed in position units per tick.
fn per_tick(tiles_per_second: f32) -> u32 {
    (tiles_per_second * UNIT as f32 / TICKS_PER_SECOND as f32).round() as u32
}

fn watts(kw: f32) -> u32 {
    (kw * 1000.0).round().max(0.0) as u32
}

fn milli(units: f32) -> u64 {
    (units * FLUID as f32).round().max(0.0) as u64
}

pub fn ticks(seconds: f32) -> u32 {
    (seconds * TICKS_PER_SECOND as f32).round().max(1.0) as u32
}

impl Tables {
    /// Intern checked content (see `mofact_content::check`). References to unknown items are dropped.
    pub fn new(content: &Content) -> Self {
        let mut t = Tables::default();
        for (i, item) in content.items.iter().enumerate() {
            t.items.push(item.id.clone());
            t.fluid.push(item.kind == ItemKind::Fluid);
            if item.shape == IconShape::Ore {
                t.ores.push(i as Item);
            }
            t.item_index.insert(item.id.clone(), i as Item);
        }
        let stacks = |t: &Tables, list: &[Stack]| -> Vec<(Item, u32)> {
            list.iter()
                .filter_map(|s| Some((t.item(&s.item)?, s.count)))
                .collect()
        };
        for (i, r) in content.recipes.iter().enumerate() {
            let def = RecipeDef {
                id: r.id.clone(),
                inputs: stacks(&t, &r.inputs),
                outputs: stacks(&t, &r.outputs),
                seconds: r.seconds,
            };
            t.recipes.push(def);
            t.recipe_index.insert(r.id.clone(), i as Recipe);
        }
        for (i, b) in content.buildings.iter().enumerate() {
            let role = match &b.role {
                BuildingRole::Hub => Role::Hub,
                BuildingRole::Belt { tiles_per_second } => Role::Belt {
                    per_tick: per_tick(*tiles_per_second),
                },
                BuildingRole::Splitter => Role::Splitter,
                BuildingRole::Underground {
                    tiles_per_second,
                    reach,
                } => Role::Underground {
                    per_tick: per_tick(*tiles_per_second),
                    reach: *reach,
                },
                BuildingRole::Miner { seconds_per_ore } => Role::Miner {
                    ticks: ticks(*seconds_per_ore),
                },
                BuildingRole::Crafter { categories, speed } => Role::Crafter {
                    recipes: content
                        .recipes
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| categories.contains(&r.category))
                        .map(|(i, _)| i as Recipe)
                        .collect(),
                    speed: *speed,
                },
                BuildingRole::Lab { speed } => Role::Lab { speed: *speed },
                BuildingRole::Pole { wire, supply } => Role::Pole {
                    wire: *wire,
                    supply: *supply,
                },
                BuildingRole::Generator { kw } => Role::Generator { watts: watts(*kw) },
                BuildingRole::Pipe { capacity } => Role::Pipe {
                    capacity: milli(*capacity),
                },
                BuildingRole::Tank { capacity } => Role::Tank {
                    capacity: milli(*capacity),
                },
                BuildingRole::UndergroundPipe { reach } => Role::UndergroundPipe {
                    reach: *reach,
                    capacity: UNDERGROUND_PIPE_UNITS * FLUID,
                },
                BuildingRole::Pump { per_second } => Role::Pump {
                    per_tick: milli(*per_second) / TICKS_PER_SECOND as u64,
                },
                // Checked content names a fluid; anything else makes a well that gives nothing.
                BuildingRole::Well { fluid, per_second } => Role::Well {
                    fluid: t.item(fluid).unwrap_or(0),
                    per_tick: if t.item(fluid).is_some() {
                        milli(*per_second) / TICKS_PER_SECOND as u64
                    } else {
                        0
                    },
                },
                BuildingRole::Engine {
                    kw,
                    fluid,
                    per_second,
                } => Role::Engine {
                    watts: watts(*kw),
                    fluid: t.item(fluid).unwrap_or(0),
                    per_tick: (milli(*per_second) / TICKS_PER_SECOND as u64).max(1),
                },
            };
            t.buildings.push(Building {
                id: b.id.clone(),
                size: b.size,
                role,
                cost: stacks(&t, &b.cost),
                power: watts(b.power),
            });
            t.building_index.insert(b.id.clone(), i as u16);
        }
        for (i, r) in content.research.iter().enumerate() {
            t.research_index.insert(r.id.clone(), i as u16);
        }
        t.recipe_lock = vec![None; t.recipes.len()];
        t.building_lock = vec![None; t.buildings.len()];
        for (i, r) in content.research.iter().enumerate() {
            let def = ResearchDef {
                id: r.id.clone(),
                requires: r.requires.iter().filter_map(|q| t.research(q)).collect(),
                cost: stacks(&t, &r.cost),
                units: r.units.max(1),
                seconds: r.seconds,
            };
            for &(item, _) in &def.cost {
                if !t.packs.contains(&item) {
                    t.packs.push(item);
                }
            }
            t.research.push(def);
            for x in &r.recipes {
                if let Some(k) = t.recipe(x) {
                    t.recipe_lock[k as usize] = Some(i as u16);
                }
            }
            for x in &r.buildings {
                if let Some(k) = t.building(x) {
                    t.building_lock[k as usize] = Some(i as u16);
                }
            }
        }
        for g in &content.goals {
            use mofact_core::content::Want;
            let goal = match &g.want {
                Want::Deliver { item, count } => {
                    t.item(item).map(|i| Goal::Deliver(i, u64::from(*count)))
                }
                Want::Build { building, count } => {
                    t.building(building).map(|b| Goal::Build(b, *count))
                }
                Want::Research { research } => t.research(research).map(Goal::Research),
            };
            // Checked content names only what exists; anything else can't be met.
            t.goals
                .push((goal.unwrap_or(Goal::Never), stacks(&t, &g.reward)));
            t.goal_ids.push(g.id.clone());
        }
        t
    }

    pub fn research(&self, id: &ResearchId) -> Option<u16> {
        self.research_index.get(id).copied()
    }

    pub fn item(&self, id: &ItemId) -> Option<Item> {
        self.item_index.get(id).copied()
    }

    pub fn recipe(&self, id: &RecipeId) -> Option<Recipe> {
        self.recipe_index.get(id).copied()
    }

    pub fn building(&self, id: &BuildingId) -> Option<u16> {
        self.building_index.get(id).copied()
    }

    /// How many of `item` make one unit in a recipe: [`FLUID`] for fluids, else 1.
    pub fn scale(&self, item: Item) -> u32 {
        if self.fluid[item as usize] {
            FLUID as u32
        } else {
            1
        }
    }
}
