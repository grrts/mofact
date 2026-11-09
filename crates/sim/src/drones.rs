//! Drones (T49, T50): ports, provider and requester chests, ghosts.
//!
//! Every drone belongs to a port. Ports whose squares meet make one
//! network; a drone works anywhere in its port's network. Ten times a
//! second the idle drones are handed jobs, nearest first: build a ghost,
//! take down what is marked, or carry items from a provider chest (or the
//! hub's stock, when the hub stands in the network) to a requester chest
//! that wants them. Drones fly straight lines. Flying drains the battery;
//! a drone that couldn't reach its target and a port after it goes to the
//! nearest port first, and charges there on the port's power.
//!
//! Construction takes what a building costs from the stock when a drone
//! sets off with it, and builds it on arrival.

use crate::entities::Entity;
use crate::tables::{Item, Recipe, Role, UNIT};
use crate::{power, Sim, State};
use mofact_core::command::Command;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::EntityId;

/// Items a drone carries at once.
pub const CARRY: u32 = 10;
/// Flight speed: 8 tiles a second, in position units a tick.
pub const SPEED: i64 = UNIT as i64 * 8 / 60;
/// A full battery flies this far: 120 tiles.
pub const BATTERY: i64 = UNIT as i64 * 120;
/// A port charges a drone this much a tick: a full battery in two seconds.
const CHARGE: i64 = BATTERY / 120;
/// Ticks between handing out jobs.
const DISPATCH_EVERY: u64 = 6;
/// Jobs handed out at most each time, so a huge paste spreads its cost.
const DISPATCH_MAX: usize = 32;
/// A requester chest hands one item out of its arrow this often.
pub const OUT_EVERY: u32 = 6;
/// A warehouse hands one item out of its arrow this often: faster than
/// an express belt takes them, so it never holds a line up.
pub const WAREHOUSE_OUT_EVERY: u32 = 2;

/// A provider or requester chest.
#[derive(Clone, Debug, Default)]
pub struct Chest {
    /// What it holds, by item.
    pub items: Vec<(Item, u32)>,
    /// What a requester asks for and how many it keeps.
    pub request: Option<(Item, u32)>,
    /// Items drones are bringing it.
    pub incoming: u32,
    /// Ticks since it last handed an item out.
    pub wait: u32,
    /// The one item a warehouse hands out, if it is set to one.
    pub filter: Option<Item>,
}

impl Chest {
    pub fn count(&self, item: Item) -> u32 {
        self.items
            .iter()
            .find(|(i, _)| *i == item)
            .map_or(0, |(_, n)| *n)
    }

    pub fn total(&self) -> u32 {
        self.items.iter().map(|(_, n)| n).sum()
    }

    pub fn add(&mut self, item: Item, n: u32) {
        match self.items.iter_mut().find(|(i, _)| *i == item) {
            Some((_, k)) => *k += n,
            None => self.items.push((item, n)),
        }
    }

