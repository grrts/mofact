//! Power grids (T29).
//!
//! Poles link to every pole within wire reach of both, and the linked poles
//! form a grid. A building is on the grid of the first pole (in id order)
//! whose supply square overlaps its footprint. Each tick a grid's
//! generators give their full output and its working machines ask for
//! theirs; the share met, its satisfaction, is how often those machines
//! move on. Half the power needed makes them run at half speed.
//!
//! Satisfaction is a fraction of [`FULL`], and whether a machine runs on a
//! tick follows from the tick number alone (see [`runs`]), so grids hold no
//! state of their own: they are rebuilt from the buildings whenever a
//! pole, generator or powered machine comes or goes, and after a load.

use crate::entities::Entities;
use crate::tables::{Role, Tables};
use mofact_core::Tick;

/// Satisfaction when a grid meets all its demand.
pub const FULL: u32 = 1024;

/// One grid: what it gives and what was asked of it this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Grid {
    /// Watts its generators give.
    pub supply: u64,
    /// Watts its working machines ask for.
    pub demand: u64,
    /// The share of demand met, out of [`FULL`].
    pub sat: u32,
}

impl Grid {
    /// Set satisfaction from supply and demand. A grid with nothing asking
    /// is satisfied if it has any supply at all.
    pub fn settle(&mut self) {
        let full = u64::from(FULL);
        self.sat = match (self.supply * full).checked_div(self.demand) {
            Some(share) => share.min(full) as u32,
            None if self.supply > 0 => FULL,
            None => 0,
        };
    }
}

#[derive(Clone, Debug, Default)]
pub struct Grids {
    /// The grid each entity slot is on, if any.
    pub of: Vec<Option<u16>>,
    pub grids: Vec<Grid>,
}

impl Grids {
    /// Work out the grids from scratch.
    pub fn build(tables: &Tables, entities: &Entities) -> Self {
        let slots = entities.iter().map(|(id, _)| id.0 as usize + 1).max();
        let mut out = Grids {
            of: vec![None; slots.unwrap_or(0)],
            grids: Vec::new(),
        };
        let poles: Vec<_> = entities
            .iter()
            .filter_map(|(id, e)| match tables.buildings[e.kind as usize].role {
                Role::Pole { wire, supply } => Some((id.0 as usize, e.pos, wire, supply)),
                _ => None,
            })
            .collect();
        // Union the poles in reach of each other.
        let mut parent: Vec<usize> = (0..poles.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for i in 0..poles.len() {
            for j in i + 1..poles.len() {
                let (a, b) = (poles[i].1, poles[j].1);
                let reach = i32::from(poles[i].2.min(poles[j].2));
                if (a.x - b.x).abs().max((a.y - b.y).abs()) <= reach {
                    let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                    // The lower index wins, so numbering follows pole ids.
                    parent[ri.max(rj)] = ri.min(rj);
                }
            }
        }
        let mut number = vec![None; poles.len()];
        let mut pole_grid = vec![0u16; poles.len()];
        for i in 0..poles.len() {
            let r = root(&mut parent, i);
            let g = *number[r].get_or_insert_with(|| {
                out.grids.push(Grid::default());
                (out.grids.len() - 1) as u16
            });
            pole_grid[i] = g;
            out.of[poles[i].0] = Some(g);
        }
        // Everything that gives or takes power joins the first pole covering it.
        for (id, e) in entities.iter() {
            let b = &tables.buildings[e.kind as usize];
            let generator = match b.role {
                Role::Generator { watts } => Some(watts),
                _ => None,
            };
            if b.power == 0 && generator.is_none() {
                continue;
            }
            let (w, h) = (i32::from(e.size.0), i32::from(e.size.1));
            let covering = poles.iter().position(|&(_, p, _, s)| {
                let s = i32::from(s);
                e.pos.x <= p.x + s
                    && e.pos.x + w > p.x - s
                    && e.pos.y <= p.y + s
                    && e.pos.y + h > p.y - s
            });
            if let Some(k) = covering {
                let g = pole_grid[k];
                out.of[id.0 as usize] = Some(g);
                if let Some(watts) = generator {
                    out.grids[g as usize].supply += u64::from(watts);
                }
            }
        }
        out
    }

    /// The grid `slot` is on.
    pub fn grid(&self, slot: usize) -> Option<u16> {
        self.of.get(slot).copied().flatten()
    }

    /// The satisfaction of the grid `slot` is on; zero off-grid.
    pub fn sat(&self, slot: usize) -> u32 {
        self.grid(slot).map_or(0, |g| self.grids[g as usize].sat)
    }
}

/// Whether a machine on a grid at satisfaction `sat` moves on during
/// `tick`: spread evenly, `sat` ticks in every [`FULL`].
pub fn runs(tick: Tick, sat: u32) -> bool {
    let sat = u64::from(sat.min(FULL));
    sat == FULL as u64 || (tick * sat) / FULL as u64 > (tick.saturating_sub(1) * sat) / FULL as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_power_runs_every_other_tick() {
        let count = |sat| (1..=1024).filter(|&t| runs(t, sat)).count();
        assert_eq!(count(FULL), 1024);
        assert_eq!(count(FULL / 2), 512);
        assert_eq!(count(FULL / 4), 256);
        assert_eq!(count(0), 0);
        assert!((1..=8).all(|t| runs(t, FULL / 2) == (t % 2 == 0)));
    }

    #[test]
    fn satisfaction_is_the_share_met() {
        let mut g = Grid {
            supply: 60_000,
            demand: 120_000,
            sat: 0,
        };
        g.settle();
        assert_eq!(g.sat, FULL / 2);
        g.demand = 30_000;
        g.settle();
        assert_eq!(g.sat, FULL);
        g.demand = 0;
        g.settle();
        assert_eq!(g.sat, FULL);
        g.supply = 0;
        g.settle();
        assert_eq!(g.sat, 0);
    }
}
