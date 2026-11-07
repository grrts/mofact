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
    /// A heap of powder, such as sulfur.
    Powder,
    /// A battery cell.
    Cell,
    /// A thin round wafer.
    Wafer,
    /// A square chip with pins on every side.
    Chip,
    /// An open frame of struts around a core.
    Frame,
    /// A horseshoe magnet.
    Magnet,
    /// A satellite: a body between two wings of solar cells.
    Satellite,
    /// A rocket nozzle's bell.
    Nozzle,
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
    /// A rail tile (T46). Rails join every rail, signal and stop beside
    /// them; trains run along them tile to tile.
    Rail,
    /// A rail tile that splits the track into blocks: a train only goes
    /// past it into a block no other train is in or bound for.
    RailSignal,
    /// A rail tile where trains stop (T47). Belts pointed into it load the
    /// train standing there; it unloads out of the side it faces.
    TrainStop,
    /// A train: runs at `tiles_per_second` along rails and carries up to
    /// `capacity` items between the stops on its schedule.
    ///
    /// A `tank` train (S5) carries `capacity` units of one fluid instead:
    /// it fills at a stop from the pipes beside it, and empties into a
    /// pipe out of the side the stop faces.
    Locomotive {
        tiles_per_second: f32,
        capacity: u32,
        #[serde(default)]
        tank: bool,
    },
    /// A drone port (T49, T50): home to `drones` drones, which fly
    /// straight lines over the square `reach` tiles round it. Ports whose
    /// squares meet make one network. Drones carry items from provider
    /// chests (and the hub) to requester chests, build ghosts and take down
    /// what is marked, and charge here on the port's power.
    DronePort {
        drones: u8,
        reach: u8,
    },
    /// A chest drones take from: belts fill it, up to `capacity` items.
    ProviderChest {
        capacity: u32,
    },
    /// A chest drones fill with the item it asks for, up to the count it
    /// asks for and at most `capacity`; it hands them out of its arrow.
    RequesterChest {
        capacity: u32,
    },
    /// The game's last build (T54): takes the items of each stage in turn
    /// from belts, and when the last stage is in, the factory has won.
    Megaproject {
        stages: Vec<Stage>,
    },
    /// Charts the land (S3): while powered, it lifts the fog from the
    /// nearest unexplored patch every `seconds_per_patch`, out to `reach`
    /// tiles away.
    Radar {
        reach: u32,
        seconds_per_patch: f32,
    },
}

/// One stage of a megaproject: its name and what it takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub needs: Vec<Stack>,
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
    /// Late-game research that never runs out (S2): each level done gives
    /// this bonus and the next level costs more units.
    #[serde(default)]
    pub repeat: Option<Repeat>,
}

/// What each level of a repeatable research gives (S2).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Repeat {
    pub bonus: Bonus,
    /// The share each level adds, such as 0.1 for ten percent faster.
    pub per_level: f32,
}

/// Which machines a repeatable research speeds up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Bonus {
    /// Drills dig faster.
    Mining,
    /// Crafters make faster.
    Crafting,
    /// Labs research faster.
    Research,
}

impl Repeat {
    /// Units for all of the first `levels` levels: the first takes `units`,
    /// the second twice that, and so on.
    pub fn total(units: u32, levels: u32) -> u64 {
        u64::from(units) * u64::from(levels) * (u64::from(levels) + 1) / 2
    }

    /// The levels `done` units have finished, and the units into the next
    /// level and that level's size.
    pub fn level(units: u32, done: u32) -> (u32, (u32, u32)) {
        let units = units.max(1);
        let mut level = 0;
        while Repeat::total(units, level + 1) <= u64::from(done) {
            level += 1;
        }
        let into = u64::from(done) - Repeat::total(units, level);
        (level, (into as u32, units * (level + 1)))
    }
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

    #[test]
    fn each_level_of_repeatable_research_costs_one_base_more_than_the_last() {
        // 50, then 100, then 150.
        assert_eq!(Repeat::level(50, 0), (0, (0, 50)));
        assert_eq!(Repeat::level(50, 49), (0, (49, 50)));
        assert_eq!(Repeat::level(50, 50), (1, (0, 100)));
        assert_eq!(Repeat::level(50, 149), (1, (99, 100)));
        assert_eq!(Repeat::level(50, 150), (2, (0, 150)));
        assert_eq!(Repeat::total(50, 3), 300);
    }
}
