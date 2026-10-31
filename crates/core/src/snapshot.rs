//! Snapshots: what the simulation publishes after each tick for the renderer and UI.
//!
//! A snapshot is read-only and owned by the reader, so a slow frame never
//! blocks the simulation (R43). The renderer interpolates between the two
//! latest snapshots using `tick`.

use crate::geom::{Dir, TilePos};
use crate::ids::{BuildingId, EntityId, ItemId, RecipeId};
use crate::Tick;
use serde::{Deserialize, Serialize};

/// What a machine is doing, as shown by its animation, sound and the starving dashboard (R40).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MachineState {
    #[default]
    Idle,
    Working,
    /// Waiting for an input.
    Starved,
    /// An output is full.
    Blocked,
    /// Not enough power on its network.
    NoPower,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EntityView {
    pub id: EntityId,
    pub building: BuildingId,
    pub pos: TilePos,
    pub dir: Dir,
    pub recipe: Option<RecipeId>,
    pub state: MachineState,
    /// Craft progress, 0.0 to 1.0.
    pub progress: f32,
}

/// One item on a belt, positioned along the belt tile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeltItemView {
    pub item: ItemId,
    pub tile: TilePos,
    /// Distance along the tile in its direction of travel, 0.0 to 1.0.
    pub along: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: Tick,
    pub entities: Vec<EntityView>,
    pub belt_items: Vec<BeltItemView>,
    /// The player's stock, sorted by item id.
    pub stock: Vec<(ItemId, u32)>,
}
