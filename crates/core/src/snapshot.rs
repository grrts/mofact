//! Snapshots: what the simulation publishes after each tick for the renderer and UI.
//!
//! A snapshot is read-only and owned by the reader, so a slow frame never
//! blocks the simulation (R43). The renderer interpolates between the two
//! latest snapshots using `tick`.

use crate::geom::{Dir, TilePos};
use crate::ids::{BuildingId, EntityId, ItemId, RecipeId, ResearchId};
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
    /// For a miner, the ore under it.
    #[serde(default)]
    pub ore: Option<ItemId>,
    /// The power grid it is on, as an index into [`Snapshot::grids`].
    #[serde(default)]
    pub grid: Option<u16>,
    /// For an underground belt, whether this end is the exit.
    #[serde(default)]
    pub exit: bool,
    /// For a pipe, tank or underground pipe: what its segment holds.
    #[serde(default)]
    pub fluid: Option<FluidView>,
    /// For anything that carries fluid, the sides it connects on, as bits
    /// by [`Dir`] index (east 1, south 2, west 4, north 8).
    #[serde(default)]
    pub links: u8,
    pub state: MachineState,
    /// Craft progress, 0.0 to 1.0.
    pub progress: f32,
}

/// The fluid in a pipe segment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FluidView {
    pub item: ItemId,
    /// How full the segment is, 0.0 to 1.0.
    pub fill: f32,
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
    pub research: ResearchView,
    /// Each power grid: poles linked by wire and what they cover.
    #[serde(default)]
    pub grids: Vec<GridView>,
    /// The goal the player is on, if any are left.
    #[serde(default)]
    pub goal: Option<GoalView>,
}

/// The current goal and how far along it is.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GoalView {
    /// Its place in the content's goals, from 0.
    pub index: u16,
    /// How much of what it wants is done, of how much.
    pub have: u32,
    pub need: u32,
}

/// One power grid this tick.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GridView {
    /// What its generators can give, in kW.
    pub supply_kw: f32,
    /// What its working machines ask for, in kW.
    pub demand_kw: f32,
    /// The share of demand met, 0.0 to 1.0: machines run this much slower.
    pub satisfaction: f32,
}

/// Where research stands, for the research screen and the toolbar's locks.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ResearchView {
    /// What labs are working on.
    pub current: Option<ResearchId>,
    /// Units done and needed for the current research.
    pub progress: (u32, u32),
    /// Finished research, in content order.
    pub done: Vec<ResearchId>,
}