    /// Take up to `n` of `item`; returns how many it had.
    pub fn take(&mut self, item: Item, n: u32) -> u32 {
        let Some(at) = self.items.iter().position(|(i, _)| *i == item) else {
            return 0;
        };
        let got = self.items[at].1.min(n);
        self.items[at].1 -= got;
        if self.items[at].1 == 0 {
            self.items.remove(at);
        }
        got
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Back to its port, to rest and charge.
    Rest,
    /// Items taken from `from` (or the stock, for the hub) for `to`;
    /// `picked` once it has been to `from`.
    Carry {
        from: EntityId,
        to: EntityId,
        item: Item,
        n: u32,
        picked: bool,
    },
    /// A building for the ghost at `at`, paid for.
    Build { at: TilePos },
    /// Take down a marked entity.
    Take { entity: EntityId },
}

#[derive(Clone, Debug)]
pub struct Drone {
    pub home: EntityId,
    /// Where it is, in position units.
    pub at: (i64, i64),
    pub battery: i64,
    pub job: Job,
    /// The port it is going to, or charging at, before it carries on.
    pub charging: Option<EntityId>,
}

/// A building planned for construction drones.
#[derive(Clone, Debug, PartialEq)]
pub struct Ghost {
    pub kind: u16,
    pub pos: TilePos,
    pub dir: Dir,
    pub recipe: Option<Recipe>,
    /// A drone is on its way with it, and its cost is spent.
    pub coming: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Fleet {
    pub drones: Vec<Drone>,
    pub ghosts: Vec<Ghost>,
    pub marked: Vec<EntityId>,
}

/// A port: its id, the middle of it in position units, its reach in
/// tiles, and its network.
#[derive(Clone, Copy, Debug)]
struct Port {
    id: EntityId,
    mid: (i64, i64),
    reach: i64,
    net: usize,
}

/// The middle of an entity, in position units.
pub fn middle(e: &Entity) -> (i64, i64) {
    let u = UNIT as i64;
    (
        e.pos.x as i64 * u + i64::from(e.size.0.max(1)) * u / 2,
        e.pos.y as i64 * u + i64::from(e.size.1.max(1)) * u / 2,
    )
}

fn tile_middle(t: TilePos, size: (u8, u8)) -> (i64, i64) {
    let u = UNIT as i64;
    (
        t.x as i64 * u + i64::from(size.0.max(1)) * u / 2,
        t.y as i64 * u + i64::from(size.1.max(1)) * u / 2,
    )
}

/// Straight-line distance, in position units.
pub fn dist(a: (i64, i64), b: (i64, i64)) -> i64 {
    let (dx, dy) = ((a.0 - b.0) as f64, (a.1 - b.1) as f64);
    dx.hypot(dy).round() as i64
}

impl Sim {
    /// Every port, with the network it is in.
    fn ports(&self) -> Vec<Port> {
        let mut ports: Vec<Port> = self
            .entities
            .iter()
            .filter_map(
                |(id, e)| match self.tables.buildings[e.kind as usize].role {
                    Role::Port { reach, .. } => Some(Port {
                        id,
                        mid: middle(e),
                        reach: i64::from(reach),
                        net: 0,
                    }),
                    _ => None,
                },
            )
            .collect();
        // Ports whose squares meet share a network: a little flood fill.
        let n = ports.len();
        let mut net = vec![usize::MAX; n];
        let u = UNIT as i64;
        for start in 0..n {
            if net[start] != usize::MAX {
                continue;
            }
            net[start] = start;
            let mut todo = vec![start];
            while let Some(i) = todo.pop() {
                for j in 0..n {
                    let (a, b) = (ports[i], ports[j]);
                    let span = (a.reach + b.reach) * u;
                    if net[j] == usize::MAX
                        && (a.mid.0 - b.mid.0).abs() <= span
                        && (a.mid.1 - b.mid.1).abs() <= span
                    {
                        net[j] = start;
                        todo.push(j);
                    }
                }
            }
        }
        for (p, k) in ports.iter_mut().zip(net) {
            p.net = k;
        }
        ports
    }

    /// Whether a drone could build at `tile`: a port covers it.
    pub fn covered(&self, tile: TilePos) -> bool {
        let at = tile_middle(tile, (1, 1));
        self.ports().iter().any(|p| covers(p, at))
    }

