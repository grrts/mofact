//! Fluids (T31): pipes, tanks and underground pipes join into segments.
//!
//! Every pipe, tank and underground pipe end that touches another (on a side
//! both are open on) is in the same segment, and so is each pair of
//! underground ends. A segment holds one fluid, as one pool: it has no
//! per-tile levels, so it costs the same to tick however long it is.
//! Anything that would join two segments holding different fluids is
//! refused; pumps sit between segments, so they keep different fluids apart.
//!
//! Length still matters: each tick a segment lets only so much fluid out,
//! and the longer it is the less (see [`Segment::throughput`]). A
//! 200-tile pipe delivers about a tenth of what a short one does; pumps
//! split a long line into short segments to restore the flow.
//!
//! Machines, wells and pumps aren't in segments. They take fluid from and
//! give it to the segments beside them. Like power grids, segments are
//! rebuilt whenever a pipe-like building comes, goes or turns, and after a
//! load; each member's share of its old segment carries over, so fluid is
//! kept across rebuilds.

use crate::entities::{Entities, Entity};
use crate::tables::{Item, Role, Tables, FLUID};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::EntityId;
use mofact_core::TICKS_PER_SECOND;

/// Units a second a one-tile segment lets out; longer ones let out less.
const FLOW: u64 = 1200;
/// The length at which a segment lets out half of [`FLOW`].
const HALF_AT: u64 = 20;

/// One segment: the pool of fluid its members share.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Segment {
    pub fluid: Option<Item>,
    /// Milli-units held, and the most it holds.
    pub amount: u64,
    pub capacity: u64,
    /// How many buildings it is made of.
    pub tiles: u32,
    /// Milli-units it may still let out this tick.
    pub budget: u64,
}

impl Segment {
    /// Milli-units it lets out a tick.
    pub fn throughput(&self) -> u64 {
        FLOW * FLUID * HALF_AT / (HALF_AT + u64::from(self.tiles)) / TICKS_PER_SECOND as u64
    }

    /// How much `fluid` it can take.
    pub fn room_for(&self, fluid: Item) -> u64 {
        if self.fluid.is_none_or(|f| f == fluid) {
            self.capacity - self.amount
        } else {
            0
        }
    }

    /// Put in up to `n` of `fluid`; returns how much went in.
    pub fn give(&mut self, fluid: Item, n: u64) -> u64 {
        let n = n.min(self.room_for(fluid));
        if n > 0 {
            self.fluid = Some(fluid);
            self.amount += n;
        }
        n
    }

    /// Take out up to `n` of `fluid`, as far as this tick's flow allows;
    /// returns how much came out.
    pub fn take(&mut self, fluid: Item, n: u64) -> u64 {
        if self.fluid != Some(fluid) {
            return 0;
        }
        let n = n.min(self.amount).min(self.budget);
        self.amount -= n;
        self.budget -= n;
        if self.amount == 0 {
            self.fluid = None;
        }
        n
    }
}

/// Two fluids would end up in one segment.
#[derive(Debug, PartialEq, Eq)]
pub struct Mix;

#[derive(Clone, Debug, Default)]
pub struct Fluids {
    /// The segment each entity slot is in, if it is a member.
    pub of: Vec<Option<u32>>,
    /// The sides each member slot is open on, as bits by [`bit`].
    pub open: Vec<u8>,
    /// What each member slot holds, in milli-units.
    cap: Vec<u64>,
    pub segs: Vec<Segment>,
}

/// The bit for `dir` in a side mask.
pub fn bit(dir: Dir) -> u8 {
    1 << dir as u8
}

/// What a pipe-like building of `role` holds, or `None` for anything else.
pub fn capacity(role: &Role) -> Option<u64> {
    match role {
        Role::Pipe { capacity } | Role::Tank { capacity } => Some(*capacity),
        Role::UndergroundPipe { capacity, .. } => Some(*capacity),
        _ => None,
    }
}

/// Each tile on the edge of `e`'s footprint with a side facing out, and the
/// tile beyond that side.
pub fn edges(e: &Entity) -> impl Iterator<Item = (Dir, TilePos)> + '_ {
    let (w, h) = (i32::from(e.size.0), i32::from(e.size.1));
    (0..h)
        .flat_map(move |dy| (0..w).map(move |dx| TilePos::new(e.pos.x + dx, e.pos.y + dy)))
        .flat_map(move |t| {
            Dir::ALL.into_iter().filter_map(move |d| {
                let n = t.step(d);
                let inside =
                    n.x >= e.pos.x && n.x < e.pos.x + w && n.y >= e.pos.y && n.y < e.pos.y + h;
                (!inside).then_some((d, n))
            })
        })
}

