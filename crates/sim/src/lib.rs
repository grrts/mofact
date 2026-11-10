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
mod drones;
pub mod entities;
mod fluids;
mod hash;
mod history;
mod jobs;
mod local;
mod orders;
mod power;
mod rails;
pub mod save;
mod tables;
#[cfg(not(target_arch = "wasm32"))]
mod thread;

pub use clock::Stepper;
pub use history::{History, UNDO_STEPS};
pub use local::LocalSim;
#[cfg(target_arch = "wasm32")]
pub use local::LocalSim as SimHandle;
pub use tables::UNIT;
#[cfg(not(target_arch = "wasm32"))]
pub use thread::SimHandle;

use entities::{Entities, Entity};
use fluids::Fluids;
use mofact_core::command::{Command, CommandError, CommandResult, Compare, Condition};
use mofact_core::content::{Bonus, Content, Repeat, SplitMode};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{EntityId, ItemId};
use mofact_core::snapshot::{
    BeltItemView, EntityView, FluidView, GoalView, GridView, MachineState, ResearchView, Snapshot,
    TrainState, TrainView,
};
use mofact_core::Tick;
use power::Grids;
use std::collections::HashMap;
use tables::{ticks, Goal, Item, Recipe, Role, Tables, FLUID};

/// Items a crafter holds in its output before it stops (the prototype's CAP).
const OUT_CAP: u32 = 5;
/// Minimum distance between items on a belt, in [`UNIT`]s (0.34 tiles).
const SPACING: u32 = UNIT * 34 / 100;
/// A full output for this long shows as blocked; no input for this long shows as starved.
const BLOCKED_AFTER: u32 = 48;
/// Packs of each kind a lab holds.
const LAB_CAP: u32 = 4;
/// Three seconds: longer than a new game's furnace waits for its first ore
/// (about two), so a fresh factory never opens on a red mark.
const STARVED_AFTER: u32 = 3 * mofact_core::TICKS_PER_SECOND;
/// Fog cells round a building that it lifts the fog from (S3): the hub
/// clears the land a new game starts on, a radar a wider ring at once.
const REVEAL: i32 = 1;
const REVEAL_HUB: i32 = 5;
const REVEAL_RADAR: i32 = 3;

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
    /// Poles and generators hold nothing: their grids follow from where they stand.
    Pole,
    Generator,
    /// The item waiting to go out, which way out it tries first (counting
    /// left, then ahead or right, by the splitter's mode), and for a filter
    /// splitter the item it sends ahead.
    Splitter {
        held: Option<Item>,
        next: u8,
        filter: Option<Item>,
    },
    Tunnel(Tunnel),
    /// Pipes and tanks: their fluid is their segment's (see [`fluids`]).
    Pipe,
    /// One end of an underground pipe; it carries no items.
    PipeTunnel(Tunnel),
    Pump,
    Well,
    /// An engine, and the share of its full output it gave on the last
    /// tick, out of [`power::FULL`].
    Engine {
        share: u32,
    },
    /// A rail or signal: trains read the track from where it lies.
    Rail,
    /// A train stop, and the train loading or unloading there.
    Stop {
        docked: Option<EntityId>,
    },
    Train(rails::Train),
    /// The megaproject: the stage it is on, and what that stage has taken.
    Mega {
        stage: u32,
        have: Vec<u32>,
    },
    /// A radar (S3): ticks towards its next sweep, and the ring of cells
    /// round it it is charting; past its reach it has nothing left to do.
    /// A drone port, and whether a drone charges there this tick (T49).
    Port {
        charging: bool,
    },
    /// A provider or requester chest (T49).
    Chest(drones::Chest),
    /// A picker, taking from the store behind it while its condition holds.
    Picker(Picker),
    Radar {
        prog: u32,
        ring: i32,
        done: bool,
    },
}

/// A picker: the one item it takes (or whatever the store holds), when it
/// runs, ticks since it last handed an item on, and whether its condition
/// held when it last looked.
#[derive(Clone, Debug, Default)]
struct Picker {
    pick: Option<Item>,
    when: Option<(Item, Compare, u32)>,
    wait: u32,
    on: bool,
}

/// The most a picker's condition can count to.
pub const MAX_CONDITION: u32 = 99_999;

/// One end of an underground belt.
#[derive(Clone, Debug, Default)]
struct Tunnel {
    exit: bool,
    /// The other end, once paired.
    partner: Option<EntityId>,
    /// At an entrance, the items under way, front first, by position in
    /// [`UNIT`]s from the entrance tile to the exit tile.
    items: Vec<(Item, u32)>,
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
    /// What is waiting to leave, one slot per recipe output in order. A slot
    /// keeps its item until empty, so a recipe change drains the old output.
    outs: Vec<(Item, u32)>,
    prog: u32,
    working: bool,
    idle: u32,
    blocked: u32,
}

/// Ore a scenario places by hand: enough for hours of one drill.
pub const PLACED_ORE: u32 = 10_000;

/// The tiles a drill digs, by offset from it, in the order it digs them:
/// its own tile, then the four beside it, then the four corners. A drill
/// reaching round itself runs dry with its patch rather than its tile.
const DRILL_REACH: [(i32, i32); 9] = [
    (0, 0),
    (0, -1),
    (1, 0),
    (0, 1),
    (-1, 0),
    (1, -1),
    (1, 1),
    (-1, 1),
    (-1, -1),
];

/// The most pins the map holds.
pub const MAX_PINS: usize = 64;

/// The simulated world.
#[derive(Default)]
pub struct Sim {
    tick: Tick,
    tables: Tables,
    entities: Entities,
    /// Parallel to entity slots; `None` for removed entities.
    states: Vec<Option<State>>,
    /// Ore under drills and placed by hand, and how much each tile has left.
    ore: HashMap<TilePos, (Item, u32), hash::Fast>,
    /// Tiles mined out, sorted: generated ore there is gone for good.
    depleted: std::sync::Arc<Vec<TilePos>>,
    /// The generated map, when the game has one: ore a drill is placed on
    /// is copied into `ore` from here, so saves carry it.
    world: Option<mofact_world::WorldGen>,
    /// Tiles whose tree or boulder has been cleared, sorted.
    cleared: std::sync::Arc<Vec<TilePos>>,
    /// The player's pins on the world map, sorted.
    pins: std::sync::Arc<Vec<TilePos>>,
    /// The land explored (S3), sorted; the rest is under the fog of war.
    explored: std::sync::Arc<Vec<mofact_core::geom::FogCell>>,
    stock: Vec<u32>,
    delivered: Vec<u64>,
    research: Research,
    grids: Grids,
    fluids: Fluids,
    rails: rails::Rails,
    /// Drones, ghosts and what is marked for them (T49, T50).
    fleet: drones::Fleet,
    /// The goal the player is on: an index into the content's goals, which
    /// is their count once all are met.
    goal: u16,
    /// The hub's orders, once the goals are done.
    orders: orders::Orders,
    /// The rules the game was started with (see [`Rules`]).
    rules: Rules,
    /// The demo's last open tier: research of a higher tier is refused
    /// until the full game is bought. Not saved: the app sets it from the
    /// store each time it starts.
    demo: Option<u8>,
}

/// Research progress, kept per research so switching loses nothing. A
/// repeatable research (S2) counts every unit ever done in `units`, and is
/// `done` once its first level is.
#[derive(Clone, Debug, Default)]
struct Research {
    current: Option<u16>,
    units: Vec<u32>,
    done: Vec<bool>,
    /// What repeatable research has added, as extra speed: drills,
    /// crafters, labs. Worked out from `units`, never saved.
    boost: [f32; 3],
}

/// How the hub's orders run, as picked for a new game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrderPace {
    /// No orders: the factory grows at its own pace.
    Off = 0,
    /// Twice the time for each order.
    Relaxed = 1,
    /// The times the content gives.
    #[default]
    Normal = 2,
    /// Two thirds of the time, for half as much again in reward.
    Hard = 3,
}

impl OrderPace {
    pub const ALL: [OrderPace; 4] = [
        OrderPace::Off,
        OrderPace::Relaxed,
        OrderPace::Normal,
        OrderPace::Hard,
    ];

    /// The pace numbered `n`; an unknown number is normal.
    pub fn from_u8(n: u8) -> Self {
        Self::ALL.get(usize::from(n)).copied().unwrap_or_default()
    }

    /// An order's time, from the time the content gives.
    pub fn ticks(self, ticks: Tick) -> Tick {
        match self {
            OrderPace::Relaxed => ticks * 2,
            OrderPace::Hard => ticks * 2 / 3,
            _ => ticks,
        }
    }

    /// An order's reward, from the reward it would pay at normal pace.
    pub fn reward(self, n: u32) -> u32 {
        match self {
            OrderPace::Hard => n + n / 2,
            _ => n,
        }
    }
}

