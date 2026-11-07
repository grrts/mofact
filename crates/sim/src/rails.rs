//! Rails and trains (T46, T47).
//!
//! Rails, signals and stops are one-tile track pieces that join every track
//! piece beside them. Trains ride on track: each stands on one tile and,
//! while moving, has also taken the next one, so two trains never share a
//! tile. Signals split the track into blocks; a train only moves into a
//! block that no other train is in or bound for, so trains that keep to
//! signalled track never meet head-on or nose to tail.
//!
//! A train visits the stops on its schedule in turn. It finds its way with
//! a search over (tile, heading) that never turns back except at the end
//! of a line. At a stop it loads what belts push into the stop until full,
//! or unloads out of the side the stop faces until empty. A tank train (S5)
//! does the same with fluid: it fills from the pipes beside the stop and
//! empties into a pipe on the side the stop faces.

use crate::entities::Entity;
use crate::tables::{Item, Role, FLUID, UNIT};
use crate::{Sim, State};
use mofact_core::command::TrainStop;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::EntityId;
use mofact_core::TICKS_PER_SECOND;
use std::collections::{HashMap, VecDeque};

/// How long a loading train that holds something waits for more before it leaves.
pub(crate) const PATIENCE: u32 = 300;
/// Milli-units a tank train pumps in or out a tick: 1200 units a second.
const TANK_FLOW: u64 = 1200 * FLUID / TICKS_PER_SECOND as u64;
/// How long a train with no way to its stop waits before it looks again.
const RETRY: u32 = 60;
/// The most (tile, heading) states one path search visits.
const SEARCH_LIMIT: usize = 100_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Phase {
    #[default]
    Idle,
    Moving,
    Waiting,
    Loading,
    Unloading,
    NoPath,
}

impl Phase {
    pub(crate) fn code(self) -> u8 {
        self as u8
    }

