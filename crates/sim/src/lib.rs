//! Simulation lane: the deterministic factory simulation.
//!
//! Only the simulation changes the world. It runs on its own thread at
//! [`TICKS_PER_SECOND`](mofact_core::TICKS_PER_SECOND) through a
//! [`SimHandle`], applies queued [`Command`]s at the start of each tick and
//! publishes a [`Snapshot`] after it.
//!
//! The rules are the phone prototype's, ported to fixed point so every
//! platform gets the same result: belts carry spaced items and side-load,
//! miners mine the ore under them, crafters take input on any side but the
//! one with the arrow and push output out of it, and the hub takes anything
//! into the player's stock.

pub use mofact_core as core;

mod clock;
pub mod entities;
mod history;
pub mod save;
mod tables;
mod thread;

pub use clock::Stepper;
pub use history::{History, UNDO_STEPS};
pub use tables::UNIT;
pub use thread::SimHandle;

use entities::{Entities, Entity};
use mofact_core::command::{Command, CommandError, CommandResult};
use mofact_core::content::Content;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{EntityId, ItemId};
use mofact_core::snapshot::{BeltItemView, EntityView, MachineState, ResearchView, Snapshot};
use mofact_core::Tick;
use std::collections::HashMap;
use tables::{ticks, Item, Recipe, Role, Tables};

/// Items a crafter holds in its output before it stops (the prototype's CAP).
const OUT_CAP: u32 = 5;
/// Minimum distance between items on a belt, in [`UNIT`]s (0.34 tiles).
const SPACING: u32 = UNIT * 34 / 100;
/// A full output for this long shows as blocked; no input for this long shows as starved.
const BLOCKED_AFTER: u32 = 48;
/// Packs of each kind a lab holds.
const LAB_CAP: u32 = 4;
const STARVED_AFTER: u32 = 90;

#[derive(Clone, Debug)]
enum State {
    Hub,
    /// A belt tile and the few items on it (at most three, given
    /// [`SPACING`]). The plan's T07 sketched gap-encoded lines spanning many
    /// tiles; per-tile lists already tick the R44 base (10,000 belts and
    /// 2,000 machines) in about 0.15 ms on desktop, well under its 1 ms
    /// target, so the simpler layout stays until a benchmark says otherwise.
    Belt {
        /// Front first: (item, position along the tile in UNITs).
        items: Vec<(Item, u32)>,
        last: Option<Item>,
        per_tick: u32,
    },
    Miner {
        prog: u32,
        out: Option<Item>,
        waiting: u32,
    },
    Crafter(Crafter),
    Lab(Lab),
}

/// A lab: packs waiting, and the unit it is working on, if any.
#[derive(Clone, Debug, Default)]
struct Lab {
    /// Packs held, by item; up to [`LAB_CAP`] of each.
    packs: Vec<(Item, u32)>,
    /// The research the current unit counts toward, and ticks into it.
    unit: Option<(u16, u32)>,
    idle: u32,
}

#[derive(Clone, Debug, Default)]
struct Crafter {
    recipe: Option<Recipe>,
    /// Chosen by the player; otherwise picked from the first input and cleared when empty.
    fixed: bool,
    inputs: Vec<u32>,
    out: u32,
    out_item: Option<Item>,
    prog: u32,
    working: bool,
    idle: u32,
    blocked: u32,
}

/// The simulated world.
#[derive(Default)]
pub struct Sim {
    tick: Tick,
    tables: Tables,
    entities: Entities,
    /// Parallel to entity slots; `None` for removed entities.
    states: Vec<Option<State>>,
    ore: HashMap<TilePos, Item>,
    stock: Vec<u32>,
    delivered: Vec<u64>,
    research: Research,
}

/// Research progress, kept per research so switching loses nothing.
#[derive(Clone, Debug, Default)]
struct Research {
    current: Option<u16>,
    units: Vec<u32>,
    done: Vec<bool>,
}

impl Sim {
    /// A world with no content, which refuses every command.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty world using `content`, which should already be checked by `mofact_content`.
    pub fn with_content(content: &Content) -> Self {
        let tables = Tables::new(content);
        let n = tables.items.len();
        let r = tables.research.len();
        Self {
            tables,
            stock: vec![0; n],
            delivered: vec![0; n],
            research: Research {
                current: None,
                units: vec![0; r],
                done: vec![false; r],
            },
            ..Self::default()
        }
    }

