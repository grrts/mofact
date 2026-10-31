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
pub fn badge(c: &mut Canvas, r: f32, starved: bool) {
    let (cx, cy) = (r, r);
    let r = (r - 0.5).max(1.0);
    let color = if starved { pal::STARVE } else { pal::ACCENT };
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

/// A splitter facing east: a short belt stub from the west into a housing
/// that sends items out north and south, shown by arrows on both sides.
pub fn splitter(c: &mut Canvas, z: f32, fine: bool) {
    let s = z;
    let sh = z * 0.07;
    // The intake: the belt's western half.
    c.fill_rect(0.0, s * 0.12, s * 0.5, s * 0.76, pal::BELT);
    c.fill_rect(0.0, s * 0.1, s * 0.5, s * 0.08, pal::BELT_RAIL);
    c.fill_rect(0.0, s * 0.82, s * 0.5, s * 0.08, pal::BELT_RAIL);
    shadow(
        c,
        rr(
            s * 0.3 + sh,
            s * 0.06 + sh * 1.3,
            s * 0.62,
            s * 0.88,
            s * 0.12,
        ),
    );
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

/// One end of an underground belt facing east. An entrance takes the belt
/// in from the west and dives under a hood on its eastern half; an exit
/// is the same turned around, the belt coming out to the east.
pub fn underground(c: &mut Canvas, z: f32, fine: bool, exit: bool) {
    let s = z;
    let sh = z * 0.07;
    let (belt_x, hood_x) = if exit { (s * 0.5, 0.0) } else { (0.0, s * 0.5) };
    c.fill_rect(belt_x, s * 0.12, s * 0.5, s * 0.76, pal::BELT);
    c.fill_rect(belt_x, s * 0.1, s * 0.5, s * 0.08, pal::BELT_RAIL);
    c.fill_rect(belt_x, s * 0.82, s * 0.5, s * 0.08, pal::BELT_RAIL);
    arrow(c, belt_x + s * 0.22, s * 0.5, 0, s * 0.4, pal::CHEVRON);
    shadow(
        c,
        rr(hood_x + sh, s * 0.06 + sh * 1.3, s * 0.5, s * 0.88, s * 0.1),
    );
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
    let h = z * 0.5;
    let sh = z * 0.06;
    c.save();
    c.translate(sh, sh * 1.3);
    for d in arms(links) {
        arm(c, z, d, 0.5, pal::SHADOW);
    }
    c.restore();
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
    // A square footing the pipes around it join onto.
    shadow(
        c,
        rr(
            s * 0.04 + sh,
            s * 0.04 + sh * 1.3,
            s * 0.92,
            s * 0.92,
            s * 0.16,
        ),
    );
    fill(
        c,
        rr(s * 0.04, s * 0.04, s * 0.92, s * 0.92, s * 0.16),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.06, s * 0.06, s * 0.88, s * 0.88, s * 0.14),
        pal::MACHINE,
    );
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

/// A pump facing east: a pipe through a motor housing, with an arrow for
/// the way it pushes.
pub fn pump(c: &mut Canvas, z: f32, fine: bool) {
    pipe(c, z, fine, 5);
    let s = z;
    let sh = z * 0.07;
    shadow(
        c,
        rr(
            s * 0.2 + sh,
            s * 0.14 + sh * 1.3,
            s * 0.6,
            s * 0.72,
            s * 0.12,
        ),
    );
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

/// One end of an underground pipe facing east. An entrance takes the pipe
/// in from the west and goes down under a cap on its eastern half; an exit
/// is the same turned around.
pub fn underground_pipe(c: &mut Canvas, z: f32, fine: bool, exit: bool) {
    pipe(c, z, fine, if exit { 1 } else { 4 });
    let s = z;
    let sh = z * 0.07;
    let cx = if exit { s * 0.38 } else { s * 0.62 };
    shadow(c, ellipse(cx + sh, s * 0.5 + sh * 1.3, s * 0.32, s * 0.32));
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

/// A water well: a concrete pad with a wellhead and its valve wheel.
pub fn well(c: &mut Canvas, z: f32, fine: bool, water: Rgba) {
    let s = z;
    let sh = z * 0.07;
    shadow(
        c,
        rr(
            s * 0.06 + sh,
            s * 0.06 + sh * 1.3,
            s * 0.88,
            s * 0.88,
            s * 0.14,
        ),
    );
    fill(
        c,
        rr(s * 0.06, s * 0.06, s * 0.88, s * 0.88, s * 0.14),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.1, s * 0.1, s * 0.8, s * 0.8, s * 0.12),
        pal::CONCRETE,
    );
    c.fill_circle(s * 0.5, s * 0.5, s * 0.3, pal::MACHINE_EDGE);
    c.fill_circle(s * 0.5, s * 0.5, s * 0.26, water);
    c.fill_circle(s * 0.5, s * 0.5, s * 0.16, shade(water, -0.35));
    c.stroke_circle(s * 0.5, s * 0.5, s * 0.2, pal::ACCENT, (z * 0.05).max(1.0));
    c.line(
        &[(s * 0.3, s * 0.5), (s * 0.7, s * 0.5)],
        pal::ACCENT,
        (z * 0.04).max(1.0),
    );
    c.line(
        &[(s * 0.5, s * 0.3), (s * 0.5, s * 0.7)],
        pal::ACCENT,
        (z * 0.04).max(1.0),
    );
    if fine {
        for (bx, by) in [(0.18, 0.18), (0.82, 0.18), (0.18, 0.82), (0.82, 0.82)] {
            bolt(c, bx * s, by * s, s * 0.03);
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
    shadow(
        c,
        rr(
            s * 0.04 + sh,
            s * 0.06 + sh * 1.3,
            s * 0.92,
            s * 0.88,
            s * 0.1,
        ),
    );
    fill(
        c,
        rr(s * 0.04, s * 0.06, s * 0.92, s * 0.88, s * 0.1),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(s * 0.06, s * 0.08, s * 0.88, s * 0.84, s * 0.08),
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
    let (wx, wy, wr) = (s * 0.7, s * 0.5, s * 0.22);
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
        for (bx, by) in [(0.1, 0.13), (0.9, 0.13), (0.1, 0.87), (0.9, 0.87)] {
            bolt(c, bx * s, by * s, z * 0.04);
        }
        // Steam pipe into the cylinder from the top.
        c.fill_rect(s * 0.24, s * 0.12, s * 0.08, s * 0.24, pal::MACHINE_EDGE);
        c.fill_rect(s * 0.255, s * 0.12, s * 0.05, s * 0.24, pal::PIPE);
    }
}
