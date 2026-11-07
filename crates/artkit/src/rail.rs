//! Track and trains (T52): rails that join every track tile beside them,
//! signals, train stops and locomotives, drawn in code like everything else.
//!
//! Track tiles are painted once per set of joined sides (bits east 1, south
//! 2, west 4, north 8) and never turned. A stop's platform and a
//! locomotive face east and are turned when drawn.

use crate::canvas::{rr, Canvas};
use crate::palette::{self as pal, shade, Rgba};
use std::f32::consts::{FRAC_PI_2, PI};

/// The four directions, as `(dx, dy)` steps, in the core's `Dir` order.
const DIRS: [(f32, f32); 4] = [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)];
/// Half the distance between the two rails, as a fraction of a tile.
const GAUGE: f32 = 0.19;
/// How wide the gravel bed and the sleepers are.
const BED: f32 = 0.78;
const TIE: f32 = 0.66;

fn fill(c: &mut Canvas, p: Option<tiny_skia::Path>, col: Rgba) {
    if let Some(p) = p {
        c.fill(&p, col);
    }
}

/// The sides joined, with none drawn as a straight run east to west.
fn sides(links: u8) -> u8 {
    match links & 15 {
        0 => 5,
        l => l,
    }
}

/// For two joined sides at right angles, the corner of the tile the curve
/// bends round, and the angle on it where the curve starts.
fn curve(links: u8) -> Option<((f32, f32), f32)> {
    match links {
        3 => Some(((1.0, 1.0), PI)),
        6 => Some(((0.0, 1.0), -FRAC_PI_2)),
        12 => Some(((0.0, 0.0), 0.0)),
        9 => Some(((1.0, 0.0), FRAC_PI_2)),
        _ => None,
    }
}

/// A rail tile joined on `links`: a gravel bed, sleepers, and two steel rails.
/// Two sides at right angles join with a curve; any other set meets in the middle.
pub fn rail(c: &mut Canvas, z: f32, fine: bool, links: u8) {
    let links = sides(links);
    if let Some(((cx, cy), a0)) = curve(links) {
        let centre = (cx * z, cy * z);
        let r = z * 0.5;
        let arc = (a0, a0 + FRAC_PI_2);
        c.stroke_arc(centre, r, arc, pal::BALLAST, z * BED);
        // Sleepers fan out round the bend.
        let n = 3;
        for k in 0..n {
            let a = a0 + FRAC_PI_2 * (k as f32 + 0.5) / n as f32;
            c.save();
            c.translate(centre.0, centre.1);
            c.rotate(a);
            fill(
                c,
                rr(r - z * TIE / 2.0, -z * 0.05, z * TIE, z * 0.1, z * 0.02),
                pal::SLEEPER,
            );
            c.restore();
        }
        for side in [-1.0, 1.0] {
            let rr_ = r + side * z * GAUGE;
            c.stroke_arc(centre, rr_, arc, shade(pal::RAIL, -0.45), z * 0.075);
            c.stroke_arc(centre, rr_, arc, pal::RAIL, z * 0.05);
            if fine {
                c.stroke_arc(centre, rr_, arc, pal::RAIL_TOP, z * 0.018);
            }
        }
        return;
    }
    let h = z * 0.5;
    let arms: Vec<(f32, f32)> = DIRS
        .iter()
        .enumerate()
        .filter(|(i, _)| links & (1 << i) != 0)
        .map(|(_, &d)| d)
        .collect();
    // From the middle out to each joined side, `w` of a tile wide.
    let span = |c: &mut Canvas, (dx, dy): (f32, f32), w: f32, col: Rgba| {
        let r = z * w / 2.0;
        let (x0, x1) = (
            h + dx.min(0.0) * h - r * dy.abs(),
            h + dx.max(0.0) * h + r * dy.abs(),
        );
        let (y0, y1) = (
            h + dy.min(0.0) * h - r * dx.abs(),
            h + dy.max(0.0) * h + r * dx.abs(),
        );
        c.fill_rect(x0, y0, x1 - x0, y1 - y0, col);
    };
    for &d in &arms {
        span(c, d, BED, pal::BALLAST);
    }
    c.fill_rect(
        h - z * BED / 2.0,
        h - z * BED / 2.0,
        z * BED,
        z * BED,
        pal::BALLAST,
    );
    // Two sleepers on each arm, and one across the middle of a straight run.
    for &(dx, dy) in &arms {
        for t in [0.125, 0.375] {
            let (x, y) = (h + dx * z * (0.5 - t), h + dy * z * (0.5 - t));
            let (w, l) = (z * 0.1, z * TIE);
            let (rw, rh) = if dx != 0.0 { (w, l) } else { (l, w) };
            fill(
                c,
                rr(x - rw / 2.0, y - rh / 2.0, rw, rh, z * 0.02),
                pal::SLEEPER,
            );
        }
    }
    for &(dx, dy) in &arms {
        for side in [-1.0, 1.0] {
            // The rail's offset from the centre line, across the arm.
            let (ox, oy) = (-dy * side * z * GAUGE, dx * side * z * GAUGE);
            let end = (h + dx * h + ox, h + dy * h + oy);
            let start = (h + ox - dx * z * GAUGE, h + oy - dy * z * GAUGE);
            c.line(&[start, end], shade(pal::RAIL, -0.45), z * 0.075);
            c.line(&[start, end], pal::RAIL, z * 0.05);
            if fine {
                c.line(&[start, end], pal::RAIL_TOP, z * 0.018);
            }
        }
    }
    if arms.len() == 1 {
        // The end of the line: a buffer stop across the rails.
        let (dx, dy) = arms[0];
        let (x, y) = (h - dx * z * 0.12, h - dy * z * 0.12);
        let (w, l) = (z * 0.12, z * 0.56);
        let (rw, rh) = if dx != 0.0 { (w, l) } else { (l, w) };
        fill(
            c,
            rr(x - rw / 2.0, y - rh / 2.0, rw, rh, z * 0.03),
            pal::MACHINE_EDGE,
        );
        fill(
            c,
            rr(
                x - rw / 2.0 + 1.0,
                y - rh / 2.0 + 1.0,
                rw - 2.0,
                rh - 2.0,
                z * 0.02,
            ),
            pal::ACCENT,
        );
    }
}

