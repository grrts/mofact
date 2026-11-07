//! The prototype's drawing code, ported call for call from its canvas
//! functions (`drawItem`, `drawBelt`, `drawBuilding`). Sizes are in pixels:
//! `z` is pixels per tile, and a building's top-left tile starts at (0, 0).
//!
//! `fine` is the prototype's close-zoom level of detail (bolts, firebrick,
//! hazard stripes); without it, the shapes are simpler for small sizes.

use crate::canvas::{ellipse, poly, rect, rr, Canvas};
use crate::palette::{self as pal, alpha, shade, Rgba};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use tiny_skia::PathBuilder;

/// The look of an item: one of the prototype's icon shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Ore,
    Plate,
    Gear,
    Wire,
    Circuit,
    Droplet,
    Pack,
    Powder,
    Cell,
    Wafer,
    Chip,
    Frame,
    Magnet,
    Satellite,
    Nozzle,
}

/// The four directions, as `(dx, dy)` steps, in the core's `Dir` order.
const DIRS: [(f32, f32); 4] = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];

/// A building's state, for the parts of it that move or glow.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    /// The way it faces, 0 to 3 (east, south, west, north).
    pub dir: u8,
    pub working: bool,
    /// Craft progress, 0 to 1.
    pub progress: f32,
    /// Seconds, for spinning and flicker.
    pub t: f32,
}

impl Pose {
    fn step(&self) -> (f32, f32) {
        DIRS[usize::from(self.dir % 4)]
    }

    /// A point on a 1×1 machine in its own frame: `fx` along the way it
    /// faces, `fy` to its right, from the tile's centre.
    fn local(&self, z: f32, fx: f32, fy: f32) -> (f32, f32) {
        let (dx, dy) = self.step();
        (z * (0.5 + dx * fx - dy * fy), z * (0.5 + dy * fx + dx * fy))
    }
}

/// A filled triangle pointing in `dir` (0 to 3), `s` pixels long.
fn arrow(c: &mut Canvas, cx: f32, cy: f32, dir: u8, s: f32, color: Rgba) {
    c.save();
    c.translate(cx, cy);
    c.rotate(f32::from(dir) * FRAC_PI_2);
    if let Some(p) = poly(&[(0.5, 0.0), (0.15, -0.28), (0.15, 0.28)], 0.0, 0.0, s) {
        c.fill(&p, color);
    }
    c.restore();
}

fn bolt(c: &mut Canvas, x: f32, y: f32, r: f32) {
    c.fill_circle(x, y, r, pal::MACHINE_EDGE);
    c.fill_circle(x - r * 0.2, y - r * 0.2, r * 0.55, pal::MACHINE_HI);
}

/// The output arrow on the side a machine faces.
fn notch(c: &mut Canvas, z: f32, pose: &Pose) {
    let (x, y) = pose.local(z, 0.38, 0.0);
    arrow(c, x, y, pose.dir % 4, z * 0.32, pal::ACCENT);
}

fn gear_points(n: usize, inner: f32, outer: f32) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            let r = if i % 2 == 1 { inner } else { outer };
            (a.cos() * r, a.sin() * r)
        })
        .collect()
}

/// How many different [`mark`]s there are, not counting none.
pub const MARKS: u8 = 10;

/// A small sign on the lower right of an item icon `s` pixels across,
/// centred on (`x`, `y`), that tells it from others of the same shape
/// without its colour (T58): `k` from 1 to [`MARKS`] is a dot, a bar, a
/// ring, a cross, a triangle, two dots, a square, a diamond, three dots or
/// a dot in a ring; 0 is none.
pub fn mark(c: &mut Canvas, k: u8, x: f32, y: f32, s: f32) {
    if k == 0 {
        return;
    }
    let r = (s * 0.2).max(2.0);
    let (cx, cy) = (x + s * 0.5 - r, y + s * 0.5 - r);
    c.fill_circle(cx, cy, r, pal::GROUND);
    let ink = pal::WHITE;
    let g = r * 0.62;
    let w = (r * 0.3).max(1.0);
    match (k - 1) % MARKS {
        0 => c.fill_circle(cx, cy, g * 0.55, ink),
        1 => c.fill_rect(cx - g, cy - w / 2.0, 2.0 * g, w, ink),
        2 => c.stroke_circle(cx, cy, g * 0.75, ink, w),
        3 => {
            c.fill_rect(cx - g, cy - w / 2.0, 2.0 * g, w, ink);
            c.fill_rect(cx - w / 2.0, cy - g, w, 2.0 * g, ink);
        }
        4 => {
            let t = [(0.0, -1.0), (0.9, 0.7), (-0.9, 0.7)];
            if let Some(p) = poly(&t, cx, cy, g * 2.0 * 0.5) {
                c.fill(&p, ink);
            }
        }
        5 => {
            c.fill_circle(cx - g * 0.5, cy, g * 0.38, ink);
            c.fill_circle(cx + g * 0.5, cy, g * 0.38, ink);
        }
        6 => c.fill_rect(cx - g * 0.6, cy - g * 0.6, g * 1.2, g * 1.2, ink),
        7 => {
            let d = [(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)];
            if let Some(p) = poly(&d, cx, cy, g * 0.9) {
                c.fill(&p, ink);
            }
        }
        8 => {
            for (dx, dy) in [(0.0, -0.55), (-0.55, 0.4), (0.55, 0.4)] {
                c.fill_circle(cx + g * dx, cy + g * dy, g * 0.32, ink);
            }
        }
        _ => {
            c.stroke_circle(cx, cy, g * 0.8, ink, w * 0.8);
            c.fill_circle(cx, cy, g * 0.3, ink);
        }
    }
}

