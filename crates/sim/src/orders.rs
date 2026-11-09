//! Hub orders: once the guided goals are done, the hub asks for one item at
//! a time against the clock. Shipping an order takes its items out of the
//! stock and pays its reward; each one shipped in a row makes the next
//! bigger and better paid, and a missed one starts the streak over. Orders
//! open and close twice a second with the goals, so they cost nothing
//! between.

use crate::tables::Item;
use crate::{OrderPace, Sim};
use mofact_core::snapshot::OrderView;
use mofact_core::{Tick, TICKS_PER_SECOND};

/// The pause after an order ships before the next opens.
pub const AFTER_SHIPPED: Tick = 45 * TICKS_PER_SECOND as Tick;
/// The longer pause after one is missed.
pub const AFTER_MISSED: Tick = 120 * TICKS_PER_SECOND as Tick;
/// How many of the latest orders the factory can fill the next is picked from.
const LATEST: usize = 4;
/// Each order shipped in a row adds a fifth, up to three times as much.
const MOST_STREAK: u32 = 10;

/// One order from the content, ready to use.
#[derive(Clone, Debug)]
pub struct OrderRow {
    /// None if its item no longer exists, so it never opens.
    pub item: Option<Item>,
    pub count: u32,
    pub ticks: Tick,
    pub reward: Vec<(Item, u32)>,
}

/// The order open now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Open {
    pub index: u16,
    pub due: Tick,
    /// How many of its item had reached the hub when it opened.
    pub base: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Orders {
    pub open: Option<Open>,
    pub streak: u32,
    /// When the next order opens, once none is.
    pub next: Tick,
    pub shipped: u32,
    pub missed: u32,
    /// The order opened last, so the next is another.
    pub last: Option<u16>,
}

/// `n` grown by a streak of `streak`.
pub fn scaled(n: u32, streak: u32) -> u32 {
    let s = streak.min(MOST_STREAK);
    (u64::from(n) * u64::from(5 + s) / 5) as u32
}

impl Sim {
    /// Ship, miss or open an order. Nothing happens until the goals are done.
    pub(crate) fn check_orders(&mut self) {
        if (self.goal as usize) < self.tables.goals.len()
            || self.tables.orders.is_empty()
            || self.rules.orders == OrderPace::Off
        {
            return;
        }
        let tick = self.tick;
        let o = &mut self.orders;
        if let Some(open) = o.open {
            let row = &self.tables.orders[open.index as usize];
            let Some(item) = row.item else {
                o.open = None;
                return;
            };
            let need = scaled(row.count, o.streak);
            let have = self.delivered[item as usize].saturating_sub(open.base);
            if have >= u64::from(need) && self.stock[item as usize] >= need {
                self.stock[item as usize] -= need;
                let pace = self.rules.orders;
                for &(i, n) in &row.reward {
                    self.stock[i as usize] += pace.reward(scaled(n, o.streak));
                }
                o.shipped += 1;
                o.streak += 1;
                o.open = None;
                o.next = tick + AFTER_SHIPPED;
            } else if tick >= open.due {
                o.missed += 1;
                o.streak = 0;
                o.open = None;
                o.next = tick + AFTER_MISSED;
            }
            return;
        }
        if tick < o.next {
            return;
        }
        // The orders whose item has reached the hub, latest last.
        let can: Vec<u16> = (0..self.tables.orders.len() as u16)
            .filter(|&k| {
                self.tables.orders[k as usize]
                    .item
                    .is_some_and(|i| self.delivered[i as usize] > 0)
            })
            .collect();
        let mut pick: Vec<u16> = can[can.len().saturating_sub(LATEST)..].to_vec();
        if pick.len() > 1 {
            pick.retain(|&k| Some(k) != o.last);
        }
        if pick.is_empty() {
            o.next = tick + AFTER_SHIPPED;
            return;
        }
        // The same save and inputs always pick the same order.
        let seq = u64::from(o.shipped + o.missed);
        let k = pick[(seq.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 33) as usize % pick.len()];
        let row = &self.tables.orders[k as usize];
        let item = row.item.expect("only orders with an item are picked");
        o.open = Some(Open {
            index: k,
            due: tick + self.rules.orders.ticks(row.ticks),
            base: self.delivered[item as usize],
        });
        o.last = Some(k);
    }

    /// The open order, for the snapshot.
    pub(crate) fn order_view(&self) -> Option<OrderView> {
        let open = self.orders.open?;
        let row = &self.tables.orders[open.index as usize];
        let item = row.item?;
        let streak = self.orders.streak;
        let need = scaled(row.count, streak);
        let have = self.delivered[item as usize].saturating_sub(open.base);
        let name = |i: Item| self.tables.items[i as usize].clone();
        Some(OrderView {
            index: open.index,
            item: name(item),
            have: have.min(u64::from(need)) as u32,
            need,
            seconds_left: (open.due.saturating_sub(self.tick) / TICKS_PER_SECOND as Tick) as u32,
            streak,
            reward: row
                .reward
                .iter()
                .map(|&(i, n)| (name(i), self.rules.orders.reward(scaled(n, streak))))
                .collect(),
        })
    }
}
