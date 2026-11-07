//! The bot: it starts the new game the app starts, then builds, researches
//! and builds again, through player commands only, until the megaproject is
//! finished. Every building is paid for from the stock its own factory has
//! delivered; when it can't afford the next one, it waits for the stock.
//!
//! The headless world has no generated map, so the bot puts ore under each
//! drill it places, as the scenarios do, and enough that it never runs
//! out: it plays the game's economy, not its map.

use crate::layout::{self, Block, Part, Powered};
use crate::plan::Planner;
use mofact_core::command::{Command, CommandError, CommandResult};
use mofact_core::content::{BuildingRole, Content};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{EntityId, ItemId, ResearchId};
use mofact_core::TICKS_PER_SECOND;
use mofact_sim::Sim;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

const STARTER: &str = include_str!("../../../scenarios/starter-line.ron");

/// Stock lines the bot keeps, by item: as many as it can make, up to the count.
const LINES: &[(&str, usize)] = &[
    ("iron-plate", 24),
    ("copper-plate", 12),
    ("gear", 12),
    ("wire", 8),
    ("brick", 4),
    ("circuit", 8),
    ("steel", 8),
    ("motor", 3),
    ("titanium-plate", 3),
    ("ai-core", 2),
];

/// Where the bot builds: shelves of blocks east of the starter factory.
const LEFT: i32 = 60;
const SHELF: i32 = 1600;

/// Labs in a station for `packs` pack types, and the trees that feed it.
pub fn station_shape(packs: usize) -> (usize, usize) {
    let labs = packs.saturating_sub(1).div_ceil(2).max(2);
    (labs, 2 * labs + 1)
}

pub struct Bot<'a> {
    pub sim: Sim,
    planner: Planner<'a>,
    content: &'a Content,
    taken: HashSet<TilePos>,
    at: (i32, i32),
    row: i32,
    lines: HashMap<ItemId, usize>,
    /// Lab stations: the packs each takes, and what it is built of.
    stations: Vec<(Vec<ItemId>, Vec<EntityId>)>,
    /// What the bot placed since it last started a block.
    placed: Vec<EntityId>,
    /// Ticks the bot may run before it gives up.
    limit: u64,
    /// What happened, with the game time of each step.
    pub log: Vec<String>,
    /// When the bot last said what it is waiting for.
    told: u64,
    /// When to stop and save, so a run can go on in another job.
    pub deadline: Option<Instant>,
}

/// What a paused run returns: save it and go on later.
pub const PAUSED: &str = "paused";

/// The bot's own state, beside the sim's save, in a checkpoint.
#[derive(Serialize, Deserialize)]
struct Saved {
    taken: Vec<TilePos>,
    at: (i32, i32),
    row: i32,
    lines: Vec<(ItemId, usize)>,
    stations: Vec<(Vec<ItemId>, Vec<EntityId>)>,
    told: u64,
}

impl<'a> Bot<'a> {
    pub fn new(content: &'a Content, hours: u64) -> Result<Self, String> {
        let scenario = mofact_headless::parse(STARTER)?;
        let sim = mofact_headless::build(content, &scenario)?;
        Ok(Bot {
            sim,
            planner: Planner::new(content),
            content,
            taken: HashSet::new(),
            at: (LEFT, 0),
            row: 0,
            lines: HashMap::new(),
            stations: Vec::new(),
            placed: Vec::new(),
            limit: hours * 3600 * u64::from(TICKS_PER_SECOND),
            log: Vec::new(),
            told: 0,
            deadline: None,
        })
    }

    /// A checkpoint: the sim's save, then the bot's own state.
    pub fn checkpoint(&self) -> Vec<u8> {
        let mut taken: Vec<TilePos> = self.taken.iter().copied().collect();
        taken.sort_by_key(|t| (t.y, t.x));
        let mut lines: Vec<(ItemId, usize)> =
            self.lines.iter().map(|(i, n)| (i.clone(), *n)).collect();
        lines.sort();
        let saved = Saved {
            taken,
            at: self.at,
            row: self.row,
            lines,
            stations: self.stations.clone(),
            told: self.told,
        };
        let sim = self.sim.save();
        let mut out = (sim.len() as u64).to_le_bytes().to_vec();
        out.extend(sim);
        out.extend(ron::to_string(&saved).unwrap_or_default().into_bytes());
        out
    }

