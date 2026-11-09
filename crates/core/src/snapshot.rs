//! Snapshots: what the simulation publishes after each tick for the renderer and UI.
//!
//! A snapshot is read-only and owned by the reader, so a slow frame never
//! blocks the simulation (R43). The renderer interpolates between the two
//! latest snapshots using `tick`.

use crate::command::TrainStop;
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
    /// For a miner, the ore left in its reach: 0 once it is mined out.
    #[serde(default)]
    pub ore_left: Option<u32>,
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
    /// For a requester chest, what it asks for and how many (T49).
    #[serde(default)]
    pub request: Option<(ItemId, u32)>,
    /// For a filter splitter, the item it sends ahead.
    #[serde(default)]
    pub filter: Option<ItemId>,
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
    /// Tiles whose tree or boulder has been cleared, sorted. Shared, so a
    /// snapshot costs nothing extra while it doesn't change.
    #[serde(default)]
    pub cleared: std::sync::Arc<Vec<TilePos>>,
    /// Ore tiles mined out, sorted; shared like `cleared`.
    #[serde(default)]
    pub depleted: std::sync::Arc<Vec<TilePos>>,
    /// The player's pins on the world map, sorted; shared like `cleared`.
    #[serde(default)]
    pub pins: std::sync::Arc<Vec<TilePos>>,
    /// Trains, which ride on rail tiles rather than standing on their own.
    #[serde(default)]
    pub trains: Vec<TrainView>,
    /// The first megaproject built, and how far along it is.
    #[serde(default)]
    pub mega: Option<MegaView>,
    /// The land explored so far (S3), sorted; the rest lies under the
    /// fog of war. Shared like `cleared`.
    #[serde(default)]
    pub explored: std::sync::Arc<Vec<crate::geom::FogCell>>,
    /// Drones in the air (T49); drones resting in a port aren't listed.
    #[serde(default)]
    pub drones: Vec<DroneView>,
    /// Ghosts waiting for construction drones (T50), and what is marked
    /// for them to take down.
    #[serde(default)]
    pub ghosts: Vec<GhostView>,
    #[serde(default)]
    pub marked: Vec<EntityId>,
}

/// A drone in flight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneView {
    /// Where it is, in tiles, and where it is going.
    pub at: [f32; 2],
    pub to: [f32; 2],
    /// What it carries: an item, or the building it is taking to a ghost.
    pub cargo: Option<ItemId>,
    pub building: Option<BuildingId>,
    /// Its battery, 0.0 to 1.0.
    pub battery: f32,
}

/// A building planned for construction drones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GhostView {
    pub building: BuildingId,
    pub pos: TilePos,
    pub dir: Dir,
    pub recipe: Option<RecipeId>,
    /// A drone is on its way with it.
    pub coming: bool,
}

/// A megaproject's progress: its stage, and what that stage still takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MegaView {
    pub id: EntityId,
    /// Stages done so far, out of `stages`; all done means the game is won.
    pub stage: u32,
    pub stages: u32,
    /// The current stage's name, empty once all are done.
    pub name: String,
    /// What the current stage takes: item, how many it has, how many it needs.
    pub needs: Vec<(ItemId, u32, u32)>,
}

impl MegaView {
    pub fn done(&self) -> bool {
        self.stage >= self.stages
    }
}

/// What a train is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrainState {
    /// No stops, or none it can reach.
    #[default]
    Idle,
    Moving,
    /// Stopped short: the track ahead is taken by another train.
    Waiting,
    /// At a stop, loading or unloading.
    Loading,
    Unloading,
    /// Its next stop is not on track it can reach.
    NoPath,
}

/// One train this tick.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainView {
    pub id: EntityId,
    pub building: BuildingId,
    /// The tile it is on, the way it faces, and how far it is toward the
    /// next tile, 0.0 to 1.0.
    pub pos: TilePos,
    pub dir: Dir,
    pub along: f32,
    /// What it carries, and how much it can.
    pub cargo: Vec<(ItemId, u32)>,
    pub capacity: u32,
    pub schedule: Vec<TrainStop>,
    /// The stop it is bound for or at, by place in the schedule.
    pub next: usize,
    pub state: TrainState,
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
    /// The levels done of each repeatable research begun (S2).
    #[serde(default)]
    pub levels: Vec<(ResearchId, u32)>,
}