    /// Mark `id` done, as a scenario that starts later in the game does.
    pub fn complete_research(&mut self, id: &mofact_core::ids::ResearchId) {
        if let Some(r) = self.tables.research(id) {
            self.finish_research(r);
        }
    }

    pub fn research_done(&self, id: &mofact_core::ids::ResearchId) -> bool {
        self.tables
            .research(id)
            .is_some_and(|r| self.research.done[r as usize])
    }

    fn finish_research(&mut self, r: u16) {
        self.research.done[r as usize] = true;
        self.research.units[r as usize] = self.tables.research[r as usize].units;
        if self.research.current == Some(r) {
            self.research.current = None;
        }
    }

    fn open(&self, lock: Option<u16>) -> bool {
        lock.is_none_or(|r| self.research.done[r as usize])
    }

    fn recipe_open(&self, r: Recipe) -> bool {
        self.open(self.tables.recipe_lock[r as usize])
    }

    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Put ore of `item` under `pos`. Stands in for world generation until T14.
    pub fn set_ore(&mut self, pos: TilePos, item: &ItemId) {
        if let Some(i) = self.tables.item(item) {
            self.ore.insert(pos, i);
        }
    }

    pub fn add_stock(&mut self, item: &ItemId, n: u32) {
        if let Some(i) = self.tables.item(item) {
            self.stock[i as usize] += n;
        }
    }

    /// Set the stock of `item` outright, as when loading a save.
    pub fn set_stock(&mut self, item: &ItemId, n: u32) {
        if let Some(i) = self.tables.item(item) {
            self.stock[i as usize] = n;
        }
    }

    /// What an entity holds, for the inspect sheet: crafter inputs then output,
    /// a miner's waiting ore, or the items on a belt tile.
    pub fn inventory(&self, id: EntityId) -> Vec<(ItemId, u32)> {
        let name = |i: Item| self.tables.items[i as usize].clone();
        match self.states.get(id.0 as usize).and_then(Option::as_ref) {
            Some(State::Crafter(c)) => {
                let mut v: Vec<_> = c.recipe.map_or(Vec::new(), |r| {
                    let def = &self.tables.recipes[r as usize];
                    def.inputs
                        .iter()
                        .zip(&c.inputs)
                        .map(|(&(i, _), &n)| (name(i), n))
                        .collect()
                });
                if let (Some(i), true) = (c.out_item, c.out > 0) {
                    v.push((name(i), c.out));
                }
                v
            }
            Some(State::Miner { out: Some(i), .. }) => vec![(name(*i), 1)],
            Some(State::Belt { items, .. }) => {
                let mut v: Vec<(ItemId, u32)> = Vec::new();
                for &(i, _) in items {
                    match v
                        .iter_mut()
                        .find(|(id, _)| *id == self.tables.items[i as usize])
                    {
                        Some((_, n)) => *n += 1,
                        None => v.push((name(i), 1)),
                    }
                }
                v
            }
            _ => Vec::new(),
        }
    }

    pub fn stock(&self, item: &ItemId) -> u32 {
        self.tables.item(item).map_or(0, |i| self.stock[i as usize])
    }

    /// Everything ever delivered to the hub, counting items since spent.
    pub fn delivered(&self, item: &ItemId) -> u64 {
        self.tables
            .item(item)
            .map_or(0, |i| self.delivered[i as usize])
    }

    pub fn entity_at(&self, pos: TilePos) -> Option<EntityId> {
        self.entities.at(pos)
    }

    /// Apply one command. A refused command changes nothing.
    pub fn apply(&mut self, command: &Command) -> CommandResult {
        match self.try_apply(command) {
            Ok((undo, created)) => CommandResult::Applied { undo, created },
            Err(e) => CommandResult::Refused(e),
        }
    }

