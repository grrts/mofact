//! The simulation thread: the only place the world changes (R43).
//!
//! The main thread talks to it through a [`SimHandle`]: it queues
//! [`Command`]s, reads their [`CommandResult`]s, and reads the two latest
//! [`Snapshot`]s to draw, interpolating between them. Nothing the main thread
//! does, including dropping frames, can stall a tick.

use crate::clock::Stepper;
use crate::Sim;
use mofact_core::command::{Command, CommandResult};
use mofact_core::snapshot::Snapshot;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// The two latest snapshots. Readers clone the `Arc`s, so the simulation
/// never waits for a frame to finish with one.
#[derive(Debug, Default)]
struct Snapshots {
    previous: Arc<Snapshot>,
    latest: Arc<Snapshot>,
}

/// Messages to the simulation thread.
enum Msg {
    Command(Command),
    Pause(bool),
}

/// Owns the simulation thread. Dropping it stops the thread.
pub struct SimHandle {
    tx: Sender<Msg>,
    results: Receiver<CommandResult>,
    snapshots: Arc<Mutex<Snapshots>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SimHandle {
    /// Start ticking `sim` on its own thread at 60 ticks a second.
    pub fn spawn(sim: Sim) -> Self {
        let (tx, rx) = mpsc::channel();
        let (result_tx, results) = mpsc::channel();
        let first = Arc::new(sim.snapshot());
        let snapshots = Arc::new(Mutex::new(Snapshots {
            previous: first.clone(),
            latest: first,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let snapshots = snapshots.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("mofact-sim".into())
                .spawn(move || run(sim, rx, result_tx, snapshots, stop))
                .expect("spawn simulation thread")
        };
        Self {
            tx,
            results,
            snapshots,
            stop,
            thread: Some(thread),
        }
    }

    /// Queue a command; it is applied at the start of the next tick.
    pub fn send(&self, command: Command) {
        let _ = self.tx.send(Msg::Command(command));
    }

    /// Results of applied commands, in the order they were sent.
    pub fn results(&self) -> impl Iterator<Item = CommandResult> + '_ {
        self.results.try_iter()
    }

    /// The two latest snapshots, `(previous, latest)`, for interpolation.
    pub fn snapshots(&self) -> (Arc<Snapshot>, Arc<Snapshot>) {
        let s = lock(&self.snapshots);
        (s.previous.clone(), s.latest.clone())
    }

    /// The latest snapshot.
    pub fn latest(&self) -> Arc<Snapshot> {
        lock(&self.snapshots).latest.clone()
    }

    /// Stop ticking while the app is in the background, and start again on resume.
    /// Time spent paused is not caught up.
    pub fn set_paused(&self, paused: bool) {
        let _ = self.tx.send(Msg::Pause(paused));
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            let _ = t.join();
        }
    }
}

fn lock(m: &Mutex<Snapshots>) -> std::sync::MutexGuard<'_, Snapshots> {
    // A panic on the sim thread leaves the snapshots as they were, which is still safe to read.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn run(
    mut sim: Sim,
    rx: Receiver<Msg>,
    results: Sender<CommandResult>,
    snapshots: Arc<Mutex<Snapshots>>,
    stop: Arc<AtomicBool>,
) {
    let mut stepper = Stepper::new();
    let mut paused = false;
    let mut queued = Vec::new();
    let mut last = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        loop {
            match rx.try_recv() {
                Ok(Msg::Command(c)) => queued.push(c),
                Ok(Msg::Pause(p)) => {
                    paused = p;
                    stepper.reset();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let now = Instant::now();
        let elapsed = now - last;
        last = now;
        if paused {
            std::thread::park_timeout(Stepper::TICK);
            continue;
        }
        for _ in 0..stepper.advance(elapsed) {
            for c in queued.drain(..) {
                let _ = results.send(sim.apply(&c));
            }
            sim.step();
            let snap = Arc::new(sim.snapshot());
            let mut s = lock(&snapshots);
            s.previous = std::mem::replace(&mut s.latest, snap);
        }
        std::thread::park_timeout(stepper.until_next());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn keeps_sixty_ticks_a_second_while_frames_drop() {
        let handle = SimHandle::spawn(Sim::new());
        let start = Instant::now();
        let t0 = handle.latest().tick;
        // A "renderer" that manages only 4 frames a second.
        while start.elapsed() < Duration::from_millis(1000) {
            let (prev, latest) = handle.snapshots();
            assert!(prev.tick <= latest.tick);
            std::thread::sleep(Duration::from_millis(250));
        }
        let ticks = handle.latest().tick - t0;
        let secs = start.elapsed().as_secs_f64();
        let rate = ticks as f64 / secs;
        // Generous bounds: shared CI runners are noisy, but a sim tied to frame rate would show about 4.
        assert!(
            (45.0..=75.0).contains(&rate),
            "{ticks} ticks in {secs:.2}s = {rate:.1}/s"
        );
    }

    #[test]
    fn commands_are_applied_in_order_and_answered() {
        use mofact_core::command::CommandError;
        use mofact_core::ids::EntityId;
        let handle = SimHandle::spawn(Sim::new());
        for i in 0..3 {
            handle.send(Command::Remove {
                entity: EntityId(i),
            });
        }
        let start = Instant::now();
        let mut got = Vec::new();
        while got.len() < 3 && start.elapsed() < Duration::from_secs(5) {
            got.extend(handle.results());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got.len(), 3);
        assert!(got
            .iter()
            .all(|r| *r == CommandResult::Refused(CommandError::NoSuchEntity)));
    }

    #[test]
    fn paused_sim_does_not_tick() {
        let handle = SimHandle::spawn(Sim::new());
        handle.set_paused(true);
        std::thread::sleep(Duration::from_millis(50));
        let t = handle.latest().tick;
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(handle.latest().tick, t);
        handle.set_paused(false);
        std::thread::sleep(Duration::from_millis(200));
        assert!(handle.latest().tick > t);
    }
}
