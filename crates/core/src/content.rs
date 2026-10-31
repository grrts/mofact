//! Schema of the content files in `data/` (R1: depth is data, not code).
//!
//! These structs are what the RON files deserialize into. Loading, hot reload
//! and validation live in other lanes; only the shape is fixed here.

use crate::ids::{BuildingId, ItemId, RecipeId};
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildingDef {
    pub id: BuildingId,
    pub name: String,
    pub size: (u8, u8),
    pub role: BuildingRole,
    pub cost: Vec<Stack>,
}

/// Everything loaded from `data/`, before validation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub items: Vec<ItemDef>,
    pub recipes: Vec<RecipeDef>,
    pub buildings: Vec<BuildingDef>,
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