/// An item icon `s` pixels across, centred on (`x`, `y`). `flat` leaves out
/// the shading, as on belts seen from far away.
pub fn item(c: &mut Canvas, shape: Shape, color: Rgba, x: f32, y: f32, s: f32, flat: bool) {
    let (hi, lo) = (shade(color, 0.35), shade(color, -0.35));
    let detail = !flat && s >= 9.0;
    if detail {
        if let Some(p) = ellipse(x + s * 0.08, y + s * 0.12, s * 0.5, s * 0.42) {
            c.fill(&p, [0, 0, 0, 77]);
        }
    }
    let fill = |c: &mut Canvas, p: Option<tiny_skia::Path>, col: Rgba| {
        if let Some(p) = p {
            c.fill(&p, col);
        }
    };
    match shape {
        Shape::Ore => {
            const P: [(f32, f32); 6] = [
                (-0.5, -0.2),
                (-0.15, -0.5),
                (0.35, -0.4),
                (0.5, 0.1),
                (0.2, 0.5),
                (-0.35, 0.4),
            ];
            fill(c, poly(&P, x, y, s), color);
            if detail {
                let dark = [(0.05, -0.05), (0.5, 0.1), (0.2, 0.5), (-0.35, 0.4)];
                fill(c, poly(&dark, x, y, s), lo);
                let light = [(-0.5, -0.2), (-0.15, -0.5), (0.05, -0.05)];
                fill(c, poly(&light, x, y, s), hi);
            } else {
                c.fill_rect(
                    x - s * 0.15,
                    y - s * 0.25,
                    s * 0.2,
                    s * 0.15,
                    [255, 255, 255, 64],
                );
            }
        }
        Shape::Plate => {
            fill(c, rr(x - s / 2.0, y - s / 2.0, s, s, s * 0.2), color);
            if detail {
                c.fill_rect(x - s * 0.4, y + s * 0.3, s * 0.8, s * 0.12, lo);
                c.fill_rect(x - s * 0.36, y - s * 0.38, s * 0.72, s * 0.1, hi);
                let y0 = y + s * 0.02;
                c.line(
                    &[(x - s * 0.2, y0), (x + s * 0.2, y0)],
                    lo,
                    (s * 0.06).max(1.0),
                );
            } else {
                c.fill_rect(
                    x - s * 0.32,
                    y - s * 0.32,
                    s * 0.64,
                    s * 0.12,
                    [255, 255, 255, 77],
                );
            }
        }
        Shape::Gear => {
            fill(c, poly(&gear_points(16, 0.36, 0.5), x, y, s), color);
            if detail {
                c.stroke_circle(x, y, s * 0.25, lo, (s * 0.07).max(1.0));
            }
            c.fill_circle(x, y, s * 0.13, pal::BELT);
        }
        Shape::Wire => {
            if detail {
                c.fill_circle(x, y, s * 0.44, lo);
            }
            let w = (s * 0.14).max(1.0);
            c.stroke_circle(x, y, s * 0.34, color, w);
            if detail {
                c.stroke_arc(
                    (x, y),
                    s * 0.34,
                    (PI * 1.1, PI * 1.6),
                    hi,
                    (s * 0.07).max(1.0),
                );
            }
            c.stroke_circle(x, y, s * 0.15, color, w);
        }
        Shape::Circuit => {
            if detail {
                for k in -1..=1 {
                    let py = y + k as f32 * s * 0.22 - s * 0.04;
                    c.fill_rect(x - s * 0.58, py, s * 0.1, s * 0.08, pal::PIN);
                    c.fill_rect(x + s * 0.48, py, s * 0.1, s * 0.08, pal::PIN);
                }
            }
            fill(c, rr(x - s / 2.0, y - s * 0.4, s, s * 0.8, s * 0.1), color);
            if detail {
                c.fill_rect(x - s * 0.16, y - s * 0.16, s * 0.32, s * 0.32, pal::CHIP);
                let trace = shade(color, 0.4);
                c.fill_rect(x - s * 0.4, y - s * 0.3, s * 0.2, s * 0.05, trace);
                c.fill_rect(x + s * 0.2, y + s * 0.24, s * 0.2, s * 0.05, trace);
            } else {
                c.fill_rect(x - s * 0.3, y - s * 0.18, s * 0.6, s * 0.1, pal::CHIP);
                c.fill_rect(x - s * 0.3, y + s * 0.06, s * 0.35, s * 0.1, pal::CHIP);
            }
        }
        Shape::Droplet => {
            // Not in the prototype: a drop for fluids, in the same flat style.
            let mut pb = PathBuilder::new();
            pb.move_to(x, y - s * 0.5);
            pb.cubic_to(
                x + s * 0.2,
                y - s * 0.2,
                x + s * 0.36,
                y - s * 0.02,
                x + s * 0.36,
                y + s * 0.14,
            );
            pb.cubic_to(
                x + s * 0.36,
                y + s * 0.36,
                x + s * 0.2,
                y + s * 0.5,
                x,
                y + s * 0.5,
            );
            pb.cubic_to(
                x - s * 0.2,
                y + s * 0.5,
                x - s * 0.36,
                y + s * 0.36,
                x - s * 0.36,
                y + s * 0.14,
            );
            pb.cubic_to(
                x - s * 0.36,
                y - s * 0.02,
                x - s * 0.2,
                y - s * 0.2,
                x,
                y - s * 0.5,
            );
            pb.close();
            fill(c, pb.finish(), color);
            if detail {
                c.fill_circle(x - s * 0.12, y + s * 0.08, s * 0.08, hi);
            }
        }
        Shape::Pack => {
            // Not in the prototype: a research pack, a stoppered flask with
            // its colour inside, in the same flat style.
            let glass = [0xe9, 0xed, 0xf0, 0xff];
            fill(
                c,
                rr(x - s * 0.13, y - s * 0.5, s * 0.26, s * 0.14, s * 0.04),
                pal::BRICK[0],
            );
            fill(
                c,
                rr(x - s * 0.1, y - s * 0.4, s * 0.2, s * 0.24, 0.0),
                glass,
            );
            if let Some(p) = PathBuilder::from_circle(x, y + s * 0.14, s * 0.36) {
                c.fill(&p, glass);
            }
            if let Some(p) = PathBuilder::from_circle(x, y + s * 0.14, s * 0.29) {
                c.fill(&p, color);
            }
            if detail {
                c.fill_circle(x - s * 0.12, y + s * 0.02, s * 0.07, hi);
                c.stroke_circle(x, y + s * 0.14, s * 0.36, lo, (s * 0.04).max(1.0));
            }
        }
        Shape::Powder => {
            // A heap of powder: a low mound with a lit crown and loose grains.
            fill(c, ellipse(x, y + s * 0.22, s * 0.5, s * 0.24), lo);
            let mound = [
                (-0.5, 0.24),
                (-0.3, -0.08),
                (-0.08, -0.3),
                (0.12, -0.3),
                (0.32, -0.06),
                (0.5, 0.24),
            ];
            fill(c, poly(&mound, x, y, s), color);
            fill(
                c,
                ellipse(x - s * 0.02, y - s * 0.14, s * 0.18, s * 0.1),
                hi,
            );
            if detail {
                for (gx, gy) in [(-0.36, 0.34), (0.3, 0.38), (0.42, 0.3)] {
                    c.fill_circle(x + gx * s, y + gy * s, s * 0.04, color);
                }
            }
        }
        Shape::Cell => {
            // A battery standing up: a terminal, a coloured wrap, a dark base.
            fill(
                c,
                rr(x - s * 0.1, y - s * 0.5, s * 0.2, s * 0.1, s * 0.03),
                pal::MACHINE_HI,
            );
            fill(
                c,
                rr(x - s * 0.28, y - s * 0.42, s * 0.56, s * 0.9, s * 0.1),
                pal::MACHINE_EDGE,
            );
            fill(
                c,
                rr(x - s * 0.24, y - s * 0.38, s * 0.48, s * 0.56, s * 0.07),
                color,
            );
            c.fill_rect(x - s * 0.24, y + s * 0.22, s * 0.48, s * 0.06, pal::ACCENT);
            if detail {
                c.fill_rect(x - s * 0.18, y - s * 0.32, s * 0.08, s * 0.44, hi);
                let w = (s * 0.06).max(1.0);
                c.line(
                    &[(x - s * 0.1, y - s * 0.1), (x + s * 0.1, y - s * 0.1)],
                    pal::WHITE,
                    w,
                );
                c.line(&[(x, y - s * 0.2), (x, y)], pal::WHITE, w);
            }
        }
        Shape::Wafer => {
            // A round wafer with a flat cut on one side and a grid of dies.
            let r = s * 0.48;
            c.save();
            if let Some(p) = rr(x - r, y - r, r * 2.0, r * 1.82, 0.0) {
                c.clip(&p);
            }
            c.fill_circle(x, y, r, lo);
            c.fill_circle(x, y - s * 0.03, r * 0.94, color);
            if detail {
                let w = (s * 0.03).max(1.0);
                for k in [-0.2, 0.0, 0.2] {
                    c.line(&[(x + k * s, y - r), (x + k * s, y + r)], lo, w);
                    c.line(&[(x - r, y + k * s), (x + r, y + k * s)], lo, w);
                }
            }
            c.restore();
            c.stroke_arc(
                (x, y - s * 0.03),
                r * 0.78,
                (PI * 1.05, PI * 1.45),
                hi,
                (s * 0.07).max(1.0),
            );
        }
        Shape::Chip => {
            // A square package with pins on all four sides and a bright die.
            if detail {
                let w = s * 0.08;
                for k in -1..=1 {
                    let o = k as f32 * s * 0.22 - w / 2.0;
                    c.fill_rect(x + o, y - s * 0.5, w, s * 0.1, pal::PIN);
                    c.fill_rect(x + o, y + s * 0.4, w, s * 0.1, pal::PIN);
                    c.fill_rect(x - s * 0.5, y + o, s * 0.1, w, pal::PIN);
                    c.fill_rect(x + s * 0.4, y + o, s * 0.1, w, pal::PIN);
                }
            } else {
                fill(c, rr(x - s * 0.5, y - s * 0.5, s, s, s * 0.08), pal::PIN);
            }
            fill(
                c,
                rr(x - s * 0.4, y - s * 0.4, s * 0.8, s * 0.8, s * 0.08),
                color,
            );
            fill(
                c,
                rr(x - s * 0.2, y - s * 0.2, s * 0.4, s * 0.4, s * 0.04),
                hi,
            );
            if detail {
                c.fill_circle(x - s * 0.28, y - s * 0.28, s * 0.04, lo);
            }
        }
        Shape::Magnet => {
            // A horseshoe magnet, its two poles tipped in steel.
            let w = s * 0.26;
            let (cy, r) = (y - s * 0.02, s * 0.3);
            c.stroke_arc((x, cy), r, (0.0, PI), lo, w * 1.25);
            c.stroke_arc((x, cy), r, (0.0, PI), color, w);
            for side in [-1.0, 1.0] {
                let px = x + side * r - w / 2.0;
                c.fill_rect(px, y - s * 0.46, w, s * 0.44, color);
                c.fill_rect(px, y - s * 0.5, w, s * 0.14, pal::DRILL_BIT);
            }
            if detail {
                c.stroke_arc(
                    (x, cy),
                    r + w * 0.3,
                    (PI * 0.6, PI * 0.9),
                    hi,
                    (s * 0.05).max(1.0),
                );
            }
        }
        Shape::Satellite => {
            // A body between two wings of solar cells, struts between.
            let w = (s * 0.06).max(1.0);
            c.line(&[(x - s * 0.3, y), (x + s * 0.3, y)], pal::PIPE_HI, w);
            for side in [-1.0, 1.0] {
                let wx = x + side * s * 0.34 - s * 0.16;
                fill(
                    c,
                    rr(wx, y - s * 0.3, s * 0.32, s * 0.6, s * 0.03),
                    pal::SOLAR,
                );
                if detail {
                    let lw = (s * 0.03).max(1.0);
                    c.line(
                        &[(wx + s * 0.16, y - s * 0.3), (wx + s * 0.16, y + s * 0.3)],
                        shade(pal::SOLAR, 0.3),
                        lw,
                    );
                    c.line(&[(wx, y), (wx + s * 0.32, y)], shade(pal::SOLAR, 0.3), lw);
                }
            }
            fill(
                c,
                rr(x - s * 0.14, y - s * 0.2, s * 0.28, s * 0.4, s * 0.06),
                color,
            );
            c.fill_circle(x, y - s * 0.04, s * 0.07, hi);
            if detail {
                c.fill_rect(x - s * 0.1, y + s * 0.1, s * 0.2, s * 0.04, lo);
            }
        }
        Shape::Nozzle => {
            // A bell opening downwards under its chamber, glowing inside.
            fill(
                c,
                rr(x - s * 0.16, y - s * 0.5, s * 0.32, s * 0.26, s * 0.08),
                lo,
            );
            let bell = [(-0.14, -0.26), (0.14, -0.26), (0.42, 0.46), (-0.42, 0.46)];
            fill(c, poly(&bell, x, y, s), color);
            fill(
                c,
                ellipse(x, y + s * 0.44, s * 0.36, s * 0.07),
                pal::FLAME[0],
            );
            if detail {
                let light = [(-0.14, -0.26), (-0.04, -0.26), (-0.22, 0.42), (-0.36, 0.42)];
                fill(c, poly(&light, x, y, s), hi);
            }
        }
        Shape::Frame => {
            // An open square of struts, braced corner to corner, round a core.
            let w = (s * 0.13).max(1.0);
            let k = s * 0.4;
            let ring = [
                (x - k, y - k),
                (x + k, y - k),
                (x + k, y + k),
                (x - k, y + k),
                (x - k, y - k),
            ];
            c.line(&ring, lo, w * 1.4);
            c.line(&ring, color, w);
            c.line(&[(x - k, y - k), (x + k, y + k)], color, w * 0.7);
            c.line(&[(x + k, y - k), (x - k, y + k)], color, w * 0.7);
            c.fill_circle(x, y, s * 0.17, pal::MACHINE_EDGE);
            c.fill_circle(x, y, s * 0.11, pal::ACCENT);
            if detail {
                for (cx, cy) in [(-k, -k), (k, -k), (k, k), (-k, k)] {
                    c.fill_circle(x + cx, y + cy, s * 0.06, hi);
                }
            }
        }
    }
}

/// A belt tile running east, `z` pixels across, with its surface moved on by
/// `t` seconds. `fine` adds the rollers, chevrons and rivets of close zoom.
pub fn belt(c: &mut Canvas, z: f32, fine: bool, t: f32, speed: f32) {
    c.save();
    c.translate(z / 2.0, z / 2.0);
    c.fill_rect(-z * 0.5, -z * 0.34, z, z * 0.78, [0, 0, 0, 64]);
    c.fill_rect(-z * 0.5, -z * 0.38, z, z * 0.76, pal::BELT);
    if !fine {
        c.fill_rect(-z * 0.5, -z * 0.38, z, z * 0.07, pal::BELT_RAIL);
        c.fill_rect(-z * 0.5, z * 0.31, z, z * 0.07, pal::BELT_RAIL);
        c.fill_rect(-z * 0.5, -z * 0.1, z, z * 0.2, alpha(pal::CHEVRON, 0.9));
        arrow(c, 0.0, 0.0, 0, z * 0.7, pal::INK);
    } else {
        c.save();
        if let Some(p) = rect(-z * 0.5, -z * 0.31, z, z * 0.62) {
            c.clip(&p);
        }
        let off = ((t * speed) % 0.5) * z;
        for k in -2..=2 {
            let x = k as f32 * z * 0.25 + (off % (z * 0.25)) - z * 0.02;
            c.fill_rect(x, -z * 0.31, z * 0.04, z * 0.62, pal::ROLLER);
        }
        for k in -2..=2 {
            let x0 = k as f32 * z * 0.5 + off;
            c.line(
                &[
                    (x0 - z * 0.1, -z * 0.17),
                    (x0 + z * 0.05, 0.0),
                    (x0 - z * 0.1, z * 0.17),
                ],
                pal::CHEVRON,
                (z * 0.05).max(1.0),
            );
        }
        c.restore();
        c.fill_rect(-z * 0.5, -z * 0.4, z, z * 0.09, pal::BELT_RAIL);
        c.fill_rect(-z * 0.5, z * 0.31, z, z * 0.09, pal::BELT_RAIL);
        c.fill_rect(-z * 0.5, -z * 0.4, z, z * 0.025, pal::RAIL_HI);
        c.fill_rect(-z * 0.5, z * 0.31, z, z * 0.025, pal::RAIL_HI);
        for bx in [-0.25, 0.25] {
            c.fill_circle(bx * z, -z * 0.355, z * 0.022, pal::RIVET);
            c.fill_circle(bx * z, z * 0.355, z * 0.022, pal::RIVET);
        }
    }
    c.restore();
}

fn shadow(c: &mut Canvas, p: Option<tiny_skia::Path>) {
    if let Some(p) = p {
        c.fill(&p, pal::SHADOW);
    }
}

fn fill(c: &mut Canvas, p: Option<tiny_skia::Path>, col: Rgba) {
    if let Some(p) = p {
        c.fill(&p, col);
    }
}