    fn try_apply(
        &mut self,
        command: &Command,
    ) -> Result<(Vec<Command>, Option<EntityId>), CommandError> {
        match command {
            Command::Place {
                building,
                pos,
                dir,
                recipe,
            } => {
                let kind = self
                    .tables
                    .building(building)
                    .ok_or(CommandError::UnknownContent)?;
                if !self.open(self.tables.building_lock[kind as usize]) {
                    return Err(CommandError::Locked);
                }
                let b = &self.tables.buildings[kind as usize];
                let recipe = match recipe {
                    Some(r) => Some(self.crafter_recipe(kind, r)?),
                    None => None,
                };
                let entity = Entity {
                    kind,
                    pos: *pos,
                    size: b.size,
                    dir: *dir,
                };
                if !self.entities.is_free(&entity) {
                    return Err(CommandError::Blocked);
                }
                if matches!(b.role, Role::Miner { .. }) && !self.ore.contains_key(pos) {
                    return Err(CommandError::Blocked);
                }
                if b.cost.iter().any(|&(i, n)| self.stock[i as usize] < n) {
                    return Err(CommandError::CannotAfford);
                }
                for &(i, n) in &b.cost {
                    self.stock[i as usize] -= n;
                }
                let state = self.initial_state(kind, recipe);
                let id = self.entities.insert(entity).expect("footprint checked");
                self.states.resize(id.0 as usize + 1, None);
                self.states[id.0 as usize] = Some(state);
                Ok((vec![Command::Remove { entity: id }], Some(id)))
            }
            Command::Remove { entity } => {
                let e = *self
                    .entities
                    .get(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                let b = &self.tables.buildings[e.kind as usize];
                if matches!(b.role, Role::Hub) {
                    return Err(CommandError::Locked);
                }
                let recipe = match &self.states[entity.0 as usize] {
                    Some(State::Crafter(c)) if c.fixed => c.recipe,
                    _ => None,
                };
                for &(i, n) in &b.cost {
                    self.stock[i as usize] += n;
                }
                let undo = Command::Place {
                    building: b.id.clone(),
                    pos: e.pos,
                    dir: e.dir,
                    recipe: recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
                };
                self.entities.remove(*entity);
                self.states[entity.0 as usize] = None;
                Ok((vec![undo], None))
            }
            Command::Rotate { entity, dir } => {
                let e = self
                    .entities
                    .get_mut(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                let old = std::mem::replace(&mut e.dir, *dir);
                Ok((
                    vec![Command::Rotate {
                        entity: *entity,
                        dir: old,
                    }],
                    None,
                ))
            }
            Command::SetRecipe { entity, recipe } => {
                let kind = self
                    .entities
                    .get(*entity)
                    .ok_or(CommandError::NoSuchEntity)?
                    .kind;
                let new = match recipe {
                    Some(r) => Some(self.crafter_recipe(kind, r)?),
                    None => None,
                };
                let Some(State::Crafter(c)) = &mut self.states[entity.0 as usize] else {
                    return Err(CommandError::UnknownContent);
                };
                let old = if c.fixed { c.recipe } else { None };
                // As in the prototype, switching recipe discards what was loaded for the old one.
                *c = Crafter {
                    fixed: new.is_some(),
                    out: c.out,
                    out_item: c.out_item,
                    ..Crafter::default()
                };
                c.recipe = new;
                c.inputs = new.map_or(Vec::new(), |r| {
                    vec![0; self.tables.recipes[r as usize].inputs.len()]
                });
                let undo = Command::SetRecipe {
                    entity: *entity,
                    recipe: old.map(|r| self.tables.recipes[r as usize].id.clone()),
                };
                Ok((vec![undo], None))
            }
            Command::SetResearch { research } => {
                let new = match research {
                    Some(id) => {
                        let r = self
                            .tables
                            .research(id)
                            .ok_or(CommandError::UnknownContent)?;
                        let def = &self.tables.research[r as usize];
                        let ready = def.requires.iter().all(|&q| self.research.done[q as usize]);
                        if self.research.done[r as usize] || !ready {
                            return Err(CommandError::Locked);
                        }
                        Some(r)
                    }
                    None => None,
                };
                let old = std::mem::replace(&mut self.research.current, new);
                let undo = Command::SetResearch {
                    research: old.map(|r| self.tables.research[r as usize].id.clone()),
                };
                Ok((vec![undo], None))
            }
        }
    }

    fn crafter_recipe(
        &self,
        kind: u16,
        id: &mofact_core::ids::RecipeId,
    ) -> Result<Recipe, CommandError> {
        let r = self.tables.recipe(id).ok_or(CommandError::UnknownContent)?;
        match &self.tables.buildings[kind as usize].role {
            Role::Crafter { recipes, .. } if recipes.contains(&r) => {
                if self.recipe_open(r) {
                    Ok(r)
                } else {
                    Err(CommandError::Locked)
                }
            }
            _ => Err(CommandError::UnknownContent),
        }
    }

    fn initial_state(&self, kind: u16, recipe: Option<Recipe>) -> State {
        match &self.tables.buildings[kind as usize].role {
            Role::Hub => State::Hub,
            Role::Belt { per_tick } => State::Belt {
                items: Vec::new(),
                last: None,
                per_tick: *per_tick,
            },
            Role::Miner { .. } => State::Miner {
                prog: 0,
                out: None,
                waiting: 0,
            },
            Role::Crafter { .. } => State::Crafter(Crafter {
                recipe,
                fixed: recipe.is_some(),
                inputs: recipe.map_or(Vec::new(), |r| {
                    vec![0; self.tables.recipes[r as usize].inputs.len()]
                }),
                ..Crafter::default()
            }),
            Role::Lab { .. } => State::Lab(Lab::default()),
        }
    }

    /// Advance the world by one tick. Entities step in id order, so the result is deterministic.
    pub fn step(&mut self) {
        self.tick += 1;
        for i in 0..self.states.len() {
            let Some(mut state) = self.states[i].take() else {
                continue;
            };
            let e = *self
                .entities
                .get(EntityId(i as u32))
                .expect("state implies entity");
            self.step_one(&e, &mut state);
            self.states[i] = Some(state);
        }
    }

    fn step_one(&mut self, e: &Entity, state: &mut State) {
        let front = e.pos.step(e.dir);
        match state {
            State::Hub => {}
            State::Belt {
                items, per_tick, ..
            } => {
                let mut limit = UNIT;
                for (_, pos) in items.iter_mut() {
                    *pos = (*pos + *per_tick).min(limit).max(*pos);
                    limit = pos.saturating_sub(SPACING);
                }
                if let Some(&(item, pos)) = items.first() {
                    if pos >= UNIT && self.push(front, item, e.dir) {
                        items.remove(0);
                    }
                }
            }
            State::Miner { prog, out, waiting } => {
                let Role::Miner { ticks } = self.tables.buildings[e.kind as usize].role else {
                    return;
                };
                if out.is_none() {
                    *waiting = 0;
                    *prog += 1;
                    if *prog >= ticks {
                        *prog = 0;
                        *out = self.ore.get(&e.pos).copied();
                    }
                } else {
                    *waiting += 1;
                }
                if let Some(item) = *out {
                    if self.push(front, item, e.dir) {
                        *out = None;
                    }
                }
            }
            State::Crafter(c) => {
                let speed = match &self.tables.buildings[e.kind as usize].role {
                    Role::Crafter { speed, .. } => *speed,
                    _ => 1.0,
                };
                if let Some(r) = c.recipe {
                    let def = &self.tables.recipes[r as usize];
                    let (out_item, out_n) = def.outputs[0];
                    let ready = def
                        .inputs
                        .iter()
                        .zip(&c.inputs)
                        .all(|(&(_, need), &have)| have >= need);
                    let fits = c.out + out_n <= OUT_CAP + 1
                        && (c.out == 0 || c.out_item == Some(out_item));
                    if !c.working && ready && fits {
                        for (have, &(_, need)) in c.inputs.iter_mut().zip(&def.inputs) {
                            *have -= need;
                        }
                        c.working = true;
                        c.prog = 0;
                    }
                    if c.working {
                        c.prog += 1;
                        c.idle = 0;
                        if c.prog >= ticks(def.seconds / speed) {
                            c.working = false;
                            c.out += out_n;
                            c.out_item = Some(out_item);
                        }
                    } else {
                        c.idle += 1;
                    }
                } else {
                    c.idle += 1;
                }
                c.blocked = if c.out >= OUT_CAP { c.blocked + 1 } else { 0 };
                if let (true, Some(item)) = (c.out > 0, c.out_item) {
                    if self.push(front, item, e.dir) {
                        c.out -= 1;
                    }
                }
                if !c.fixed && !c.working && c.out == 0 && c.inputs.iter().all(|&n| n == 0) {
                    c.recipe = None;
                    c.inputs.clear();
                }
            }
            State::Lab(lab) => {
                let speed = match self.tables.buildings[e.kind as usize].role {
                    Role::Lab { speed } => speed,
                    _ => 1.0,
                };
                if lab.unit.is_none() {
                    if let Some(r) = self.research.current {
                        let cost = &self.tables.research[r as usize].cost;
                        let have =
                            |item: Item| lab.packs.iter().find(|p| p.0 == item).map_or(0, |p| p.1);
                        if cost.iter().all(|&(i, n)| have(i) >= n) {
                            for &(i, n) in cost {
                                if let Some(p) = lab.packs.iter_mut().find(|p| p.0 == i) {
                                    p.1 -= n;
                                }
                            }
                            lab.unit = Some((r, 0));
                        }
                    }
                }
                match &mut lab.unit {
                    Some((r, prog)) => {
                        lab.idle = 0;
                        *prog += 1;
                        let def = &self.tables.research[*r as usize];
                        if *prog >= ticks(def.seconds / speed) {
                            let r = *r as usize;
                            lab.unit = None;
                            if !self.research.done[r] {
                                self.research.units[r] += 1;
                                if self.research.units[r] >= self.tables.research[r].units {
                                    self.finish_research(r as u16);
                                }
                            }
                        }
                    }
                    None => lab.idle += 1,
                }
            }
        }
    }

    /// Put one `item` straight into `entity`'s input, as a belt behind it
    /// would. False if it is full or doesn't take that item.
    pub fn feed(&mut self, entity: EntityId, item: &ItemId) -> bool {
        let (Some(e), Some(item)) = (self.entities.get(entity).copied(), self.tables.item(item))
        else {
            return false;
        };
        self.push(e.pos, item, e.dir)
    }

    /// Hand `item`, travelling in `dir`, to whatever is on `tile`.
    fn push(&mut self, tile: TilePos, item: Item, dir: Dir) -> bool {
        let Some(id) = self.entities.at(tile) else {
            return false;
        };
        let target = *self.entities.get(id).expect("indexed");
        let Some(state) = self.states[id.0 as usize].as_mut() else {
            return false;
        };
        match state {
            State::Hub => {
                self.stock[item as usize] += 1;
                self.delivered[item as usize] += 1;
                true
            }
            State::Belt { items, last, .. } => {
                if target.dir == dir.opposite() {
                    return false;
                }
                // Straight on enters at the start; from the side, at the middle.
                let at = if target.dir == dir { 0 } else { UNIT / 2 };
                if items.iter().any(|&(_, p)| p.abs_diff(at) < SPACING) {
                    return false;
                }
                let i = items.partition_point(|&(_, p)| p > at);
                items.insert(i, (item, at));
                *last = Some(item);
                true
            }
            State::Miner { .. } => false,
            State::Lab(lab) => {
                if !self.tables.packs.contains(&item) {
                    return false;
                }
                match lab.packs.iter_mut().find(|p| p.0 == item) {
                    Some(p) if p.1 >= LAB_CAP => false,
                    Some(p) => {
                        p.1 += 1;
                        true
                    }
                    None => {
                        lab.packs.push((item, 1));
                        true
                    }
                }
            }
            State::Crafter(c) => {
                if dir.opposite() == target.dir {
                    return false; // the output face
                }
                if c.recipe.is_none() {
                    let Role::Crafter { recipes, .. } =
                        &self.tables.buildings[target.kind as usize].role
                    else {
                        return false;
                    };
                    let auto = recipes.iter().copied().find(|&r| {
                        let ins = &self.tables.recipes[r as usize].inputs;
                        let lock = self.tables.recipe_lock[r as usize];
                        let open = lock.is_none_or(|q| self.research.done[q as usize]);
                        ins.len() == 1 && ins[0].0 == item && open
                    });
                    let Some(r) = auto else { return false };
                    c.recipe = Some(r);
                    c.inputs = vec![0];
                }
                let def = &self.tables.recipes[c.recipe.expect("set above") as usize];
                let Some(slot) = def.inputs.iter().position(|&(i, _)| i == item) else {
                    return false;
                };
                let need = def.inputs[slot].1;
                let cap = if c.fixed {
                    need * 2
                } else {
                    (need * 2).max(OUT_CAP)
                };
                if c.inputs[slot] >= cap {
                    return false;
                }
                c.inputs[slot] += 1;
                true
            }
        }
    }

    /// A read-only view of the world for the renderer and UI.
    pub fn snapshot(&self) -> Snapshot {
        let mut snap = Snapshot {
            tick: self.tick,
            entities: Vec::with_capacity(self.states.len()),
            // Belts hold up to four items; most hold one or two.
            belt_items: Vec::with_capacity(self.states.len() * 2),
            ..Snapshot::default()
        };
        for (id, e) in self.entities.iter() {
            let b = &self.tables.buildings[e.kind as usize];
            let (state, progress, recipe) = match self.states[id.0 as usize].as_ref() {
                Some(State::Belt { items, .. }) => {
                    for &(item, p) in items {
                        snap.belt_items.push(BeltItemView {
                            item: self.tables.items[item as usize].clone(),
                            tile: e.pos,
                            along: p as f32 / UNIT as f32,
                        });
                    }
                    (MachineState::Working, 0.0, None)
                }
                Some(State::Miner { prog, out, waiting }) => {
                    let Role::Miner { ticks } = b.role else {
                        unreachable!()
                    };
                    let st = if out.is_some() && *waiting > BLOCKED_AFTER {
                        MachineState::Blocked
                    } else {
                        MachineState::Working
                    };
                    (st, *prog as f32 / ticks as f32, None)
                }
                Some(State::Crafter(c)) => {
                    let st = if c.blocked > BLOCKED_AFTER {
                        MachineState::Blocked
                    } else if c.idle > STARVED_AFTER {
                        MachineState::Starved
                    } else if c.working || c.recipe.is_some() {
                        MachineState::Working
                    } else {
                        MachineState::Idle
                    };
                    let p = c.recipe.map_or(0.0, |r| {
                        let speed = match b.role {
                            Role::Crafter { speed, .. } => speed,
                            _ => 1.0,
                        };
                        c.prog as f32
                            / ticks(self.tables.recipes[r as usize].seconds / speed) as f32
                    });
                    (
                        st,
                        if c.working { p } else { 0.0 },
                        c.recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
                    )
                }
                Some(State::Lab(lab)) => {
                    let speed = match b.role {
                        Role::Lab { speed } => speed,
                        _ => 1.0,
                    };
                    match lab.unit {
                        Some((r, prog)) => {
                            let def = &self.tables.research[r as usize];
                            let p = prog as f32 / ticks(def.seconds / speed) as f32;
                            (MachineState::Working, p, None)
                        }
                        None if self.research.current.is_none() => (MachineState::Idle, 0.0, None),
                        None if lab.idle > STARVED_AFTER => (MachineState::Starved, 0.0, None),
                        None => (MachineState::Working, 0.0, None),
                    }
                }
                _ => (MachineState::Working, 0.0, None),
            };
            snap.entities.push(EntityView {
                id,
                building: b.id.clone(),
                pos: e.pos,
                dir: e.dir,
                recipe,
                ore: matches!(b.role, Role::Miner { .. })
                    .then(|| self.ore.get(&e.pos))
                    .flatten()
                    .map(|&i| self.tables.items[i as usize].clone()),
                state,
                progress,
            });
        }
        let mut stock: Vec<_> = self
            .stock
            .iter()
            .enumerate()
            .filter(|(_, &n)| n > 0)
            .map(|(i, &n)| (self.tables.items[i].clone(), n))
            .collect();
        stock.sort();
        snap.stock = stock;
        let t = &self.tables;
        snap.research = ResearchView {
            current: self
                .research
                .current
                .map(|r| t.research[r as usize].id.clone()),
            progress: self.research.current.map_or((0, 0), |r| {
                (
                    self.research.units[r as usize],
                    t.research[r as usize].units,
                )
            }),
            done: (0..t.research.len())
                .filter(|&r| self.research.done[r])
                .map(|r| t.research[r].id.clone())
                .collect(),
        };
        snap
    }
}

#[cfg(test)]
mod tests;
