//! Simulation lane: the deterministic factory simulation.
//!
//! Only the simulation changes the world. It runs on its own thread at
//! [`TICKS_PER_SECOND`](mofact_core::TICKS_PER_SECOND), applies queued
//! [`Command`]s at the start of each tick and publishes a [`Snapshot`] after it.
//!
//! [`SimHandle`] runs it on that thread (T05). Entity storage arrives in T06,
//! transport lines in T07 and machines in T08.

pub use mofact_core as core;

mod clock;
pub mod entities;
mod thread;

pub use clock::Stepper;
pub use thread::SimHandle;

use mofact_core::command::{Command, CommandError, CommandResult};
use mofact_core::snapshot::Snapshot;
use mofact_core::Tick;

/// The simulated world. For now it only counts ticks.
#[derive(Debug, Default)]
pub struct Sim {
    tick: Tick,
}

impl Sim {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Apply one command. The skeleton has no world yet, so every command is refused.
    pub fn apply(&mut self, _command: &Command) -> CommandResult {
        CommandResult::Refused(CommandError::NoSuchEntity)
    }

    /// Advance the world by one tick.
    pub fn step(&mut self) {
        self.tick += 1;
    }

    /// A read-only view of the world for the renderer and UI.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            tick: self.tick,
            ..Snapshot::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_of_ticks() {
        let mut sim = Sim::new();
        for _ in 0..mofact_core::TICKS_PER_SECOND {
            sim.step();
        }
        assert_eq!(sim.snapshot().tick, 60);
    }
}