/// The rules a game was started with, picked on the new-game form and
/// kept in its save.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    /// Whether drills use up the ore they dig. Off, a patch lasts forever.
    pub ore_runs_out: bool,
    /// How the hub's orders run.
    pub orders: OrderPace,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            ore_runs_out: true,
            orders: OrderPace::Normal,
        }
    }
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
                boost: [0.0; 3],
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

    /// The rules this game was started with.
    pub fn rules(&self) -> Rules {
        self.rules
    }

    /// Play under `rules`, as a new game set up on the form does. With
    /// orders turned off, an order open now is dropped.
    pub fn set_rules(&mut self, rules: Rules) {
        self.rules = rules;
        if rules.orders == OrderPace::Off {
            self.orders.open = None;
        }
    }

    /// Gate research above `tier` behind the full game, or open it all
    /// with `None`. Research already under way above the gate stops.
    pub fn set_demo(&mut self, tier: Option<u8>) {
        self.demo = tier;
        if let Some(r) = self.research.current {
            if self.gated(r) {
                self.research.current = None;
            }
        }
    }

    /// Whether research `r` is past the demo's gate.
    fn gated(&self, r: u16) -> bool {
        self.demo
            .is_some_and(|t| self.tables.research[r as usize].tier > t)
    }

    pub fn research_done(&self, id: &mofact_core::ids::ResearchId) -> bool {
        self.tables
            .research(id)
            .is_some_and(|r| self.research.done[r as usize])
    }

    fn finish_research(&mut self, r: u16) {
        let def = &self.tables.research[r as usize];
        let units = &mut self.research.units[r as usize];
        if def.repeat.is_some() {
            // A level more, and labs carry on with the next.
            let (level, _) = Repeat::level(def.units, *units);
            *units = Repeat::total(def.units, level + 1) as u32;
        } else {
            *units = def.units;
            if self.research.current == Some(r) {
                self.research.current = None;
            }
        }
        self.research.done[r as usize] = true;
        self.boost();
    }

    /// The levels done of repeatable research `r` (S2): 0 for other research.
    fn level(&self, r: usize) -> u32 {
        let def = &self.tables.research[r];
        match def.repeat {
            Some(_) => Repeat::level(def.units, self.research.units[r]).0,
            None => 0,
        }
    }

    /// Work out what repeatable research adds, after a level or a load.
    fn boost(&mut self) {
        let mut boost = [0.0; 3];
        for r in 0..self.tables.research.len() {
            if let Some(rep) = self.tables.research[r].repeat {
                let k = match rep.bonus {
                    Bonus::Mining => 0,
                    Bonus::Crafting => 1,
                    Bonus::Research => 2,
                };
                boost[k] += rep.per_level * self.level(r) as f32;
            }
        }
        self.research.boost = boost;
    }

    /// Ticks a drill takes per ore, with research's boost.
    fn mine_ticks(&self, ticks: u32) -> u32 {
        ((ticks as f32 / (1.0 + self.research.boost[0])).round() as u32).max(1)
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

    /// Put ore of `item` under `pos`, as a scenario places it by hand.
    pub fn set_ore(&mut self, pos: TilePos, item: &ItemId) {
        self.set_ore_amount(pos, item, PLACED_ORE);
    }

    /// Put `amount` ore of `item` under `pos`.
    pub fn set_ore_amount(&mut self, pos: TilePos, item: &ItemId, amount: u32) {
        if let Some(i) = self.tables.item(item) {
            self.ore.insert(pos, (i, amount.max(1)));
        }
    }

    /// How much ore is left under `pos`, if a drill or a scenario holds it.
    pub fn ore_left(&self, pos: TilePos) -> Option<u32> {
        self.ore.get(&pos).map(|&(_, n)| n)
    }

    /// Mine the map generated from `seed`, the same one the player sees.
    pub fn set_world(&mut self, seed: u64) {
        self.set_world_gen(seed, mofact_world::Preset::Standard);
    }

    /// Mine the map generated from `seed` under `preset` (S4).
    pub fn set_world_gen(&mut self, seed: u64, preset: mofact_world::Preset) {
        let n = self.tables.ores.len() as u8;
        self.world = (n > 0).then(|| mofact_world::WorldGen::new(seed, n).with_preset(preset));
    }

    /// The seed and preset of the map, if there is one. A loaded save
    /// brings its own, so the player finds the same map again.
    pub fn world_seed(&self) -> Option<(u64, mofact_world::Preset)> {
        self.world.map(|w| (w.seed, w.preset))
    }

    /// The tiles a drill at `pos` digs, in the order it digs them: its own
    /// tile first, then the eight around it.
    fn reach(pos: TilePos) -> impl Iterator<Item = TilePos> {
        DRILL_REACH
            .iter()
            .map(move |&(dx, dy)| TilePos::new(pos.x + dx, pos.y + dy))
    }

    /// Copy the generated ore around a drill at `pos` into `ore`, so the
    /// drill can dig it once its own tile runs dry: only the same ore as
    /// under the drill, and only what nothing has claimed or mined out.
    fn claim(&mut self, pos: TilePos) {
        let Some(&(item, _)) = self.ore.get(&pos) else {
            return;
        };
        for t in Self::reach(pos).skip(1) {
            if self.ore.contains_key(&t) {
                continue;
            }
            if let Some((i, n)) = self.ore_item(t) {
                if i == item {
                    self.ore.insert(t, (i, n));
                }
            }
        }
    }

    /// The ore a drill at `pos` still has in reach, and what it is.
    fn ore_in_reach(&self, pos: TilePos) -> (Option<Item>, u32) {
        let mut what = None;
        let mut left = 0u32;
        for t in Self::reach(pos) {
            if let Some(&(i, n)) = self.ore.get(&t) {
                what.get_or_insert(i);
                left = left.saturating_add(n);
            }
        }
        (what, left)
    }

    /// Dig one ore for the drill at `pos`: from under it while that lasts,
    /// then from the tiles around it. A tile is mined out when it runs dry.
    fn mine(&mut self, pos: TilePos) -> Option<Item> {
        let pos = Self::reach(pos).find(|t| self.ore.contains_key(t))?;
        let endless = !self.rules.ore_runs_out;
        let (item, left) = self.ore.get_mut(&pos)?;
        let item = *item;
        if endless {
            // Ore that never runs out: the drill digs without using it up.
            return Some(item);
        }
        *left -= 1;
        if *left == 0 {
            self.ore.remove(&pos);
            let depleted = std::sync::Arc::make_mut(&mut self.depleted);
            if let Err(at) = depleted.binary_search(&pos) {
                depleted.insert(at, pos);
            }
        }
        Some(item)
    }

    /// Whether a `size` footprint at `pos` stands on open ground: no water
    /// or cliff, and no tree or boulder that hasn't been cleared.
    fn ground_clear(&self, pos: TilePos, size: (u8, u8)) -> Result<(), CommandError> {
        let Some(world) = &self.world else {
            return Ok(());
        };
        let mut overgrown = false;
        for dy in 0..i32::from(size.1) {
            for dx in 0..i32::from(size.0) {
                let t = TilePos::new(pos.x + dx, pos.y + dy);
                match mofact_world::obstacle(world, t) {
                    Some(mofact_world::Obstacle::Terrain) => return Err(CommandError::Terrain),
                    Some(mofact_world::Obstacle::Tree) => {
                        overgrown |= self.cleared.binary_search(&t).is_err();
                    }
                    None => {}
                }
            }
        }
        if overgrown {
            Err(CommandError::Overgrown)
        } else {
            Ok(())
        }
    }

    /// The ore under `pos`: placed by hand, or else generated.
    pub fn ore_at(&self, pos: TilePos) -> Option<ItemId> {
        self.ore_item(pos)
            .map(|(i, _)| self.tables.items[i as usize].clone())
    }

    fn ore_item(&self, pos: TilePos) -> Option<(Item, u32)> {
        if let Some(&o) = self.ore.get(&pos) {
            return Some(o);
        }
        if self.depleted.binary_search(&pos).is_ok() {
            return None;
        }
        let ore = mofact_world::ore_at(self.world.as_ref()?, pos)?;
        let item = *self.tables.ores.get(usize::from(ore.resource))?;
        Some((item, ore.amount))
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
                // Fluids count in whole units.
                let units = |i: Item, n: u32| n / self.tables.scale(i);
                let mut v: Vec<_> = c.recipe.map_or(Vec::new(), |r| {
                    let def = &self.tables.recipes[r as usize];
                    def.inputs
                        .iter()
                        .zip(&c.inputs)
                        .map(|(&(i, _), &n)| (name(i), units(i, n)))
                        .collect()
                });
                for &(i, n) in c.outs.iter().filter(|o| o.1 > 0) {
                    v.push((name(i), units(i, n)));
                }
                v
            }
            Some(State::Miner { out: Some(i), .. }) => vec![(name(*i), 1)],
            Some(State::Chest(c)) => c.items.iter().map(|&(i, n)| (name(i), n)).collect(),
            // A pipe shows its whole segment: that's what flows through it.
            Some(State::Pipe | State::PipeTunnel(_)) => {
                let seg = self.fluids.of.get(id.0 as usize).copied().flatten();
                match seg.map(|s| self.fluids.segs[s as usize]) {
                    Some(fluids::Segment {
                        fluid: Some(f),
                        amount,
                        ..
                    }) => vec![(name(f), (amount / FLUID) as u32)],
                    _ => Vec::new(),
                }
            }
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
                // A signal or stop put on a rail (or the other way about) takes its place.
                if let Some(old) = self.entities.at(*pos).filter(|_| b.role.is_track()) {
                    let o = *self.entities.get(old).expect("indexed");
                    let ob = &self.tables.buildings[o.kind as usize];
                    if o.kind != kind && ob.role.is_track() && !self.train_on(*pos) {
                        let can = b.cost.iter().all(|&(i, n)| {
                            let back = ob.cost.iter().find(|c| c.0 == i).map_or(0, |c| c.1);
                            self.stock[i as usize] + back >= n
                        });
                        if !can {
                            return Err(CommandError::CannotAfford);
                        }
                        let (mut undo, _) = self.try_apply(&Command::Remove { entity: old })?;
                        let (placed, id) = self.try_apply(command)?;
                        undo.splice(0..0, placed);
                        return Ok((undo, id));
                    }
                }
                let train = matches!(b.role, Role::Train { .. });
                let entity = Entity {
                    kind,
                    pos: *pos,
                    // Trains ride on track rather than taking tiles of their own.
                    size: if train { (0, 0) } else { b.size },
                    dir: *dir,
                };
                if !self.entities.is_free(&entity) || (train && !self.train_fits(&entity)) {
                    return Err(CommandError::Blocked);
                }
                self.ground_clear(*pos, b.size)?;
                let mined = if matches!(b.role, Role::Miner { .. }) {
                    Some(self.ore_item(*pos).ok_or(CommandError::NoOre)?)
                } else {
                    None
                };
                if b.cost.iter().any(|&(i, n)| self.stock[i as usize] < n) {
                    return Err(CommandError::CannotAfford);
                }
                let cost = b.cost.clone();
                let track = b.role.is_track();
                let mut state = self.initial_state(kind, recipe);
                if let State::Train(t) = &mut state {
                    t.at = *pos;
                    t.heading = *dir;
                }
                let id = self.entities.insert(entity).expect("footprint checked");
                self.reveal_around(&entity);
                self.states.resize(id.0 as usize + 1, None);
                self.states[id.0 as usize] = Some(state);
                self.pair(id);
                if self.refluid(kind).is_err() {
                    // Joining two fluids: take it all back.
                    self.unpair(id);
                    self.entities.remove(id);
                    self.states[id.0 as usize] = None;
                    self.refluid(kind).expect("the world before held no mix");
                    return Err(CommandError::FluidsMix);
                }
                for &(i, n) in &cost {
                    self.stock[i as usize] -= n;
                }
                if let Some(ore) = mined {
                    self.ore.insert(*pos, ore);
                    self.claim(*pos);
                }
                self.regrid(kind);
                if track {
                    self.rebuild_blocks();
                }
                if train {
                    self.rails.trains.push(id);
                }
                let size = self.tables.buildings[kind as usize].size;
                if let Role::Port { drones, .. } = self.tables.buildings[kind as usize].role {
                    self.port_built(id, &entity, drones);
                }
                self.clear_ghosts(*pos, size);
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
                // Track can't be lifted from under a train.
                if b.role.is_track() && self.train_on(e.pos) {
                    return Err(CommandError::Blocked);
                }
                let recipe = match &self.states[entity.0 as usize] {
                    Some(State::Crafter(c)) if c.fixed => c.recipe,
                    _ => None,
                };
                for &(i, n) in &b.cost {
                    self.stock[i as usize] += n;
                }
                let mut undo = vec![Command::Place {
                    building: b.id.clone(),
                    pos: e.pos,
                    dir: e.dir,
                    recipe: recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
                }];
                let track = b.role.is_track();
                let port = matches!(b.role, Role::Port { .. });
                if let Some(State::Train(t)) = &self.states[entity.0 as usize] {
                    // Its cargo comes back too; undo gives back the train and its schedule.
                    for &(i, n) in &t.cargo {
                        self.stock[i as usize] += n;
                    }
                    if !t.schedule.is_empty() {
                        undo.push(Command::SetSchedule {
                            entity: *entity,
                            stops: t.schedule.clone(),
                        });
                    }
                    let at = t.at;
                    self.undock(at, *entity);
                    self.rails.trains.retain(|x| x != entity);
                }
                // Undo sets a filter splitter or warehouse to the same item again.
                if let Some(
                    State::Splitter {
                        filter: Some(i), ..
                    }
                    | State::Chest(drones::Chest {
                        filter: Some(i), ..
                    }),
                ) = &self.states[entity.0 as usize]
                {
                    undo.push(Command::SetFilter {
                        entity: *entity,
                        filter: Some(self.tables.items[*i as usize].clone()),
                    });
                }
                // Undo sets a picker the same way again.
                if let Some(State::Picker(p)) = &self.states[entity.0 as usize] {
                    if let Some(i) = p.pick {
                        undo.push(Command::SetFilter {
                            entity: *entity,
                            filter: Some(self.tables.items[i as usize].clone()),
                        });
                    }
                    if p.when.is_some() {
                        undo.push(Command::SetCondition {
                            entity: *entity,
                            when: self.condition(p),
                        });
                    }
                }
                // A chest's contents go back to the stock; undo asks for the same again.
                if let Some(State::Chest(c)) = &self.states[entity.0 as usize] {
                    for &(i, n) in &c.items {
                        self.stock[i as usize] += n;
                    }
                    if c.keep > 0 {
                        undo.push(Command::SetKeep {
                            entity: *entity,
                            keep: c.keep,
                        });
                    }
                    if let Some((i, n)) = c.request {
                        undo.push(Command::SetRequest {
                            entity: *entity,
                            request: Some((self.tables.items[i as usize].clone(), n)),
                        });
                    }
                }
                self.unpair(*entity);
                self.entities.remove(*entity);
                self.states[entity.0 as usize] = None;
                self.fleet.marked.retain(|m| m != entity);
                if port {
                    self.port_removed(*entity);
                }
                self.regrid(e.kind);
                // Splitting a segment never mixes fluids.
                let _ = self.refluid(e.kind);
                if track {
                    self.rebuild_blocks();
                }
                Ok((undo, None))
            }
            Command::Rotate { entity, dir } => {
                if let Some(State::Train(t)) = self
                    .states
                    .get_mut(entity.0 as usize)
                    .and_then(Option::as_mut)
                {
                    if t.to.is_some() {
                        return Err(CommandError::Blocked);
                    }
                    t.heading = *dir;
                    t.path.clear();
                }
                let e = self
                    .entities
                    .get_mut(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                let kind = e.kind;
                let old = std::mem::replace(&mut e.dir, *dir);
                // An underground end turned away from its pair finds a new one.
                let repair = |sim: &mut Sim| {
                    if sim.tunnel(*entity).is_some() {
                        sim.unpair(*entity);
                        if let Some(t) = sim.tunnel_mut(*entity) {
                            t.exit = false;
                        }
                        sim.pair(*entity);
                    }
                };
                repair(self);
                if self.refluid(kind).is_err() {
                    if let Some(e) = self.entities.get_mut(*entity) {
                        e.dir = old;
                    }
                    repair(self);
                    self.refluid(kind).expect("the world before held no mix");
                    return Err(CommandError::FluidsMix);
                }
                Ok((
                    vec![Command::Rotate {
                        entity: *entity,
                        dir: old,
                    }],
                    None,
                ))
            }
            Command::Clear { tile } => {
                let world = self.world.as_ref().ok_or(CommandError::NoSuchEntity)?;
                if mofact_world::obstacle(world, *tile) != Some(mofact_world::Obstacle::Tree) {
                    return Err(CommandError::NoSuchEntity);
                }
                let cleared = std::sync::Arc::make_mut(&mut self.cleared);
                let Err(at) = cleared.binary_search(tile) else {
                    return Err(CommandError::NoSuchEntity);
                };
                cleared.insert(at, *tile);
                Ok((vec![Command::Regrow { tile: *tile }], None))
            }
            Command::Regrow { tile } => {
                let Ok(at) = self.cleared.binary_search(tile) else {
                    return Err(CommandError::NoSuchEntity);
                };
                if self.entities.at(*tile).is_some() {
                    return Err(CommandError::Blocked);
                }
                std::sync::Arc::make_mut(&mut self.cleared).remove(at);
                Ok((vec![Command::Clear { tile: *tile }], None))
            }
            Command::Pin { tile } => {
                let pins = std::sync::Arc::make_mut(&mut self.pins);
                let Err(at) = pins.binary_search(tile) else {
                    return Err(CommandError::Blocked);
                };
                if pins.len() >= MAX_PINS {
                    return Err(CommandError::Blocked);
                }
                pins.insert(at, *tile);
                Ok((vec![Command::Unpin { tile: *tile }], None))
            }
            Command::Unpin { tile } => {
                let Ok(at) = self.pins.binary_search(tile) else {
                    return Err(CommandError::NoSuchEntity);
                };
                std::sync::Arc::make_mut(&mut self.pins).remove(at);
                Ok((vec![Command::Pin { tile: *tile }], None))
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
                    outs: std::mem::take(&mut c.outs),
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
                        let again = def.repeat.is_some();
                        if (self.research.done[r as usize] && !again) || !ready {
                            return Err(CommandError::Locked);
                        }
                        if self.gated(r) {
                            return Err(CommandError::FullGame);
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
            Command::SetRequest { entity, request } => {
                let e = *self
                    .entities
                    .get(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                let Role::Chest {
                    capacity,
                    requester: true,
                    ..
                } = self.tables.buildings[e.kind as usize].role
                else {
                    return Err(CommandError::UnknownContent);
                };
                let new = match request {
                    Some((item, n)) => Some((
                        self.tables.item(item).ok_or(CommandError::UnknownContent)?,
                        (*n).clamp(1, capacity),
                    )),
                    None => None,
                };
                let Some(Some(State::Chest(c))) = self.states.get_mut(entity.0 as usize) else {
                    return Err(CommandError::UnknownContent);
                };
                let old = std::mem::replace(&mut c.request, new);
                // What it held of something else goes back to the stock.
                let keep = new.map(|(i, _)| i);
                let mut back = Vec::new();
                c.items.retain(|&(i, n)| {
                    let stay = Some(i) == keep;
                    if !stay {
                        back.push((i, n));
                    }
                    stay
                });
                for (i, n) in back {
                    self.stock[i as usize] += n;
                }
                Ok((
                    vec![Command::SetRequest {
                        entity: *entity,
                        request: old.map(|(i, n)| (self.tables.items[i as usize].clone(), n)),
                    }],
                    None,
                ))
            }
            Command::SetFilter { entity, filter } => {
                let e = *self
                    .entities
                    .get(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                // A filter splitter's item ahead, or the one item a warehouse hands out.
                if !matches!(
                    self.tables.buildings[e.kind as usize].role,
                    Role::Splitter {
                        mode: SplitMode::Filter,
                    } | Role::Chest {
                        warehouse: true,
                        ..
                    } | Role::Picker { .. }
                ) {
                    return Err(CommandError::UnknownContent);
                }
                let new = match filter {
                    Some(item) => Some(self.tables.item(item).ok_or(CommandError::UnknownContent)?),
                    None => None,
                };
                let f = match self.states.get_mut(entity.0 as usize) {
                    Some(Some(State::Splitter { filter: f, .. })) => f,
                    Some(Some(State::Chest(c))) => &mut c.filter,
                    Some(Some(State::Picker(p))) => &mut p.pick,
                    _ => return Err(CommandError::UnknownContent),
                };
                let old = std::mem::replace(f, new);
                Ok((
                    vec![Command::SetFilter {
                        entity: *entity,
                        filter: old.map(|i| self.tables.items[i as usize].clone()),
                    }],
                    None,
                ))
            }
            Command::SetKeep { entity, keep } => {
                let e = *self
                    .entities
                    .get(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                let Role::Chest {
                    capacity,
                    warehouse: true,
                    ..
                } = self.tables.buildings[e.kind as usize].role
                else {
                    return Err(CommandError::UnknownContent);
                };
                let Some(Some(State::Chest(c))) = self.states.get_mut(entity.0 as usize) else {
                    return Err(CommandError::UnknownContent);
                };
                let old = std::mem::replace(&mut c.keep, (*keep).min(capacity));
                Ok((
                    vec![Command::SetKeep {
                        entity: *entity,
                        keep: old,
                    }],
                    None,
                ))
            }
            Command::SetCondition { entity, when } => {
                if self.entities.get(*entity).is_none() {
                    return Err(CommandError::NoSuchEntity);
                }
                let new = match when {
                    Some(c) => Some((
                        self.tables
                            .item(&c.item)
                            .ok_or(CommandError::UnknownContent)?,
                        c.compare,
                        c.amount.min(MAX_CONDITION),
                    )),
                    None => None,
                };
                let Some(Some(State::Picker(p))) = self.states.get_mut(entity.0 as usize) else {
                    return Err(CommandError::UnknownContent);
                };
                let old = std::mem::replace(&mut p.when, new);
                let undo = old.map(|(i, compare, amount)| Condition {
                    item: self.tables.items[i as usize].clone(),
                    compare,
                    amount,
                });
                Ok((
                    vec![Command::SetCondition {
                        entity: *entity,
                        when: undo,
                    }],
                    None,
                ))
            }
            Command::Plan {
                building,
                pos,
                dir,
                recipe,
            } => {
                let kind = self
                    .tables
                    .building(building)
                    .ok_or(CommandError::UnknownContent)?;
                let recipe = match recipe {
                    Some(r) => Some(self.crafter_recipe(kind, r)?),
                    None => None,
                };
                self.plan(kind, *pos, *dir, recipe)?;
                Ok((vec![Command::Unplan { pos: *pos }], None))
            }
            Command::Unplan { pos } => {
                let g = self.unplan(*pos).ok_or(CommandError::NoSuchEntity)?;
                Ok((
                    vec![Command::Plan {
                        building: self.tables.buildings[g.kind as usize].id.clone(),
                        pos: g.pos,
                        dir: g.dir,
                        recipe: g.recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
                    }],
                    None,
                ))
            }
            Command::Mark { entity, on } => {
                let e = *self
                    .entities
                    .get(*entity)
                    .ok_or(CommandError::NoSuchEntity)?;
                if matches!(self.tables.buildings[e.kind as usize].role, Role::Hub) {
                    return Err(CommandError::Locked);
                }
                let marked = self.fleet.marked.contains(entity);
                if marked == *on {
                    return Err(CommandError::Blocked);
                }
                if *on {
                    self.fleet.marked.push(*entity);
                } else {
                    self.fleet.marked.retain(|m| m != entity);
                }
                Ok((
                    vec![Command::Mark {
                        entity: *entity,
                        on: !*on,
                    }],
                    None,
                ))
            }
            Command::SetSchedule { entity, stops } => {
                if self.entities.get(*entity).is_none() {
                    return Err(CommandError::NoSuchEntity);
                }
                let Some(Some(State::Train(t))) = self.states.get_mut(entity.0 as usize) else {
                    return Err(CommandError::UnknownContent);
                };
                let old = std::mem::replace(&mut t.schedule, stops.clone());
                // A new schedule starts from its first stop; a train between
                // tiles finishes its step first.
                t.next = 0;
                t.path.clear();
                t.wait = 0;
                if t.to.is_none() {
                    t.phase = rails::Phase::Idle;
                }
                let at = t.at;
                self.undock(at, *entity);
                Ok((
                    vec![Command::SetSchedule {
                        entity: *entity,
                        stops: old,
                    }],
                    None,
                ))
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
            Role::Crafter { recipes, .. } => {
                // A machine with one recipe and nothing solid to choose it
                // by (a refinery) is set to that recipe from the start.
                let t = &self.tables;
                let recipe = recipe.or(match recipes[..] {
                    [only]
                        if t.recipes[only as usize]
                            .inputs
                            .iter()
                            .all(|&(i, _)| t.fluid[i as usize]) =>
                    {
                        Some(only)
                    }
                    _ => None,
                });
                State::Crafter(Crafter {
                    recipe,
                    fixed: recipe.is_some(),
                    inputs: recipe
                        .map_or(Vec::new(), |r| vec![0; t.recipes[r as usize].inputs.len()]),
                    ..Crafter::default()
                })
            }
            Role::Lab { .. } => State::Lab(Lab::default()),
            Role::Mega { stages, .. } => State::Mega {
                stage: 0,
                have: vec![0; stages.first().map_or(0, Vec::len)],
            },
            Role::Pole { .. } => State::Pole,
            Role::Generator { .. } => State::Generator,
            Role::Splitter { .. } => State::Splitter {
                held: None,
                next: 0,
                filter: None,
            },
            Role::Underground { .. } => State::Tunnel(Tunnel::default()),
            Role::Pipe { .. } | Role::Tank { .. } => State::Pipe,
            Role::UndergroundPipe { .. } => State::PipeTunnel(Tunnel::default()),
            Role::Pump { .. } => State::Pump,
            Role::Well { .. } => State::Well,
            Role::Engine { .. } => State::Engine { share: 0 },
            Role::Rail { .. } => State::Rail,
            Role::Stop => State::Stop { docked: None },
            Role::Train { .. } => State::Train(rails::Train::default()),
            Role::Port { .. } => State::Port { charging: false },
            Role::Chest { .. } => State::Chest(drones::Chest::default()),
            Role::Picker { .. } => State::Picker(Picker::default()),
            Role::Radar { .. } => State::Radar {
                prog: 0,
                ring: 0,
                done: false,
            },
        }
    }

    /// Whether the fog has lifted from `cell` (S3).
    pub fn explored(&self, cell: mofact_core::geom::FogCell) -> bool {
        self.explored.binary_search(&cell).is_ok()
    }

    /// Lift the fog from `cell`; false if it was already explored.
    fn reveal(&mut self, cell: mofact_core::geom::FogCell) -> bool {
        let Err(at) = self.explored.binary_search(&cell) else {
            return false;
        };
        std::sync::Arc::make_mut(&mut self.explored).insert(at, cell);
        true
    }

    /// Lift the fog round a building: a few hundred tiles round the hub,
    /// so a new game opens on cleared land, and a cell round anything else.
    pub(crate) fn reveal_around(&mut self, e: &Entity) {
        let radius = match self.tables.buildings[e.kind as usize].role {
            Role::Hub => REVEAL_HUB,
            Role::Radar { .. } => REVEAL_RADAR,
            _ => REVEAL,
        };
        let (w, h) = (i32::from(e.size.0), i32::from(e.size.1));
        let mid = TilePos::new(e.pos.x + w / 2, e.pos.y + h / 2).fog();
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= radius * radius + radius {
                    self.reveal(mofact_core::geom::FogCell::new(mid.x + dx, mid.y + dy));
                }
            }
        }
    }

    /// Lift the fog a few cells round a building from a save made before
    /// the fog of war, whose player had seen the land round their factory.
    pub(crate) fn reveal_wide(&mut self, e: &Entity) {
        let mid = e.pos.fog();
        let r = REVEAL_RADAR;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r + r {
                    self.reveal(mofact_core::geom::FogCell::new(mid.x + dx, mid.y + dy));
                }
            }
        }
    }

    /// A radar's sweep: lift the fog from the nearest cell round it still
    /// hidden, working out ring by ring. Returns false once nothing within
    /// `reach` is left.
    fn sweep(&mut self, e: &Entity, ring: &mut i32, reach: i32) -> bool {
        let (w, h) = (i32::from(e.size.0), i32::from(e.size.1));
        let mid = TilePos::new(e.pos.x + w / 2, e.pos.y + h / 2).fog();
        while *ring <= reach {
            let r = *ring;
            let mut best: Option<(i32, mofact_core::geom::FogCell)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    // Only the ring's edge, inside the round reach.
                    let d = dx * dx + dy * dy;
                    if dx.abs().max(dy.abs()) != r || d > reach * reach + reach {
                        continue;
                    }
                    let cell = mofact_core::geom::FogCell::new(mid.x + dx, mid.y + dy);
                    if !self.explored(cell) && best.is_none_or(|(bd, bc)| (d, cell) < (bd, bc)) {
                        best = Some((d, cell));
                    }
                }
            }
            match best {
                Some((_, cell)) => {
                    self.reveal(cell);
                    return true;
                }
                None => *ring += 1,
            }
        }
        false
    }

    /// An underground belt or pipe end.
    fn tunnel(&self, id: EntityId) -> Option<&Tunnel> {
        match self.states.get(id.0 as usize)?.as_ref()? {
            State::Tunnel(t) | State::PipeTunnel(t) => Some(t),
            _ => None,
        }
    }

    fn tunnel_mut(&mut self, id: EntityId) -> Option<&mut Tunnel> {
        match self.states.get_mut(id.0 as usize)?.as_mut()? {
            State::Tunnel(t) | State::PipeTunnel(t) => Some(t),
            _ => None,
        }
    }

    /// Pair the underground belt end `id` with another of its kind facing
    /// the same way: an unpaired entrance behind it makes it an exit,
    /// otherwise an unpaired exit ahead of it makes it an entrance. The
    /// first end of its kind met on either side stops the search.
    fn pair(&mut self, id: EntityId) {
        let Some(e) = self.entities.get(id).copied() else {
            return;
        };
        let (Role::Underground { reach, .. } | Role::UndergroundPipe { reach, .. }) =
            self.tables.buildings[e.kind as usize].role
        else {
            return;
        };
        let find = |sim: &Sim, dir: Dir| {
            let mut tile = e.pos;
            for _ in 0..reach {
                tile = tile.step(dir);
                let Some(other) = sim.entities.at(tile) else {
                    continue;
                };
                let o = sim.entities.get(other).expect("indexed");
                if o.kind == e.kind && o.dir == e.dir {
                    let t = sim.tunnel(other)?;
                    return Some((other, t.exit, t.partner.is_none()));
                }
            }
            None
        };
        let (exit, partner) = match find(self, e.dir.opposite()) {
            Some((other, false, true)) => (true, Some(other)),
            _ => match find(self, e.dir) {
                Some((other, true, true)) => (false, Some(other)),
                _ => (false, None),
            },
        };
        if let Some(other) = partner {
            if let Some(t) = self.tunnel_mut(other) {
                t.partner = Some(id);
            }
        }
        if let Some(t) = self.tunnel_mut(id) {
            t.exit = exit;
            t.partner = partner;
        }
    }

    /// Unpair `id` from its other end, if it has one.
    fn unpair(&mut self, id: EntityId) {
        let partner = self.tunnel_mut(id).and_then(|t| t.partner.take());
        if let Some(t) = partner.and_then(|p| self.tunnel_mut(p)) {
            t.partner = None;
        }
    }

    /// How far items travel inside the entrance `e`: to its exit's tile, or
    /// one tile when it has none.
    fn tunnel_length(&self, e: &Entity, t: &Tunnel) -> u32 {
        let tiles = t.partner.and_then(|p| self.entities.get(p)).map_or(1, |x| {
            (x.pos.x - e.pos.x).abs().max((x.pos.y - e.pos.y).abs())
        });
        tiles.max(1) as u32 * UNIT
    }

    /// Rebuild the pipe segments if a building of `kind` is part of them.
    /// Fails, changing nothing, if that would mix two fluids.
    fn refluid(&mut self, kind: u16) -> Result<(), fluids::Mix> {
        if fluids::capacity(&self.tables.buildings[kind as usize].role).is_some() {
            let old = &self.fluids;
            self.fluids = self.build_fluids(|slot| old.share(slot))?;
        }
        Ok(())
    }

    /// The pipe segments as the buildings stand, each member bringing `content`.
    fn build_fluids(
        &self,
        content: impl Fn(usize) -> Option<(Item, u64)>,
    ) -> Result<Fluids, fluids::Mix> {
        Fluids::build(
            &self.tables,
            &self.entities,
            |id| match self.states.get(id.0 as usize)?.as_ref()? {
                State::PipeTunnel(t) => Some((t.exit, t.partner)),
                _ => None,
            },
            content,
        )
    }

    /// Rebuild the power grids if a building of `kind` changes them.
    fn regrid(&mut self, kind: u16) {
        let b = &self.tables.buildings[kind as usize];
        if b.power > 0
            || matches!(
                b.role,
                Role::Pole { .. } | Role::Generator { .. } | Role::Engine { .. }
            )
        {
            self.rebuild_grids();
        }
    }

    fn rebuild_grids(&mut self) {
        self.grids = Grids::build(&self.tables, &self.entities);
        self.settle_grids();
    }

    /// Whether a machine in `state` would draw power this tick.
    fn wants_power(state: &State) -> bool {
        match state {
            State::Miner { out, .. } => out.is_none(),
            State::Crafter(c) => c.working,
            State::Lab(lab) => lab.unit.is_some(),
            State::Pump | State::Well => true,
            State::Radar { done, .. } => !done,
            State::Port { charging } => *charging,
            _ => false,
        }
    }

    /// Add up what each grid is asked for and settle its satisfaction.
    fn settle_grids(&mut self) {
        if self.grids.grids.is_empty() {
            return;
        }
        for g in &mut self.grids.grids {
            g.demand = 0;
            g.supply = g.base;
        }
        for (id, e) in self.entities.iter() {
            let slot = id.0 as usize;
            // What each engine gave on the last tick.
            if let (Role::Engine { watts, .. }, Some(g), Some(State::Engine { share })) = (
                &self.tables.buildings[e.kind as usize].role,
                self.grids.grid(slot),
                self.states[slot].as_ref(),
            ) {
                self.grids.grids[g as usize].supply +=
                    u64::from(*watts) * u64::from(*share) / u64::from(power::FULL);
            }
            let watts = self.tables.buildings[e.kind as usize].power;
            if let (true, Some(g), Some(state)) =
                (watts > 0, self.grids.grid(slot), self.states[slot].as_ref())
            {
                if Self::wants_power(state) {
                    self.grids.grids[g as usize].demand += u64::from(watts);
                }
            }
        }
        for g in &mut self.grids.grids {
            g.settle();
            let short = g.demand.saturating_sub(g.base) * u64::from(power::FULL);
            g.load = short
                .checked_div(g.engines)
                .map_or(0, |l| l.min(u64::from(power::FULL)) as u32);
        }
    }

    /// Advance the world by one tick. Entities step in id order, so the result is deterministic.
    pub fn step(&mut self) {
        self.tick += 1;
        // Goals move on twice a second; nobody needs it sooner.
        if self.tick % 30 == 0 {
            self.check_goals();
            self.check_orders();
        }
        self.settle_grids();
        self.fluids.refill_budgets();
        for i in 0..self.states.len() {
            let Some(mut state) = self.states[i].take() else {
                continue;
            };
            let e = *self
                .entities
                .get(EntityId(i as u32))
                .expect("state implies entity");
            if let (
                State::Train(t),
                Role::Train {
                    per_tick,
                    capacity,
                    tank,
                },
            ) = (&mut state, &self.tables.buildings[e.kind as usize].role)
            {
                let (speed, capacity, tank) = (*per_tick, *capacity, *tank);
                self.step_train(EntityId(i as u32), t, capacity, speed, tank);
                self.states[i] = Some(state);
                continue;
            }
            let go = self.tables.buildings[e.kind as usize].power == 0
                || power::runs(self.tick, self.grids.sat(i));
            if let (State::Radar { prog, ring, done }, Role::Radar { reach, every }) =
                (&mut state, &self.tables.buildings[e.kind as usize].role)
            {
                let (reach, every) = (*reach, *every);
                if go && !*done {
                    *prog += 1;
                    if *prog >= every {
                        *prog = 0;
                        *done = !self.sweep(&e, ring, reach);
                    }
                }
                self.states[i] = Some(state);
                continue;
            }
            self.step_one(&e, &mut state, go);
            self.states[i] = Some(state);
        }
        self.step_drones();
    }

    /// Step one entity; `go` is false when its grid can't power it this tick.
    fn step_one(&mut self, e: &Entity, state: &mut State, go: bool) {
        let front = e.pos.step(e.dir);
        match state {
            State::Hub
            | State::Pole
            | State::Generator
            | State::Pipe
            | State::PipeTunnel(_)
            | State::Rail
            | State::Stop { .. }
            | State::Train(_)
            | State::Mega { .. }
            | State::Port { .. }
            | State::Radar { .. } => {}
            // A warehouse hands what it holds out of its arrow, onto any
            // tile along that side: in turn, or only the item it is set to.
            State::Chest(c)
                if matches!(
                    self.tables.buildings[e.kind as usize].role,
                    Role::Chest {
                        warehouse: true,
                        ..
                    }
                ) =>
            {
                if c.items.is_empty() {
                    return;
                }
                c.wait += 1;
                if c.wait < drones::WAREHOUSE_OUT_EVERY {
                    return;
                }
                // Only what it holds above its safety stock goes out.
                let keep = c.keep;
                let item = match c.filter {
                    Some(f) if c.count(f) > keep => f,
                    Some(_) => return,
                    None => match c.items.iter().find(|&&(_, n)| n > keep) {
                        Some(&(i, _)) => i,
                        None => return,
                    },
                };
                let side = e.dir;
                let out = fluids::edges(e)
                    .filter(|&(d, _)| d == side)
                    .map(|(_, n)| n)
                    .find(|&n| self.push(n, item, side));
                if out.is_some() {
                    c.take(item, 1);
                    c.wait = 0;
                    // The next item out is the next kind it holds.
                    if c.filter.is_none() {
                        if let Some(k) = c.items.iter().position(|&(i, _)| i == item) {
                            let held = c.items.remove(k);
                            c.items.push(held);
                        }
                    }
                }
            }
            // A picker takes from the store behind it onto what is ahead,
            // only while its condition holds.
            State::Picker(p) => {
                let Role::Picker { every } = self.tables.buildings[e.kind as usize].role else {
                    return;
                };
                p.wait = p.wait.saturating_add(1);
                if p.wait < every {
                    return;
                }
                let back = e.pos.step(e.dir.opposite());
                let (item, on) = self.pickable(back, p);
                p.on = on;
                if let (Some(item), true) = (item, on) {
                    if self.push(front, item, e.dir) {
                        self.take_from(back, item);
                        p.wait = 0;
                    }
                }
            }
            // A requester chest hands what drones bring out of its arrow.
            State::Chest(c) => {
                if c.request.is_none() || c.items.is_empty() {
                    return;
                }
                c.wait += 1;
                if c.wait >= drones::OUT_EVERY {
                    let item = c.items[0].0;
                    if self.push(front, item, e.dir) {
                        c.take(item, 1);
                        c.wait = 0;
                    }
                }
            }
            State::Pump => {
                let Role::Pump { per_tick } = self.tables.buildings[e.kind as usize].role else {
                    return;
                };
                let from = self.fluids.toward(&self.entities, e.pos, e.dir.opposite());
                let to = self.fluids.toward(&self.entities, e.pos, e.dir);
                if let (true, Some(a), Some(b)) = (go, from, to) {
                    let (a, b) = (a as usize, b as usize);
                    if let (Some(f), true) = (self.fluids.segs[a].fluid, a != b) {
                        let room = self.fluids.segs[b].room_for(f);
                        let n = self.fluids.segs[a].take(f, per_tick.min(room));
                        self.fluids.segs[b].give(f, n);
                    }
                }
            }
            State::Engine { share } => {
                let Role::Engine {
                    fluid, per_tick, ..
                } = self.tables.buildings[e.kind as usize].role
                else {
                    return;
                };
                // Burn for the share of output the grid needs, and give
                // only as much as there was fluid for.
                let slot = self.entities.at(e.pos).map_or(0, |id| id.0 as usize);
                let load = self
                    .grids
                    .grid(slot)
                    .map_or(0, |g| self.grids.grids[g as usize].load);
                let want = per_tick * u64::from(load) / u64::from(power::FULL);
                let got = self.fluids.pull(&self.entities, e, |_| true, fluid, want);
                *share = (got * u64::from(power::FULL) / per_tick) as u32;
            }
            State::Well => {
                if let (true, Role::Well { fluid, per_tick }) =
                    (go, &self.tables.buildings[e.kind as usize].role)
                {
                    self.fluids
                        .fill(&self.entities, e, |_| true, *fluid, *per_tick);
                }
            }
            State::Splitter { held, next, filter } => {
                let Some(item) = *held else {
                    return;
                };
                let Role::Splitter { mode } = self.tables.buildings[e.kind as usize].role else {
                    return;
                };
                let r = e.dir.rotate_cw();
                let l = r.opposite();
                // Ahead first, if this item goes ahead; then the ways it
                // takes in turn.
                let (first, turns): (Option<Dir>, &[Dir]) = match mode {
                    SplitMode::Even => (None, &[l, r]),
                    SplitMode::ThreeWay => (None, &[l, e.dir, r]),
                    SplitMode::Priority => (Some(e.dir), &[l, r]),
                    SplitMode::Filter if *filter == Some(item) => (Some(e.dir), &[]),
                    SplitMode::Filter => (None, &[l, r]),
                };
                if let Some(d) = first {
                    if self.push(e.pos.step(d), item, d) {
                        *held = None;
                        return;
                    }
                }
                let n = turns.len();
                for k in 0..n {
                    let i = (usize::from(*next) + k) % n;
                    let side = turns[i];
                    if self.push(e.pos.step(side), item, side) {
                        *held = None;
                        *next = ((i + 1) % n) as u8;
                        break;
                    }
                }
            }
            State::Tunnel(t) => {
                if t.exit {
                    return;
                }
                let Role::Underground { per_tick, .. } =
                    self.tables.buildings[e.kind as usize].role
                else {
                    return;
                };
                let length = self.tunnel_length(e, t);
                let mut limit = length;
                for (_, pos) in t.items.iter_mut() {
                    *pos = (*pos + per_tick).min(limit).max(*pos);
                    limit = pos.saturating_sub(SPACING);
                }
                let out = t.partner.and_then(|p| self.entities.get(p)).map(|x| x.pos);
                if let (Some(&(item, pos)), Some(exit)) = (t.items.first(), out) {
                    if pos >= length && self.push(exit.step(e.dir), item, e.dir) {
                        t.items.remove(0);
                    }
                }
            }
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
                let ticks = self.mine_ticks(ticks);
                if out.is_none() {
                    *waiting = 0;
                    *prog += u32::from(go);
                    if *prog >= ticks {
                        *prog = 0;
                        *out = self.mine(e.pos);
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
                } * (1.0 + self.research.boost[1]);
                let t = &self.tables;
                // A recipe's first output leaves by the front, a by-product
                // by the right, a third by the left; fluid inputs come in by
                // any side no fluid leaves by.
                let sides = [e.dir, e.dir.rotate_cw(), e.dir.opposite().rotate_cw()];
                let mut used = Vec::new();
                if let Some(r) = c.recipe {
                    for (k, o) in t.recipes[r as usize].outputs.iter().enumerate() {
                        if t.fluid[o.0 as usize] {
                            used.push(sides[k.min(sides.len() - 1)]);
                        }
                    }
                }
                let used = &used[..];
                // Any one full output stops the machine: a refinery whose tar
                // backs up stops making light oil too.
                let mut full = c
                    .outs
                    .iter()
                    .any(|&(i, n)| !t.fluid[i as usize] && n >= OUT_CAP);
                if let Some(r) = c.recipe {
                    let def = &t.recipes[r as usize];
                    for (have, &(item, need)) in c.inputs.iter_mut().zip(&def.inputs) {
                        let want = (need * t.scale(item) * 2).saturating_sub(*have);
                        if t.fluid[item as usize] && want > 0 {
                            let got = self.fluids.pull(
                                &self.entities,
                                e,
                                |d| !used.contains(&d),
                                item,
                                u64::from(want),
                            );
                            *have += got as u32;
                        }
                    }
                    let ready = def
                        .inputs
                        .iter()
                        .zip(&c.inputs)
                        .all(|(&(i, need), &have)| have >= need * t.scale(i));
                    let mut fits = true;
                    for (k, &(item, n)) in def.outputs.iter().enumerate() {
                        let n = n * t.scale(item);
                        let (held, have) = c.outs.get(k).copied().unwrap_or((item, 0));
                        let cap = if t.fluid[item as usize] {
                            full |= have >= n * 2;
                            n * 2
                        } else {
                            OUT_CAP + 1
                        };
                        // An output that can't take another craft's worth
                        // holds the machine up as surely as a full one.
                        let fit = have + n <= cap && (have == 0 || held == item);
                        full |= !fit;
                        fits &= fit;
                    }
                    if !c.working && ready && fits {
                        for (have, &(i, need)) in c.inputs.iter_mut().zip(&def.inputs) {
                            *have -= need * t.scale(i);
                        }
                        c.working = true;
                        c.prog = 0;
                    }
                    if c.working {
                        c.prog += u32::from(go);
                        c.idle = 0;
                        if c.prog >= ticks(def.seconds / speed) {
                            c.working = false;
                            if c.outs.len() < def.outputs.len() {
                                c.outs.resize(def.outputs.len(), (0, 0));
                            }
                            for (slot, &(item, n)) in c.outs.iter_mut().zip(&def.outputs) {
                                *slot = (item, slot.1 + n * t.scale(item));
                            }
                        }
                    } else {
                        c.idle += 1;
                    }
                } else {
                    c.idle += 1;
                }
                // Only a machine kept from crafting by its output counts as
                // blocked: one feeding a slower lab or belt that has just
                // taken a product is at work again, though its output still
                // holds the next few.
                c.blocked = if full && !c.working { c.blocked + 1 } else { 0 };
                // Each output goes out its own side, so a by-product can't
                // run back into an input line or onto the product's belt. One
                // solid leaves each side per tick, onto any tile along it.
                let mut pushed = [false; 3];
                for k in 0..c.outs.len() {
                    let (item, n) = c.outs[k];
                    let at = k.min(sides.len() - 1);
                    let side = sides[at];
                    if self.tables.fluid[item as usize] {
                        if n > 0 {
                            let got = self.fluids.fill(
                                &self.entities,
                                e,
                                |d| d == side,
                                item,
                                u64::from(n),
                            );
                            c.outs[k].1 -= got as u32;
                        }
                    } else if n > 0 && !pushed[at] {
                        let out = fluids::edges(e)
                            .filter(|&(d, _)| d == side)
                            .map(|(_, n)| n)
                            .find(|&n| self.push(n, item, side));
                        if out.is_some() {
                            c.outs[k].1 -= 1;
                            pushed[at] = true;
                        }
                    }
                }
                let empty = c.outs.iter().all(|o| o.1 == 0);
                if !c.fixed && !c.working && empty && c.inputs.iter().all(|&n| n == 0) {
                    c.recipe = None;
                    c.inputs.clear();
                    c.outs.clear();
                }
            }
            State::Lab(lab) => {
                let speed = match self.tables.buildings[e.kind as usize].role {
                    Role::Lab { speed } => speed,
                    _ => 1.0,
                } * (1.0 + self.research.boost[2]);
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
                        *prog += u32::from(go);
                        let def = &self.tables.research[*r as usize];
                        if *prog >= ticks(def.seconds / speed) {
                            let r = *r as usize;
                            lab.unit = None;
                            let def = &self.tables.research[r];
                            if def.repeat.is_some() {
                                self.research.units[r] += 1;
                                let (_, (into, _)) =
                                    Repeat::level(def.units, self.research.units[r]);
                                if into == 0 {
                                    self.research.done[r] = true;
                                    self.boost();
                                }
                            } else if !self.research.done[r] {
                                self.research.units[r] += 1;
                                if self.research.units[r] >= self.tables.research[r].units {
                                    self.finish_research(r as u16);
                                }
                            }
                        }
                    }
                    None => lab.idle += 1,
                }
                self.share_packs(e, lab);
            }
        }
    }

    /// A lab hands packs to the labs beside it, one at a time to whichever
    /// holds fewer, so a row of labs pools the belts brought to its sides:
    /// research needing more pack types than a lab has sides still runs.
    fn share_packs(&mut self, e: &Entity, lab: &mut Lab) {
        for d in Dir::ALL {
            let Some(id) = self.entities.at(e.pos.step(d)) else {
                continue;
            };
            let Some(State::Lab(next)) = self.states[id.0 as usize].as_mut() else {
                continue;
            };
            for p in lab.packs.iter_mut() {
                let theirs = next.packs.iter().position(|q| q.0 == p.0);
                let has = theirs.map_or(0, |k| next.packs[k].1);
                if p.1 > has + 1 && has < LAB_CAP {
                    p.1 -= 1;
                    match theirs {
                        Some(k) => next.packs[k].1 += 1,
                        None => next.packs.push((p.0, 1)),
                    }
                }
            }
        }
    }

    /// Whether the crafter in `slot` has a recipe that takes or makes a fluid.
    fn uses_fluid(&self, slot: usize) -> bool {
        match self.states.get(slot).and_then(Option::as_ref) {
            Some(State::Crafter(Crafter {
                recipe: Some(r), ..
            })) => {
                let def = &self.tables.recipes[*r as usize];
                let fluid = |&(i, _): &(Item, u32)| self.tables.fluid[i as usize];
                def.inputs.iter().any(fluid) || def.outputs.iter().any(fluid)
            }
            _ => false,
        }
    }

    /// The sides `e` carries fluid across, to draw its pipe joins: bits by
    /// direction, as in [`EntityView::links`].
    fn links(&self, id: EntityId, e: &Entity) -> u8 {
        let slot = id.0 as usize;
        match self.states.get(slot).and_then(Option::as_ref) {
            Some(State::Pump) => [e.dir, e.dir.opposite()]
                .into_iter()
                .filter(|&d| self.fluids.toward(&self.entities, e.pos, d).is_some())
                .fold(0, |m, d| m | fluids::bit(d)),
            Some(State::Well | State::Engine { .. }) => Dir::ALL
                .into_iter()
                .filter(|&d| self.fluids.toward(&self.entities, e.pos, d).is_some())
                .fold(0, |m, d| m | fluids::bit(d)),
            Some(State::Pipe | State::PipeTunnel(_)) => {
                let open = self.fluids.open.get(slot).copied().unwrap_or(0);
                let mut mask = 0;
                for (d, tile) in fluids::edges(e) {
                    let Some(other) = self
                        .entities
                        .at(tile)
                        .filter(|_| open & fluids::bit(d) != 0)
                    else {
                        continue;
                    };
                    let o = other.0 as usize;
                    let joins = match self.states.get(o).and_then(Option::as_ref) {
                        Some(State::Pipe | State::PipeTunnel(_)) => self
                            .fluids
                            .open
                            .get(o)
                            .is_some_and(|m| m & fluids::bit(d.opposite()) != 0),
                        Some(State::Pump) => self
                            .entities
                            .get(other)
                            .is_some_and(|p| p.dir == d || p.dir == d.opposite()),
                        Some(State::Well | State::Engine { .. }) => true,
                        Some(State::Crafter(_)) => self.uses_fluid(o),
                        _ => false,
                    };
                    if joins {
                        mask |= fluids::bit(d);
                    }
                }
                mask
            }
            Some(State::Rail | State::Stop { .. }) => self.track_links(e.pos),
            _ => 0,
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

    /// What a picker would take from the store on `tile` (a warehouse,
    /// chest or the hub), and whether its condition holds there now. A
    /// picker set to nothing takes whatever a chest holds first; from the
    /// hub it must be set to an item.
    fn pickable(&self, tile: TilePos, p: &Picker) -> (Option<Item>, bool) {
        let Some(id) = self.entities.at(tile) else {
            return (None, false);
        };
        let state = self.states[id.0 as usize].as_ref();
        let count = |i: Item| match state {
            Some(State::Chest(c)) => c.count(i),
            Some(State::Hub) => self.stock[i as usize],
            _ => 0,
        };
        // It never takes a warehouse's safety stock, but its condition
        // counts all of it.
        let spare = |i: Item| match state {
            Some(State::Chest(c)) => c.count(i).saturating_sub(c.keep),
            _ => count(i),
        };
        let item = match (state, p.pick) {
            (Some(State::Chest(_) | State::Hub), Some(i)) => Some(i).filter(|&i| spare(i) > 0),
            (Some(State::Chest(c)), None) => c.items.iter().find(|x| x.1 > c.keep).map(|x| x.0),
            (Some(State::Hub), None) => None,
            _ => return (None, false),
        };
        let on = p.when.is_none_or(|(i, compare, amount)| match compare {
            Compare::AtLeast => count(i) >= amount,
            Compare::Below => count(i) < amount,
        });
        (item, on)
    }

    /// Take one `item` from the store on `tile`, for a picker.
    fn take_from(&mut self, tile: TilePos, item: Item) {
        let Some(id) = self.entities.at(tile) else {
            return;
        };
        match self.states[id.0 as usize].as_mut() {
            Some(State::Chest(c)) => {
                c.take(item, 1);
            }
            Some(State::Hub) => {
                let n = &mut self.stock[item as usize];
                *n = n.saturating_sub(1);
            }
            _ => {}
        }
    }

    /// A picker's condition, by item id.
    fn condition(&self, p: &Picker) -> Option<Condition> {
        p.when.map(|(i, compare, amount)| Condition {
            item: self.tables.items[i as usize].clone(),
            compare,
            amount,
        })
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
        if let State::Stop { docked } = state {
            let docked = *docked;
            return self.load_train(docked, item);
        }
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
            State::Miner { .. }
            | State::Pole
            | State::Generator
            | State::Pipe
            | State::PipeTunnel(_)
            | State::Pump
            | State::Well
            | State::Engine { .. }
            | State::Rail
            | State::Stop { .. }
            | State::Train(_)
            | State::Radar { .. }
            | State::Picker(_)
            | State::Port { .. } => false,
            // A provider chest takes from belts on any side; a warehouse
            // from any side but the one it hands out of.
            State::Chest(c) => match self.tables.buildings[target.kind as usize].role {
                Role::Chest {
                    capacity,
                    requester: false,
                    warehouse,
                } if c.total() < capacity && !(warehouse && dir == target.dir.opposite()) => {
                    c.add(item, 1);
                    true
                }
                _ => false,
            },
            State::Splitter { held, .. } => {
                // Only from behind.
                if dir != target.dir || held.is_some() {
                    return false;
                }
                *held = Some(item);
                true
            }
            State::Tunnel(t) => {
                if t.exit || dir != target.dir {
                    return false;
                }
                if t.items.last().is_some_and(|&(_, p)| p < SPACING) {
                    return false;
                }
                t.items.push((item, 0));
                true
            }
            State::Mega { stage, have } => {
                let Role::Mega { stages, .. } = &self.tables.buildings[target.kind as usize].role
                else {
                    return false;
                };
                let Some(needs) = stages.get(*stage as usize) else {
                    return false; // built: it takes nothing more
                };
                let Some(k) = needs.iter().position(|&(i, _)| i == item) else {
                    return false;
                };
                if have[k] >= needs[k].1 {
                    return false;
                }
                have[k] += 1;
                // A stage is in when all it needs is: the next one starts.
                if have.iter().zip(needs).all(|(h, n)| *h >= n.1) {
                    *stage += 1;
                    *have = vec![0; stages.get(*stage as usize).map_or(0, Vec::len)];
                }
                true
            }
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
                    // The recipe whose only solid input is this item; its
                    // fluid inputs come from pipes (a boiler takes coal).
                    let auto = recipes.iter().copied().find(|&r| {
                        let ins = &self.tables.recipes[r as usize].inputs;
                        let lock = self.tables.recipe_lock[r as usize];
                        let open = lock.is_none_or(|q| self.research.done[q as usize]);
                        let mut solid =
                            ins.iter().filter(|&&(i, _)| !self.tables.fluid[i as usize]);
                        solid.next().is_some_and(|&(i, _)| i == item)
                            && solid.next().is_none()
                            && open
                    });
                    let Some(r) = auto else { return false };
                    c.recipe = Some(r);
                    c.inputs = vec![0; self.tables.recipes[r as usize].inputs.len()];
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
            cleared: self.cleared.clone(),
            depleted: self.depleted.clone(),
            pins: self.pins.clone(),
            explored: self.explored.clone(),
            ..Snapshot::default()
        };
        for (id, e) in self.entities.iter() {
            let b = &self.tables.buildings[e.kind as usize];
            let slot = id.0 as usize;
            if let (Some(State::Train(t)), Role::Train { capacity, tank, .. }) =
                (self.states[slot].as_ref(), &b.role)
            {
                snap.trains
                    .push(self.train_view(id, b, t, *capacity, *tank));
                continue;
            }
            if let (Some(State::Mega { stage, have }), Role::Mega { stages, names }, None) =
                (self.states[slot].as_ref(), &b.role, &snap.mega)
            {
                let needs = stages.get(*stage as usize);
                snap.mega = Some(mofact_core::snapshot::MegaView {
                    id,
                    stage: *stage,
                    stages: stages.len() as u32,
                    name: names.get(*stage as usize).cloned().unwrap_or_default(),
                    needs: needs.map_or(Vec::new(), |n| {
                        n.iter()
                            .zip(have)
                            .map(|(&(i, want), &h)| {
                                (self.tables.items[i as usize].clone(), h, want)
                            })
                            .collect()
                    }),
                });
            }
            let (state, progress, recipe) = match self.states[id.0 as usize].as_ref() {
                Some(State::Splitter {
                    held: Some(item), ..
                }) => {
                    snap.belt_items.push(BeltItemView {
                        item: self.tables.items[*item as usize].clone(),
                        tile: e.pos,
                        along: 0.5,
                    });
                    (MachineState::Working, 0.0, None)
                }
                Some(State::Tunnel(t)) if !t.exit => {
                    // Items show only where they go in and come out.
                    let length = self.tunnel_length(e, t);
                    let exit = t.partner.and_then(|p| self.entities.get(p)).map(|x| x.pos);
                    for &(item, p) in &t.items {
                        let (tile, along) = if p < UNIT / 2 {
                            (e.pos, p as f32 / UNIT as f32)
                        } else if let (Some(x), true) = (exit, p + UNIT / 2 > length) {
                            (x, (p + UNIT - length) as f32 / UNIT as f32)
                        } else {
                            continue;
                        };
                        snap.belt_items.push(BeltItemView {
                            item: self.tables.items[item as usize].clone(),
                            tile,
                            along,
                        });
                    }
                    (MachineState::Working, 0.0, None)
                }
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
                    let ticks = self.mine_ticks(ticks);
                    let st = if out.is_some() && *waiting > BLOCKED_AFTER {
                        MachineState::Blocked
                    } else if out.is_none() && self.ore_in_reach(e.pos).1 == 0 {
                        // Mined out: nothing left to dig.
                        MachineState::Starved
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
                        } * (1.0 + self.research.boost[1]);
                        c.prog as f32
                            / ticks(self.tables.recipes[r as usize].seconds / speed) as f32
                    });
                    (
                        st,
                        if c.working { p } else { 0.0 },
                        c.recipe.map(|r| self.tables.recipes[r as usize].id.clone()),
                    )
                }
                Some(State::Radar { prog, done, .. }) => {
                    let every = match b.role {
                        Role::Radar { every, .. } => every,
                        _ => 1,
                    };
                    let st = if *done {
                        MachineState::Idle
                    } else {
                        MachineState::Working
                    };
                    (st, *prog as f32 / every as f32, None)
                }
                Some(State::Mega { stage, have }) => {
                    let (n, frac) = match &b.role {
                        Role::Mega { stages, .. } => {
                            let part = stages.get(*stage as usize).map_or(0.0, |needs| {
                                let want: u32 = needs.iter().map(|x| x.1).sum();
                                have.iter().sum::<u32>() as f32 / want.max(1) as f32
                            });
                            (stages.len().max(1) as f32, part)
                        }
                        _ => (1.0, 0.0),
                    };
                    let p = ((*stage as f32 + frac) / n).min(1.0);
                    (MachineState::Working, p, None)
                }
                Some(State::Lab(lab)) => {
                    let speed = match b.role {
                        Role::Lab { speed } => speed,
                        _ => 1.0,
                    } * (1.0 + self.research.boost[2]);
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
                Some(State::Engine { share }) => {
                    let load = self
                        .grids
                        .grid(slot)
                        .map_or(0, |g| self.grids.grids[g as usize].load);
                    let st = match (*share, load) {
                        (0, 0) => MachineState::Idle,
                        (0, _) => MachineState::Starved,
                        _ => MachineState::Working,
                    };
                    // Progress shows how hard it is working, from 0 to 1.
                    (st, *share as f32 / power::FULL as f32, None)
                }
                Some(State::Port { charging: true }) => (MachineState::Working, 0.0, None),
                Some(State::Port { .. }) => (MachineState::Idle, 0.0, None),
                Some(State::Picker(p)) if !p.on => (MachineState::Idle, 0.0, None),
                Some(State::Chest(c)) if c.items.is_empty() => (MachineState::Idle, 0.0, None),
                _ => (MachineState::Working, 0.0, None),
            };
            // Powered machines off the grid, or on one with nothing to give, say so.
            let state = match self.states[slot].as_ref() {
                Some(s)
                    if b.power > 0
                        && (self.grids.grid(slot).is_none()
                            || (self.grids.sat(slot) == 0 && Self::wants_power(s))) =>
                {
                    MachineState::NoPower
                }
                _ => state,
            };
            let miner = matches!(b.role, Role::Miner { .. }).then(|| self.ore_in_reach(e.pos));
            snap.entities.push(EntityView {
                id,
                building: b.id.clone(),
                pos: e.pos,
                dir: e.dir,
                recipe,
                ore: miner
                    .and_then(|(i, _)| i)
                    .map(|i| self.tables.items[i as usize].clone()),
                ore_left: miner.map(|(_, n)| n),
                grid: self.grids.grid(slot),
                exit: matches!(
                    self.states[slot],
                    Some(State::Tunnel(Tunnel { exit: true, .. }))
                        | Some(State::PipeTunnel(Tunnel { exit: true, .. }))
                ),
                fluid: self.fluids.of.get(slot).copied().flatten().and_then(|s| {
                    let seg = &self.fluids.segs[s as usize];
                    Some(FluidView {
                        item: self.tables.items[seg.fluid? as usize].clone(),
                        fill: seg.amount as f32 / seg.capacity.max(1) as f32,
                    })
                }),
                links: self.links(id, e),
                state,
                progress,
                request: match &self.states[slot] {
                    Some(State::Chest(c)) => c
                        .request
                        .map(|(i, n)| (self.tables.items[i as usize].clone(), n)),
                    _ => None,
                },
                filter: match &self.states[slot] {
                    Some(
                        State::Splitter {
                            filter: Some(i), ..
                        }
                        | State::Chest(drones::Chest {
                            filter: Some(i), ..
                        })
                        | State::Picker(Picker { pick: Some(i), .. }),
                    ) => Some(self.tables.items[*i as usize].clone()),
                    _ => None,
                },
                keep: match &self.states[slot] {
                    Some(State::Chest(c)) => c.keep,
                    _ => 0,
                },
                when: match &self.states[slot] {
                    Some(State::Picker(p)) => self.condition(p),
                    _ => None,
                },
                held: match &self.states[slot] {
                    Some(State::Chest(c)) => c
                        .items
                        .iter()
                        .map(|&(i, n)| (self.tables.items[i as usize].clone(), n))
                        .collect(),
                    _ => Vec::new(),
                },
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
        snap.grids = self
            .grids
            .grids
            .iter()
            .map(|g| GridView {
                supply_kw: g.supply as f32 / 1000.0,
                demand_kw: g.demand as f32 / 1000.0,
                satisfaction: g.sat as f32 / power::FULL as f32,
            })
            .collect();
        let t = &self.tables;
        snap.research = ResearchView {
            current: self
                .research
                .current
                .map(|r| t.research[r as usize].id.clone()),
            progress: self.research.current.map_or((0, 0), |r| {
                let def = &t.research[r as usize];
                let units = self.research.units[r as usize];
                match def.repeat {
                    Some(_) => Repeat::level(def.units, units).1,
                    None => (units, def.units),
                }
            }),
            done: (0..t.research.len())
                .filter(|&r| self.research.done[r])
                .map(|r| t.research[r].id.clone())
                .collect(),
            levels: (0..t.research.len())
                .filter(|&r| t.research[r].repeat.is_some() && self.research.units[r] > 0)
                .map(|r| (t.research[r].id.clone(), self.level(r)))
                .collect(),
        };
        snap.goal = self.goal_progress().map(|(have, need)| GoalView {
            index: self.goal,
            have,
            need,
        });
        snap.order = self.order_view();
        snap.orders_shipped = self.orders.shipped;
        snap.orders_missed = self.orders.missed;
        self.drone_views(&mut snap);
        snap
    }

    fn train_view(
        &self,
        id: EntityId,
        b: &tables::Building,
        t: &rails::Train,
        capacity: u32,
        tank: bool,
    ) -> TrainView {
        let item = |i: Item| self.tables.items[i as usize].clone();
        // A tank's milli-units show as whole units.
        let units = |n: u32| if tank { n / FLUID as u32 } else { n };
        TrainView {
            id,
            building: b.id.clone(),
            pos: t.at,
            dir: t.heading,
            along: if t.to.is_some() {
                t.along as f32 / UNIT as f32
            } else {
                0.0
            },
            cargo: t.cargo.iter().map(|&(i, n)| (item(i), units(n))).collect(),
            capacity: units(capacity),
            schedule: t.schedule.clone(),
            next: t.next,
            state: match t.phase {
                rails::Phase::Idle => TrainState::Idle,
                rails::Phase::Moving => TrainState::Moving,
                rails::Phase::Waiting => TrainState::Waiting,
                rails::Phase::Loading => TrainState::Loading,
                rails::Phase::Unloading => TrainState::Unloading,
                rails::Phase::NoPath => TrainState::NoPath,
            },
        }
    }

    /// The goal the player is on, from 0; the number of goals once all are met.
    pub fn goal(&self) -> u16 {
        self.goal
    }

    /// The current goal's id, for saves; none once all are met.
    fn goal_id(&self) -> Option<String> {
        let ids = &self.tables.goal_ids;
        ids.get(self.goal as usize).cloned()
    }

    /// How far along the current goal is, as (have, need); none once all are met.
    fn goal_progress(&self) -> Option<(u32, u32)> {
        let (goal, _) = self.tables.goals.get(self.goal as usize)?;
        Some(match *goal {
            Goal::Deliver(item, n) => (self.delivered[item as usize].min(n) as u32, n as u32),
            Goal::Build(kind, n) => {
                let have = self.entities.iter().filter(|(_, e)| e.kind == kind).count();
                ((have as u32).min(n), n)
            }
            Goal::Research(r) => (u32::from(self.research.done[r as usize]), 1),
            Goal::Never => (0, 1),
        })
    }

    /// Move past every goal that is met, paying out their rewards.
    fn check_goals(&mut self) {
        while let Some((have, need)) = self.goal_progress() {
            if have < need {
                break;
            }
            for &(item, n) in &self.tables.goals[self.goal as usize].1 {
                self.stock[item as usize] += n;
            }
            self.goal += 1;
        }
    }
}

#[cfg(test)]
mod rail_tests;
#[cfg(test)]
mod tests;
