//! How long the bot's playthrough takes, worked out from the content
//! instead of played: `mofact-bot --estimate`. It follows the bot's plan,
//! research by research, with the trees the planner picks at each point,
//! so content can be balanced in seconds and the nightly run confirms it.
//!
//! Each tree has one machine per node, so a tree makes its product as fast
//! as its slowest node allows. Research runs as fast as the labs, or as the
//! slowest pack's trees, whichever is slower.

use crate::bot::station_shape;
use crate::plan::{Planner, Way};
use mofact_core::content::{BuildingRole, Content};
use mofact_core::ids::{ItemId, ResearchId};
use std::collections::HashSet;

/// One step of the estimate: what it was, and how long it took.
pub struct Step {
    pub what: String,
    pub seconds: f64,
    /// The slowest thing in it: the labs or a pack.
    pub limit: String,
}

impl Planner<'_> {
    /// Seconds the slowest node of `item`'s tree takes to make `demand` of
    /// it, or `None` if it can't be made.
    pub fn interval(&self, item: &ItemId, demand: f64) -> Option<f64> {
        let speed = |b: &str| match self.building(b).role {
            BuildingRole::Crafter { speed, .. } => f64::from(speed),
            _ => 1.0,
        };
        match &self.way(item)?.1 {
            Way::Ore => match self.building("drill").role {
                BuildingRole::Miner { seconds_per_ore } => {
                    Some(demand * f64::from(seconds_per_ore))
                }
                _ => None,
            },
            Way::Well(b) => match self.building(b.as_str()).role {
                BuildingRole::Well { per_second, .. } => Some(demand / f64::from(per_second)),
                _ => None,
            },
            Way::Craft {
                recipe,
                k,
                building,
            } => {
                let r = &self.content.recipes[*recipe];
                let crafts = demand / f64::from(r.outputs[*k].count);
                let mut t = crafts * f64::from(r.seconds) / speed(building.as_str());
                for s in &r.inputs {
                    t = t.max(self.interval(&s.item, crafts * f64::from(s.count))?);
                }
                for (j, o) in r.outputs.iter().enumerate() {
                    if j == *k || !self.is_fluid(&o.item) {
                        continue;
                    }
                    // A fluid by-product's sink keeps up or stops the machine.
                    let s = &self.content.recipes[self.sink(&o.item)?];
                    let take = s.inputs.iter().find(|i| i.item == o.item)?;
                    let sink = crafts * f64::from(o.count) / f64::from(take.count);
                    let m = self.content.buildings.iter().find(|b| match &b.role {
                        BuildingRole::Crafter { categories, .. } => {
                            categories.contains(&s.category) && self.building_open(&b.id)
                        }
                        _ => false,
                    })?;
                    t = t.max(sink * f64::from(s.seconds) / speed(m.id.as_str()));
                    for i in s.inputs.iter().filter(|i| i.item != o.item) {
                        t = t.max(self.interval(&i.item, sink * f64::from(i.count))?);
                    }
                }
                Some(t)
            }
        }
    }
}

/// Trees of each item when `slots` trees go round `items` in turn.
fn trees(items: &[ItemId], slots: usize) -> Vec<usize> {
    (0..items.len())
        .map(|p| (0..slots).filter(|i| i % items.len() == p).count())
        .collect()
}

/// The bot's playthrough, step by step.
pub fn estimate(content: &Content, plan: &[ResearchId]) -> Result<Vec<Step>, String> {
    let mut planner = Planner::new(content);
    let mut done = HashSet::new();
    let mut steps = Vec::new();
    for id in plan {
        let def = content
            .research
            .iter()
            .find(|r| &r.id == id)
            .ok_or_else(|| format!("no research {id}"))?;
        let packs: Vec<ItemId> = def.cost.iter().map(|s| s.item.clone()).collect();
        let (labs, slots) = station_shape(packs.len());
        let mut per_unit = f64::from(def.seconds) / labs as f64;
        let mut limit = format!("{labs} labs");
        for (p, n) in packs.iter().zip(trees(&packs, slots)) {
            let t = planner
                .interval(p, 1.0)
                .ok_or_else(|| format!("{id}: can't make {p}"))?
                / n as f64;
            if t > per_unit {
                per_unit = t;
                limit = format!("{p} ({n} trees)");
            }
        }
        steps.push(Step {
            what: def.name.clone(),
            seconds: per_unit * f64::from(def.units),
            limit,
        });
        done.insert(id.clone());
        planner.update(done.clone());
    }
    for b in &content.buildings {
        let BuildingRole::Megaproject { stages } = &b.role else {
            continue;
        };
        let mut items: Vec<ItemId> = Vec::new();
        for s in stages {
            for n in &s.needs {
                if !items.contains(&n.item) {
                    items.push(n.item.clone());
                }
            }
        }
        let n = trees(&items, 2 * usize::from(b.size.0) + 1);
        for s in stages {
            let mut seconds = 0.0f64;
            let mut limit = String::new();
            for need in &s.needs {
                let i = items.iter().position(|x| x == &need.item).unwrap_or(0);
                let t = planner
                    .interval(&need.item, f64::from(need.count))
                    .ok_or_else(|| format!("can't make {}", need.item))?
                    / n[i] as f64;
                if t > seconds {
                    seconds = t;
                    limit = format!("{} ({} trees)", need.item, n[i]);
                }
            }
            steps.push(Step {
                what: format!("{} stage {}", b.name, s.name),
                seconds,
                limit,
            });
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::Bot;
    use std::path::Path;

    #[test]
    fn a_playthrough_takes_about_a_hundred_hours_and_the_start_is_quick() {
        let content =
            mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
                .unwrap();
        let bot = Bot::new(&content, 1).unwrap();
        let plan = bot.plan(&bot.goal().unwrap());
        let steps = estimate(&content, &plan).unwrap();
        let hours = steps.iter().map(|s| s.seconds).sum::<f64>() / 3600.0;
        assert!((80.0..=120.0).contains(&hours), "{hours:.1} hours");
        // The first tiers stay quick on a phone: motors in minutes.
        let motors = steps.iter().position(|s| s.what == "Motors").unwrap();
        let start: f64 = steps[..=motors].iter().map(|s| s.seconds).sum();
        assert!(start < 10.0 * 60.0, "motors after {start} s");
    }
}