    /// Go on from a checkpoint.
    pub fn resume(content: &'a Content, hours: u64, bytes: &[u8]) -> Result<Self, String> {
        let bad = || "not a bot checkpoint".to_owned();
        let head: [u8; 8] = bytes
            .get(..8)
            .ok_or_else(bad)?
            .try_into()
            .map_err(|_| bad())?;
        let end = usize::try_from(u64::from_le_bytes(head))
            .ok()
            .and_then(|n| n.checked_add(8))
            .ok_or_else(bad)?;
        let (sim, _) =
            Sim::load(content, bytes.get(8..end).ok_or_else(bad)?).map_err(|e| format!("{e:?}"))?;
        let text = std::str::from_utf8(&bytes[end..]).map_err(|_| bad())?;
        let saved: Saved = ron::from_str(text).map_err(|e| e.to_string())?;
        let mut bot = Bot::new(content, hours)?;
        bot.sim = sim;
        bot.taken = saved.taken.into_iter().collect();
        bot.at = saved.at;
        bot.row = saved.row;
        bot.lines = saved.lines.into_iter().collect();
        bot.stations = saved.stations;
        bot.told = saved.told;
        bot.refresh();
        bot.note("going on from a checkpoint".into());
        Ok(bot)
    }

    /// Whether it's time to stop and save.
    fn pause(&self) -> Result<(), String> {
        match self.deadline {
            Some(d) if Instant::now() >= d => Err(PAUSED.into()),
            _ => Ok(()),
        }
    }

    /// Game time, as h:mm:ss.
    pub fn clock(&self) -> String {
        let s = self.sim.tick() / u64::from(TICKS_PER_SECOND);
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    }

    fn note(&mut self, what: String) {
        self.told = self.sim.tick();
        let line = format!("[{}] {what}", self.clock());
        println!("{line}");
        self.log.push(line);
    }

    fn run(&mut self, seconds: u32, waiting: &dyn Fn() -> String) -> Result<(), String> {
        for _ in 0..seconds * TICKS_PER_SECOND {
            self.sim.step();
        }
        if self.sim.tick() > self.limit {
            return Err(format!("out of time at {}, {}", self.clock(), waiting()));
        }
        // Every game hour spent waiting, say on what.
        let hour = 3600 * u64::from(TICKS_PER_SECOND);
        if self.sim.tick() - self.told >= hour {
            self.told = self.sim.tick();
            self.note(waiting());
        }
        Ok(())
    }

    /// Fails when `progress` hasn't moved in two hours of game time:
    /// something is stuck, and waiting out the limit would only cost time.
    fn stalled(&self, moved: &mut (u64, u64), progress: u64, what: &str) -> Result<(), String> {
        if progress != moved.0 {
            *moved = (progress, self.sim.tick());
        } else if self.sim.tick() - moved.1 > 2 * 3600 * u64::from(TICKS_PER_SECOND) {
            return Err(format!(
                "stalled at {}: {what} made no progress in two hours",
                self.clock()
            ));
        }
        Ok(())
    }

    fn missing(&self, building: &str) -> String {
        let b = self.planner.building(building);
        let short: Vec<String> = b
            .cost
            .iter()
            .filter(|s| self.sim.stock(&s.item) < s.count)
            .map(|s| format!("{} {}/{}", s.item, self.sim.stock(&s.item), s.count))
            .collect();
        format!("waiting to afford a {building}: {}", short.join(", "))
    }

    fn place(&mut self, p: &Part) -> Result<(), String> {
        if let Some(ore) = &p.ore {
            // Enough for the whole game: a scenario's hand-placed ore runs
            // out in a few hours of digging, and a player would move the
            // drill to fresh ore. The bot plays the economy, not the map.
            self.sim.set_ore_amount(p.pos, ore, u32::MAX / 2);
        }
        let cmd = Command::Place {
            building: p.building.clone(),
            pos: p.pos,
            dir: p.dir,
            recipe: p.recipe.clone(),
        };
        loop {
            match self.sim.apply(&cmd) {
                CommandResult::Applied { created, .. } => {
                    self.placed.extend(created);
                    break;
                }
                CommandResult::Refused(CommandError::CannotAfford) => {
                    let why = self.missing(p.building.as_str());
                    self.run(5, &|| why.clone())?;
                }
                CommandResult::Refused(e) => {
                    return Err(format!("placing {} at {:?}: {e:?}", p.building, p.pos))
                }
            }
        }
        self.taken.extend(p.tiles());
        Ok(())
    }

