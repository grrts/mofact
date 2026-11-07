//! The app icon (T68), drawn in code like the rest of the art: a hub-yellow
//! gear over a belt carrying plates, on the factory floor.
//!
//! It is drawn on a square `s` pixels across. Android's adaptive icons crop
//! it to a circle or squircle of about two thirds of the square and move it
//! a little, so everything that matters sits inside the middle 60%; the
//! belt runs off both edges, so any crop still reads as a belt.

use crate::canvas::{rect, Canvas};
use crate::draw::{self, Shape};
use crate::palette::{self as pal, alpha, shade, Rgba};
use std::f32::consts::TAU;
use tiny_skia::PathBuilder;

/// Which part of the icon to draw: the whole of it, or one of Android's
/// two adaptive layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Full,
    /// The floor only.
    Background,
    /// The belt and gear, on transparent.
    Foreground,
}

/// The iron and copper plates on the belt, as the game colours them.
const IRON: Rgba = [0xa9, 0xb6, 0xc3, 0xff];
const COPPER: Rgba = [0xe0, 0x8a, 0x4f, 0xff];

/// Draw the icon on `c`, `s` pixels square.
pub fn icon(c: &mut Canvas, s: f32, layer: Layer) {
    if layer != Layer::Foreground {
        floor(c, s);
    }
    if layer != Layer::Background {
        belt(c, s);
        gear(c, s);
    }
}

/// The factory floor: the ground, its tile grid and a soft light from above.
fn floor(c: &mut Canvas, s: f32) {
    c.fill_rect(0.0, 0.0, s, s, pal::GROUND);
    // Light falling from the top centre, in rings.
    for k in (0..24).rev() {
        let r = s * (0.25 + k as f32 * 0.045);
        let a = 0.035 * (1.0 - k as f32 / 24.0);
        c.fill_circle(s * 0.5, s * 0.32, r, alpha(pal::MACHINE_HI, a));
    }
    let tile = s / 6.0;
    let w = (s * 0.004).max(1.0);
    for k in 1..6 {
        let p = k as f32 * tile;
        c.fill_rect(p - w / 2.0, 0.0, w, s, alpha(pal::GROUND2, 0.9));
        c.fill_rect(0.0, p - w / 2.0, s, w, alpha(pal::GROUND2, 0.9));
    }
}

/// A belt across the lower part, with plates riding east on it.
fn belt(c: &mut Canvas, s: f32) {
    let z = s * 0.26;
    let y = s * 0.66;
    let n = (s / z).ceil() as i32 + 1;
    for k in -1..n {
        c.save();
        c.translate(k as f32 * z, y);
        draw::belt(c, z, true, 0.0, 1.0);
        c.restore();
    }
    // Plates, evenly spaced and off-centre so they read as moving.
    for (k, col) in [(0, IRON), (1, COPPER), (2, IRON), (3, COPPER)] {
        let x = s * (0.08 + k as f32 * 0.28);
        draw::item(c, Shape::Plate, col, x, y + z * 0.5, z * 0.5, false);
    }
}

/// The big gear, in the hub's yellow, with a dark hub and a hazard ring.
fn gear(c: &mut Canvas, s: f32) {
    let (cx, cy) = (s * 0.5, s * 0.43);
    let r = s * 0.25;
    let teeth = 10;
    let path = |r: f32, depth: f32| {
        let mut pb = PathBuilder::new();
        let n = teeth * 4;
        for i in 0..n {
            let a = i as f32 / n as f32 * TAU - TAU / 4.0;
            // Each tooth: up, across the top, down, across the root.
            let rr = if i % 4 == 1 || i % 4 == 2 {
                r
            } else {
                r - depth
            };
            let (x, y) = (cx + a.cos() * rr, cy + a.sin() * rr);
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        pb.close();
        pb.finish()
    };
    let depth = r * 0.2;
    // Its shadow on the belt and floor.
    c.save();
    c.translate(s * 0.012, s * 0.03);
    if let Some(p) = path(r, depth) {
        c.fill(&p, [0, 0, 0, 110]);
    }
    c.restore();
    // The rim, a lighter face, and a darker lower half for depth.
    if let Some(p) = path(r, depth) {
        c.fill(&p, shade(pal::ACCENT, -0.28));
    }
    if let Some(p) = path(r * 0.96, depth * 0.9) {
        c.fill(&p, pal::ACCENT);
        c.save();
        c.clip(&p);
        if let Some(lower) = rect(0.0, cy + r * 0.15, s, s) {
            c.fill(&lower, shade(pal::ACCENT, -0.1));
        }
        c.restore();
    }
    // A hazard band, then the dark bore with its axle.
    let band = r * 0.56;
    c.fill_circle(cx, cy, band, pal::HAZARD_DARK);
    c.save();
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, band * 0.94);
    if let Some(p) = pb.finish() {
        c.clip(&p);
    }
    c.fill_rect(0.0, 0.0, s, s, shade(pal::ACCENT, -0.05));
    let w = band * 0.24;
    let mut k = -6.0;
    while k < 6.0 {
        let x = cx + k * w * 2.0;
        c.line(
            &[(x, cy - band), (x - band * 2.0, cy + band)],
            pal::HAZARD_DARK,
            w,
        );
        k += 1.0;
    }
    c.restore();
    c.fill_circle(cx, cy, band * 0.68, pal::MACHINE_EDGE);
    c.fill_circle(cx, cy, band * 0.58, pal::PAD);
    c.fill_circle(cx, cy, band * 0.26, pal::MACHINE_EDGE);
    c.fill_circle(
        cx - band * 0.06,
        cy - band * 0.06,
        band * 0.14,
        pal::MACHINE_HI,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gear and belt stay inside Android's safe zone: the middle 66%
    /// of the foreground is where everything but the belt's ends is drawn.
    #[test]
    fn the_foreground_keeps_its_corners_clear() {
        let s = 216;
        let mut c = Canvas::new(s, s);
        icon(&mut c, s as f32, Layer::Foreground);
        let px = c.rgba();
        let at = |x: u32, y: u32| px[((y * s + x) * 4 + 3) as usize];
        // Top corners and the top edge's middle are clear of the gear.
        for (x, y) in [(2, 2), (s - 3, 2), (s / 2, 4)] {
            assert_eq!(at(x, y), 0, "({x}, {y}) should be clear");
        }
        // The gear's face covers the middle.
        assert!(at(s / 2 + s / 6, s * 43 / 100) > 200);
    }
}
