//! Undo and redo (T10, R38): every command is applied through the history,
//! which keeps the commands that reverse it.
//!
//! Undoing a removal places a new entity with a new id (ids are never
//! reused), so the history remaps ids: older entries that named the removed
//! entity now name its replacement.

use crate::Sim;
use mofact_core::command::{Command, CommandResult};
use mofact_core::ids::EntityId;
use std::collections::VecDeque;

/// How many player actions can be undone.
pub const UNDO_STEPS: usize = 20;

#[derive(Debug, Default)]
pub struct History {
    /// Each entry undoes one action, and remembers the entity it created, if any.
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    commands: Vec<Command>,
    /// The entity whose removal these commands reverse; when they re-create it,
    /// every other entry that named it is remapped to the new id.
    revives: Option<EntityId>,
}

fn removed(c: &Command) -> Option<EntityId> {
    match c {
        Command::Remove { entity } => Some(*entity),
        _ => None,
    }
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply a player action and remember how to undo it. Clears the redo list.
    pub fn apply(&mut self, sim: &mut Sim, command: &Command) -> CommandResult {
        let result = sim.apply(command);
        if let CommandResult::Applied { undo, .. } = &result {
            self.redo.clear();
            self.push_undo(Entry {
                commands: undo.clone(),
                revives: removed(command),
            });
        }
        result
    }

    /// Undo the latest action. Returns false if there was nothing to undo or it no longer applies.
    pub fn undo(&mut self, sim: &mut Sim) -> bool {
        let Some(entry) = self.undo.pop_back() else {
            return false;
        };
        match self.replay(sim, &entry) {
            Some(back) => {
                self.redo.push(back);
                true
            }
            None => false,
        }
    }

    /// Redo the latest undone action.
    pub fn redo(&mut self, sim: &mut Sim) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        match self.replay(sim, &entry) {
            Some(back) => {
                self.push_undo(back);
                true
            }
            None => false,
        }
    }

    fn push_undo(&mut self, entry: Entry) {
        if self.undo.len() == UNDO_STEPS {
            self.undo.pop_front();
        }
        self.undo.push_back(entry);
    }

    /// Apply an entry's commands and build the entry that reverses them.
    /// Stops at the first refused command (the world changed underneath it).
    fn replay(&mut self, sim: &mut Sim, entry: &Entry) -> Option<Entry> {
        let mut reverse = Entry {
            commands: Vec::new(),
            revives: None,
        };
        for c in &entry.commands {
            let CommandResult::Applied { undo, created } = sim.apply(c) else {
                return None;
            };
            reverse.commands.splice(0..0, undo);
            reverse.revives = reverse.revives.or(removed(c));
            if let (Some(new), Some(old)) = (created, entry.revives) {
                self.remap(old, new);
            }
        }
        Some(reverse)
    }

    fn remap(&mut self, old: EntityId, new: EntityId) {
        for e in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            for c in &mut e.commands {
                if let Command::Remove { entity }
                | Command::Rotate { entity, .. }
                | Command::SetRecipe { entity, .. } = c
                {
                    if *entity == old {
                        *entity = new;
                    }
                }
            }
            if e.revives == Some(old) {
                e.revives = Some(new);
            }
        }
    }
}