/// The hub: a 3×3 landing pad.
pub fn hub(c: &mut Canvas, z: f32, fine: bool, t: f32) {
    let s = z * 3.0;
    let sh = z * 0.07;
    shadow(
        c,
        rr(
            z * 0.05 + sh,
            z * 0.05 + sh * 1.3,
            s - z * 0.1,
            s - z * 0.1,
            z * 0.35,
        ),
    );
    fill(
        c,
        rr(z * 0.05, z * 0.05, s - z * 0.1, s - z * 0.1, z * 0.35),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(z * 0.15, z * 0.15, s - z * 0.3, s - z * 0.3, z * 0.3),
        pal::MACHINE,
    );
    if fine {
        // A hazard band around the landing pad.
        c.save();
        if let Some(p) = rr(z * 0.3, z * 0.3, s - z * 0.6, s - z * 0.6, z * 0.22) {
            c.clip(&p);
        }
        c.fill_rect(0.0, 0.0, s, s, pal::ACCENT);
        for k in -6..12 {
            let x = k as f32 * z * 0.4;
            c.line(&[(x, 0.0), (x - s, s)], pal::HAZARD_DARK, z * 0.12);
        }
        c.restore();
        fill(c, rr(z * 0.5, z * 0.5, s - z, s - z, z * 0.16), pal::PAD);
        for k in 1..4 {
            let y = z * 0.5 + k as f32 * (s - z) / 4.0;
            c.line(&[(z * 0.5, y), (s - z * 0.5, y)], pal::MACHINE, 1.0);
        }
        // Intake arrows on each side, and corner lamps.
        for (d, (dx, dy)) in DIRS.iter().enumerate() {
            let (x, y) = (s / 2.0 - dx * s * 0.41, s / 2.0 - dy * s * 0.41);
            arrow(c, x, y, d as u8, z * 0.28, pal::MACHINE_HI);
        }
        let lamp = if (t * 3.0).sin() > 0.0 {
            pal::ACCENT
        } else {
            pal::LAMP_OFF
        };
        for (qx, qy) in [(0.22, 0.22), (2.78, 0.22), (0.22, 2.78), (2.78, 2.78)] {
            c.fill_circle(qx * z, qy * z, z * 0.06, lamp);
        }
    } else if let Some(p) = rr(z * 0.5, z * 0.5, s - z, s - z, z * 0.2) {
        c.stroke(&p, pal::ACCENT, (z * 0.06).max(1.5));
    }
}

/// A drill over ore of colour `ore`.
pub fn drill(c: &mut Canvas, z: f32, ore: Rgba, fine: bool, pose: &Pose) {
    let (s, sh, lw) = (z, z * 0.07, (z * 0.05).max(1.0));
    let (cx, cy) = (s / 2.0, s / 2.0);
    shadow(
        c,
        rr(
            s * 0.06 + sh,
            s * 0.06 + sh * 1.3,
            s * 0.88,
            s * 0.88,
            s * 0.12,
        ),
    );
    fill(
        c,
        rr(s * 0.06, s * 0.06, s * 0.88, s * 0.88, s * 0.12),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.1, s * 0.1, s * 0.8, s * 0.8, s * 0.1),
        pal::MACHINE,
    );
    if fine {
        fill(
            c,
            rr(s * 0.1, s * 0.1, s * 0.8, s * 0.12, s * 0.06),
            shade(pal::MACHINE, 0.12),
        );
        for (bx, by) in [(0.18, 0.18), (0.82, 0.18), (0.18, 0.82), (0.82, 0.82)] {
            bolt(c, bx * s, by * s, s * 0.035);
        }
        // The exhaust stack at the back.
        let (ex, ey) = pose.local(z, -0.3, -0.3);
        c.fill_circle(ex, ey, s * 0.07, pal::MACHINE_EDGE);
    }
    c.fill_circle(cx, cy, s * 0.28, shade(ore, -0.45));
    c.fill_circle(cx, cy, s * 0.23, ore);
    c.save();
    c.translate(cx, cy);
    c.rotate(if pose.working { pose.t * 5.0 } else { 0.0 });
    if fine {
        for _ in 0..3 {
            c.rotate(TAU / 3.0);
            let blade = [(-0.04, 0.0), (0.04, 0.0), (0.02, -0.2), (-0.05, -0.17)];
            fill(c, poly(&blade, 0.0, 0.0, s), pal::DRILL_BIT);
        }
        c.fill_circle(0.0, 0.0, s * 0.06, pal::MACHINE_EDGE);
    } else {
        let mut pb = PathBuilder::new();
        pb.move_to(-s * 0.18, 0.0);
        pb.line_to(s * 0.18, 0.0);
        pb.move_to(0.0, -s * 0.18);
        pb.line_to(0.0, s * 0.18);
        if let Some(p) = pb.finish() {
            c.stroke(&p, pal::INK, lw * 1.4);
        }
    }
    c.restore();
    notch(c, z, pose);
}

/// A furnace; `ring` is the colour of what it smelts, if it has started.
pub fn furnace(c: &mut Canvas, z: f32, ring: Option<Rgba>, fine: bool, pose: &Pose) {
    let (s, sh, lw) = (z, z * 0.07, (z * 0.05).max(1.0));
    let (cx, cy) = (s / 2.0, s / 2.0);
    shadow(
        c,
        PathBuilder::from_circle(cx + sh, cy + sh * 1.3, s * 0.46),
    );
    c.fill_circle(cx, cy, s * 0.46, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.41, pal::MACHINE);
    let ring = ring.unwrap_or(pal::MACHINE_HI);
    if fine {
        // Firebrick around the hearth.
        let n = 10;
        for k in 0..n {
            let a0 = k as f32 / n as f32 * TAU + 0.06;
            let a1 = (k + 1) as f32 / n as f32 * TAU - 0.06;
            c.stroke_arc((cx, cy), s * 0.345, (a0, a1), pal::BRICK[k % 2], s * 0.09);
        }
        c.stroke_circle(cx, cy, s * 0.41, ring, lw);
        let (chx, chy) = pose.local(z, -0.32, 0.3);
        fill(
            c,
            rr(chx - s * 0.08, chy - s * 0.08, s * 0.16, s * 0.16, s * 0.03),
            pal::MACHINE_EDGE,
        );
        c.fill_rect(
            chx - s * 0.045,
            chy - s * 0.045,
            s * 0.09,
            s * 0.09,
            pal::SOOT,
        );
    } else {
        c.stroke_circle(cx, cy, s * 0.34, ring, lw * 1.4);
    }
    if pose.working {
        let fl = 0.8 + 0.2 * (pose.t * 11.0).sin();
        c.fill_circle(cx, cy, s * 0.24, pal::FLAME[0]);
        c.fill_circle(cx, cy, s * 0.16 * fl, pal::FLAME[1]);
        c.fill_circle(cx, cy, s * 0.07 * fl, pal::FLAME[2]);
    } else {
        c.fill_circle(cx, cy, s * 0.24, pal::HEARTH);
    }
    if fine {
        let w = (s * 0.025).max(1.0);
        for k in [-0.08, 0.0, 0.08] {
            c.line(
                &[(cx - s * 0.2, cy + k * s), (cx + s * 0.2, cy + k * s)],
                pal::SOOT,
                w,
            );
        }
    }
    notch(c, z, pose);
}

/// An assembler making `product` (its shape and colour), if it has a recipe.
pub fn assembler(c: &mut Canvas, z: f32, product: Option<(Shape, Rgba)>, fine: bool, pose: &Pose) {
    let (s, sh) = (z, z * 0.07);
    let (cx, cy) = (s / 2.0, s / 2.0);
    let col = product.map_or(pal::MACHINE_HI, |p| p.1);
    shadow(
        c,
        rr(
            s * 0.04 + sh,
            s * 0.04 + sh * 1.3,
            s * 0.92,
            s * 0.92,
            s * 0.08,
        ),
    );
    fill(
        c,
        rr(s * 0.04, s * 0.04, s * 0.92, s * 0.92, s * 0.08),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.08, s * 0.08, s * 0.84, s * 0.84, s * 0.06),
        pal::MACHINE,
    );
    // The tier stripe, in the product's colour.
    c.fill_rect(s * 0.08, s * 0.08, s * 0.84, s * 0.12, col);
    if fine {
        c.save();
        if let Some(p) = rect(s * 0.08, s * 0.08, s * 0.84, s * 0.12) {
            c.clip(&p);
        }
        for k in 0..8 {
            let k = k as f32;
            c.line(
                &[
                    (s * (0.1 + k * 0.12), s * 0.2),
                    (s * (0.16 + k * 0.12), s * 0.08),
                ],
                shade(col, -0.35),
                s * 0.04,
            );
        }
        c.restore();
        c.line(
            &[(s * 0.08, s * 0.74), (s * 0.92, s * 0.74)],
            pal::MACHINE_EDGE,
            1.0,
        );
        for (bx, by) in [(0.16, 0.3), (0.84, 0.3), (0.16, 0.84), (0.84, 0.84)] {
            bolt(c, bx * s, by * s, s * 0.03);
        }
        // A window onto the working gear, behind the product.
        c.fill_circle(cx, cy + s * 0.02, s * 0.25, pal::WINDOW);
        c.save();
        c.translate(cx, cy + s * 0.02);
        c.rotate(if pose.working { pose.t * 2.0 } else { 0.0 });
        c.set_alpha(c.alpha() * 0.35);
        fill(
            c,
            poly(&gear_points(20, 0.19, 0.24), 0.0, 0.0, s),
            pal::MACHINE_HI,
        );
        c.restore();
        c.stroke_circle(
            cx,
            cy + s * 0.02,
            s * 0.25,
            pal::MACHINE_HI,
            (s * 0.02).max(1.0),
        );
    }
    if let Some((shape, color)) = product {
        item(c, shape, color, cx, cy + s * 0.02, s * 0.3, !fine);
    }
    c.fill_rect(s * 0.24, s * 0.78, s * 0.52, s * 0.06, pal::WINDOW);
    if pose.working {
        let w = s * 0.52 * pose.progress.clamp(0.0, 1.0);
        c.fill_rect(s * 0.24, s * 0.78, w, s * 0.06, pal::ACCENT);
    }
    notch(c, z, pose);
}

/// The badge over a stalled machine, a disc of radius `r` centred in a
/// `2r`-pixel square: "!" when it waits for input, "=" when its output is full.
/// With `safe`, in the colour-blind set (T58).
pub fn badge(c: &mut Canvas, r: f32, starved: bool, safe: bool) {
    let (cx, cy) = (r, r);
    let r = (r - 0.5).max(1.0);
    let status = if starved {
        pal::Status::Starved
    } else {
        pal::Status::Blocked
    };
    let color = pal::status(status, safe);
    c.fill_circle(cx, cy, r, color);
    if starved {
        c.fill_rect(cx - r * 0.14, cy - r * 0.6, r * 0.28, r * 0.7, pal::GROUND);
        c.fill_rect(cx - r * 0.14, cy + r * 0.3, r * 0.28, r * 0.28, pal::GROUND);
    } else {
        c.fill_rect(cx - r * 0.5, cy - r * 0.3, r, r * 0.22, pal::GROUND);
        c.fill_rect(cx - r * 0.5, cy + r * 0.08, r, r * 0.22, pal::GROUND);
    }
}

/// A power pole seen from above: a post with a cross arm and insulators,
/// small in its tile so the ground and belts around it stay readable.
pub fn pole(c: &mut Canvas, z: f32, fine: bool) {
    let s = z;
    let (cx, cy) = (s / 2.0, s / 2.0);
    c.fill_circle(cx + z * 0.05, cy + z * 0.07, s * 0.16, pal::SHADOW);
    let arm = (s * 0.07).max(1.5);
    c.line(
        &[(s * 0.2, cy), (s * 0.8, cy)],
        pal::MACHINE_EDGE,
        arm * 1.6,
    );
    c.line(&[(s * 0.22, cy), (s * 0.78, cy)], pal::POLE, arm);
    c.fill_circle(cx, cy, s * 0.13, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.1, pal::POLE);
    if fine {
        for x in [0.22, 0.78] {
            c.fill_circle(x * s, cy, s * 0.06, pal::MACHINE_EDGE);
            c.fill_circle(x * s, cy, s * 0.04, pal::ACCENT);
        }
        c.fill_circle(
            cx - s * 0.03,
            cy - s * 0.03,
            s * 0.035,
            shade(pal::POLE, 0.25),
        );
    }
}

