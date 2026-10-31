//! Belts that join up: a straight run, a corner, a side-load and the two ends
//! are each drawn so that neighbouring tiles read as one continuous belt.
//!
//! A belt tile is painted in two layers, both running east and turned when
//! drawn. The surface (body, rollers, chevrons) moves: it is painted at
//! [`FRAMES`] phases so the renderer can step through them. The frame (rails,
//! rivets, end caps and the openings where another belt feeds in from the
//! side) sits on top and does not move. Laid together, a plain straight tile
//! looks exactly like [`crate::draw::belt`].

use crate::canvas::{arc, poly, rect, rr, Canvas};
use crate::palette::{self as pal, alpha, Rgba};
use std::f32::consts::{FRAC_PI_2, PI};
use tiny_skia::PathBuilder;

/// How many phases of the moving surface are painted. The surface repeats
/// every half tile, so each frame moves it on by a sixteenth of a tile.
pub const FRAMES: u8 = 8;

/// Which way a belt tile runs, as painted facing east.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Bend {
    /// In from the west edge, out of the east edge.
    #[default]
    Straight,
    /// A corner: in from its left (the north edge), out of the east edge.
    FromLeft,
    /// A corner: in from its right (the south edge), out of the east edge.
    FromRight,
}

impl Bend {
    pub const ALL: [Bend; 3] = [Bend::Straight, Bend::FromLeft, Bend::FromRight];

    /// Where on a corner the centre of the arc is, by y: -1 north, 1 south.
    fn side(self) -> f32 {
        match self {
            Bend::FromLeft => -1.0,
            _ => 1.0,
        }
    }
}

/// What a belt tile's frame shows at its ends and sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Joins {
    /// Nothing feeds it from behind: the rails close off at the west edge.
    pub start: bool,
    /// Nothing takes from it ahead: the rails close off at the east edge.
    pub end: bool,
    /// A belt feeds in from the left (north) side: that rail opens.
    pub left: bool,
    /// A belt feeds in from the right (south) side: that rail opens.
    pub right: bool,
}

impl Joins {
    /// The sixteen straight variants, as bits: start 1, end 2, left 4, right 8.
    pub fn bits(self) -> u8 {
        u8::from(self.start)
            | u8::from(self.end) << 1
            | u8::from(self.left) << 2
            | u8::from(self.right) << 3
    }

    pub fn from_bits(b: u8) -> Self {
        Joins {
            start: b & 1 != 0,
            end: b & 2 != 0,
            left: b & 4 != 0,
            right: b & 8 != 0,
        }
    }
}

/// Where along a corner an item `along` its tile is, in tiles from the
/// tile's centre, as painted facing east. Items join a corner half way along
/// its tile (they come in from the side), so the second half of the tile is
/// spread over the whole arc.
pub fn corner_point(bend: Bend, along: f32) -> (f32, f32) {
    let s = bend.side();
    let u = ((along - 0.5) * 2.0).clamp(0.0, 1.0);
    let a = angle(s, u);
    (0.5 + 0.5 * a.cos(), s * 0.5 + 0.5 * a.sin())
}

/// The angle around a corner's centre at `u` (0 in, 1 out).
fn angle(s: f32, u: f32) -> f32 {
    PI + s * u * FRAC_PI_2
}

/// The quarter ring of a corner between radii `r0` and `r1`, in pixels around
/// the tile's centre, for a tile `z` pixels across.
fn ring(z: f32, s: f32, r0: f32, r1: f32) -> Option<tiny_skia::Path> {
    let (cx, cy) = (0.5 * z, s * 0.5 * z);
    let (a0, a1) = (angle(s, 0.0), angle(s, 1.0));
    let mut pb = PathBuilder::new();
    arc(&mut pb, cx, cy, r1 * z, a0, a1, true);
    arc(&mut pb, cx, cy, r0 * z, a1, a0, false);
    pb.close();
    pb.finish()
}

fn fill(c: &mut Canvas, p: Option<tiny_skia::Path>, col: Rgba) {
    if let Some(p) = p {
        c.fill(&p, col);
    }
}

/// Move the canvas to the point `u` of the way round a corner, at `r` tiles
/// from its centre, turned to face the way items go there.
fn on_corner(c: &mut Canvas, z: f32, s: f32, u: f32, r: f32) {
    let a = angle(s, u);
    c.translate(0.5 * z + r * z * a.cos(), s * 0.5 * z + r * z * a.sin());
    // Items go round towards the exit: the tangent, turned to match.
    c.rotate(a + s * FRAC_PI_2);
}

