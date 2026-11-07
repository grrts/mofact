//! Haptics: what the game wants the phone to feel like. The game only
//! names the feel; a platform that can buzz plays it (Android through the
//! activity, iOS through UIKit's feedback generators), and the rest ignore
//! it. The phone's own touch-feedback setting still decides.

use mofact_core::command::CommandResult;
use mofact_core::snapshot::Snapshot;

/// A feel, from lightest to strongest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Haptic {
    /// Something was built or removed.
    Tick,
    /// The game said no.
    Warning,
    /// A goal was met or a research finished.
    Success,
}

impl Haptic {
    /// Its number across the platform bridges.
    pub fn code(self) -> i32 {
        match self {
            Haptic::Tick => 0,
            Haptic::Warning => 1,
            Haptic::Success => 2,
        }
    }
}

/// Works out the feels for each frame: at most one, the strongest.
#[derive(Default)]
pub struct Haptics {
    /// The goal and the research count last seen, once there is a snapshot.
    seen: Option<(Option<u16>, usize)>,
}

impl Haptics {
    /// The feel for this frame's command results and latest snapshot.
    pub fn frame(&mut self, results: &[CommandResult], snap: &Snapshot) -> Option<Haptic> {
        let mut feel = None;
        for r in results {
            let f = match r {
                CommandResult::Refused(_) => Haptic::Warning,
                CommandResult::Applied { created, undo }
                    if created.is_some() || !undo.is_empty() =>
                {
                    Haptic::Tick
                }
                CommandResult::Applied { .. } => continue,
            };
            feel = feel.max(Some(f));
        }
        let now = (
            snap.goal.as_ref().map(|g| g.index),
            snap.research.done.len(),
        );
        if let Some((goal, done)) = self.seen.replace(now) {
            // A game switched or loaded starts afresh, so only steps forward count.
            let met = matches!((goal, now.0), (Some(a), Some(b)) if b == a + 1)
                || (goal.is_some() && now.0.is_none());
            if met || now.1 == done + 1 {
                feel = Some(Haptic::Success);
            }
        }
        feel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofact_core::command::{Command, CommandError};
    use mofact_core::ids::EntityId;
    use mofact_core::snapshot::GoalView;

    #[test]
    fn building_ticks_a_refusal_warns_and_a_met_goal_wins() {
        let mut h = Haptics::default();
        let mut snap = mofact_sim::Sim::new().snapshot();
        assert_eq!(h.frame(&[], &snap), None, "the first frame only looks");
        let placed = CommandResult::Applied {
            undo: vec![Command::Remove {
                entity: EntityId(1),
            }],
            created: Some(EntityId(1)),
        };
        assert_eq!(
            h.frame(std::slice::from_ref(&placed), &snap),
            Some(Haptic::Tick)
        );
        let no = CommandResult::Refused(CommandError::Blocked);
        assert_eq!(h.frame(&[placed.clone(), no], &snap), Some(Haptic::Warning));
        snap.goal = Some(GoalView {
            index: 0,
            have: 0,
            need: 50,
        });
        assert_eq!(h.frame(&[], &snap), None, "a goal showing up is no step");
        snap.goal.as_mut().unwrap().index = 1;
        assert_eq!(h.frame(&[placed], &snap), Some(Haptic::Success));
        snap.research.done.push("steel-working".into());
        assert_eq!(h.frame(&[], &snap), Some(Haptic::Success));
        assert_eq!(h.frame(&[], &snap), None);
        // Loading a game far along is no success.
        snap.research.done = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(h.frame(&[], &snap), None);
    }
}
