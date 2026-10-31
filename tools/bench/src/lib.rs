//! Benchmark bases and the harness that times them (T23, R44).
//!
//! The three bases are built in code, so they always load with the current
//! simulation; `mofact-bench --write DIR` saves them for the device runs.
//!
//! - **starter:** one copy of the circuit chain, the end of the prototype.
//! - **budget:** the R44 base, as far as v1's buildings go: 10,000 belt tiles
//!   and 2,000 machines, as 200 copies of a circuit-chain cell, all working.
//!   (Pipes, drones and trains join it when they exist.)
//! - **stress:** twice the budget, to see how far past it the game goes.

use mofact_core::content::Content;
use mofact_core::geom::Dir;
use mofact_headless::{Build, Scenario};
use mofact_sim::Sim;
use std::time::{Duration, Instant};

/// One circuit-chain cell: 10 machines and 50 belt tiles, delivering circuits
/// to its own hub, laid out from (x, y). It is 41 tiles wide and 5 high.
fn cell(x: i32, y: i32, s: &mut Scenario) {
    let at = |b: &str, dx: i32, dy: i32, recipe: Option<&str>| Build::At {
        building: b.into(),
        x: x + dx,
        y: y + dy,
        dir: Dir::East,
        recipe: recipe.map(Into::into),
    };
    let belts = |dx: i32, dy: i32, len: u32, dir: Dir| Build::Belts {
        x: x + dx,
        y: y + dy,
        len,
        dir,
    };
    s.ore.push((x, y + 1, "iron-ore".into()));
    s.ore.push((x, y + 3, "copper-ore".into()));
    s.ore.push((x, y + 4, "copper-ore".into()));
    s.build.push(at("hub", 38, 0, None));
    for (dy, recipe) in [(1, "gear"), (3, "wire"), (4, "wire")] {
        s.build.push(at("drill", 0, dy, None));
        s.build.push(belts(1, dy, 2, Dir::East));
        s.build.push(at("furnace", 3, dy, None));
        s.build.push(belts(4, dy, 1, Dir::East));
        s.build.push(at("assembler", 5, dy, Some(recipe)));
    }
    s.build.push(belts(6, 1, 4, Dir::East));
    s.build.push(belts(10, 1, 1, Dir::South));
    s.build.push(belts(6, 3, 3, Dir::East));
    s.build.push(belts(6, 4, 3, Dir::East));
    s.build.push(belts(9, 4, 2, Dir::North));
    s.build.push(belts(9, 2, 1, Dir::East));
    s.build.push(at("assembler", 10, 2, Some("circuit")));
    s.build.push(belts(11, 2, 27, Dir::East));
}

/// Cells per base.
pub const BASES: [(&str, usize); 3] = [("starter", 1), ("budget", 200), ("stress", 400)];

/// A base of `cells` circuit-chain cells, in rows of 16.
pub fn base(name: &str, cells: usize) -> Scenario {
    let mut s = Scenario {
        name: name.into(),
        stock: vec![
            ("iron-plate".into(), 10_000_000),
            ("copper-plate".into(), 10_000_000),
        ],
        ore: Vec::new(),
        build: Vec::new(),
        warmup_seconds: 0,
        measure_seconds: 0,
        expect: Vec::new(),
    };
    for i in 0..cells {
        let (cx, cy) = ((i % 16) as i32, (i / 16) as i32);
        cell(cx * 42, cy * 6, &mut s);
    }
    s
}

/// What one run measured.
#[derive(Debug)]
pub struct Timing {
    pub belts: usize,
    pub machines: usize,
    /// Tick times, sorted.
    pub ticks: Vec<Duration>,
    /// Time to take one snapshot, as the sim thread does each tick.
    pub snapshot: Duration,
    pub save_bytes: usize,
}

impl Timing {
    pub fn mean(&self) -> Duration {
        self.ticks.iter().sum::<Duration>() / self.ticks.len().max(1) as u32
    }

    /// The `q` quantile (0 to 1) of tick time.
    pub fn quantile(&self, q: f64) -> Duration {
        let i = ((self.ticks.len() as f64 - 1.0) * q).round() as usize;
        self.ticks.get(i).copied().unwrap_or_default()
    }
}

/// Run `sim` for `warmup` ticks, then time `ticks` more.
pub fn time(sim: &mut Sim, warmup: u32, ticks: u32) -> Timing {
    for _ in 0..warmup {
        sim.step();
    }
    let mut times: Vec<Duration> = (0..ticks)
        .map(|_| {
            let t = Instant::now();
            sim.step();
            t.elapsed()
        })
        .collect();
    times.sort();
    // The mean of a few snapshots, which allocate and so vary more.
    let t = Instant::now();
    for _ in 0..19 {
        std::hint::black_box(sim.snapshot());
    }
    let snap = sim.snapshot();
    let snapshot = t.elapsed() / 20;
    let belts = snap
        .entities
        .iter()
        .filter(|e| e.building.as_str() == "belt")
        .count();
    let machines = snap
        .entities
        .iter()
        .filter(|e| !matches!(e.building.as_str(), "belt" | "hub"))
        .count();
    Timing {
        belts,
        machines,
        ticks: times,
        snapshot,
        save_bytes: sim.save().len(),
    }
}

/// Build the named base.
pub fn build(content: &Content, name: &str, cells: usize) -> Result<Sim, String> {
    mofact_headless::build(content, &base(name, cells))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> Content {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        mofact_content::load_dir(&dir).expect("loads")
    }

    #[test]
    fn the_budget_base_is_the_r44_size() {
        let s = base("budget", 200);
        let belts: u32 = s
            .build
            .iter()
            .map(|b| match b {
                Build::Belts { len, .. } => *len,
                _ => 0,
            })
            .sum();
        let machines = s
            .build
            .iter()
            .filter(|b| matches!(b, Build::At { building, .. } if building.as_str() != "hub"))
            .count();
        assert_eq!(belts, 10_000);
        assert_eq!(machines, 2_000);
    }

    #[test]
    fn cells_deliver_circuits() {
        let mut sim = build(&content(), "starter", 2).expect("builds");
        for _ in 0..60 * 120 {
            sim.step();
        }
        // Two cells at 25 a minute each, once the chains have filled.
        assert!(
            sim.delivered(&"circuit".into()) >= 40,
            "circuits reach the hubs"
        );
    }
}
