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
    /// Each entry undoes one player action: a tap, or a whole drag.
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
}

/// The commands that undo one action, in the order to apply them.
#[derive(Debug, Clone, Default)]
struct Entry(Vec<Step>);

#[derive(Debug, Clone)]
struct Step {
    command: Command,
    /// The entity whose removal this command reverses; when it re-creates
    /// it, every other entry that named it is remapped to the new id.
    revives: Option<EntityId>,
}

fn removed(c: &Command) -> Option<EntityId> {
    match c {
        Command::Remove { entity } => Some(*entity),
        _ => None,
    }
}

/// The steps that reverse `command`, given the undo commands it returned.
fn reverse<'a>(command: &Command, undo: &'a [Command]) -> impl Iterator<Item = Step> + 'a {
    let revives = removed(command);
    undo.iter()
        .cloned()
        .map(move |command| Step { command, revives })
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
        self.apply_all(sim, std::slice::from_ref(command))
            .pop()
            .expect("one result per command")
    }

    /// Apply several commands as one player action, such as a belt drawn in
    /// one drag: a single undo reverses all of them. Commands the
    /// simulation refuses are skipped. Clears the redo list if any applied.
    pub fn apply_all(&mut self, sim: &mut Sim, commands: &[Command]) -> Vec<CommandResult> {
        let mut entry = Entry::default();
        let results: Vec<_> = commands
            .iter()
            .map(|c| {
                let result = sim.apply(c);
                if let CommandResult::Applied { undo, .. } = &result {
                    entry.0.splice(0..0, reverse(c, undo));
                }
                result
            })
            .collect();
        if !entry.0.is_empty() {
            self.redo.clear();
            self.push_undo(entry);
        }
        results
    }

    /// Undo the latest action. Returns false if there was nothing to undo or it no longer applies.
    pub fn undo(&mut self, sim: &mut Sim) -> bool {
        let Some(entry) = self.undo.pop_back() else {
            return false;
        };
        match self.replay(sim, entry) {
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
        match self.replay(sim, entry) {
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
    fn replay(&mut self, sim: &mut Sim, mut entry: Entry) -> Option<Entry> {
        let mut back = Entry::default();
        for i in 0..entry.0.len() {
            let step = entry.0[i].clone();
            let CommandResult::Applied { undo, created } = sim.apply(&step.command) else {
                return None;
            };
            back.0.splice(0..0, reverse(&step.command, &undo));
            if let (Some(new), Some(old)) = (created, step.revives) {
                self.remap(old, new);
                // Later steps of this entry may name it too.
                for e in [&mut entry, &mut back] {
                    remap_entry(e, old, new);
                }
            }
        }
        Some(back)
    }

    fn remap(&mut self, old: EntityId, new: EntityId) {
        for e in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            remap_entry(e, old, new);
        }
    }
}

fn remap_entry(e: &mut Entry, old: EntityId, new: EntityId) {
    for step in &mut e.0 {
        if let Command::Remove { entity }
        | Command::Rotate { entity, .. }
        | Command::SetRecipe { entity, .. } = &mut step.command
        {
            if *entity == old {
                *entity = new;
            }
        }
        if step.revives == Some(old) {
            step.revives = Some(new);
        }
    }
}