/// A signal: rails joined on `links`, with a lamp head on two opposite
/// corners, one for each way through, showing their stop and go lamps.
pub fn signal(c: &mut Canvas, z: f32, fine: bool, links: u8) {
    rail(c, z, fine, links);
    match curve(sides(links)) {
        // On a curve, one head in the open corner across from the bend.
        Some(((cx, cy), _)) => {
            let at = |k: f32| z * (0.24 + 0.52 * (1.0 - k));
            lamp_head(c, z, fine, at(cx), at(cy));
        }
        None => {
            for (px, py) in [(0.8, 0.26), (0.2, 0.74)] {
                lamp_head(c, z, fine, z * px, z * py);
            }
        }
    }
}

fn lamp_head(c: &mut Canvas, z: f32, fine: bool, x: f32, y: f32) {
    let (w, hgt) = (z * 0.32, z * 0.5);
    fill(
        c,
        rr(
            x - w / 2.0 + z * 0.04,
            y - hgt / 2.0 + z * 0.05,
            w,
            hgt,
            z * 0.06,
        ),
        pal::SHADOW,
    );
    fill(
        c,
        rr(x - w / 2.0, y - hgt / 2.0, w, hgt, z * 0.06),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(
            x - w / 2.0 + 1.0,
            y - hgt / 2.0 + 1.0,
            w - 2.0,
            hgt - 2.0,
            z * 0.05,
        ),
        pal::MACHINE,
    );
    let (r, d) = (z * 0.095, z * 0.12);
    c.fill_circle(x, y - d, r, pal::STARVE);
    c.fill_circle(x, y + d, r, pal::GO);
    if fine {
        c.fill_circle(x - r * 0.3, y - d - r * 0.3, r * 0.35, pal::WHITE);
        c.fill_circle(x - r * 0.3, y + d - r * 0.3, r * 0.35, pal::WHITE);
    }
}

/// A train stop's rails: track joined on `links`, with yellow marks where
/// a train halts.
pub fn stop(c: &mut Canvas, z: f32, fine: bool, links: u8) {
    rail(c, z, fine, links);
    let links = sides(links);
    for (i, (dx, dy)) in DIRS.iter().enumerate() {
        if links & (1 << i) == 0 {
            continue;
        }
        // A stripe across the bed where each arm meets the middle.
        let (x, y) = (z * (0.5 + dx * 0.3), z * (0.5 + dy * 0.3));
        let (rw, rh) = if *dx != 0.0 {
            (z * 0.06, z * BED)
        } else {
            (z * BED, z * 0.06)
        };
        c.fill_rect(x - rw / 2.0, y - rh / 2.0, rw, rh, pal::ACCENT);
    }
}

