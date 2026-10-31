//! Content interned into dense indices for the simulation (R1).

use mofact_core::content::{BuildingRole, Content, Stack};
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
}

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
    pub recipes: Vec<RecipeDef>,
    pub buildings: Vec<Building>,
    pub research: Vec<ResearchDef>,
    /// The research that unlocks each recipe and building, if any.
    pub recipe_lock: Vec<Option<u16>>,
    pub building_lock: Vec<Option<u16>>,
    /// Items any research consumes: what labs take from belts.
    pub packs: Vec<Item>,
    research_index: HashMap<ResearchId, u16>,
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

pub fn ticks(seconds: f32) -> u32 {
    (seconds * TICKS_PER_SECOND as f32).round().max(1.0) as u32
}

impl Tables {
    /// Intern checked content (see `mofact_content::check`). References to unknown items are dropped.
    pub fn new(content: &Content) -> Self {
        let mut t = Tables::default();
        for (i, item) in content.items.iter().enumerate() {
            t.items.push(item.id.clone());
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
}