/// A wind turbine seen from above: a round base and three blades, turned
/// a little by `t` (seconds) so a row of them doesn't look stamped.
pub fn turbine(c: &mut Canvas, z: f32, fine: bool, t: f32) {
    let s = z;
    let (cx, cy) = (s / 2.0, s / 2.0);
    let sh = z * 0.07;
    shadow(c, ellipse(cx + sh, cy + sh * 1.3, s * 0.4, s * 0.4));
    c.fill_circle(cx, cy, s * 0.4, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.36, pal::MACHINE);
    if fine {
        c.stroke_circle(
            cx,
            cy,
            s * 0.3,
            shade(pal::MACHINE, 0.12),
            (z * 0.03).max(1.0),
        );
    }
    c.save();
    c.translate(cx, cy);
    c.rotate(t * 2.0);
    for _ in 0..3 {
        c.rotate(TAU / 3.0);
        let blade = [(-0.035, 0.0), (0.035, 0.0), (0.05, -0.42), (-0.02, -0.44)];
        fill(c, poly(&blade, 0.0, 0.0, s), pal::BLADE);
    }
    c.fill_circle(0.0, 0.0, s * 0.08, pal::MACHINE_EDGE);
    c.fill_circle(0.0, 0.0, s * 0.05, pal::ACCENT);
    c.restore();
}

/// Casts no shadow: it is turned when drawn, so the renderer casts one
/// in world space.
/// A splitter facing east: a short belt stub from the west into a housing
/// that sends items out north and south, shown by arrows on both sides.
pub fn splitter(c: &mut Canvas, z: f32, fine: bool) {
    let s = z;
    // The intake: the belt's western half.
    c.fill_rect(0.0, s * 0.12, s * 0.5, s * 0.76, pal::BELT);
    c.fill_rect(0.0, s * 0.1, s * 0.5, s * 0.08, pal::BELT_RAIL);
    c.fill_rect(0.0, s * 0.82, s * 0.5, s * 0.08, pal::BELT_RAIL);
    fill(
        c,
        rr(s * 0.3, s * 0.06, s * 0.62, s * 0.88, s * 0.12),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.34, s * 0.1, s * 0.54, s * 0.8, s * 0.1),
        pal::MACHINE,
    );
    // A divider down the middle, and the two ways out.
    c.line(
        &[(s * 0.4, s * 0.5), (s * 0.82, s * 0.5)],
        pal::MACHINE_EDGE,
        (z * 0.05).max(1.0),
    );
    arrow(c, s * 0.61, s * 0.27, 3, s * 0.42, pal::ACCENT);
    arrow(c, s * 0.61, s * 0.73, 1, s * 0.42, pal::ACCENT);
    if fine {
        for (bx, by) in [(0.4, 0.16), (0.82, 0.16), (0.4, 0.84), (0.82, 0.84)] {
            bolt(c, bx * s, by * s, s * 0.025);
        }
    }
}

/// Casts no shadow: it is turned when drawn, so the renderer casts one
/// in world space.
/// One end of an underground belt facing east. An entrance takes the belt
/// in from the west and dives under a hood on its eastern half; an exit
/// is the same turned around, the belt coming out to the east.
pub fn underground(c: &mut Canvas, z: f32, fine: bool, exit: bool) {
    let s = z;
    let (belt_x, hood_x) = if exit { (s * 0.5, 0.0) } else { (0.0, s * 0.5) };
    c.fill_rect(belt_x, s * 0.12, s * 0.5, s * 0.76, pal::BELT);
    c.fill_rect(belt_x, s * 0.1, s * 0.5, s * 0.08, pal::BELT_RAIL);
    c.fill_rect(belt_x, s * 0.82, s * 0.5, s * 0.08, pal::BELT_RAIL);
    arrow(c, belt_x + s * 0.22, s * 0.5, 0, s * 0.4, pal::CHEVRON);
    fill(
        c,
        rr(hood_x, s * 0.06, s * 0.5, s * 0.88, s * 0.1),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(hood_x + s * 0.04, s * 0.1, s * 0.42, s * 0.8, s * 0.08),
        pal::MACHINE,
    );
    // The dark mouth faces the belt.
    let mouth_x = if exit {
        hood_x + s * 0.34
    } else {
        hood_x + s * 0.04
    };
    c.fill_rect(mouth_x, s * 0.2, s * 0.12, s * 0.6, pal::SOOT);
    if fine {
        let stripe_x = if exit {
            hood_x + s * 0.1
        } else {
            hood_x + s * 0.3
        };
        for k in 0..3 {
            let y = s * (0.24 + k as f32 * 0.2);
            c.fill_rect(stripe_x, y, s * 0.06, s * 0.1, pal::ACCENT);
        }
    }
}

/// The sides a pipe joins, from a mask of bits (east 1, south 2, west 4,
/// north 8). One joined on no side is drawn as a stub running east to west.
fn arms(links: u8) -> impl Iterator<Item = (f32, f32)> {
    let links = if links & 15 == 0 { 5 } else { links };
    DIRS.into_iter()
        .enumerate()
        .filter(move |(i, _)| links & (1 << i) != 0)
        .map(|(_, d)| d)
}

/// A run from the tile's centre to the side `d`, `w` of a tile wide.
fn arm(c: &mut Canvas, z: f32, (dx, dy): (f32, f32), w: f32, col: Rgba) {
    let (h, r) = (z * 0.5, z * w / 2.0);
    let (x0, x1) = (
        h + dx.min(0.0) * h - r * dy.abs(),
        h + dx.max(0.0) * h + r * dy.abs(),
    );
    let (y0, y1) = (
        h + dy.min(0.0) * h - r * dx.abs(),
        h + dy.max(0.0) * h + r * dx.abs(),
    );
    c.fill_rect(x0, y0, x1 - x0, y1 - y0, col);
}

/// The channel down a pipe's middle, where its fluid shows.
const CHANNEL: f32 = 0.18;

/// A pipe tile joined on the sides set in `links` (bits east 1, south 2,
/// west 4, north 8): a steel run with a dark channel down its middle. An
/// empty pipe shows the channel dark; [`pipe_fluid`] fills it.
pub fn pipe(c: &mut Canvas, z: f32, fine: bool, links: u8) {
    let sh = z * 0.06;
    c.save();
    c.translate(sh, sh * 1.3);
    for d in arms(links) {
        arm(c, z, d, 0.5, pal::SHADOW);
    }
    c.restore();
    pipe_body(c, z, fine, links);
}

/// [`pipe`] without its shadow, for parts that are turned when drawn and so
/// have theirs cast by the renderer.
fn pipe_body(c: &mut Canvas, z: f32, fine: bool, links: u8) {
    let h = z * 0.5;
    for d in arms(links) {
        arm(c, z, d, 0.5, pal::MACHINE_EDGE);
    }
    c.fill_circle(h, h, z * 0.25, pal::MACHINE_EDGE);
    for d in arms(links) {
        arm(c, z, d, 0.42, pal::PIPE);
    }
    c.fill_circle(h, h, z * 0.21, pal::PIPE);
    if fine {
        // A lit edge along each run, and a flange where it meets the next tile.
        for (dx, dy) in arms(links) {
            let (ex, ey) = (h + dx * z * 0.42, h + dy * z * 0.42);
            let (fw, fl) = (z * 0.08, z * 0.54);
            if dx != 0.0 {
                c.fill_rect(ex - fw / 2.0, h - fl / 2.0, fw, fl, pal::MACHINE_EDGE);
                c.fill_rect(
                    ex - fw / 2.0 + 1.0,
                    h - fl / 2.0 + 1.0,
                    fw - 2.0,
                    fl - 2.0,
                    pal::PIPE_HI,
                );
            } else {
                c.fill_rect(h - fl / 2.0, ey - fw / 2.0, fl, fw, pal::MACHINE_EDGE);
                c.fill_rect(
                    h - fl / 2.0 + 1.0,
                    ey - fw / 2.0 + 1.0,
                    fl - 2.0,
                    fw - 2.0,
                    pal::PIPE_HI,
                );
            }
        }
    }
    for d in arms(links) {
        arm(c, z, d, CHANNEL + 0.05, pal::MACHINE_EDGE);
    }
    c.fill_circle(h, h, z * (CHANNEL + 0.05) / 2.0, pal::MACHINE_EDGE);
    pipe_channel(c, z, links, pal::PIPE_EMPTY);
}

fn pipe_channel(c: &mut Canvas, z: f32, links: u8, col: Rgba) {
    for d in arms(links) {
        arm(c, z, d, CHANNEL, col);
    }
    c.fill_circle(z * 0.5, z * 0.5, z * CHANNEL / 2.0, col);
}

/// The fluid in a pipe joined on `links`, in white to be tinted by the
/// fluid's colour and faded by how full the pipe is.
pub fn pipe_fluid(c: &mut Canvas, z: f32, links: u8) {
    pipe_channel(c, z, links, pal::WHITE);
}

/// The window in a tank's lid, as a fraction of its 2×2 footprint.
const TANK_WINDOW: f32 = 0.3;

/// A 2×2 storage tank seen from above: a round steel drum on a square
/// footing, with pipe stubs out of each side and a window on what it holds.
pub fn tank(c: &mut Canvas, z: f32, fine: bool) {
    let s = z * 2.0;
    let (cx, cy) = (s / 2.0, s / 2.0);
    let sh = z * 0.08;
    // A pipe stub out to each side, where the pipes around it join on.
    for &(dx, dy) in &DIRS {
        let (w, h) = if dx != 0.0 {
            (s * 0.5, s * 0.16)
        } else {
            (s * 0.16, s * 0.5)
        };
        let x = cx + dx * s * 0.23 - w / 2.0;
        let y = cy + dy * s * 0.23 - h / 2.0;
        fill(c, rr(x + sh, y + sh * 1.3, w, h, s * 0.03), pal::SHADOW);
        fill(c, rr(x, y, w, h, s * 0.03), pal::MACHINE_EDGE);
        let (iw, ih) = if dx != 0.0 {
            (w, h - s * 0.05)
        } else {
            (w - s * 0.05, h)
        };
        fill(
            c,
            rr(x + (w - iw) / 2.0, y + (h - ih) / 2.0, iw, ih, s * 0.02),
            pal::PIPE,
        );
        // A flange at the edge.
        let (fw, fh) = if dx != 0.0 {
            (s * 0.05, s * 0.2)
        } else {
            (s * 0.2, s * 0.05)
        };
        let fx = cx + dx * s * 0.47 - fw / 2.0;
        let fy = cy + dy * s * 0.47 - fh / 2.0;
        fill(c, rr(fx, fy, fw, fh, s * 0.01), pal::MACHINE_EDGE);
    }
    shadow(c, ellipse(cx + sh * 0.5, cy + sh * 0.6, s * 0.42, s * 0.42));
    c.fill_circle(cx, cy, s * 0.42, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.39, pal::PIPE);
    c.fill_circle(cx, cy, s * 0.33, shade(pal::PIPE, -0.2));
    if fine {
        c.stroke_circle(cx, cy, s * 0.36, pal::PIPE_HI, (z * 0.03).max(1.0));
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU;
            bolt(
                c,
                cx + a.cos() * s * 0.365,
                cy + a.sin() * s * 0.365,
                z * 0.035,
            );
        }
    }
    c.fill_circle(cx, cy, s * (TANK_WINDOW + 0.025), pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * TANK_WINDOW, pal::PIPE_EMPTY);
}

/// What a tank holds, as a white disc filling its window, to be tinted and
/// drawn smaller as the tank empties.
pub fn tank_fluid(c: &mut Canvas, z: f32) {
    let s = z * 2.0;
    c.fill_circle(s / 2.0, s / 2.0, s * TANK_WINDOW, pal::WHITE);
}

/// Casts no shadow: it is turned when drawn, so the renderer casts one
/// in world space.
/// A pump facing east: a pipe through a motor housing, with an arrow for
/// the way it pushes.
pub fn pump(c: &mut Canvas, z: f32, fine: bool) {
    pipe_body(c, z, fine, 5);
    let s = z;
    fill(
        c,
        rr(s * 0.2, s * 0.14, s * 0.6, s * 0.72, s * 0.12),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.24, s * 0.18, s * 0.52, s * 0.64, s * 0.1),
        pal::MACHINE,
    );
    c.fill_circle(s * 0.5, s * 0.5, s * 0.2, pal::MACHINE_EDGE);
    c.fill_circle(s * 0.5, s * 0.5, s * 0.17, pal::MACHINE_HI);
    arrow(c, s * 0.5, s * 0.5, 0, s * 0.4, pal::ACCENT);
    if fine {
        for (bx, by) in [(0.3, 0.24), (0.7, 0.24), (0.3, 0.76), (0.7, 0.76)] {
            bolt(c, bx * s, by * s, s * 0.025);
        }
    }
}