    pub(crate) fn from_code(c: u8) -> Self {
        match c {
            1 => Phase::Moving,
            2 => Phase::Waiting,
            3 => Phase::Loading,
            4 => Phase::Unloading,
            5 => Phase::NoPath,
            _ => Phase::Idle,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Train {
    pub cargo: Vec<(Item, u32)>,
    pub schedule: Vec<TrainStop>,
    /// The stop it is bound for or at, by place in the schedule.
    pub next: usize,
    /// The tile it stands on, and the way it heads.
    pub at: TilePos,
    pub heading: Dir,
    /// The tile it is moving onto, and how far it is, in [`UNIT`]s.
    pub to: Option<TilePos>,
    pub along: u32,
    /// The tiles still to go after `to`, last first.
    pub path: Vec<TilePos>,
    pub phase: Phase,
    /// Ticks since cargo last changed at a stop, or until the next search.
    pub wait: u32,
}

impl Train {
    pub(crate) fn load(&self) -> u32 {
        self.cargo.iter().map(|c| c.1).sum()
    }
}

/// The track's blocks, and where the trains are.
#[derive(Clone, Debug, Default)]
pub(crate) struct Rails {
    /// The block each track tile but a signal is in.
    pub blocks: HashMap<TilePos, u32>,
    /// Every train, in id order.
    pub trains: Vec<EntityId>,
}

impl Sim {
    fn role_at(&self, tile: TilePos) -> Option<&Role> {
        let id = self.entities.at(tile)?;
        let e = self.entities.get(id)?;
        Some(&self.tables.buildings[e.kind as usize].role)
    }

    pub(crate) fn is_track(&self, tile: TilePos) -> bool {
        self.role_at(tile).is_some_and(Role::is_track)
    }

    fn is_signal(&self, tile: TilePos) -> bool {
        matches!(self.role_at(tile), Some(Role::Rail { signal: true }))
    }

    /// Whether `tile` is track with only one track tile beside it.
    fn dead_end(&self, tile: TilePos) -> bool {
        Dir::ALL
            .iter()
            .filter(|&&d| self.is_track(tile.step(d)))
            .count()
            <= 1
    }

    /// Which track tiles beside `tile` join it, as a bit per [`Dir::ALL`].
    pub(crate) fn track_links(&self, tile: TilePos) -> u8 {
        Dir::ALL
            .iter()
            .enumerate()
            .filter(|(_, &d)| self.is_track(tile.step(d)))
            .fold(0, |m, (i, _)| m | 1 << i)
    }

    /// Work out the blocks again, after track was laid or lifted.
    pub(crate) fn rebuild_blocks(&mut self) {
        let mut blocks = HashMap::new();
        let mut next = 0;
        let tiles: Vec<TilePos> = self
            .entities
            .iter()
            .filter(|(_, e)| self.tables.buildings[e.kind as usize].role.is_track())
            .map(|(_, e)| e.pos)
            .collect();
        for start in tiles {
            if blocks.contains_key(&start) || self.is_signal(start) {
                continue;
            }
            let mut todo = vec![start];
            blocks.insert(start, next);
            while let Some(t) = todo.pop() {
                for d in Dir::ALL {
                    let n = t.step(d);
                    if !blocks.contains_key(&n) && self.is_track(n) && !self.is_signal(n) {
                        blocks.insert(n, next);
                        todo.push(n);
                    }
                }
            }
            next += 1;
        }
        self.rails.blocks = blocks;
    }

    /// The tiles each other train has: the one it is on and the one it is
    /// moving onto. The train being stepped is out of `states`, so it is skipped.
    fn other_trains(&self) -> impl Iterator<Item = (TilePos, Option<TilePos>)> + '_ {
        self.rails.trains.iter().filter_map(|id| {
            match self.states.get(id.0 as usize)?.as_ref()? {
                State::Train(t) => Some((t.at, t.to)),
                _ => None,
            }
        })
    }

    /// Whether any train has `tile`.
    pub(crate) fn train_on(&self, tile: TilePos) -> bool {
        self.other_trains()
            .any(|(at, to)| at == tile || to == Some(tile))
    }

    /// Whether a train on `from` may move onto `to`: no other train has
    /// that tile, and if it is a new block, no other train is in it or bound for it.
    fn can_enter(&self, from: TilePos, to: TilePos) -> bool {
        if self.train_on(to) {
            return false;
        }
        let block = self.rails.blocks.get(&to);
        match block {
            Some(b) if self.rails.blocks.get(&from) != Some(b) => {
                let mine = |t: TilePos| self.rails.blocks.get(&t) == Some(b);
                !self
                    .other_trains()
                    .any(|(at, next)| mine(at) || next.is_some_and(mine))
            }
            _ => true,
        }
    }

    /// The shortest way along track from `at`, heading `heading`, to
    /// `target`, as the tiles to visit, last first. Trains turn back only
    /// at the end of a line.
    fn find_path(&self, at: TilePos, heading: Dir, target: TilePos) -> Option<Vec<TilePos>> {
        let mut came: HashMap<(TilePos, Dir), (TilePos, Dir)> = HashMap::new();
        let mut todo = VecDeque::from([(at, heading)]);
        came.insert((at, heading), (at, heading));
        while let Some((t, h)) = todo.pop_front() {
            if t == target {
                let mut path = Vec::new();
                let mut s = (t, h);
                while s != (at, heading) {
                    path.push(s.0);
                    s = came[&s];
                }
                return Some(path);
            }
            if came.len() > SEARCH_LIMIT {
                return None;
            }
            let back = self.dead_end(t);
            for d in Dir::ALL {
                let n = t.step(d);
                if (d == h.opposite() && !back) || !self.is_track(n) {
                    continue;
                }
                if let std::collections::hash_map::Entry::Vacant(v) = came.entry((n, d)) {
                    v.insert((t, h));
                    todo.push_back((n, d));
                }
            }
        }
        None
    }

    /// Step the train `id`.
    pub(crate) fn step_train(
        &mut self,
        id: EntityId,
        t: &mut Train,
        capacity: u32,
        speed: u32,
        tank: bool,
    ) {
        match t.phase {
            Phase::Loading => {
                t.wait += 1;
                if tank {
                    self.fill_tank(t, capacity);
                }
                if t.load() >= capacity || (t.load() > 0 && t.wait >= PATIENCE) {
                    self.depart(t);
                }
                return;
            }
            Phase::Unloading => {
                let Some(stop) = self.entities.at(t.at).and_then(|s| self.entities.get(s)) else {
                    self.depart(t);
                    return;
                };
                let (front, dir) = (stop.pos.step(stop.dir), stop.dir);
                if tank {
                    let stop = *stop;
                    self.drain_tank(&stop, t);
                } else if let Some(slot) = t.cargo.iter().position(|c| c.1 > 0) {
                    if self.push(front, t.cargo[slot].0, dir) {
                        t.cargo[slot].1 -= 1;
                    }
                }
                t.cargo.retain(|c| c.1 > 0);
                if t.cargo.is_empty() {
                    self.depart(t);
                }
                return;
            }
            _ => {}
        }
        if t.schedule.is_empty() {
            t.phase = Phase::Idle;
            t.path.clear();
            return;
        }
        t.next %= t.schedule.len();
        if t.to.is_none() && !self.start_step(id, t) {
            return;
        }
        t.along += speed;
        while t.along >= UNIT {
            t.along -= UNIT;
            t.at = t.to.take().expect("moving");
            if let Some(e) = self.entities.get_mut(id) {
                e.pos = t.at;
                e.dir = t.heading;
            }
            if !self.start_step(id, t) {
                t.along = 0;
                return;
            }
        }
    }

    /// At a standstill on a tile: arrive if this is the stop, else take
    /// the next tile toward it. False if it doesn't move on.
    fn start_step(&mut self, id: EntityId, t: &mut Train) -> bool {
        let target = t.schedule[t.next].at;
        if t.at == target {
            self.arrive(id, t);
            return false;
        }
        if t.phase == Phase::NoPath && t.wait > 0 {
            t.wait -= 1;
            return false;
        }
        if t.path.last().is_none_or(|&n| !self.is_track(n)) {
            match self.find_path(t.at, t.heading, target) {
                Some(p) => t.path = p,
                None => {
                    t.phase = Phase::NoPath;
                    t.wait = RETRY;
                    t.path.clear();
                    return false;
                }
            }
        }
        let n = *t.path.last().expect("a path");
        if !self.can_enter(t.at, n) {
            t.phase = Phase::Waiting;
            return false;
        }
        t.path.pop();
        t.heading = Dir::ALL
            .into_iter()
            .find(|&d| t.at.step(d) == n)
            .unwrap_or(t.heading);
        t.to = Some(n);
        t.phase = Phase::Moving;
        true
    }

    /// Reached the stop it was bound for.
    fn arrive(&mut self, id: EntityId, t: &mut Train) {
        let load = t.schedule[t.next].load;
        let at_stop = matches!(self.role_at(t.at), Some(Role::Stop));
        let sid = self.entities.at(t.at);
        if !at_stop {
            // A waypoint: on to the next.
            self.depart(t);
            return;
        }
        t.wait = 0;
        t.phase = if load {
            Phase::Loading
        } else if t.cargo.is_empty() {
            self.depart(t);
            return;
        } else {
            Phase::Unloading
        };
        if let Some(Some(State::Stop { docked })) = sid.map(|s| &mut self.states[s.0 as usize]) {
            *docked = Some(id);
        }
    }

    /// Clear the stop at `at` if the train `id` is docked there.
    pub(crate) fn undock(&mut self, at: TilePos, id: EntityId) {
        if let Some(s) = self.entities.at(at) {
            if let Some(State::Stop { docked }) = &mut self.states[s.0 as usize] {
                if *docked == Some(id) {
                    *docked = None;
                }
            }
        }
    }

    /// Leave the stop for the next one on the schedule.
    fn depart(&mut self, t: &mut Train) {
        if let Some(s) = self.entities.at(t.at) {
            if let Some(State::Stop { docked }) = &mut self.states[s.0 as usize] {
                *docked = None;
            }
        }
        t.next = (t.next + 1) % t.schedule.len().max(1);
        t.phase = Phase::Idle;
        t.path.clear();
        t.wait = 0;
    }

    /// Pump fluid into a tank train loading at a stop, from the pipes on
    /// every side of the stop but the one it faces. A tank holds one fluid:
    /// an empty one takes the first it finds.
    fn fill_tank(&mut self, t: &mut Train, capacity: u32) {
        let Some(stop) = self
            .entities
            .at(t.at)
            .and_then(|s| self.entities.get(s))
            .copied()
        else {
            return;
        };
        let room = capacity.saturating_sub(t.load());
        if room == 0 {
            return;
        }
        let back = |d: Dir| d != stop.dir;
        let fluid = match t.cargo.first() {
            Some(&(f, _)) => f,
            None => {
                let segs = self.fluids.around(&self.entities, &stop, back);
                match segs
                    .iter()
                    .find_map(|&s| self.fluids.segs[s as usize].fluid)
                {
                    Some(f) => f,
                    None => return,
                }
            }
        };
        let want = u64::from(room).min(TANK_FLOW);
        let got = self.fluids.pull(&self.entities, &stop, back, fluid, want) as u32;
        if got > 0 {
            match t.cargo.first_mut() {
                Some(c) => c.1 += got,
                None => t.cargo.push((fluid, got)),
            }
            t.wait = 0;
        }
    }

    /// Pump a tank train's fluid out into the pipe on the side `stop` faces.
    fn drain_tank(&mut self, stop: &Entity, t: &mut Train) {
        let Some(&(fluid, n)) = t.cargo.first() else {
            return;
        };
        let out = |d: Dir| d == stop.dir;
        let give = u64::from(n).min(TANK_FLOW);
        let given = self.fluids.fill(&self.entities, stop, out, fluid, give) as u32;
        t.cargo[0].1 -= given;
    }

    /// Load one `item` into the train docked at a stop. False if none is
    /// loading there, it is full, or the item is a fluid.
    pub(crate) fn load_train(&mut self, docked: Option<EntityId>, item: Item) -> bool {
        let Some(id) = docked else { return false };
        let capacity = self
            .entities
            .get(id)
            .map(|e| &self.tables.buildings[e.kind as usize].role);
        let Some(&Role::Train { capacity, tank, .. }) = capacity else {
            return false;
        };
        if tank || self.tables.fluid[item as usize] {
            return false;
        }
        let Some(Some(State::Train(t))) = self.states.get_mut(id.0 as usize) else {
            return false;
        };
        if t.phase != Phase::Loading || t.load() >= capacity {
            return false;
        }
        match t.cargo.iter_mut().find(|c| c.0 == item) {
            Some(c) => c.1 += 1,
            None => t.cargo.push((item, 1)),
        }
        t.wait = 0;
        true
    }

    /// Point every stop at the train loading or unloading there, as after a load.
    pub(crate) fn redock(&mut self) {
        for &id in &self.rails.trains {
            let Some(Some(State::Train(t))) = self.states.get(id.0 as usize) else {
                continue;
            };
            if !matches!(t.phase, Phase::Loading | Phase::Unloading) {
                continue;
            }
            let at = t.at;
            if let Some(s) = self.entities.at(at) {
                if let Some(State::Stop { docked }) = &mut self.states[s.0 as usize] {
                    *docked = Some(id);
                }
            }
        }
    }

    /// Where a train may be placed: on track no train has.
    pub(crate) fn train_fits(&self, e: &Entity) -> bool {
        self.is_track(e.pos) && !self.train_on(e.pos)
    }
}