/// The moving layer of a belt tile `z` pixels across, at `phase` (0 to 1)
/// of its half-tile cycle: shadow, body, rollers and chevrons, or at far
/// zoom (`fine` off) the plain band and arrow.
pub fn surface(c: &mut Canvas, z: f32, fine: bool, bend: Bend, phase: f32) {
    let phase = phase.rem_euclid(1.0);
    c.save();
    c.translate(z / 2.0, z / 2.0);
    if bend == Bend::Straight {
        c.fill_rect(-z * 0.5, -z * 0.34, z, z * 0.78, [0, 0, 0, 64]);
        c.fill_rect(-z * 0.5, -z * 0.38, z, z * 0.76, pal::BELT);
        if !fine {
            c.fill_rect(-z * 0.5, -z * 0.1, z, z * 0.2, alpha(pal::CHEVRON, 0.9));
            arrow(c, z * 0.7);
        } else {
            c.save();
            if let Some(p) = rect(-z * 0.5, -z * 0.31, z, z * 0.62) {
                c.clip(&p);
            }
            let off = phase * 0.5 * z;
            for k in -2..=2 {
                let x = k as f32 * z * 0.25 + (off % (z * 0.25)) - z * 0.02;
                c.fill_rect(x, -z * 0.31, z * 0.04, z * 0.62, pal::ROLLER);
            }
            for k in -2..=2 {
                c.save();
                c.translate(k as f32 * z * 0.5 + off, 0.0);
                chevron(c, z);
                c.restore();
            }
            c.restore();
        }
        c.restore();
        return;
    }
    let s = bend.side();
    c.save();
    c.translate(0.0, z * 0.05);
    fill(c, ring(z, s, 0.11, 0.89), [0, 0, 0, 64]);
    c.restore();
    fill(c, ring(z, s, 0.12, 0.88), pal::BELT);
    if !fine {
        fill(c, ring(z, s, 0.4, 0.6), alpha(pal::CHEVRON, 0.9));
        c.save();
        on_corner(c, z, s, 0.5, 0.5);
        arrow(c, z * 0.7);
        c.restore();
    } else {
        c.save();
        if let Some(p) = ring(z, s, 0.19, 0.81) {
            c.clip(&p);
        }
        // A corner is crossed in half the time of a straight tile (items
        // join it half way), so the surface goes round it once a cycle.
        let rollers = (phase * 2.0).fract();
        for k in -1..=2 {
            c.save();
            on_corner(c, z, s, (k as f32 + rollers) * 0.5, 0.5);
            c.fill_rect(-z * 0.02, -z * 0.31, z * 0.04, z * 0.62, pal::ROLLER);
            c.restore();
        }
        for k in -1..=1 {
            c.save();
            on_corner(c, z, s, k as f32 + phase, 0.5);
            chevron(c, z);
            c.restore();
        }
        c.restore();
    }
    c.restore();
}

/// One chevron, pointing east, at the canvas origin.
fn chevron(c: &mut Canvas, z: f32) {
    c.line(
        &[(-z * 0.1, -z * 0.17), (z * 0.05, 0.0), (-z * 0.1, z * 0.17)],
        pal::CHEVRON,
        (z * 0.05).max(1.0),
    );
}

/// The far-zoom arrow, pointing east, at the canvas origin.
fn arrow(c: &mut Canvas, s: f32) {
    if let Some(p) = poly(&[(0.5, 0.0), (0.15, -0.28), (0.15, 0.28)], 0.0, 0.0, s) {
        c.fill(&p, pal::INK);
    }
}

/// The still layer of a belt tile: rails and rivets, closed off at an end
/// with nothing beyond it and opened where a belt feeds in from the side.
/// On a corner only `joins.end` counts.
pub fn frame(c: &mut Canvas, z: f32, fine: bool, bend: Bend, joins: Joins) {
    c.save();
    c.translate(z / 2.0, z / 2.0);
    // Rail band across the belt, as fractions of a tile from its centre line.
    let (outer, inner) = if fine { (0.4, 0.31) } else { (0.38, 0.31) };
    if bend == Bend::Straight {
        for (side, open) in [(-1.0f32, joins.left), (1.0, joins.right)] {
            let y = if side < 0.0 { -outer } else { inner };
            let spans: &[(f32, f32)] = if open {
                &[(-0.5, -outer), (outer, 0.5)]
            } else {
                &[(-0.5, 0.5)]
            };
            for &(x0, x1) in spans {
                c.fill_rect(
                    x0 * z,
                    y * z,
                    (x1 - x0) * z,
                    (outer - inner) * z,
                    pal::BELT_RAIL,
                );
                if fine {
                    c.fill_rect(x0 * z, y * z, (x1 - x0) * z, z * 0.025, pal::RAIL_HI);
                }
            }
            if open {
                // The feeding belt's floor and rails run on to meet this one.
                let (y0, y1) = if side < 0.0 {
                    (-0.5, -inner)
                } else {
                    (inner, 0.5)
                };
                c.fill_rect(-0.38 * z, y0 * z, 0.76 * z, (y1 - y0) * z, pal::BELT);
                for x in [-outer, inner] {
                    c.fill_rect(
                        x * z,
                        y0 * z,
                        (outer - inner) * z,
                        (y1 - y0) * z,
                        pal::BELT_RAIL,
                    );
                    if fine {
                        c.fill_rect(x * z, y0 * z, z * 0.025, (y1 - y0) * z, pal::RAIL_HI);
                    }
                }
            } else if fine {
                let ry = side * 0.355;
                for bx in [-0.25, 0.25] {
                    c.fill_circle(bx * z, ry * z, z * 0.022, pal::RIVET);
                }
            }
        }
        if joins.start {
            cap(c, z, fine, -0.5);
        }
    } else {
        let s = bend.side();
        for (r0, r1) in [(0.5 - outer, 0.5 - inner), (0.5 + inner, 0.5 + outer)] {
            fill(c, ring(z, s, r0, r1), pal::BELT_RAIL);
            if fine {
                fill(c, ring(z, s, r0, r0 + 0.025), pal::RAIL_HI);
            }
        }
        if fine {
            for u in [0.25, 0.75] {
                for r in [0.5 - 0.355, 0.5 + 0.355] {
                    let a = angle(s, u);
                    let (x, y) = (0.5 + r * a.cos(), s * 0.5 + r * a.sin());
                    c.fill_circle(x * z, y * z, z * 0.022, pal::RIVET);
                }
            }
        }
    }
    if joins.end {
        cap(c, z, fine, 0.5 - 0.09);
    }
    c.restore();
}