/// Casts no shadow: it is turned when drawn, so the renderer casts one
/// in world space.
/// One end of an underground pipe facing east. An entrance takes the pipe
/// in from the west and goes down under a cap on its eastern half; an exit
/// is the same turned around.
pub fn underground_pipe(c: &mut Canvas, z: f32, fine: bool, exit: bool) {
    pipe_body(c, z, fine, if exit { 1 } else { 4 });
    let s = z;
    let cx = if exit { s * 0.38 } else { s * 0.62 };
    c.fill_circle(cx, s * 0.5, s * 0.32, pal::MACHINE_EDGE);
    c.fill_circle(cx, s * 0.5, s * 0.28, pal::MACHINE);
    c.fill_circle(cx, s * 0.5, s * 0.16, pal::MACHINE_EDGE);
    c.fill_circle(cx, s * 0.5, s * 0.12, pal::PIPE);
    if fine {
        for k in 0..4 {
            let a = k as f32 / 4.0 * TAU + PI / 4.0;
            bolt(
                c,
                cx + a.cos() * s * 0.22,
                s * 0.5 + a.sin() * s * 0.22,
                s * 0.025,
            );
        }
    }
}

/// A boiler: a steel drum lying along the way it faces, a firebox at its
/// back that glows while it works, and the steam outlet on its front.
pub fn boiler(c: &mut Canvas, z: f32, fine: bool, pose: &Pose) {
    let s = z;
    let sh = z * 0.07;
    let along = pose.dir % 2 == 0;
    // The drum's box in its own frame: long along the facing.
    let (w, h) = if along {
        (s * 0.86, s * 0.62)
    } else {
        (s * 0.62, s * 0.86)
    };
    let (x, y) = ((s - w) / 2.0, (s - h) / 2.0);
    shadow(c, rr(x + sh, y + sh * 1.3, w, h, s * 0.28));
    fill(c, rr(x, y, w, h, s * 0.28), pal::MACHINE_EDGE);
    fill(
        c,
        rr(
            x + s * 0.04,
            y + s * 0.04,
            w - s * 0.08,
            h - s * 0.08,
            s * 0.24,
        ),
        pal::PIPE,
    );
    // A lit stripe along the drum's top edge.
    if along {
        c.fill_rect(
            x + s * 0.14,
            y + s * 0.1,
            w - s * 0.28,
            s * 0.06,
            pal::PIPE_HI,
        );
    } else {
        c.fill_rect(
            x + s * 0.1,
            y + s * 0.14,
            s * 0.06,
            h - s * 0.28,
            pal::PIPE_HI,
        );
    }
    if fine {
        // Riveted bands round the drum.
        for f in [-0.18, 0.18] {
            let (a, b) = (pose.local(z, f, -0.27), pose.local(z, f, 0.27));
            c.line(&[a, b], pal::MACHINE_EDGE, (s * 0.04).max(1.0));
            for k in [-0.15, 0.0, 0.15] {
                let (bx, by) = pose.local(z, f, k);
                c.fill_circle(bx, by, s * 0.018, pal::RIVET);
            }
        }
    }
    // The firebox at the back.
    let (fx, fy) = pose.local(z, -0.25, 0.0);
    c.fill_circle(fx, fy, s * 0.15, pal::MACHINE_EDGE);
    if pose.working {
        let fl = 0.8 + 0.2 * (pose.t * 11.0).sin();
        c.fill_circle(fx, fy, s * 0.12, pal::FLAME[0]);
        c.fill_circle(fx, fy, s * 0.08 * fl, pal::FLAME[1]);
        c.fill_circle(fx, fy, s * 0.035 * fl, pal::FLAME[2]);
    } else {
        c.fill_circle(fx, fy, s * 0.12, pal::HEARTH);
    }
    // A pressure gauge on top.
    let (gx, gy) = pose.local(z, 0.08, 0.0);
    c.fill_circle(gx, gy, s * 0.09, pal::MACHINE_EDGE);
    c.fill_circle(gx, gy, s * 0.07, pal::INK);
    let (nx, ny) = pose.local(z, 0.12, -0.03);
    c.line(&[(gx, gy), (nx, ny)], pal::STARVE, (s * 0.025).max(1.0));
    notch(c, z, pose);
}

/// A 2×2 steam engine: a cylinder and its piston rod driving a flywheel.
/// `t` turns the flywheel a little, so a row of them doesn't look stamped.
pub fn engine(c: &mut Canvas, z: f32, fine: bool, t: f32) {
    let s = z * 2.0;
    let sh = z * 0.08;
    // A skid under the cylinder; the flywheel stands free beside it.
    shadow(
        c,
        rr(
            s * 0.04 + sh,
            s * 0.26 + sh * 1.3,
            s * 0.6,
            s * 0.48,
            s * 0.06,
        ),
    );
    fill(
        c,
        rr(s * 0.04, s * 0.26, s * 0.6, s * 0.48, s * 0.06),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.06, s * 0.28, s * 0.56, s * 0.44, s * 0.05),
        pal::MACHINE,
    );
    // The cylinder on the left, lying east to west.
    let (cy0, ch) = (s * 0.36, s * 0.28);
    fill(
        c,
        rr(s * 0.1, cy0, s * 0.36, ch, s * 0.06),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.12, cy0 + s * 0.02, s * 0.32, ch - s * 0.04, s * 0.05),
        pal::PIPE,
    );
    c.fill_rect(s * 0.14, cy0 + s * 0.04, s * 0.28, s * 0.03, pal::PIPE_HI);
    // The flywheel on the right, and the rod between them.
    let (wx, wy, wr) = (s * 0.7, s * 0.5, s * 0.27);
    let a = t * 2.0;
    let crank = (wx + a.cos() * wr * 0.45, wy + a.sin() * wr * 0.45);
    shadow(c, ellipse(wx + sh * 0.6, wy + sh * 0.8, wr, wr));
    c.fill_circle(wx, wy, wr, pal::MACHINE_EDGE);
    c.fill_circle(wx, wy, wr * 0.86, shade(pal::MACHINE, 0.12));
    c.fill_circle(wx, wy, wr * 0.66, pal::MACHINE_EDGE);
    for k in 0..6 {
        let b = a + k as f32 / 6.0 * TAU;
        c.line(
            &[(wx, wy), (wx + b.cos() * wr * 0.8, wy + b.sin() * wr * 0.8)],
            shade(pal::MACHINE, 0.12),
            (s * 0.03).max(1.0),
        );
    }
    c.line(
        &[(s * 0.44, s * 0.5), crank],
        pal::DRILL_BIT,
        (s * 0.035).max(1.0),
    );
    c.fill_circle(wx, wy, s * 0.05, pal::ACCENT);
    c.fill_circle(crank.0, crank.1, s * 0.03, pal::ACCENT);
    if fine {
        for (bx, by) in [(0.09, 0.31), (0.09, 0.69), (0.42, 0.31), (0.42, 0.69)] {
            bolt(c, bx * s, by * s, z * 0.04);
        }
        // Steam pipe into the cylinder from the top.
        c.fill_rect(s * 0.24, s * 0.1, s * 0.08, s * 0.26, pal::MACHINE_EDGE);
        c.fill_rect(s * 0.255, s * 0.1, s * 0.05, s * 0.26, pal::PIPE);
        c.fill_rect(s * 0.22, s * 0.08, s * 0.12, s * 0.04, pal::MACHINE_EDGE);
    }
}

/// A pipe port on the edge of a machine `s` pixels across: a flanged stub
/// at `(fx, fy)` in the machine's own frame, capped in its fluid's colour.
fn port(c: &mut Canvas, s: f32, pose: &Pose, fx: f32, fy: f32, fluid: Rgba) {
    let (x, y) = pose.local(s, fx, fy);
    c.fill_circle(x, y, s * 0.075, pal::MACHINE_EDGE);
    c.fill_circle(x, y, s * 0.058, pal::PIPE);
    c.fill_circle(x, y, s * 0.034, fluid);
}

/// A 2×2 oil refinery: two distillation columns over a fired heater. Light
/// oil leaves by the port on its front, tar by the port on its right, and
/// the heater glows while it works.
pub fn refinery(c: &mut Canvas, z: f32, fine: bool, pose: &Pose, light: Rgba, tar: Rgba) {
    let s = z * 2.0;
    let sh = z * 0.08;
    // A concrete slab under the heater at the back; the columns stand
    // forward of it on their own footings.
    let (a, b) = (pose.local(s, -0.45, -0.45), pose.local(s, 0.16, 0.45));
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (w, h) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    shadow(c, rr(x0 + sh, y0 + sh * 1.3, w, h, s * 0.08));
    fill(c, rr(x0, y0, w, h, s * 0.08), pal::MACHINE_EDGE);
    fill(
        c,
        rr(
            x0 + s * 0.02,
            y0 + s * 0.02,
            w - s * 0.04,
            h - s * 0.04,
            s * 0.06,
        ),
        pal::CONCRETE,
    );
    if fine {
        // Hazard stripes along the back, where the crude comes in.
        for k in 0..6 {
            let f = -0.4;
            let a = pose.local(s, f, -0.36 + k as f32 * 0.14);
            let b = pose.local(s, f + 0.05, -0.31 + k as f32 * 0.14);
            c.line(&[a, b], pal::ACCENT, (s * 0.02).max(1.0));
        }
    }
    // The fired heater at the back, glowing through its window when lit.
    let (hx, hy) = pose.local(s, -0.22, 0.0);
    let along = pose.dir % 2 == 0;
    let (hw, hh) = if along {
        (s * 0.2, s * 0.62)
    } else {
        (s * 0.62, s * 0.2)
    };
    fill(
        c,
        rr(hx - hw / 2.0, hy - hh / 2.0, hw, hh, s * 0.04),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(
            hx - hw / 2.0 + s * 0.015,
            hy - hh / 2.0 + s * 0.015,
            hw - s * 0.03,
            hh - s * 0.03,
            s * 0.03,
        ),
        pal::MACHINE,
    );
    let (ww, wh) = if along {
        (s * 0.08, s * 0.42)
    } else {
        (s * 0.42, s * 0.08)
    };
    let glow = if pose.working {
        pal::FLAME[0]
    } else {
        pal::HEARTH
    };
    fill(c, rr(hx - ww / 2.0, hy - wh / 2.0, ww, wh, s * 0.02), glow);
    if pose.working {
        let (gw, gh) = if along {
            (ww * 0.5, wh * 0.8)
        } else {
            (ww * 0.8, wh * 0.5)
        };
        fill(
            c,
            rr(hx - gw / 2.0, hy - gh / 2.0, gw, gh, s * 0.015),
            pal::FLAME[1],
        );
    }
    // Two columns, seen from above: a tall one for light oil, a squat one
    // for tar, each ringed by its trays and capped by a walkway.
    for (fy, r, cap) in [(-0.22, 0.21, light), (0.22, 0.18, tar)] {
        let (x, y) = pose.local(s, 0.22, fy);
        shadow(c, ellipse(x + sh * 0.8, y + sh, r * s, r * s));
        c.fill_circle(x, y, r * s, pal::MACHINE_EDGE);
        c.fill_circle(x, y, (r - 0.02) * s, pal::PIPE);
        c.fill_circle(
            x - r * s * 0.25,
            y - r * s * 0.25,
            r * s * 0.45,
            pal::PIPE_HI,
        );
        c.fill_circle(x, y, (r - 0.02) * s * 0.62, pal::PIPE);
        c.stroke_circle(
            x,
            y,
            (r - 0.02) * s * 0.78,
            pal::MACHINE_EDGE,
            (s * 0.012).max(1.0),
        );
        c.fill_circle(x, y, r * s * 0.32, pal::MACHINE_EDGE);
        c.fill_circle(x, y, r * s * 0.22, cap);
        if fine {
            for k in 0..6 {
                let a = k as f32 / 6.0 * TAU;
                bolt(
                    c,
                    x + a.cos() * r * s * 0.86,
                    y + a.sin() * r * s * 0.86,
                    s * 0.01,
                );
            }
        }
    }
    // A pipe from each column to its port.
    let w = (s * 0.045).max(1.0);
    let front = pose.local(s, 0.44, 0.0);
    c.line(
        &[
            pose.local(s, 0.22, -0.22),
            pose.local(s, 0.44, -0.22),
            front,
        ],
        pal::MACHINE_EDGE,
        w * 1.4,
    );
    c.line(
        &[
            pose.local(s, 0.22, -0.22),
            pose.local(s, 0.44, -0.22),
            front,
        ],
        pal::PIPE,
        w,
    );
    let right = pose.local(s, 0.08, 0.44);
    let from = [pose.local(s, 0.22, 0.22), pose.local(s, 0.08, 0.3), right];
    c.line(&from, pal::MACHINE_EDGE, w * 1.4);
    c.line(&from, pal::PIPE, w);
    port(c, s, pose, 0.44, 0.0, light);
    port(c, s, pose, 0.08, 0.44, tar);
    let (ax, ay) = pose.local(s, 0.3, 0.0);
    arrow(c, ax, ay, pose.dir % 4, z * 0.32, pal::ACCENT);
}