    /// Build `b` on the next free ground, and power what needs it.
    fn build(&mut self, mut b: Block) -> Result<(), String> {
        self.placed.clear();
        if let Some(f) = b.fault() {
            return Err(f);
        }
        let (x0, y0, x1, y1) = b.bounds();
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        if self.at.0 + w > LEFT + SHELF {
            self.at = (LEFT, self.at.1 + self.row + 8);
            self.row = 0;
        }
        b.shift(self.at.0 - x0, self.at.1 - y0);
        self.at.0 += w + 8;
        self.row = self.row.max(h);
        for p in &b.parts {
            self.place(p)?;
        }
        for m in &b.powered {
            self.power(m)?;
        }
        Ok(())
    }

    /// A pole beside `m` and enough wind turbines round the pole.
    fn power(&mut self, m: &Powered) -> Result<(), String> {
        let turbine = self.planner.building("wind-turbine");
        let kw = match turbine.role {
            BuildingRole::Generator { kw } => kw,
            _ => 60.0,
        };
        let need = (m.kw / kw).ceil() as usize;
        let (w, h) = (i32::from(m.size.0), i32::from(m.size.1));
        let free = |t: &TilePos| !self.taken.contains(t);
        let near = |c: TilePos, lo: (i32, i32), hi: (i32, i32)| {
            let mut v = Vec::new();
            for y in lo.1 - 2..=hi.1 + 2 {
                for x in lo.0 - 2..=hi.0 + 2 {
                    let t = TilePos::new(x, y);
                    if t != c {
                        v.push(t);
                    }
                }
            }
            v
        };
        let foot = (m.pos.x, m.pos.y);
        let end = (m.pos.x + w - 1, m.pos.y + h - 1);
        for pole in near(m.pos, foot, end).into_iter().filter(&free) {
            let spots: Vec<_> = near(pole, (pole.x, pole.y), (pole.x, pole.y))
                .into_iter()
                .filter(|t| free(t) && *t != pole)
                .take(need)
                .collect();
            if spots.len() == need {
                self.place(&Part::new("pole", (1, 1), pole.x, pole.y, Dir::East))?;
                for t in spots {
                    self.place(&Part::new("wind-turbine", (1, 1), t.x, t.y, Dir::East))?;
                }
                return Ok(());
            }
        }
        // Packed in: a pole beside it, wired to a pole further out with
        // room for the turbines round it.
        for pole in near(m.pos, foot, end).into_iter().filter(&free) {
            for r in 3..=7 {
                let ring = near(
                    pole,
                    (pole.x - r + 2, pole.y - r + 2),
                    (pole.x + r - 2, pole.y + r - 2),
                );
                for far in ring
                    .into_iter()
                    .filter(|t| free(t) && (t.x - pole.x).abs().max((t.y - pole.y).abs()) == r)
                {
                    let spots: Vec<_> = near(far, (far.x, far.y), (far.x, far.y))
                        .into_iter()
                        .filter(|t| free(t) && *t != pole)
                        .take(need)
                        .collect();
                    if spots.len() == need {
                        for p in [pole, far] {
                            self.place(&Part::new("pole", (1, 1), p.x, p.y, Dir::East))?;
                        }
                        for t in spots {
                            self.place(&Part::new("wind-turbine", (1, 1), t.x, t.y, Dir::East))?;
                        }
                        return Ok(());
                    }
                }
            }
        }
        Err(format!("no room to power the machine at {:?}", m.pos))
    }

    fn done(&self) -> HashSet<ResearchId> {
        self.content
            .research
            .iter()
            .filter(|r| self.sim.research_done(&r.id))
            .map(|r| r.id.clone())
            .collect()
    }

    /// Lines into hubs for the items buildings cost, as research opens them.
    pub fn economy(&mut self) -> Result<(), String> {
        for &(item, want) in LINES {
            let item = ItemId::from(item);
            while self.planner.can_make(&item) && self.lines.get(&item).copied().unwrap_or(0) < want
            {
                let b = layout::into_hub(self.planner.block(&item)?);
                self.build(b)?;
                *self.lines.entry(item.clone()).or_default() += 1;
                let n = self.lines[&item];
                self.note(format!("built stock line {n} for {item}"));
            }
        }
        Ok(())
    }

