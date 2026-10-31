//! The work queued for the simulation, shared by the simulation thread and
//! [`LocalSim`](crate::LocalSim): player actions go through the undo
//! [`History`], so one undo reverses a whole gesture.

use crate::{History, Sim};
use mofact_core::command::{Command, CommandResult};

/// Work applied at the start of the next tick, in the order it was sent.
#[derive(Debug)]
pub(crate) enum Job {
    /// One player action: a single undo reverses all of its commands.
    Action(Vec<Command>),
    Undo,
    Redo,
}

/// Whether there is something to undo and to redo, packed for an atomic.
pub(crate) fn pack(history: &History) -> u8 {
    u8::from(history.can_undo()) | (u8::from(history.can_redo()) << 1)
}

pub(crate) fn unpack(bits: u8) -> (bool, bool) {
    (bits & 1 != 0, bits & 2 != 0)
}

/// Apply one job to `sim` through `history`, answering each command.
pub(crate) fn work(
    sim: &mut Sim,
    history: &mut History,
    job: Job,
    mut answer: impl FnMut(CommandResult),
) {
    match job {
        Job::Action(commands) => history
            .apply_all(sim, &commands)
            .into_iter()
            .for_each(&mut answer),
        Job::Undo => {
            history.undo(sim);
        }
        Job::Redo => {
            history.redo(sim);
        }
    }
}