    /// Hand idle drones their jobs.
    fn dispatch(&mut self) {
        if self.fleet.drones.is_empty() {
            return;
        }
        let ports = self.ports();
        let net_of = |at: (i64, i64)| ports.iter().find(|p| covers(p, at)).map(|p| p.net);
        let home_net = |d: &Drone| ports.iter().find(|p| p.id == d.home).map(|p| p.net);
        let mut handed = 0;
        let idle = |fleet: &Fleet, net: usize, from: (i64, i64)| {
            fleet
                .drones
                .iter()
                .enumerate()
                .filter(|(_, d)| d.job == Job::Rest && home_net(d) == Some(net))
                .min_by_key(|(_, d)| dist(d.at, from))
                .map(|(i, _)| i)
        };
        // Construction first: ghosts in the order they were planned.
        for g in 0..self.fleet.ghosts.len() {
            if handed >= DISPATCH_MAX {
                return;
            }
            let ghost = self.fleet.ghosts[g].clone();
            if ghost.coming {
                continue;
            }
            let b = &self.tables.buildings[ghost.kind as usize];
            let at = tile_middle(ghost.pos, b.size);
            let Some(net) = net_of(at) else { continue };
            if b.cost.iter().any(|&(i, n)| self.stock[i as usize] < n) {
                continue;
            }
            let Some(d) = idle(&self.fleet, net, at) else {
                continue;
            };
            for &(i, n) in &b.cost {
                self.stock[i as usize] -= n;
            }
            self.fleet.ghosts[g].coming = true;
            self.fleet.drones[d].job = Job::Build { at: ghost.pos };
            handed += 1;
        }
        for m in 0..self.fleet.marked.len() {
            if handed >= DISPATCH_MAX {
                return;
            }
            let entity = self.fleet.marked[m];
            if self
                .fleet
                .drones
                .iter()
                .any(|d| d.job == Job::Take { entity })
            {
                continue;
            }
            let Some(e) = self.entities.get(entity).copied() else {
                continue;
            };
            let Some(net) = net_of(middle(&e)) else {
                continue;
            };
            let Some(d) = idle(&self.fleet, net, middle(&e)) else {
                continue;
            };
            self.fleet.drones[d].job = Job::Take { entity };
            handed += 1;
        }
        // Requester chests that want more than they have and are getting.
        let mut wants: Vec<(EntityId, (i64, i64), Item, u32)> = Vec::new();
        for (id, e) in self.entities.iter() {
            if let Some(State::Chest(c)) = &self.states[id.0 as usize] {
                if let Some((item, want)) = c.request {
                    let have = c.count(item) + c.incoming;
                    if have < want {
                        wants.push((id, middle(e), item, want - have));
                    }
                }
            }
        }
        for (to, at, item, need) in wants {
            if handed >= DISPATCH_MAX {
                return;
            }
            let Some(net) = net_of(at) else { continue };
            // The nearest provider in the network with some, or the hub.
            let mut best: Option<(i64, EntityId, (i64, i64), u32)> = None;
            for (id, e) in self.entities.iter() {
                let mid = middle(e);
                let has = match (
                    &self.states[id.0 as usize],
                    &self.tables.buildings[e.kind as usize].role,
                ) {
                    (
                        Some(State::Chest(c)),
                        Role::Chest {
                            requester: false, ..
                        },
                    ) => c.count(item),
                    (Some(State::Hub), _) => self.stock[item as usize],
                    _ => 0,
                };
                if has == 0 || net_of(mid) != Some(net) {
                    continue;
                }
                let d = dist(mid, at);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, id, mid, has));
                }
            }
            let Some((_, from, from_at, has)) = best else {
                continue;
            };
            let Some(d) = idle(&self.fleet, net, from_at) else {
                continue;
            };
            let n = need.min(CARRY).min(has);
            match &mut self.states[from.0 as usize] {
                Some(State::Chest(c)) => {
                    c.take(item, n);
                }
                _ => self.stock[item as usize] -= n,
            }
            if let Some(State::Chest(c)) = &mut self.states[to.0 as usize] {
                c.incoming += n;
            }
            self.fleet.drones[d].job = Job::Carry {
                from,
                to,
                item,
                n,
                picked: false,
            };
            handed += 1;
        }
    }

    /// Where a drone's job takes it, if anywhere still stands.
    fn target(&self, d: &Drone) -> Option<(i64, i64)> {
        let of = |id: EntityId| self.entities.get(id).map(middle);
        match &d.job {
            Job::Rest => of(d.home),
            Job::Carry {
                from, to, picked, ..
            } => of(if *picked { *to } else { *from }),
            Job::Build { at } => {
                let g = self.fleet.ghosts.iter().find(|g| g.pos == *at)?;
                Some(tile_middle(
                    g.pos,
                    self.tables.buildings[g.kind as usize].size,
                ))
            }
            Job::Take { entity } => of(*entity),
        }
    }

    /// Give back what a dropped job holds: its items, or a ghost's cost.
    fn drop_job(&mut self, job: &Job) {
        match *job {
            Job::Carry { to, item, n, .. } => {
                self.stock[item as usize] += n;
                if let Some(Some(State::Chest(c))) = self.states.get_mut(to.0 as usize) {
                    c.incoming = c.incoming.saturating_sub(n);
                }
            }
            Job::Build { at } => {
                if let Some(g) = self.fleet.ghosts.iter_mut().find(|g| g.pos == at) {
                    g.coming = false;
                    let cost = self.tables.buildings[g.kind as usize].cost.clone();
                    for (i, n) in cost {
                        self.stock[i as usize] += n;
                    }
                }
            }
            Job::Rest | Job::Take { .. } => {}
        }
    }

    /// Where a drone bound for `target` stops to charge first, if it must:
    /// a trip longer than a battery hops from port to port along the
    /// network, each hop a port it can reach that is nearer the target.
    fn charge_stop(&self, ports: &[Port], d: &Drone, target: (i64, i64)) -> Option<EntityId> {
        let after = self
            .nearest_port(ports, d.home, target)
            .map_or(0, |p| dist(target, p.mid));
        let left = dist(d.at, target);
        if d.battery >= left + after {
            return None;
        }
        let net = ports.iter().find(|p| p.id == d.home)?.net;
        let hop = ports
            .iter()
            .filter(|p| p.net == net && dist(d.at, p.mid) <= d.battery)
            .filter(|p| dist(p.mid, target) + UNIT as i64 <= left)
            .min_by_key(|p| dist(p.mid, target));
        match hop {
            Some(p) => Some(p.id),
            // Full and no port on the way: go anyway, and limp if need be.
            None if d.battery >= BATTERY => None,
            None => self.nearest_port(ports, d.home, d.at).map(|p| p.id),
        }
    }

    /// The nearest port to `at`, in the same network as `home`.
    fn nearest_port(&self, ports: &[Port], home: EntityId, at: (i64, i64)) -> Option<Port> {
        let net = ports.iter().find(|p| p.id == home)?.net;
        ports
            .iter()
            .filter(|p| p.net == net)
            .min_by_key(|p| dist(p.mid, at))
            .copied()
    }

    /// Move every drone a tick on, charge those at ports, and finish
    /// the jobs of those that arrive.
    pub(crate) fn step_drones(&mut self) {
        if self.fleet.drones.is_empty() {
            return;
        }
        if self.tick % DISPATCH_EVERY == 0 {
            self.dispatch();
        }
        let ports = self.ports();
        for p in &ports {
            if let Some(Some(State::Port { charging })) = self.states.get_mut(p.id.0 as usize) {
                *charging = false;
            }
        }
        // Builds and take-downs wait until every drone has moved: they
        // may add or remove ports, and drones with them.
        let mut finish = Vec::new();
        for k in 0..self.fleet.drones.len() {
            let mut d = self.fleet.drones[k].clone();
            let Some(target) = self.target(&d) else {
                // What it was going to is gone.
                let job = std::mem::replace(&mut d.job, Job::Rest);
                self.drop_job(&job);
                self.fleet.drones[k] = d;
                continue;
            };
            // Short of charge for the trip and a port after it: charge first.
            if d.charging.is_none() && d.at != target {
                d.charging = self.charge_stop(&ports, &d, target);
            }
            let resting = d.job == Job::Rest;
            let goal = match d.charging.or(resting.then_some(d.home)) {
                Some(port) => match self.entities.get(port) {
                    Some(e) => middle(e),
                    None => {
                        d.charging = None;
                        target
                    }
                },
                None => target,
            };
            let gap = dist(d.at, goal);
            if gap > 0 {
                // An empty battery still limps on, at a quarter of the speed.
                let speed = if d.battery > 0 { SPEED } else { SPEED / 4 };
                let step = speed.min(gap);
                d.at.0 += (goal.0 - d.at.0) * step / gap;
                d.at.1 += (goal.1 - d.at.1) * step / gap;
                d.battery = (d.battery - step).max(0);
                if dist(d.at, goal) <= 1 {
                    d.at = goal;
                }
            }
            if d.at != goal {
                self.fleet.drones[k] = d;
                continue;
            }
            // At a port: charge on its power until full.
            if let Some(port) = d.charging.or(resting.then_some(d.home)) {
                if d.battery < BATTERY {
                    let slot = port.0 as usize;
                    let powered = power::runs(self.tick, self.grids.sat(slot))
                        && self.grids.grid(slot).is_some();
                    if let Some(Some(State::Port { charging })) = self.states.get_mut(slot) {
                        *charging = true;
                    }
                    if powered {
                        d.battery = (d.battery + CHARGE).min(BATTERY);
                    }
                    if d.battery < BATTERY {
                        self.fleet.drones[k] = d;
                        continue;
                    }
                }
                d.charging = None;
                self.fleet.drones[k] = d;
                continue;
            }
            // At the job.
            let job = std::mem::replace(&mut d.job, Job::Rest);
            match job {
                Job::Carry {
                    from,
                    to,
                    item,
                    n,
                    picked: false,
                } => {
                    d.job = Job::Carry {
                        from,
                        to,
                        item,
                        n,
                        picked: true,
                    };
                }
                Job::Carry { to, item, n, .. } => {
                    if let Some(Some(State::Chest(c))) = self.states.get_mut(to.0 as usize) {
                        c.incoming = c.incoming.saturating_sub(n);
                        c.add(item, n);
                    }
                }
                Job::Build { .. } | Job::Take { .. } => finish.push(job),
                Job::Rest => {}
            }
            self.fleet.drones[k] = d;
        }
        for job in finish {
            match job {
                Job::Build { at } => self.build_ghost(at),
                Job::Take { entity } => {
                    self.fleet.marked.retain(|m| *m != entity);
                    self.apply(&Command::Remove { entity });
                }
                _ => {}
            }
        }
    }

    /// A drone has brought the building for the ghost at `at`: build it.
    fn build_ghost(&mut self, at: TilePos) {
        let Some(i) = self.fleet.ghosts.iter().position(|g| g.pos == at) else {
            return;
        };
        let g = self.fleet.ghosts.remove(i);
        let b = &self.tables.buildings[g.kind as usize];
        // It was paid for when the drone set off: place pays again.
        for &(i, n) in &b.cost.clone() {
            self.stock[i as usize] += n;
        }
        let command = Command::Place {
            building: b.id.clone(),
            pos: g.pos,
            dir: g.dir,
            recipe: g.recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
        };
        let _ = self.apply(&command);
    }

    /// A port was built: its drones start in it, charged.
    pub(crate) fn port_built(&mut self, id: EntityId, e: &Entity, drones: u8) {
        for _ in 0..drones {
            self.fleet.drones.push(Drone {
                home: id,
                at: middle(e),
                battery: BATTERY,
                job: Job::Rest,
                charging: None,
            });
        }
    }

    /// A port is gone: so are its drones, giving back what they carried.
    pub(crate) fn port_removed(&mut self, id: EntityId) {
        let gone: Vec<Job> = self
            .fleet
            .drones
            .iter()
            .filter(|d| d.home == id)
            .map(|d| d.job.clone())
            .collect();
        self.fleet.drones.retain(|d| d.home != id);
        for job in gone {
            self.drop_job(&job);
        }
        for d in &mut self.fleet.drones {
            if d.charging == Some(id) {
                d.charging = None;
            }
        }
    }

    /// Whether a ghost or entity stands anywhere on a footprint.
    pub(crate) fn ghost_on(&self, pos: TilePos, size: (u8, u8)) -> bool {
        self.fleet.ghosts.iter().any(|g| {
            let s = self.tables.buildings[g.kind as usize].size;
            overlap(g.pos, s, pos, size)
        })
    }
}