    /// Labs fed with every pack in `packs`, a tree for each belt they take.
    fn station(&mut self, packs: &[ItemId]) -> Result<(), String> {
        let (labs, slots) = station_shape(packs.len());
        let mut trees = Vec::new();
        for i in 0..slots {
            trees.push(self.planner.block(&packs[i % packs.len()])?);
        }
        let west = trees.remove(0);
        let upper: Vec<_> = trees.drain(..labs).collect();
        let lab = self.planner.building("lab");
        let b = layout::fan_in("lab", lab.size, labs, lab.power, upper, Some(west), trees);
        self.build(b)?;
        // Older stations take only packs this one takes too: take them
        // down, so they stop making packs nobody will use.
        let placed = std::mem::take(&mut self.placed);
        let mut old = Vec::new();
        self.stations.retain(|(p, ids)| {
            let keep = !p.iter().all(|x| packs.contains(x));
            if !keep {
                old.extend(ids.iter().copied());
            }
            keep
        });
        for id in old.into_iter().rev() {
            self.sim.apply(&Command::Remove { entity: id });
        }
        self.stations.push((packs.to_vec(), placed));
        let names: Vec<&str> = packs.iter().map(|p| p.as_str()).collect();
        self.note(format!("built {labs} labs for {}", names.join(", ")));
        Ok(())
    }

    /// Work the planner's choices out again from what the sim has researched.
    pub fn refresh(&mut self) {
        self.planner.update(self.done());
    }

    pub fn research(&mut self, id: &ResearchId) -> Result<(), String> {
        let name = self.begin(id)?;
        let mut moved = (0, self.sim.tick());
        while !self.sim.research_done(id) {
            let r = self.sim.snapshot().research.progress;
            self.stalled(&mut moved, u64::from(r.0), &format!("researching {id}"))?;
            self.pause()?;
            self.run(30, &|| {
                format!("researching {id}, {} of {} units", r.0, r.1)
            })?;
        }
        self.refresh();
        self.note(format!("researched {name}"));
        Ok(())
    }

    /// Build a station for `id` if none takes its packs, and pick it.
    /// Returns its name.
    pub fn begin(&mut self, id: &ResearchId) -> Result<String, String> {
        let def = self
            .content
            .research
            .iter()
            .find(|r| &r.id == id)
            .ok_or_else(|| format!("no research {id}"))?;
        let packs: Vec<ItemId> = def.cost.iter().map(|s| s.item.clone()).collect();
        if !self
            .stations
            .iter()
            .any(|(s, _)| packs.iter().all(|p| s.contains(p)))
        {
            self.station(&packs)?;
        }
        let pick = Command::SetResearch {
            research: Some(id.clone()),
        };
        if let CommandResult::Refused(e) = self.sim.apply(&pick) {
            return Err(format!("researching {id}: {e:?}"));
        }
        Ok(def.name.clone())
    }

    /// The research the megaproject needs, in an order labs can do it.
    pub fn plan(&self, goal: &ResearchId) -> Vec<ResearchId> {
        let mut need = HashSet::new();
        let mut stack = vec![goal.clone()];
        while let Some(r) = stack.pop() {
            if need.insert(r.clone()) {
                if let Some(def) = self.content.research.iter().find(|d| d.id == r) {
                    stack.extend(def.requires.iter().cloned());
                }
            }
        }
        mofact_balance::analyse(self.content)
            .research_order
            .into_iter()
            .filter(|r| need.contains(r))
            .collect()
    }

