//! Commands: the only way anything outside the simulation thread changes the world.
//!
//! The main thread queues commands; the simulation applies them at the start of
//! the next tick. Undo and redo (R38) and blueprints (R37) are built on top:
//! applying a command yields the command that reverses it.

use crate::geom::{Dir, TilePos};
use crate::ids::{BuildingId, EntityId, ItemId, RecipeId, ResearchId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Command {
    /// Place a building with its top-left tile at `pos`.
    Place {
        building: BuildingId,
        pos: TilePos,
        dir: Dir,
        recipe: Option<RecipeId>,
    },
    /// Remove a placed entity; its contents return to the player's stock.
    Remove { entity: EntityId },
    /// Turn an entity to face `dir`.
    Rotate { entity: EntityId, dir: Dir },
    /// Choose the recipe an assembler-type building crafts.
    SetRecipe {
        entity: EntityId,
        recipe: Option<RecipeId>,
    },
    /// Choose what labs work on; `None` stops research. Locked until its
    /// prerequisites are done; a finished research can't be chosen again.
    SetResearch { research: Option<ResearchId> },
    /// Clear the tree or boulder on a tile so it can be built on.
    Clear { tile: TilePos },
    /// Put a cleared tree or boulder back: the undo of [`Command::Clear`].
    Regrow { tile: TilePos },
    /// Put a pin on the world map, to find a place again.
    Pin { tile: TilePos },
    /// Take a pin off the map.
    Unpin { tile: TilePos },
    /// Give a train its schedule: the stops it visits in turn.
    SetSchedule {
        entity: EntityId,
        stops: Vec<TrainStop>,
    },
    /// Choose what a requester chest asks drones for, and how many to
    /// keep; `None` asks for nothing (T49).
    SetRequest {
        entity: EntityId,
        request: Option<(ItemId, u32)>,
    },
    /// Leave a ghost for construction drones to build (T50), as `Place`
    /// would build it now. Refused where something stands or is planned.
    Plan {
        building: BuildingId,
        pos: TilePos,
        dir: Dir,
        recipe: Option<RecipeId>,
    },
    /// Take away the ghost whose top-left tile is `pos`.
    Unplan { pos: TilePos },
    /// Mark an entity for construction drones to take down, or with
    /// `false` unmark it.
    Mark { entity: EntityId, on: bool },
    /// Choose the item a filter splitter sends ahead; `None` sends
    /// everything to the sides.
    SetFilter {
        entity: EntityId,
        filter: Option<ItemId>,
    },
}

/// One stop on a train's schedule: the train stop at `at`, where it
/// loads until full or unloads until empty.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrainStop {
    pub at: TilePos,
    pub load: bool,
}

/// Why the simulation refused a command. Refused commands change nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandError {
    /// The footprint overlaps another entity or blocked terrain.
    Blocked,
    /// The player can't afford the building.
    CannotAfford,
    /// The building or recipe isn't unlocked yet.
    Locked,
    /// The entity no longer exists.
    NoSuchEntity,
    /// The id isn't in the loaded content.
    UnknownContent,
    /// It would join two pipe segments holding different fluids.
    FluidsMix,
    /// A drill has to stand on ore.
    NoOre,
    /// Water or a cliff is in the way: nothing is built there.
    Terrain,
    /// A tree or boulder is in the way until it is cleared.
    Overgrown,
    /// The research is past the demo: it needs the full game.
    FullGame,
}

/// The outcome of applying one command, sent back to the main thread.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandResult {
    /// Applied. `undo` reverses it exactly; `created` is set by `Place`.
    Applied {
        undo: Vec<Command>,
        created: Option<EntityId>,
    },
    Refused(CommandError),
}