fn covers(p: &Port, at: (i64, i64)) -> bool {
    let r = p.reach * UNIT as i64;
    (p.mid.0 - at.0).abs() <= r && (p.mid.1 - at.1).abs() <= r
}

fn overlap(a: TilePos, sa: (u8, u8), b: TilePos, sb: (u8, u8)) -> bool {
    a.x < b.x + i32::from(sb.0)
        && b.x < a.x + i32::from(sa.0)
        && a.y < b.y + i32::from(sb.1)
        && b.y < a.y + i32::from(sa.1)
}

impl Sim {
    /// Plan a ghost (T50): `Place`'s checks, but nothing is spent yet.
    pub(crate) fn plan(
        &mut self,
        kind: u16,
        pos: TilePos,
        dir: Dir,
        recipe: Option<Recipe>,
    ) -> Result<(), mofact_core::command::CommandError> {
        use mofact_core::command::CommandError as E;
        let b = &self.tables.buildings[kind as usize];
        if !self.open(self.tables.building_lock[kind as usize]) {
            return Err(E::Locked);
        }
        if matches!(b.role, Role::Train { .. } | Role::Hub) {
            return Err(E::Blocked);
        }
        let size = b.size;
        let e = Entity {
            kind,
            pos,
            size,
            dir,
        };
        if !self.entities.is_free(&e) || self.ghost_on(pos, size) {
            return Err(E::Blocked);
        }
        self.ground_clear(pos, size)?;
        self.fleet.ghosts.push(Ghost {
            kind,
            pos,
            dir,
            recipe,
            coming: false,
        });
        Ok(())
    }