/// A stop's platform along its east side, to be turned the way it faces:
/// the side it unloads out of. Casts no shadow: the renderer casts it.
pub fn platform(c: &mut Canvas, z: f32, fine: bool) {
    let (x, w) = (z * 0.8, z * 0.2);
    c.fill_rect(x, 0.0, w, z, pal::MACHINE_EDGE);
    c.fill_rect(x + 1.0, 1.0, w - 2.0, z - 2.0, pal::CONCRETE);
    c.fill_rect(x, 0.0, z * 0.04, z, pal::ACCENT);
    if fine {
        // A hazard edge, and a sign with an arrow out.
        for k in 0..5 {
            let y = z * (0.05 + k as f32 * 0.2);
            c.fill_rect(x, y, z * 0.04, z * 0.08, pal::HAZARD_DARK);
        }
    }
    let (sx, sy) = (z * 0.9, z * 0.5);
    c.fill_circle(sx, sy, z * 0.08, pal::MACHINE_EDGE);
    c.fill_circle(sx, sy, z * 0.065, pal::ACCENT);
    let a = z * 0.035;
    fill(
        c,
        crate::canvas::poly(&[(-1.0, -1.0), (1.2, 0.0), (-1.0, 1.0)], sx, sy, a),
        pal::MACHINE_EDGE,
    );
}

/// A stop as an icon: straight track east to west with its platform along
/// the south side.
pub fn stop_icon(c: &mut Canvas, z: f32, fine: bool) {
    stop(c, z, fine, 5);
    c.save();
    c.translate(z, 0.0);
    c.rotate(FRAC_PI_2);
    platform(c, z, fine);
    c.restore();
}

/// A locomotive heading east, a little over a tile long: a cab at the
/// back, a long bonnet and headlamps at the front. Casts no shadow: the
/// renderer casts it, since the train is turned when drawn.
pub fn locomotive(c: &mut Canvas, z: f32, fine: bool) {
    let (x0, x1) = (z * 0.04, z * 0.96);
    let (y0, y1) = (z * 0.2, z * 0.8);
    let (w, h) = (x1 - x0, y1 - y0);
    fill(c, rr(x0, y0, w, h, z * 0.12), shade(pal::LOCO, -0.55));
    fill(
        c,
        rr(x0 + 1.5, y0 + 1.5, w - 3.0, h - 3.0, z * 0.11),
        pal::LOCO,
    );
    // The bonnet's roof, narrower, with grilles; the cab at the back.
    let (bx, bw) = (z * 0.38, z * 0.5);
    fill(
        c,
        rr(bx, y0 + h * 0.2, bw, h * 0.6, z * 0.06),
        pal::LOCO_ROOF,
    );
    let (cx, cw) = (z * 0.1, z * 0.24);
    fill(
        c,
        rr(cx, y0 + h * 0.08, cw, h * 0.84, z * 0.06),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(
            cx + 1.0,
            y0 + h * 0.08 + 1.0,
            cw - 2.0,
            h * 0.84 - 2.0,
            z * 0.05,
        ),
        shade(pal::LOCO, 0.25),
    );
    // Cab windows, front corners.
    c.fill_rect(
        cx + cw * 0.62,
        y0 + h * 0.16,
        cw * 0.26,
        h * 0.2,
        pal::GLASS,
    );
    c.fill_rect(
        cx + cw * 0.62,
        y0 + h * 0.64,
        cw * 0.26,
        h * 0.2,
        pal::GLASS,
    );
    if fine {
        for k in 0..4 {
            let gx = bx + bw * (0.12 + k as f32 * 0.22);
            c.fill_rect(
                gx,
                y0 + h * 0.3,
                bw * 0.08,
                h * 0.4,
                shade(pal::LOCO_ROOF, -0.35),
            );
        }
        c.fill_circle(bx + bw * 0.9, y0 + h * 0.5, z * 0.04, pal::RIVET);
    }
    // A white stripe down each side, and the headlamps.
    c.fill_rect(x0 + z * 0.08, y0 + 1.5, w - z * 0.16, z * 0.03, pal::INK);
    c.fill_rect(
        x0 + z * 0.08,
        y1 - 1.5 - z * 0.03,
        w - z * 0.16,
        z * 0.03,
        pal::INK,
    );
    for y in [y0 + h * 0.22, y1 - h * 0.22] {
        c.fill_circle(x1 - z * 0.05, y, z * 0.045, pal::ACCENT);
    }
}