/// A chemical plant: a round reaction vat holding its product's colour, a
/// stirring paddle, and the outlet on its front. Bubbles rise while it works.
pub fn chemical_plant(c: &mut Canvas, z: f32, fine: bool, pose: &Pose, product: Option<Rgba>) {
    let s = z;
    let sh = z * 0.07;
    let liquid = product.unwrap_or(pal::PIPE_EMPTY);
    // A feed tank on each corner, braced to the vat in the middle.
    let corners = [(0.19, 0.19), (0.81, 0.19), (0.19, 0.81), (0.81, 0.81)];
    let w = (s * 0.12).max(1.0);
    for &(x, y) in &corners {
        c.line(&[(x * s, y * s), (s * 0.5, s * 0.5)], pal::MACHINE_EDGE, w);
        c.line(&[(x * s, y * s), (s * 0.5, s * 0.5)], pal::MACHINE, w * 0.6);
    }
    for &(x, y) in &corners {
        shadow(
            c,
            ellipse(x * s + sh * 0.6, y * s + sh * 0.9, s * 0.16, s * 0.16),
        );
        c.fill_circle(x * s, y * s, s * 0.17, pal::MACHINE_EDGE);
        c.fill_circle(x * s, y * s, s * 0.14, pal::MACHINE_HI);
        c.fill_circle(
            x * s - s * 0.03,
            y * s - s * 0.03,
            s * 0.06,
            shade(pal::MACHINE_HI, 0.25),
        );
    }
    shadow(
        c,
        ellipse(s * 0.5 + sh * 0.6, s * 0.5 + sh, s * 0.33, s * 0.33),
    );
    // The vat, and what's in it.
    let (vx, vy) = (s * 0.5, s * 0.5);
    c.fill_circle(vx, vy, s * 0.31, pal::MACHINE_EDGE);
    c.fill_circle(vx, vy, s * 0.27, pal::PIPE);
    c.fill_circle(vx, vy, s * 0.22, shade(liquid, -0.3));
    c.fill_circle(vx - s * 0.03, vy - s * 0.03, s * 0.17, liquid);
    if pose.working {
        for (k, (bx, by)) in [(-0.08, -0.06), (0.07, 0.04), (-0.02, 0.1)]
            .into_iter()
            .enumerate()
        {
            let r = s * (0.025 + 0.012 * ((pose.t * 7.0 + k as f32 * 2.1).sin() + 1.0));
            c.stroke_circle(
                vx + bx * s,
                vy + by * s,
                r,
                shade(liquid, 0.45),
                (s * 0.015).max(1.0),
            );
        }
    }
    paddle(c, s, pose);
    if fine {
        for &(bx, by) in &corners {
            bolt(c, bx * s, by * s, s * 0.025);
        }
    }
    notch(c, z, pose);
}

/// A chemical plant's vat in white, to be tinted with what it makes.
pub fn chemical_vat(c: &mut Canvas, z: f32) {
    let (vx, vy) = (z * 0.5, z * 0.5);
    c.fill_circle(vx, vy, z * 0.22, shade(pal::WHITE, -0.3));
    c.fill_circle(vx - z * 0.03, vy - z * 0.03, z * 0.17, pal::WHITE);
}

/// A chemical plant's paddle and hub alone, facing east, to draw over its vat.
pub fn chemical_paddle(c: &mut Canvas, z: f32) {
    let pose = Pose {
        t: 1.0 / 11.0,
        ..Pose::default()
    };
    paddle(c, z, &pose);
}

/// The paddle across a chemical plant's vat, on its hub.
fn paddle(c: &mut Canvas, s: f32, pose: &Pose) {
    let (vx, vy) = (s * 0.5, s * 0.5);
    let a = pose.t * 3.0 + f32::from(pose.dir) * FRAC_PI_2;
    let (dx, dy) = (a.cos() * s * 0.2, a.sin() * s * 0.2);
    c.line(
        &[(vx - dx, vy - dy), (vx + dx, vy + dy)],
        pal::MACHINE_EDGE,
        (s * 0.06).max(1.0),
    );
    c.line(
        &[(vx - dx, vy - dy), (vx + dx, vy + dy)],
        pal::PIPE_HI,
        (s * 0.03).max(1.0),
    );
    c.fill_circle(vx, vy, s * 0.06, pal::MACHINE_EDGE);
    c.fill_circle(vx, vy, s * 0.035, pal::ACCENT);
}

/// A soft white dot filling a `s`-pixel square, for smoke, dust and
/// bubbles: tinted and faded per instance.
pub fn fx_dot(c: &mut Canvas, s: f32) {
    let r = s / 2.0;
    for k in 0..8 {
        let f = 1.0 - k as f32 / 8.0;
        c.fill_circle(r, r, (r - 0.5) * f, [255, 255, 255, 34]);
    }
}

/// A thin white ring filling a `s`-pixel square, for a stalled machine's
/// pulse.
pub fn fx_ring(c: &mut Canvas, s: f32) {
    let w = (s * 0.07).max(1.0);
    c.stroke_circle(s / 2.0, s / 2.0, s / 2.0 - w, pal::WHITE, w);
}

/// Plain white filling a `w`×`h` canvas, for progress bars and veils.
pub fn fx_bar(c: &mut Canvas, w: f32, h: f32) {
    c.fill_rect(0.0, 0.0, w, h, pal::WHITE);
}

/// A closed shape through `points`, given as fractions of an `s`-pixel
/// square at (`x`, `y`).
fn shape_at(c: &mut Canvas, points: &[(f32, f32)], x: f32, y: f32, s: f32, col: Rgba) {
    fill(c, poly(points, x, y, s), col);
}

/// An octagon filling the unit square, its corners cut `k` deep and inset `i`.
fn octagon(k: f32, i: f32) -> [(f32, f32); 8] {
    let (a, b) = (i, 1.0 - i);
    [
        (a + k, a),
        (b - k, a),
        (b, a + k),
        (b, b - k),
        (b - k, b),
        (a + k, b),
        (a, b - k),
        (a, a + k),
    ]
}

/// A faster belt's icon: the belt, its rails in its tier's `trim`.
pub fn belt_icon(c: &mut Canvas, z: f32, fine: bool, trim: Option<Rgba>) {
    belt(c, z, fine, 0.0, 2.0);
    let Some(t) = trim else {
        return;
    };
    let (h, rail) = if fine {
        (0.09, shade(t, -0.3))
    } else {
        (0.07, shade(t, -0.25))
    };
    let top = if fine { 0.1 } else { 0.12 };
    for y in [top, 0.81] {
        c.fill_rect(0.0, z * y, z, z * h, rail);
        c.fill_rect(0.0, z * y, z, z * 0.025, t);
    }
}

/// A lab: a six-sided housing round a glass dome, where the research pack
/// it studies glows in its colour.
pub fn lab(c: &mut Canvas, z: f32, fine: bool, pack: Rgba) {
    let (s, sh) = (z, z * 0.07);
    let hex = [
        (0.03, 0.5),
        (0.26, 0.07),
        (0.74, 0.07),
        (0.97, 0.5),
        (0.74, 0.93),
        (0.26, 0.93),
    ];
    let inner: Vec<(f32, f32)> = hex
        .iter()
        .map(|&(x, y)| (0.5 + (x - 0.5) * 0.9, 0.5 + (y - 0.5) * 0.9))
        .collect();
    shape_at(c, &hex, sh, sh * 1.3, s, pal::SHADOW);
    shape_at(c, &hex, 0.0, 0.0, s, pal::MACHINE_EDGE);
    shape_at(c, &inner, 0.0, 0.0, s, pal::MACHINE);
    // A lit top edge, as the other machines have.
    let top: Vec<(f32, f32)> = [(0.3, 0.12), (0.7, 0.12), (0.74, 0.19), (0.26, 0.19)].into();
    shape_at(c, &top, 0.0, 0.0, s, shade(pal::MACHINE, 0.12));
    let (cx, cy) = (s * 0.5, s * 0.52);
    // The pack's colour round the dome, and the dome itself.
    c.fill_circle(cx, cy, s * 0.3, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.27, shade(pack, -0.3));
    c.fill_circle(cx, cy, s * 0.21, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.185, shade(pal::GLASS, -0.45));
    c.fill_circle(cx, cy + s * 0.03, s * 0.12, pack);
    c.fill_circle(cx, cy + s * 0.03, s * 0.06, shade(pack, 0.45));
    c.fill_circle(
        cx - s * 0.07,
        cy - s * 0.08,
        s * 0.045,
        alpha(pal::WHITE, 0.7),
    );
    if fine {
        // Flasks at the two side points, and bolts round the dome.
        for fx in [0.15, 0.85] {
            c.fill_circle(fx * s, s * 0.5, s * 0.06, pal::MACHINE_EDGE);
            c.fill_circle(fx * s, s * 0.5, s * 0.04, shade(pack, 0.2));
        }
        for k in 0..6 {
            let a = k as f32 / 6.0 * TAU + PI / 6.0;
            bolt(
                c,
                cx + a.cos() * s * 0.24,
                cy + a.sin() * s * 0.24,
                s * 0.02,
            );
        }
    }
}

/// A faster assembler: the assembler's works in an eight-sided housing
/// trimmed in its tier's colour.
pub fn fast_assembler(
    c: &mut Canvas,
    z: f32,
    product: Option<(Shape, Rgba)>,
    fine: bool,
    pose: &Pose,
    trim: Rgba,
) {
    let (s, sh) = (z, z * 0.07);
    let (cx, cy) = (s / 2.0, s / 2.0);
    let outer = octagon(0.22, 0.03);
    let inner = octagon(0.2, 0.07);
    shape_at(c, &outer, sh, sh * 1.3, s, pal::SHADOW);
    shape_at(c, &outer, 0.0, 0.0, s, pal::MACHINE_EDGE);
    shape_at(c, &inner, 0.0, 0.0, s, pal::MACHINE);
    // Tier trim on the four cut corners.
    let w = (s * 0.05).max(1.0);
    for (a, b) in [(0, 7), (1, 2), (3, 4), (5, 6)] {
        let p = |i: usize| (inner[i].0 * s, inner[i].1 * s);
        c.line(&[p(a), p(b)], trim, w);
    }
    let col = product.map_or(trim, |p| p.1);
    c.save();
    if let Some(p) = poly(&inner, 0.0, 0.0, s) {
        c.clip(&p);
    }
    c.fill_rect(0.0, s * 0.07, s, s * 0.12, col);
    if fine {
        for k in 0..8 {
            let k = k as f32;
            c.line(
                &[
                    (s * (0.1 + k * 0.12), s * 0.19),
                    (s * (0.16 + k * 0.12), s * 0.07),
                ],
                shade(col, -0.35),
                s * 0.04,
            );
        }
    }
    c.restore();
    // Two windows onto twin gears: it works twice as fast.
    for gx in [-0.14, 0.14] {
        let (x, y) = (cx + gx * s, cy + s * 0.04);
        c.fill_circle(x, y, s * 0.15, pal::WINDOW);
        if fine {
            c.save();
            c.translate(x, y);
            c.rotate(if pose.working {
                pose.t * 3.0 * gx.signum()
            } else {
                0.0
            });
            c.set_alpha(c.alpha() * 0.4);
            fill(
                c,
                poly(&gear_points(14, 0.1, 0.135), 0.0, 0.0, s),
                pal::MACHINE_HI,
            );
            c.restore();
        }
        c.stroke_circle(x, y, s * 0.15, trim, (s * 0.02).max(1.0));
    }
    if let Some((shape, color)) = product {
        item(c, shape, color, cx, cy + s * 0.04, s * 0.26, !fine);
    }
    c.fill_rect(s * 0.26, s * 0.8, s * 0.48, s * 0.05, pal::WINDOW);
    if pose.working {
        let w = s * 0.48 * pose.progress.clamp(0.0, 1.0);
        c.fill_rect(s * 0.26, s * 0.8, w, s * 0.05, trim);
    }
    notch(c, z, pose);
}