    /// Take the ghost at `pos` away; one already on its way is called back.
    pub(crate) fn unplan(&mut self, pos: TilePos) -> Option<Ghost> {
        let i = self.fleet.ghosts.iter().position(|g| g.pos == pos)?;
        let g = self.fleet.ghosts[i].clone();
        if g.coming {
            self.drop_job(&Job::Build { at: pos });
            for d in &mut self.fleet.drones {
                if d.job == (Job::Build { at: pos }) {
                    d.job = Job::Rest;
                }
            }
        }
        self.fleet.ghosts.remove(i);
        Some(g)
    }

    /// Ghosts a building now covers aren't wanted any more.
    pub(crate) fn clear_ghosts(&mut self, pos: TilePos, size: (u8, u8)) {
        let gone: Vec<TilePos> = self
            .fleet
            .ghosts
            .iter()
            .filter(|g| {
                !g.coming
                    && overlap(
                        g.pos,
                        self.tables.buildings[g.kind as usize].size,
                        pos,
                        size,
                    )
            })
            .map(|g| g.pos)
            .collect();
        for p in gone {
            self.unplan(p);
        }
    }
}

impl Sim {
    /// Drones in flight, ghosts and what is marked, for the snapshot.
    pub(crate) fn drone_views(&self, snap: &mut mofact_core::snapshot::Snapshot) {
        use mofact_core::snapshot::{DroneView, GhostView};
        let u = UNIT as f32;
        let tiles = |p: (i64, i64)| [p.0 as f32 / u, p.1 as f32 / u];
        for d in &self.fleet.drones {
            let home = self.entities.get(d.home).map(middle);
            if d.job == Job::Rest && Some(d.at) == home {
                continue; // resting in its port
            }
            let to = d
                .charging
                .and_then(|p| self.entities.get(p).map(middle))
                .or_else(|| self.target(d))
                .unwrap_or(d.at);
            let (cargo, building) = match &d.job {
                Job::Carry { item, .. } => (Some(self.tables.items[*item as usize].clone()), None),
                Job::Build { at } => (
                    None,
                    self.fleet
                        .ghosts
                        .iter()
                        .find(|g| g.pos == *at)
                        .map(|g| self.tables.buildings[g.kind as usize].id.clone()),
                ),
                _ => (None, None),
            };
            snap.drones.push(DroneView {
                at: tiles(d.at),
                to: tiles(to),
                cargo,
                building,
                battery: d.battery as f32 / BATTERY as f32,
            });
        }
        snap.ghosts = self
            .fleet
            .ghosts
            .iter()
            .map(|g| GhostView {
                building: self.tables.buildings[g.kind as usize].id.clone(),
                pos: g.pos,
                dir: g.dir,
                recipe: g.recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
                coming: g.coming,
            })
            .collect();
        snap.marked = self.fleet.marked.clone();
    }

    /// Drones a port holds, resting or out working (T49).
    pub fn port_drones(&self, port: EntityId) -> usize {
        self.fleet.drones.iter().filter(|d| d.home == port).count()
    }
}