/// A tank train (S5), facing east: the locomotive's cab at the back and a
/// round-ended steel tank for its bonnet, hooped, with a filler dome.
pub fn tank_train(c: &mut Canvas, z: f32, fine: bool) {
    let (x0, x1) = (z * 0.04, z * 0.96);
    let (y0, y1) = (z * 0.2, z * 0.8);
    let (w, h) = (x1 - x0, y1 - y0);
    // The same orange chassis as a locomotive, so it reads as a train.
    fill(c, rr(x0, y0, w, h, z * 0.12), shade(pal::LOCO, -0.55));
    fill(
        c,
        rr(x0 + 1.5, y0 + 1.5, w - 3.0, h - 3.0, z * 0.11),
        pal::LOCO,
    );
    // The cab.
    let (cx, cw) = (z * 0.1, z * 0.22);
    fill(
        c,
        rr(cx, y0 + h * 0.08, cw, h * 0.84, z * 0.06),
        pal::MACHINE_EDGE,
    );
    fill(
        c,
        rr(
            cx + 1.0,
            y0 + h * 0.08 + 1.0,
            cw - 2.0,
            h * 0.84 - 2.0,
            z * 0.05,
        ),
        shade(pal::LOCO, 0.25),
    );
    c.fill_rect(
        cx + cw * 0.62,
        y0 + h * 0.16,
        cw * 0.26,
        h * 0.2,
        pal::GLASS,
    );
    c.fill_rect(
        cx + cw * 0.62,
        y0 + h * 0.64,
        cw * 0.26,
        h * 0.2,
        pal::GLASS,
    );
    // The tank: a long capsule, lit from the top.
    let (tx, tw) = (z * 0.36, z * 0.56);
    let (ty, th) = (y0 + h * 0.1, h * 0.8);
    fill(c, rr(tx, ty, tw, th, th / 2.0), pal::MACHINE_EDGE);
    fill(
        c,
        rr(tx + 1.0, ty + 1.0, tw - 2.0, th - 2.0, th / 2.0 - 1.0),
        pal::PIPE,
    );
    fill(
        c,
        rr(
            tx + th * 0.3,
            ty + th * 0.16,
            tw - th * 0.6,
            th * 0.2,
            th * 0.1,
        ),
        pal::PIPE_HI,
    );
    fill(
        c,
        rr(
            tx + th * 0.3,
            ty + th * 0.72,
            tw - th * 0.6,
            th * 0.12,
            th * 0.06,
        ),
        shade(pal::PIPE, -0.3),
    );
    if fine {
        // Hoops round the tank.
        for k in [0.3, 0.7] {
            c.fill_rect(
                tx + tw * k - z * 0.015,
                ty + 1.0,
                z * 0.03,
                th - 2.0,
                shade(pal::PIPE, -0.4),
            );
        }
    }
    // The filler dome, with a hatch.
    c.fill_circle(tx + tw * 0.5, ty + th * 0.5, th * 0.2, pal::MACHINE_EDGE);
    c.fill_circle(tx + tw * 0.5, ty + th * 0.5, th * 0.16, pal::PIPE_HI);
    c.fill_circle(tx + tw * 0.5, ty + th * 0.5, th * 0.06, pal::RIVET);
    // The stripes and headlamps of every train.
    c.fill_rect(x0 + z * 0.08, y0 + 1.5, w - z * 0.16, z * 0.03, pal::INK);
    c.fill_rect(
        x0 + z * 0.08,
        y1 - 1.5 - z * 0.03,
        w - z * 0.16,
        z * 0.03,
        pal::INK,
    );
    for y in [y0 + h * 0.22, y1 - h * 0.22] {
        c.fill_circle(x1 - z * 0.05, y, z * 0.045, pal::ACCENT);
    }
}