    /// Build the megaproject, fed by trees for everything its stages take,
    /// and run until it is finished.
    pub fn megaproject(&mut self) -> Result<(), String> {
        let (id, size, stages) = self
            .content
            .buildings
            .iter()
            .find_map(|b| match &b.role {
                BuildingRole::Megaproject { stages } => {
                    Some((b.id.clone(), b.size, stages.clone()))
                }
                _ => None,
            })
            .ok_or("no megaproject")?;
        let mut items: Vec<ItemId> = Vec::new();
        for s in &stages {
            for n in &s.needs {
                if !items.contains(&n.item) {
                    items.push(n.item.clone());
                }
            }
        }
        let cols = usize::from(size.0);
        let slots = 2 * cols + 1;
        let mut trees = Vec::new();
        for i in 0..slots {
            trees.push(self.planner.block(&items[i % items.len()])?);
        }
        let west = trees.remove(0);
        let upper: Vec<_> = trees.drain(..cols).collect();
        // Going on from a checkpoint, it may be standing already.
        if self.sim.snapshot().mega.is_none() {
            let b = layout::fan_in(id.as_str(), size, 1, 0.0, upper, Some(west), trees);
            self.build(b)?;
            self.note(format!("built the {id}"));
        }
        let mut stage = u32::MAX;
        let mut moved = (0, self.sim.tick());
        loop {
            let mega = self
                .sim
                .snapshot()
                .mega
                .ok_or("the megaproject is not standing")?;
            if mega.done() {
                break;
            }
            if mega.stage != stage {
                stage = mega.stage;
                self.note(format!(
                    "{} stage {} of {}: {}",
                    id,
                    stage + 1,
                    mega.stages,
                    mega.name
                ));
            }
            let have: u64 = mega.needs.iter().map(|(_, h, _)| u64::from(*h)).sum();
            self.stalled(
                &mut moved,
                have + u64::from(mega.stage) * 1_000_000,
                "building the megaproject",
            )?;
            self.pause()?;
            let needs: Vec<String> = mega
                .needs
                .iter()
                .map(|(i, h, n)| format!("{i} {h}/{n}"))
                .collect();
            self.run(30, &|| format!("building the {id}: {}", needs.join(", ")))?;
        }
        self.note(format!("finished the {id}: the factory has won"));
        Ok(())
    }

    /// The whole game.
    pub fn play(&mut self) -> Result<(), String> {
        let goal = self.goal()?;
        self.economy()?;
        for r in self.plan(&goal) {
            // Going on from a checkpoint, what's done stays done.
            if self.sim.research_done(&r) {
                continue;
            }
            self.research(&r)?;
            self.economy()?;
        }
        self.megaproject()
    }

    /// The research that opens the megaproject.
    pub fn goal(&self) -> Result<ResearchId, String> {
        self.content
            .research
            .iter()
            .find(|r| {
                r.buildings.iter().any(|b| {
                    self.content
                        .buildings
                        .iter()
                        .any(|d| &d.id == b && matches!(d.role, BuildingRole::Megaproject { .. }))
                })
            })
            .map(|r| r.id.clone())
            .ok_or_else(|| "no research opens the megaproject".to_owned())
    }

    pub fn entities(&self) -> usize {
        self.sim.snapshot().entities.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The start of the nightly playthrough, quick enough for every pull
    /// request: stock lines, a lab station, and the first two research.
    #[test]
    fn the_bot_builds_its_economy_and_starts_researching() {
        let content =
            mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
                .unwrap();
        let mut bot = Bot::new(&content, 2).unwrap();
        bot.economy().unwrap();
        let plan = bot.plan(&"skyhook".into());
        assert_eq!(plan.last().map(|r| r.as_str()), Some("skyhook"));
        for r in &plan[..2] {
            bot.research(r).unwrap();
            bot.economy().unwrap();
        }
        assert!(bot.sim.research_done(&plan[1]));
        assert!(bot.sim.stock(&"steel".into()) > 0, "steel lines deliver");
    }

    /// The nightly run plays in legs: a checkpoint goes on where it stopped.
    #[test]
    fn a_checkpoint_goes_on_where_the_bot_stopped() {
        let content =
            mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
                .unwrap();
        let mut bot = Bot::new(&content, 2).unwrap();
        bot.economy().unwrap();
        let first = bot.plan(&bot.goal().unwrap())[0].clone();
        bot.begin(&first).unwrap();
        for _ in 0..600 {
            bot.sim.step();
        }
        let mut back = Bot::resume(&content, 2, &bot.checkpoint()).unwrap();
        assert_eq!(back.sim.tick(), bot.sim.tick());
        assert_eq!(back.entities(), bot.entities());
        assert_eq!(
            (back.at, back.row, back.lines.clone()),
            (bot.at, bot.row, bot.lines.clone())
        );
        assert_eq!(back.stations, bot.stations);
        // Both go on the same way.
        for _ in 0..600 {
            bot.sim.step();
            back.sim.step();
        }
        assert_eq!(back.sim.save(), bot.sim.save());
        assert!(bot.checkpoint().len() > 8);
        assert!(Bot::resume(&content, 2, b"junk").is_err());
    }
}
