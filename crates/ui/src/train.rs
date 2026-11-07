//! The train sheet (T48): what a train carries and the stops it runs
//! between, set up by touch. Each stop is a row a finger wide: tap its
//! pill to switch between loading and unloading, × to drop it, and "Add
//! stop", then a train stop on the map, to add one.

use super::*;
use mofact_core::command::TrainStop;
use mofact_core::snapshot::{TrainState, TrainView};

/// The most stops a schedule shows; more scroll off the sheet's top.
const ROWS: usize = 6;

/// A train's state, as its chip says it.
fn train_state(s: TrainState) -> (&'static str, Rgba) {
    match s {
        TrainState::Idle => ("No stops", MUTED),
        TrainState::Moving => ("On its way", WORKING),
        TrainState::Waiting => ("Waiting for track", palette::ACCENT),
        TrainState::Loading => ("Loading", WORKING),
        TrainState::Unloading => ("Unloading", WORKING),
        TrainState::NoPath => ("No route", palette::STARVE),
    }
}

/// `stops` with the stop at `tile` added: loading if it is the first,
/// otherwise the opposite of the one before it.
pub fn add_stop(stops: &[TrainStop], tile: mofact_core::geom::TilePos) -> Vec<TrainStop> {
    let load = stops.last().is_none_or(|s| !s.load);
    let mut v = stops.to_vec();
    v.push(TrainStop { at: tile, load });
    v
}

impl Hud {
    /// The train whose schedule is waiting for a stop to be tapped on the map.
    pub fn picking_stop(&self) -> Option<EntityId> {
        self.picking.filter(|_| self.selected == self.picking)
    }

    /// The stop was tapped, or the pick given up.
    pub fn stop_picked(&mut self) {
        self.picking = None;
    }

