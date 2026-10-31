//! Commands: the only way anything outside the simulation thread changes the world.
//!
//! The main thread queues commands; the simulation applies them at the start of
//! the next tick. Undo and redo (R38) and blueprints (R37) are built on top:
//! applying a command yields the command that reverses it.

use crate::geom::{Dir, TilePos};
use crate::ids::{BuildingId, EntityId, RecipeId, ResearchId};
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