/// A water well: a wellhead on a diamond of concrete, the water showing
/// through its grate.
pub fn water_well(c: &mut Canvas, z: f32, fine: bool, water: Rgba) {
    let (s, sh) = (z, z * 0.07);
    let pad = [(0.5, 0.01), (0.99, 0.5), (0.5, 0.99), (0.01, 0.5)];
    let inner = [(0.5, 0.07), (0.93, 0.5), (0.5, 0.93), (0.07, 0.5)];
    shape_at(c, &pad, sh, sh * 1.3, s, pal::SHADOW);
    shape_at(c, &pad, 0.0, 0.0, s, pal::MACHINE_EDGE);
    shape_at(c, &inner, 0.0, 0.0, s, pal::CONCRETE);
    let (cx, cy) = (s * 0.5, s * 0.5);
    c.fill_circle(cx, cy, s * 0.27, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.23, water);
    c.fill_circle(cx, cy, s * 0.14, shade(water, -0.35));
    c.stroke_circle(cx, cy, s * 0.18, pal::ACCENT, (z * 0.045).max(1.0));
    let w = (z * 0.04).max(1.0);
    c.line(&[(s * 0.3, cy), (s * 0.7, cy)], pal::ACCENT, w);
    c.line(&[(cx, s * 0.3), (cx, s * 0.7)], pal::ACCENT, w);
    if fine {
        for (bx, by) in [(0.5, 0.14), (0.86, 0.5), (0.5, 0.86), (0.14, 0.5)] {
            bolt(c, bx * s, by * s, s * 0.03);
        }
    }
}

/// An oil well: a pumpjack seen from above, its walking beam running from
/// the crank weights at the back to the horse head over the wellhead.
pub fn pumpjack(c: &mut Canvas, z: f32, fine: bool, oil: Rgba, t: f32) {
    let (s, sh) = (z, z * 0.07);
    // The skid it stands on, and the wellhead at the front.
    shadow(
        c,
        rr(
            s * 0.08 + sh,
            s * 0.36 + sh * 1.3,
            s * 0.84,
            s * 0.28,
            s * 0.05,
        ),
    );
    fill(
        c,
        rr(s * 0.08, s * 0.36, s * 0.84, s * 0.28, s * 0.05),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.1, s * 0.38, s * 0.8, s * 0.24, s * 0.04),
        pal::CONCRETE,
    );
    c.fill_circle(s * 0.82, s * 0.5, s * 0.11, pal::MACHINE_EDGE);
    c.fill_circle(s * 0.82, s * 0.5, s * 0.075, oil);
    // Crank weights either side at the back, turning with the crank.
    let a = t * 2.5;
    for side in [-1.0f32, 1.0] {
        let (x, y) = (s * 0.24, s * (0.5 + side * 0.27));
        shadow(c, ellipse(x + sh * 0.6, y + sh * 0.8, s * 0.13, s * 0.13));
        c.fill_circle(x, y, s * 0.14, pal::MACHINE_EDGE);
        c.fill_circle(x, y, s * 0.115, pal::MACHINE_HI);
        let (px, py) = (x + a.cos() * s * 0.06, y + a.sin() * s * 0.06 * side);
        c.fill_circle(px, py, s * 0.04, pal::ACCENT);
    }
    // The samson post, an A-frame under the middle of the beam.
    let w = (s * 0.05).max(1.0);
    for side in [-1.0f32, 1.0] {
        c.line(
            &[(s * 0.5, s * 0.5), (s * 0.42, s * (0.5 + side * 0.3))],
            pal::MACHINE_EDGE,
            w * 1.5,
        );
        c.line(
            &[(s * 0.5, s * 0.5), (s * 0.42, s * (0.5 + side * 0.3))],
            pal::PIPE,
            w * 0.8,
        );
    }
    // The walking beam, and the horse head at its front end.
    shadow(
        c,
        rr(
            s * 0.2 + sh,
            s * 0.44 + sh * 1.3,
            s * 0.56,
            s * 0.12,
            s * 0.03,
        ),
    );
    fill(
        c,
        rr(s * 0.2, s * 0.44, s * 0.56, s * 0.12, s * 0.03),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.22, s * 0.46, s * 0.52, s * 0.08, s * 0.02),
        pal::ACCENT,
    );
    let head = [
        (0.66, 0.34),
        (0.8, 0.38),
        (0.86, 0.5),
        (0.8, 0.62),
        (0.66, 0.66),
    ];
    shape_at(c, &head, 0.0, 0.0, s, pal::MACHINE_EDGE);
    let head_in: Vec<(f32, f32)> = head
        .iter()
        .map(|&(x, y)| (0.73 + (x - 0.73) * 0.75, 0.5 + (y - 0.5) * 0.75))
        .collect();
    shape_at(c, &head_in, 0.0, 0.0, s, pal::MACHINE_HI);
    if fine {
        c.fill_circle(s * 0.5, s * 0.5, s * 0.04, pal::MACHINE_EDGE);
        c.fill_circle(s * 0.5, s * 0.5, s * 0.022, pal::DRILL_BIT);
        for k in 0..5 {
            let x = s * (0.28 + k as f32 * 0.09);
            c.line(
                &[(x, s * 0.46), (x + s * 0.04, s * 0.54)],
                pal::HAZARD_DARK,
                w * 0.5,
            );
        }
    }
}

/// Tier stripes: `n` short slanted bars in the tier's colour along a
/// machine's front edge, `w` pixels wide in all, from (`x`, `y`).
fn stripes(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, n: usize, trim: Rgba) {
    c.fill_rect(x, y, w, h, pal::HAZARD_DARK);
    let step = w / n as f32;
    for k in 0..n {
        let x0 = x + k as f32 * step;
        let bar = [
            (x0 + step * 0.15, y + h),
            (x0 + step * 0.45, y),
            (x0 + step * 0.85, y),
            (x0 + step * 0.55, y + h),
        ];
        fill(c, poly(&bar, 0.0, 0.0, 1.0), trim);
    }
}

/// An electric furnace: a square cabinet, no firebrick, round a crucible
/// wound with a copper coil that glows white hot while it works. `ring` is
/// what it smelts and `trim` its tier.
pub fn electric_furnace(
    c: &mut Canvas,
    z: f32,
    ring: Option<Rgba>,
    fine: bool,
    pose: &Pose,
    trim: Rgba,
) {
    let (s, sh) = (z, z * 0.07);
    let (cx, cy) = (s / 2.0, s / 2.0);
    shadow(
        c,
        rr(
            s * 0.06 + sh,
            s * 0.06 + sh * 1.3,
            s * 0.88,
            s * 0.88,
            s * 0.12,
        ),
    );
    fill(
        c,
        rr(s * 0.06, s * 0.06, s * 0.88, s * 0.88, s * 0.12),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.09, s * 0.09, s * 0.82, s * 0.82, s * 0.1),
        pal::MACHINE,
    );
    // Cooling fins down both sides.
    if fine {
        let w = (s * 0.025).max(1.0);
        for k in 0..4 {
            let y = s * (0.3 + k as f32 * 0.12);
            c.line(&[(s * 0.12, y), (s * 0.2, y)], pal::MACHINE_HI, w);
            c.line(&[(s * 0.8, y), (s * 0.88, y)], pal::MACHINE_HI, w);
        }
    }
    stripes(c, s * 0.2, s * 0.12, s * 0.6, s * 0.08, 4, trim);
    let ring = ring.unwrap_or(pal::MACHINE_HI);
    c.fill_circle(cx, cy + s * 0.05, s * 0.3, pal::MACHINE_EDGE);
    // The coil: copper turns round the crucible.
    let coil = pal::item("copper");
    let w = (s * 0.04).max(1.0);
    c.stroke_circle(cx, cy + s * 0.05, s * 0.26, coil, w);
    if fine {
        c.stroke_circle(cx, cy + s * 0.05, s * 0.21, shade(coil, -0.3), w);
    }
    c.stroke_circle(cx, cy + s * 0.05, s * 0.3, ring, (s * 0.03).max(1.0));
    if pose.working {
        let fl = 0.85 + 0.15 * (pose.t * 7.0).sin();
        c.fill_circle(cx, cy + s * 0.05, s * 0.17, pal::FLAME[1]);
        c.fill_circle(cx, cy + s * 0.05, s * 0.11 * fl, pal::FLAME[2]);
        c.fill_circle(cx, cy + s * 0.05, s * 0.05 * fl, pal::WHITE);
    } else {
        c.fill_circle(cx, cy + s * 0.05, s * 0.17, pal::HEARTH);
        if fine {
            c.fill_circle(cx - s * 0.05, cy, s * 0.04, pal::SOOT);
        }
    }
    notch(c, z, pose);
}

/// A 2×2 solar panel: four tilted panes of dark blue cells in a steel
/// frame, with a little inverter box between them.
pub fn solar(c: &mut Canvas, z: f32, fine: bool) {
    let s = z * 2.0;
    let sh = z * 0.08;
    shadow(
        c,
        rr(
            s * 0.04 + sh,
            s * 0.04 + sh * 1.3,
            s * 0.92,
            s * 0.92,
            s * 0.04,
        ),
    );
    fill(
        c,
        rr(s * 0.04, s * 0.04, s * 0.92, s * 0.92, s * 0.04),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.06, s * 0.06, s * 0.88, s * 0.88, s * 0.03),
        pal::PIPE,
    );
    let cell = pal::SOLAR;
    let line = shade(cell, 0.25);
    for (px, py) in [(0.08, 0.08), (0.52, 0.08), (0.08, 0.52), (0.52, 0.52)] {
        let (x, y, w) = (s * px, s * py, s * 0.4);
        fill(c, rr(x, y, w, w, s * 0.015), cell);
        if fine {
            let lw = (s * 0.006).max(1.0);
            for k in 1..4 {
                let o = w * k as f32 / 4.0;
                c.line(&[(x + o, y), (x + o, y + w)], line, lw);
                c.line(&[(x, y + o), (x + w, y + o)], line, lw);
            }
        } else {
            c.line(
                &[(x + w / 2.0, y), (x + w / 2.0, y + w)],
                line,
                (s * 0.01).max(1.0),
            );
        }
        // The sky's sheen across each pane.
        let sheen = [(0.0, 0.62), (0.62, 0.0), (0.82, 0.0), (0.0, 0.82)];
        c.save();
        c.set_alpha(c.alpha() * 0.18);
        fill(c, poly(&sheen, x, y, w), pal::WHITE);
        c.restore();
    }
    // The inverter where the panes meet.
    fill(
        c,
        rr(s * 0.42, s * 0.42, s * 0.16, s * 0.16, s * 0.03),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.44, s * 0.44, s * 0.12, s * 0.12, s * 0.02),
        pal::MACHINE,
    );
    c.fill_circle(s * 0.5, s * 0.5, s * 0.025, pal::GO);
}