    pub(crate) fn train_sheet(
        &mut self,
        view: &View,
        o: &mut Overlay,
        t: &TrainView,
        at: [f32; 2],
        width: f32,
    ) {
        let u = self.unit;
        let pad = 16.0 * u;
        let row = 44.0 * u;
        let gap = 6.0 * u;
        let picking = self.picking == Some(t.id);
        let shown = t.schedule.len().min(ROWS);
        let first = t.schedule.len() - shown;
        let height = pad
            + 30.0 * u // title
            + 26.0 * u // cargo
            + 20.0 * u // "Stops" or the empty hint
            + shown as f32 * (row + gap)
            + if picking { 28.0 * u } else { 0.0 }
            + 44.0 * u // buttons
            + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let inner = r[2] - 2.0 * pad;
        let mut y = r[1] + pad;

        // Title, state chip, close.
        let name = view
            .building(&t.building)
            .map_or(t.building.as_str(), |d| d.name.as_str());
        let px = 20.0 * u;
        o.text(name, [left, y + o.cap_height(px) + 4.0 * u], px, TEXT);
        let close = 32.0 * u;
        let c = [r[0] + r[2] - pad - close, y - 2.0 * u, close, close];
        o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
        let xw = o.measure("×", 22.0 * u);
        o.text(
            "×",
            [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
            22.0 * u,
            TEXT,
        );
        self.hits.push((c, Hit::Close));
        let (label, color) = train_state(t.state);
        let spx = 12.0 * u;
        let sw = o.measure(label, spx) + 16.0 * u;
        let sx = c[0] - 8.0 * u - sw;
        o.panel(
            [sx, y + 3.0 * u],
            [sw, 22.0 * u],
            11.0 * u,
            [color[0], color[1], color[2], 0x40],
        );
        o.text(
            label,
            [
                sx + 8.0 * u,
                y + 3.0 * u + (22.0 * u + o.cap_height(spx)) / 2.0,
            ],
            spx,
            color,
        );
        y += 30.0 * u;

        // What it carries, out of how much.
        let px = 14.0 * u;
        let s = 18.0 * u;
        let base = y + (26.0 * u + o.cap_height(px)) / 2.0 - 2.0 * u;
        let held: u32 = t.cargo.iter().map(|c| c.1).sum();
        let mut x = left;
        for (item, n) in &t.cargo {
            let start = x;
            if let Some((sprite, tint)) = view.look.item(item) {
                o.icon(sprite, [x, base - s + 3.0 * u], s, tint);
            }
            x += s + 2.0 * u;
            x += o.text(&format!("{n}"), [x, base], px, TEXT) + 10.0 * u;
            self.hits
                .push(([start, y, x - start, 26.0 * u], Hit::Item(item.clone())));
        }
        let text = if t.cargo.is_empty() {
            format!("Empty · holds {}", t.capacity)
        } else {
            match t.cargo.len() {
                1 => format!("of {}", t.capacity),
                _ => format!("{held} of {}", t.capacity),
            }
        };
        o.text(&text, [x, base], px, MUTED);
        y += 26.0 * u;

        // The stops, in the order it visits them.
        let px = 13.0 * u;
        let hint = if t.schedule.is_empty() {
            "Add a stop to load at, then one to unload at."
        } else {
            "Stops"
        };
        o.text(hint, [left, y + o.cap_height(px) + 2.0 * u], px, MUTED);
        y += 20.0 * u;
        for k in first..t.schedule.len() {
            let stop = &t.schedule[k];
            let next = k == t.next % t.schedule.len().max(1);
            let b = [left, y, inner, row];
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, RAISED);
            if next {
                // The one it is bound for or at.
                o.panel([b[0], b[1]], [4.0 * u, row], 2.0 * u, palette::ACCENT);
            }
            let tpx = 15.0 * u;
            let mid = y + (row + o.cap_height(tpx)) / 2.0;
            o.text(
                &format!("{}. Stop at {}, {}", k + 1, stop.at.x, stop.at.y),
                [left + 12.0 * u, mid],
                tpx,
                TEXT,
            );
            // Drop it: a round × at the end, a finger wide.
            let d = row - 8.0 * u;
            let xb = [b[0] + inner - d - 4.0 * u, y + 4.0 * u, d, d];
            o.panel([xb[0], xb[1]], [d, d], d / 2.0, PRESSED);
            let xw = o.measure("×", 20.0 * u);
            o.text(
                "×",
                [xb[0] + (d - xw) / 2.0, xb[1] + d * 0.7],
                20.0 * u,
                palette::STARVE,
            );
            let mut without = t.schedule.clone();
            without.remove(k);
            self.hits.push((xb, Hit::Schedule(t.id, without)));
            // Load or unload: tap to switch.
            let (word, ink) = if stop.load {
                ("Load", WORKING)
            } else {
                ("Unload", palette::ACCENT)
            };
            let ppx = 14.0 * u;
            let pw = o.measure("Unload", ppx) + 24.0 * u;
            let pb = [xb[0] - 8.0 * u - pw, y + 4.0 * u, pw, d];
            o.panel(
                [pb[0], pb[1]],
                [pw, d],
                d / 2.0,
                [ink[0], ink[1], ink[2], 0x40],
            );
            let ww = o.measure(word, ppx);
            o.text(
                word,
                [
                    pb[0] + (pw - ww) / 2.0,
                    pb[1] + (d + o.cap_height(ppx)) / 2.0,
                ],
                ppx,
                ink,
            );
            let mut flipped = t.schedule.clone();
            flipped[k].load = !stop.load;
            self.hits.push((pb, Hit::Schedule(t.id, flipped)));
            y += row + gap;
        }
        if picking {
            let px = 14.0 * u;
            o.text(
                "Tap a train stop on the map to add it.",
                [left, y + o.cap_height(px) + 4.0 * u],
                px,
                palette::ACCENT,
            );
            y += 28.0 * u;
        }

        // Add a stop (or stop adding one), and remove the train.
        let bh = 40.0 * u;
        let bw = (inner - 8.0 * u) / 2.0;
        let add = if picking { "Cancel" } else { "Add stop" };
        let buttons = [
            (add, Hit::PickStop(t.id), TEXT),
            ("Remove", Hit::Remove(t.id), palette::STARVE),
        ];
        for (i, (label, hit, ink)) in buttons.into_iter().enumerate() {
            let b = [left + i as f32 * (bw + 8.0 * u), y, bw, bh];
            o.panel([b[0], b[1]], [bw, bh], 10.0 * u, RAISED);
            let px = 15.0 * u;
            let lw = o.measure(label, px);
            o.text(
                label,
                [b[0] + (bw - lw) / 2.0, b[1] + (bh + o.cap_height(px)) / 2.0],
                px,
                ink,
            );
            self.hits.push((b, hit));
        }
    }
}
