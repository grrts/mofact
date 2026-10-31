//! Schema of the content files in `data/` (R1: depth is data, not code).
//!
//! These structs are what the RON files deserialize into. Loading, hot reload
//! and validation live in other lanes; only the shape is fixed here.

use crate::ids::{BuildingId, ItemId, RecipeId, ResearchId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Solid,
    Fluid,
}

/// How the art kit draws an item's icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IconShape {
    Ore,
    Plate,
    Gear,
    Wire,
    Circuit,
    Droplet,
    /// A research pack, consumed by labs.
    Pack,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: ItemId,
    pub name: String,
    pub kind: ItemKind,
    pub shape: IconShape,
    /// Palette key from the art kit's palette file, such as `"iron"`.
    pub color: String,
    /// The research tier that first uses this item.
    pub tier: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    pub item: ItemId,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecipeDef {
    pub id: RecipeId,
    pub name: String,
    pub inputs: Vec<Stack>,
    /// Several outputs make a by-product recipe; a full output blocks the machine (R5).
    pub outputs: Vec<Stack>,
    /// Craft time in seconds at crafting speed 1.0.
    pub seconds: f32,
    /// Which building categories can craft it, such as `"smelting"`.
    pub category: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum BuildingRole {
    Belt {
        tiles_per_second: f32,
    },
    Miner {
        seconds_per_ore: f32,
    },
    Crafter {
        categories: Vec<String>,
        speed: f32,
    },
    /// The player's hub: everything delivered to it joins the stock.
    Hub,
    /// Takes research packs from belts and works on the current research.
    Lab {
        speed: f32,
    },
    /// Carries power: links to poles up to `wire` tiles away and powers
    /// buildings that overlap the square `supply` tiles around it.
    Pole {
        wire: u8,
        supply: u8,
    },
    /// Puts `kw` into the grid of the pole covering it.
    Generator {
        kw: f32,
    },
    /// Takes items from behind and hands them out to its left and right in
    /// turn, so a line splits evenly.
    Splitter,
    /// One end of an underground belt. Placed facing the same way within
    /// `reach` tiles after an entrance, it becomes that entrance's exit;
    /// items pass between them under whatever is built above.
    Underground {
        tiles_per_second: f32,
        reach: u8,
    },
    /// A pipe tile holding `capacity` units of fluid. Pipes join every
    /// pipe, tank and fluid port beside them into one segment, which holds
    /// one fluid; the longer a segment, the slower fluid moves through it.
    Pipe {
        capacity: f32,
    },
    /// A big store of fluid, joined into segments like a pipe on every side.
    Tank {
        capacity: f32,
    },
    /// One end of an underground pipe: pairs like an underground belt,
    /// within `reach` tiles, and joins the segments at both ends.
    UndergroundPipe {
        reach: u8,
    },
    /// Moves up to `per_second` units of fluid from the segment behind it to
    /// the one ahead, which may be any length. Splits segments, so it also
    /// keeps two lines of different fluids apart.
    Pump {
        per_second: f32,
    },
    /// Draws `fluid` from the ground, `per_second` units into the segments
    /// beside it.
    Well {
        fluid: ItemId,
        per_second: f32,
    },
    /// Burns `per_second` units of `fluid` from the segments beside it to
    /// put up to `kw` into its pole's grid; short of fluid, it gives that
    /// much less. A steam engine.
    Engine {
        kw: f32,
        fluid: ItemId,
        per_second: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildingDef {
    pub id: BuildingId,
    pub name: String,
    pub size: (u8, u8),
    pub role: BuildingRole,
    pub cost: Vec<Stack>,
    /// What it draws from the grid while working, in kW. Zero needs no power.
    #[serde(default)]
    pub power: f32,
}

/// One research: labs consume `cost` once per `units`, `seconds` each, and
/// when it is done the recipes and buildings in `unlocks` can be used.
/// Recipes and buildings no research unlocks are open from the start.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResearchDef {
    pub id: ResearchId,
    pub name: String,
    /// The tier it opens; research packs of this tier or below pay for it.
    pub tier: u8,
    /// Research that must be done first.
    #[serde(default)]
    pub requires: Vec<ResearchId>,
    /// Packs consumed per unit.
    pub cost: Vec<Stack>,
    pub units: u32,
    /// Lab time per unit at lab speed 1.0.
    pub seconds: f32,
    #[serde(default)]
    pub recipes: Vec<RecipeId>,
    #[serde(default)]
    pub buildings: Vec<BuildingId>,
}

/// What a goal asks for (T38, R42).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Want {
    /// This many of an item delivered to the hub, all told.
    Deliver { item: ItemId, count: u32 },
    /// This many of a building standing.
    Build { building: BuildingId, count: u32 },
    /// A research finished.
    Research { research: ResearchId },
}

/// One step of the guided first hour: the goals come one at a time, in
/// order, each meant to take five to ten minutes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GoalDef {
    pub id: String,
    /// What to do, short enough for one line: "Smelt 20 iron plates".
    pub title: String,
    /// How to do it, for a player who doesn't know yet.
    pub hint: String,
    pub want: Want,
    /// Added to the stock when the goal is met.
    #[serde(default)]
    pub reward: Vec<Stack>,
}

/// Everything loaded from `data/`, before validation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub items: Vec<ItemDef>,
    pub recipes: Vec<RecipeDef>,
    pub buildings: Vec<BuildingDef>,
    #[serde(default)]
    pub research: Vec<ResearchDef>,
    #[serde(default)]
    pub goals: Vec<GoalDef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The prototype's circuit recipe, written the way `data/recipes.ron` will hold it.
    const CIRCUIT: &str = r#"(
        id: "circuit",
        name: "Circuit",
        inputs: [(item: "gear", count: 1), (item: "wire", count: 3)],
        outputs: [(item: "circuit", count: 1)],
        seconds: 1.5,
        category: "assembling",
    )"#;

    #[test]
    fn recipe_round_trips_through_ron() {
        let r: RecipeDef = ron::from_str(CIRCUIT).expect("parses");
        assert_eq!(r.id, RecipeId::from("circuit"));
        assert_eq!(r.inputs[1].item.as_str(), "wire");
        assert_eq!(r.inputs[1].count, 3);
        let again: RecipeDef = ron::from_str(&ron::to_string(&r).unwrap()).unwrap();
        assert_eq!(r, again);
    }

    #[test]
    fn building_roles_parse() {
        let b: BuildingDef = ron::from_str(
            r#"(id: "furnace", name: "Furnace", size: (1, 1),
                role: Crafter(categories: ["smelting"], speed: 1.0),
                cost: [(item: "iron-plate", count: 5)])"#,
        )
        .expect("parses");
        assert_eq!(
            b.role,
            BuildingRole::Crafter {
                categories: vec!["smelting".into()],
                speed: 1.0
            }
        );
    }
}