/// A lip across the belt at `x` (tiles from the centre) closing off its rails.
fn cap(c: &mut Canvas, z: f32, fine: bool, x: f32) {
    let w = 0.09;
    if let Some(p) = rr(x * z, -0.4 * z, w * z, 0.8 * z, 0.03 * z) {
        c.fill(&p, pal::BELT_RAIL);
    }
    if fine {
        c.fill_rect(x * z, -0.4 * z, w * z, 0.025 * z, pal::RAIL_HI);
        c.fill_rect(
            (x + w * 0.5) * z - 0.01 * z,
            -0.3 * z,
            0.02 * z,
            0.6 * z,
            pal::RIVET,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw;

    fn layers(z: f32, fine: bool, bend: Bend, joins: Joins, phase: f32) -> Canvas {
        let mut c = Canvas::new(z as u32, z as u32);
        surface(&mut c, z, fine, bend, phase);
        frame(&mut c, z, fine, bend, joins);
        c
    }

    /// The two layers of a plain straight tile add up to the one-piece belt
    /// the prototype draws, which the golden test holds to the prototype.
    #[test]
    fn a_plain_straight_tile_is_the_prototype_belt() {
        for fine in [true, false] {
            for t in [0.0, 0.1, 0.2] {
                let z = 64.0;
                let mut one = Canvas::new(64, 64);
                draw::belt(&mut one, z, fine, t, 2.0);
                let phase = ((t * 2.0) % 0.5) / 0.5;
                let two = layers(z, fine, Bend::Straight, Joins::default(), phase);
                let (a, b) = (one.rgba(), two.rgba());
                let worst = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max();
                assert!(worst <= Some(2), "fine {fine}, t {t}: off by {worst:?}");
            }
        }
    }

    fn opaque(c: &Canvas, x: u32, y: u32) -> bool {
        c.rgba()[((y * c.width() + x) * 4 + 3) as usize] > 200
    }

    #[test]
    fn a_corner_meets_the_edges_it_joins_and_leaves_the_others_open() {
        let z = 64.0;
        let c = layers(z, true, Bend::FromLeft, Joins::default(), 0.0);
        assert!(opaque(&c, 32, 1), "joins the north edge in the middle");
        assert!(opaque(&c, 62, 32), "joins the east edge in the middle");
        assert!(!opaque(&c, 1, 32), "the west edge is open");
        assert!(!opaque(&c, 32, 62), "the south edge is open");
        let c = layers(z, true, Bend::FromRight, Joins::default(), 0.0);
        assert!(opaque(&c, 32, 62) && !opaque(&c, 32, 1));
    }

    #[test]
    fn a_side_load_opens_the_rail_and_reaches_the_edge() {
        let z = 64.0;
        let shut = layers(z, true, Bend::Straight, Joins::default(), 0.0);
        let open = layers(
            z,
            true,
            Bend::Straight,
            Joins {
                left: true,
                ..Joins::default()
            },
            0.0,
        );
        assert!(
            !opaque(&shut, 32, 2),
            "a plain belt stops short of its side"
        );
        assert!(opaque(&open, 32, 2), "a side-load reaches the side");
        assert!(!opaque(&open, 32, 62), "only on that side");
    }

    #[test]
    fn corner_points_run_from_the_side_to_the_exit() {
        let (x, y) = corner_point(Bend::FromLeft, 0.5);
        assert!(x.abs() < 1e-4 && (y + 0.5).abs() < 1e-4, "{x} {y}");
        let (x, y) = corner_point(Bend::FromLeft, 1.0);
        assert!((x - 0.5).abs() < 1e-4 && y.abs() < 1e-4, "{x} {y}");
        let (x, y) = corner_point(Bend::FromRight, 0.5);
        assert!(x.abs() < 1e-4 && (y - 0.5).abs() < 1e-4, "{x} {y}");
    }

    #[test]
    fn joins_round_trip_through_bits() {
        for b in 0..16 {
            assert_eq!(Joins::from_bits(b).bits(), b);
        }
    }
}