/// A 2×2 precision assembler: a cabinet in its tier's stripes, round a
/// glass work cell where a robot arm builds `product`.
pub fn precision_assembler(
    c: &mut Canvas,
    z: f32,
    product: Option<(Shape, Rgba)>,
    fine: bool,
    pose: &Pose,
    trim: Rgba,
) {
    let s = z * 2.0;
    let sh = z * 0.08;
    let (cx, cy) = (s / 2.0, s / 2.0);
    let outer = octagon(0.18, 0.04);
    let inner = octagon(0.16, 0.07);
    shape_at(c, &outer, sh, sh * 1.3, s, pal::SHADOW);
    shape_at(c, &outer, 0.0, 0.0, s, pal::MACHINE_EDGE);
    shape_at(c, &inner, 0.0, 0.0, s, pal::MACHINE);
    stripes(c, s * 0.24, s * 0.08, s * 0.52, s * 0.07, 6, trim);
    stripes(c, s * 0.24, s * 0.85, s * 0.52, s * 0.07, 6, trim);
    // Four feeder hatches, one on each side.
    for (hx, hy) in [(0.14, 0.5), (0.86, 0.5)] {
        fill(
            c,
            rr(
                s * hx - s * 0.04,
                s * hy - s * 0.1,
                s * 0.08,
                s * 0.2,
                s * 0.02,
            ),
            pal::WINDOW,
        );
    }
    // The work cell.
    c.fill_circle(cx, cy, s * 0.29, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.26, pal::WINDOW);
    c.stroke_circle(cx, cy, s * 0.27, trim, (s * 0.012).max(1.0));
    if let Some((shape, color)) = product {
        item(c, shape, color, cx, cy + s * 0.06, s * 0.16, !fine);
    }
    // The arm, from its base at the back of the cell to the work.
    let a = if pose.working {
        (pose.t * 2.0).sin() * 0.6
    } else {
        0.0
    };
    let (bx, by) = (cx - s * 0.13, cy - s * 0.12);
    let (ex, ey) = (
        bx + s * 0.14 * (0.4 + a).cos(),
        by + s * 0.14 * (0.4 + a).sin(),
    );
    let (tx, ty) = (cx + s * 0.02, cy + s * 0.02);
    let w = (s * 0.035).max(1.0);
    c.line(&[(bx, by), (ex, ey), (tx, ty)], pal::MACHINE_EDGE, w * 1.5);
    c.line(&[(bx, by), (ex, ey), (tx, ty)], pal::ACCENT, w);
    c.fill_circle(bx, by, s * 0.04, pal::MACHINE_HI);
    c.fill_circle(ex, ey, s * 0.022, pal::MACHINE_EDGE);
    if fine {
        c.fill_circle(tx, ty, s * 0.018, pal::WHITE);
        for (x, y) in [(0.2, 0.2), (0.8, 0.2), (0.2, 0.8), (0.8, 0.8)] {
            bolt(c, s * x, s * y, s * 0.015);
        }
    }
    // Progress, under the cell.
    c.fill_rect(s * 0.36, s * 0.79, s * 0.28, s * 0.03, pal::WINDOW);
    if pose.working {
        let w = s * 0.28 * pose.progress.clamp(0.0, 1.0);
        c.fill_rect(s * 0.36, s * 0.79, w, s * 0.03, trim);
    }
    let (nx, ny) = pose.local(s, 0.42, 0.0);
    arrow(c, nx, ny, pose.dir % 4, s * 0.16, pal::ACCENT);
}

/// An assembler's stripe in white, to be tinted with what it makes: the
/// first kind's band across its top, or the faster kind's band clipped to
/// its octagon. Hatching shows as the darker lines.
pub fn stripe(c: &mut Canvas, z: f32, fine: bool, octagon: bool) {
    let s = z;
    let white = pal::WHITE;
    let hatch = shade(white, -0.35);
    c.save();
    let (y0, h) = if octagon {
        if let Some(p) = poly(&octagon_points(), 0.0, 0.0, s) {
            c.clip(&p);
        }
        (0.07, 0.12)
    } else {
        if let Some(p) = rect(s * 0.08, s * 0.08, s * 0.84, s * 0.12) {
            c.clip(&p);
        }
        (0.08, 0.12)
    };
    c.fill_rect(0.0, s * y0, s, s * h, white);
    if fine {
        for k in 0..8 {
            let k = k as f32;
            c.line(
                &[
                    (s * (0.1 + k * 0.12), s * (y0 + h)),
                    (s * (0.16 + k * 0.12), s * y0),
                ],
                hatch,
                s * 0.04,
            );
        }
    }
    c.restore();
}

/// The faster assembler's inner octagon, for its stripe.
fn octagon_points() -> [(f32, f32); 8] {
    octagon(0.2, 0.07)
}

/// A deep drill: an octagonal housing in its tier's stripes over a wide
/// four-bladed bit, ringed in what it mines, with a motor housing behind.
pub fn deep_drill(c: &mut Canvas, z: f32, ore: Rgba, fine: bool, pose: &Pose, trim: Rgba) {
    let (s, sh) = (z, z * 0.07);
    let (cx, cy) = (s / 2.0, s / 2.0);
    let outer = octagon(0.2, 0.04);
    let inner = octagon(0.18, 0.08);
    shape_at(c, &outer, sh, sh * 1.3, s, pal::SHADOW);
    shape_at(c, &outer, 0.0, 0.0, s, pal::MACHINE_EDGE);
    shape_at(c, &inner, 0.0, 0.0, s, pal::MACHINE);
    // The tier's stripes on all four flat sides.
    let w = (s * 0.05).max(1.0);
    for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7)] {
        let p = |i: usize| (inner[i].0 * s, inner[i].1 * s);
        c.line(&[p(a), p(b)], trim, w);
    }
    // The motor behind the bit.
    let (mx, my) = pose.local(z, -0.27, 0.0);
    c.fill_circle(mx, my, s * 0.11, pal::MACHINE_EDGE);
    c.fill_circle(mx, my, s * 0.08, pal::item("electric-engine"));
    c.fill_circle(cx, cy, s * 0.3, shade(ore, -0.45));
    c.fill_circle(cx, cy, s * 0.25, ore);
    c.save();
    c.translate(cx, cy);
    c.rotate(if pose.working { pose.t * 9.0 } else { 0.4 });
    for _ in 0..4 {
        c.rotate(TAU / 4.0);
        let blade = [(-0.045, 0.0), (0.045, 0.0), (0.025, -0.23), (-0.06, -0.19)];
        fill(c, poly(&blade, 0.0, 0.0, s), pal::DRILL_BIT);
    }
    c.fill_circle(0.0, 0.0, s * 0.07, pal::MACHINE_EDGE);
    c.fill_circle(0.0, 0.0, s * 0.035, trim);
    c.restore();
    if fine {
        for (bx, by) in [(0.2, 0.2), (0.8, 0.2), (0.2, 0.8), (0.8, 0.8)] {
            bolt(c, bx * s, by * s, s * 0.025);
        }
    }
    notch(c, z, pose);
}

/// A furnace's ring in white, to be tinted with what it smelts: round the
/// hearth of a fire furnace, or the crucible of an electric one.
pub fn furnace_ring(c: &mut Canvas, z: f32, fine: bool, electric: bool) {
    let s = z;
    let lw = (s * 0.05).max(1.0);
    if electric {
        c.stroke_circle(s / 2.0, s * 0.55, s * 0.3, pal::WHITE, (s * 0.03).max(1.0));
    } else if fine {
        c.stroke_circle(s / 2.0, s / 2.0, s * 0.41, pal::WHITE, lw);
    } else {
        c.stroke_circle(s / 2.0, s / 2.0, s * 0.34, pal::WHITE, lw * 1.4);
    }
}

/// A machine's output arrow alone, facing east, to draw over its ring.
pub fn notch_east(c: &mut Canvas, z: f32) {
    notch(c, z, &Pose::default());
}

/// The megaproject's anchor, `tiles` across: a round platform on a pad in
/// hazard stripes, ringed with lamps, three pylons holding the tether's
/// hub at its middle, where the cable climbs out of sight.
pub fn skyhook(c: &mut Canvas, z: f32, fine: bool, tiles: f32) {
    let s = z * tiles;
    let (cx, cy) = (s / 2.0, s / 2.0);
    let sh = z * 0.1;
    shadow(
        c,
        rr(
            s * 0.03 + sh,
            s * 0.03 + sh * 1.3,
            s * 0.94,
            s * 0.94,
            s * 0.06,
        ),
    );
    fill(
        c,
        rr(s * 0.03, s * 0.03, s * 0.94, s * 0.94, s * 0.06),
        pal::PAD,
    );
    // Hazard stripes round the pad's edge.
    let band = s * 0.05;
    c.save();
    if let Some(p) = rr(s * 0.05, s * 0.05, s * 0.9, s * 0.9, s * 0.04) {
        c.clip(&p);
    }
    let n = if fine { 28 } else { 14 };
    for k in 0..n {
        let t = k as f32 / n as f32 * 4.0;
        let (x, y) = match t as u32 {
            0 => (s * 0.05 + t.fract() * s * 0.9, s * 0.05),
            1 => (s * 0.95 - band, s * 0.05 + t.fract() * s * 0.9),
            2 => (s * 0.95 - t.fract() * s * 0.9, s * 0.95 - band),
            _ => (s * 0.05, s * 0.95 - t.fract() * s * 0.9),
        };
        let col = if k % 2 == 0 {
            pal::ACCENT
        } else {
            pal::HAZARD_DARK
        };
        c.fill_rect(x - band * 0.5, y, band * 1.6, band, col);
        c.fill_rect(x, y - band * 0.5, band, band * 1.6, col);
    }
    c.restore();
    fill(
        c,
        rr(s * 0.1, s * 0.1, s * 0.8, s * 0.8, s * 0.04),
        pal::PAD,
    );
    // The platform.
    c.fill_circle(cx, cy, s * 0.42, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.39, pal::MACHINE);
    c.stroke_circle(
        cx,
        cy,
        s * 0.3,
        shade(pal::MACHINE, 0.12),
        (s * 0.012).max(1.0),
    );
    // Lamps round the rim.
    let lamps = if fine { 16 } else { 8 };
    for k in 0..lamps {
        let a = k as f32 / lamps as f32 * TAU;
        let (x, y) = (cx + a.cos() * s * 0.355, cy + a.sin() * s * 0.355);
        c.fill_circle(x, y, s * 0.018, pal::MACHINE_EDGE);
        c.fill_circle(x, y, s * 0.012, pal::ACCENT);
    }
    // Three pylons from the rim to the hub.
    let w = (s * 0.035).max(1.5);
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU - FRAC_PI_2;
        let (x, y) = (cx + a.cos() * s * 0.33, cy + a.sin() * s * 0.33);
        let (hx, hy) = (cx + a.cos() * s * 0.1, cy + a.sin() * s * 0.1);
        c.line(&[(x, y), (hx, hy)], pal::MACHINE_EDGE, w * 1.5);
        c.line(&[(x, y), (hx, hy)], pal::PIPE, w);
        c.fill_circle(x, y, s * 0.04, pal::MACHINE_EDGE);
        c.fill_circle(x, y, s * 0.028, pal::PIPE_HI);
    }
    // The tether's hub, and the cable rising from it.
    let glow = pal::TIER[1];
    c.fill_circle(cx, cy, s * 0.14, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, s * 0.11, shade(glow, -0.3));
    c.fill_circle(cx, cy, s * 0.075, glow);
    c.fill_circle(cx, cy, s * 0.035, pal::WHITE);
    if fine {
        for (bx, by) in [(0.15, 0.15), (0.85, 0.15), (0.15, 0.85), (0.85, 0.85)] {
            bolt(c, bx * s, by * s, s * 0.012);
        }
    }
}
