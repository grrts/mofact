//! The simulation on the caller's thread, for the browser, where the game
//! gets no thread of its own.
//!
//! [`LocalSim`] has the same calls as [`SimHandle`](crate::SimHandle) plus
//! [`pump`](LocalSim::pump), which the frame loop calls once a frame to run
//! the ticks that real time says are due. Ticks still follow the clock, not
//! the frame rate: a slow frame runs more of them, up to
//! [`Stepper::MAX_CATCH_UP`].

use crate::clock::Stepper;
use crate::jobs::{pack, unpack, work, Job};
use crate::{History, Sim};
use mofact_core::command::{Command, CommandResult};
use mofact_core::snapshot::Snapshot;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use web_time::Instant;

pub struct LocalSim {
    inner: RefCell<Inner>,
}

struct Inner {
    sim: Sim,
    stepper: Stepper,
    last: Instant,
    paused: bool,
    history: History,
    queued: Vec<Job>,
    results: Vec<CommandResult>,
    previous: Arc<Snapshot>,
    latest: Arc<Snapshot>,
}

impl LocalSim {
    /// Hold `sim`, ticking it whenever [`pump`](Self::pump) is called.
    pub fn spawn(sim: Sim) -> Self {
        let first = Arc::new(sim.snapshot());
        Self {
            inner: RefCell::new(Inner {
                sim,
                stepper: Stepper::new(),
                last: Instant::now(),
                paused: false,
                history: History::new(),
                queued: Vec::new(),
                results: Vec::new(),
                previous: first.clone(),
                latest: first,
            }),
        }
    }

    /// Queue a command; it is applied at the start of the next tick, as
    /// one player action that undo can reverse.
    pub fn send(&self, command: Command) {
        self.send_all(vec![command]);
    }

    /// Queue several commands as one player action: one undo reverses them all.
    pub fn send_all(&self, commands: Vec<Command>) {
        if !commands.is_empty() {
            self.inner.borrow_mut().queued.push(Job::Action(commands));
        }
    }

    /// Undo the latest player action, at the start of the next tick.
    pub fn undo(&self) {
        self.inner.borrow_mut().queued.push(Job::Undo);
    }

    /// Redo the latest undone action, at the start of the next tick.
    pub fn redo(&self) {
        self.inner.borrow_mut().queued.push(Job::Redo);
    }

    /// Whether there is an action to undo, and one to redo.
    pub fn history(&self) -> (bool, bool) {
        unpack(pack(&self.inner.borrow().history))
    }

    /// Results of applied commands, in the order they were sent.
    pub fn results(&self) -> std::vec::IntoIter<CommandResult> {
        std::mem::take(&mut self.inner.borrow_mut().results).into_iter()
    }

    /// The two latest snapshots, `(previous, latest)`, for interpolation.
    pub fn snapshots(&self) -> (Arc<Snapshot>, Arc<Snapshot>) {
        let s = self.inner.borrow();
        (s.previous.clone(), s.latest.clone())
    }

    /// The latest snapshot.
    pub fn latest(&self) -> Arc<Snapshot> {
        self.inner.borrow().latest.clone()
    }

    /// Stop ticking, or start again. Time spent paused is not caught up.
    pub fn set_paused(&self, paused: bool) {
        let mut s = self.inner.borrow_mut();
        s.paused = paused;
        s.stepper.reset();
        s.last = Instant::now();
    }

    /// A save of the world, already waiting on the returned channel.
    pub fn save(&self) -> Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(self.inner.borrow().sim.save());
        rx
    }

    /// Run the ticks due since the last call.
    pub fn pump(&self) {
        self.pump_at(Instant::now());
    }

    /// [`pump`](Self::pump) as if the clock read `now`.
    fn pump_at(&self, now: Instant) {
        let mut guard = self.inner.borrow_mut();
        let s = &mut *guard;
        let elapsed = now - s.last;
        s.last = now;
        if s.paused {
            return;
        }
        for _ in 0..s.stepper.advance(elapsed) {
            for job in std::mem::take(&mut s.queued) {
                work(&mut s.sim, &mut s.history, job, |r| s.results.push(r));
            }
            s.sim.step();
            let snap = Arc::new(s.sim.snapshot());
            s.previous = std::mem::replace(&mut s.latest, snap);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn ticks_follow_the_clock_not_the_calls() {
        let sim = LocalSim::spawn(Sim::new());
        let start = sim.inner.borrow().last;
        // One second of frames, some slow and some fast, some repeated.
        let mut t = Duration::ZERO;
        for (i, ms) in [16u64, 16, 33, 7, 16, 50, 16, 0, 16]
            .iter()
            .cycle()
            .enumerate()
        {
            if t >= Duration::from_secs(1) {
                break;
            }
            t = (t + Duration::from_millis(*ms)).min(Duration::from_secs(1));
            sim.pump_at(start + t);
            if i % 3 == 0 {
                sim.pump_at(start + t);
            }
        }
        assert_eq!(sim.latest().tick, 60);
        let (prev, latest) = sim.snapshots();
        assert_eq!(prev.tick + 1, latest.tick);
    }

    #[test]
    fn commands_are_answered_and_pausing_stops_the_clock() {
        use mofact_core::command::CommandError;
        use mofact_core::ids::EntityId;
        let sim = LocalSim::spawn(Sim::new());
        sim.send(Command::Remove {
            entity: EntityId(0),
        });
        let start = sim.inner.borrow().last;
        sim.pump_at(start + Duration::from_millis(30));
        let got: Vec<_> = sim.results().collect();
        assert_eq!(
            got,
            vec![CommandResult::Refused(CommandError::NoSuchEntity)]
        );
        sim.set_paused(true);
        let t = sim.latest().tick;
        let start = sim.inner.borrow().last;
        sim.pump_at(start + Duration::from_millis(500));
        assert_eq!(sim.latest().tick, t);
        assert!(sim
            .save()
            .try_recv()
            .unwrap()
            .starts_with(crate::save::MAGIC));
    }
}
