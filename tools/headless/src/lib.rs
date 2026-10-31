//! Headless runner (T22, R49): plays scenario files with no renderer, for a
//! fixed number of ticks, and checks items per minute delivered to the hub
//! against expected values.
//!
//! The simulation has no camera: every entity steps every tick wherever the
//! player is looking, so the same scenario gives the same rate with the camera
//! away (R18) by construction. The runner checks that scenarios are
//! deterministic by running each twice and comparing the result.
//!
//! A scenario is a RON file in `scenarios/`; see `scenarios/README.md`.

use mofact_core::command::{Command, CommandResult};
use mofact_core::content::Content;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId, RecipeId};
use mofact_core::TICKS_PER_SECOND;
use mofact_sim::Sim;
use serde::Deserialize;
use std::fmt;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub stock: Vec<(ItemId, u32)>,
    #[serde(default)]
    pub ore: Vec<(i32, i32, ItemId)>,
    pub build: Vec<Build>,
    /// Seconds to run before measuring, so the chain fills up.
    pub warmup_seconds: u32,
    pub measure_seconds: u32,
    pub expect: Vec<Expect>,
}

#[derive(Debug, Deserialize)]
pub enum Build {
    /// One building with its top-left tile at (x, y).
    At {
        building: BuildingId,
        x: i32,
        y: i32,
        dir: Dir,
        #[serde(default)]
        recipe: Option<RecipeId>,
    },
    /// A straight run of belts from (x, y), `len` tiles long, all facing `dir`.
    Belts { x: i32, y: i32, len: u32, dir: Dir },
}

#[derive(Debug, Deserialize)]
pub struct Expect {
    pub item: ItemId,
    /// Items per minute delivered to the hub.
    pub per_minute: f64,
    pub tolerance: f64,
}

#[derive(Debug, PartialEq)]
pub struct Measured {
    pub item: ItemId,
    pub per_minute: f64,
    pub expected: f64,
    pub tolerance: f64,
}

impl Measured {
    pub fn ok(&self) -> bool {
        (self.per_minute - self.expected).abs() <= self.tolerance
    }
}

impl fmt::Display for Measured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:<14} {:>6.1}/min (expected {} ± {})",
            if self.ok() { "ok  " } else { "FAIL" },
            self.item.as_str(),
            self.per_minute,
            self.expected,
            self.tolerance
        )
    }
}

pub fn parse(text: &str) -> Result<Scenario, String> {
    ron::from_str(text).map_err(|e| e.to_string())
}

/// Build the scenario and run it. Errors if a building can't be placed.
pub fn run(content: &Content, s: &Scenario) -> Result<Vec<Measured>, String> {
    let mut sim = build(content, s)?;
    run_built(&mut sim, s)
}

/// A simulation with the scenario's stock, ore and buildings, not yet run.
pub fn build(content: &Content, s: &Scenario) -> Result<Sim, String> {
    let mut sim = Sim::with_content(content);
    for (item, n) in &s.stock {
        sim.add_stock(item, *n);
    }
    for (x, y, item) in &s.ore {
        sim.set_ore(TilePos::new(*x, *y), item);
    }
    let mut place = |building: &BuildingId, pos: TilePos, dir: Dir, recipe: &Option<RecipeId>| {
        let cmd = Command::Place {
            building: building.clone(),
            pos,
            dir,
            recipe: recipe.clone(),
        };
        match sim.apply(&cmd) {
            CommandResult::Applied { .. } => Ok(()),
            CommandResult::Refused(e) => Err(format!(
                "{}: placing {building} at {},{}: {e:?}",
                s.name, pos.x, pos.y
            )),
        }
    };
    for b in &s.build {
        match b {
            Build::At {
                building,
                x,
                y,
                dir,
                recipe,
            } => place(building, TilePos::new(*x, *y), *dir, recipe)?,
            Build::Belts { x, y, len, dir } => {
                let mut pos = TilePos::new(*x, *y);
                for _ in 0..*len {
                    place(&BuildingId::from("belt"), pos, *dir, &None)?;
                    pos = pos.step(*dir);
                }
            }
        }
    }
    Ok(sim)
}

fn run_built(sim: &mut Sim, s: &Scenario) -> Result<Vec<Measured>, String> {
    let ticks = |secs: u32| secs * TICKS_PER_SECOND;
    for _ in 0..ticks(s.warmup_seconds) {
        sim.step();
    }
    let before: Vec<u64> = s.expect.iter().map(|e| sim.delivered(&e.item)).collect();
    for _ in 0..ticks(s.measure_seconds) {
        sim.step();
    }
    let minutes = f64::from(s.measure_seconds) / 60.0;
    Ok(s.expect
        .iter()
        .zip(before)
        .map(|(e, b)| Measured {
            item: e.item.clone(),
            per_minute: (sim.delivered(&e.item) - b) as f64 / minutes,
            expected: e.per_minute,
            tolerance: e.tolerance,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn root() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
    }

    fn scenario(name: &str) -> Scenario {
        parse(&std::fs::read_to_string(root().join("scenarios").join(name)).unwrap()).unwrap()
    }

    #[test]
    fn repo_scenarios_pass() {
        let content = mofact_content::load_dir(&root().join("data")).unwrap();
        for entry in std::fs::read_dir(root().join("scenarios")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "ron") {
                let s = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
                for m in run(&content, &s).unwrap() {
                    assert!(m.ok(), "{}: {m}", s.name);
                }
            }
        }
    }

    #[test]
    fn a_broken_ratio_fails() {
        // Make gears take four times as long: the circuit chain must now miss its rate.
        let mut content = mofact_content::load_dir(&root().join("data")).unwrap();
        content
            .recipes
            .iter_mut()
            .find(|r| r.id.as_str() == "gear")
            .unwrap()
            .seconds *= 4.0;
        let results = run(&content, &scenario("circuit-chain.ron")).unwrap();
        assert!(results.iter().any(|m| !m.ok()), "{results:?}");
    }
}