impl Fluids {
    /// Work out the segments from scratch. `tunnel` gives an underground
    /// pipe end's (exit, partner); `content` what a member slot brings in.
    pub fn build(
        tables: &Tables,
        entities: &Entities,
        tunnel: impl Fn(EntityId) -> Option<(bool, Option<EntityId>)>,
        content: impl Fn(usize) -> Option<(Item, u64)>,
    ) -> Result<Self, Mix> {
        let slots = entities.iter().map(|(id, _)| id.0 as usize + 1).max();
        let n = slots.unwrap_or(0);
        let mut out = Fluids {
            of: vec![None; n],
            open: vec![0; n],
            cap: vec![0; n],
            segs: Vec::new(),
        };
        let mut members = Vec::new();
        for (id, e) in entities.iter() {
            let role = &tables.buildings[e.kind as usize].role;
            let Some(cap) = capacity(role) else {
                continue;
            };
            let slot = id.0 as usize;
            out.cap[slot] = cap;
            out.open[slot] = match role {
                Role::UndergroundPipe { .. } => match tunnel(id) {
                    Some((true, _)) => bit(e.dir),
                    _ => bit(e.dir.opposite()),
                },
                _ => 0b1111,
            };
            members.push((id, *e));
        }
        // Union the members that touch on open sides, and paired ends.
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let join = |parent: &mut Vec<usize>, a: usize, b: usize| {
            let (ra, rb) = (root(parent, a), root(parent, b));
            parent[ra.max(rb)] = ra.min(rb);
        };
        for (id, e) in &members {
            let a = id.0 as usize;
            for (d, tile) in edges(e) {
                if out.open[a] & bit(d) == 0 {
                    continue;
                }
                let Some(other) = entities.at(tile) else {
                    continue;
                };
                let b = other.0 as usize;
                if out.cap[b] > 0 && out.open[b] & bit(d.opposite()) != 0 {
                    join(&mut parent, a, b);
                }
            }
            if let Some((_, Some(p))) = tunnel(*id) {
                if out.cap.get(p.0 as usize).is_some_and(|&c| c > 0) {
                    join(&mut parent, a, p.0 as usize);
                }
            }
        }
        let mut number = vec![None; n];
        for (id, _) in &members {
            let slot = id.0 as usize;
            let r = root(&mut parent, slot);
            let s = *number[r].get_or_insert_with(|| {
                out.segs.push(Segment::default());
                (out.segs.len() - 1) as u32
            });
            out.of[slot] = Some(s);
            let seg = &mut out.segs[s as usize];
            seg.capacity += out.cap[slot];
            seg.tiles += 1;
            if let Some((fluid, amount)) = content(slot) {
                if seg.fluid.is_some_and(|f| f != fluid) {
                    return Err(Mix);
                }
                seg.fluid = Some(fluid);
                seg.amount += amount;
            }
        }
        for seg in &mut out.segs {
            seg.amount = seg.amount.min(seg.capacity);
            if seg.amount == 0 {
                seg.fluid = None;
            }
            seg.budget = seg.throughput();
        }
        Ok(out)
    }

    /// What member `slot` holds: its share of its segment.
    pub fn share(&self, slot: usize) -> Option<(Item, u64)> {
        let seg = &self.segs[self.of.get(slot).copied().flatten()? as usize];
        let fluid = seg.fluid?;
        let amount = seg.amount * self.cap[slot] / seg.capacity.max(1);
        Some((fluid, amount))
    }

    /// Start a tick: each segment may let out its throughput again.
    pub fn refill_budgets(&mut self) {
        for seg in &mut self.segs {
            seg.budget = seg.throughput();
        }
    }

    /// The segment beside `tile`, across its `dir` side, if that member is open toward it.
    pub fn toward(&self, entities: &Entities, tile: TilePos, dir: Dir) -> Option<u32> {
        let slot = entities.at(tile.step(dir))?.0 as usize;
        let seg = self.of.get(slot).copied().flatten()?;
        (self.open[slot] & bit(dir.opposite()) != 0).then_some(seg)
    }

    /// The segments around `e`, each once, in side order.
    pub fn around(&self, entities: &Entities, e: &Entity) -> Vec<u32> {
        let mut segs = Vec::new();
        for (d, tile) in edges(e) {
            let Some(seg) = self.toward(entities, tile.step(d.opposite()), d) else {
                continue;
            };
            if !segs.contains(&seg) {
                segs.push(seg);
            }
        }
        segs
    }

    /// Take up to `want` of `fluid` from the segments around `e`.
    pub fn pull(&mut self, entities: &Entities, e: &Entity, fluid: Item, want: u64) -> u64 {
        let mut got = 0;
        for s in self.around(entities, e) {
            got += self.segs[s as usize].take(fluid, want - got);
            if got == want {
                break;
            }
        }
        got
    }

    /// Give up to `n` of `fluid` to the segments around `e`.
    pub fn fill(&mut self, entities: &Entities, e: &Entity, fluid: Item, n: u64) -> u64 {
        let mut given = 0;
        for s in self.around(entities, e) {
            given += self.segs[s as usize].give(fluid, n - given);
            if given == n {
                break;
            }
        }
        given
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longer_segments_let_less_through() {
        let rate = |tiles| {
            Segment {
                tiles,
                ..Segment::default()
            }
            .throughput()
                * TICKS_PER_SECOND as u64
                / FLUID
        };
        assert!(rate(1) > 1000);
        assert_eq!(rate(20), 600);
        assert!(rate(200) < rate(1) / 9, "{} vs {}", rate(200), rate(1));
    }

    #[test]
    fn a_segment_holds_one_fluid() {
        let mut s = Segment {
            capacity: 10 * FLUID,
            budget: u64::MAX,
            ..Segment::default()
        };
        assert_eq!(s.give(1, 4 * FLUID), 4 * FLUID);
        assert_eq!(s.give(2, FLUID), 0, "another fluid can't go in");
        assert_eq!(s.give(1, 20 * FLUID), 6 * FLUID, "only up to capacity");
        assert_eq!(s.take(2, FLUID), 0);
        assert_eq!(s.take(1, 20 * FLUID), 10 * FLUID);
        assert_eq!(s.fluid, None, "an empty segment takes any fluid again");
        assert_eq!(s.give(2, FLUID), FLUID);
    }
}
