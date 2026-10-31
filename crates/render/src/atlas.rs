//! Texture atlases, one per zoom band (R66).
//!
//! Every sprite is painted at the band's pixel size, so a building is never
//! drawn more than about 1.5 times bigger or smaller than it was painted. The
//! sprites come from the art kit, through [`crate::Look::paint`].

use std::collections::HashMap;

/// Pixels per tile for each zoom band, smallest first.
///
/// The largest is for phones: a tile at the default zoom covers about 100
/// screen pixels on a 3x display, so smaller bands would be blown up and blur.
pub const BANDS: [u32; 4] = [16, 32, 64, 128];

/// The band to draw with when a tile covers `pixels_per_tile` screen pixels:
/// the smallest band at least that big, so sprites are only ever scaled down.
pub fn band_for(pixels_per_tile: f32) -> usize {
    BANDS
        .iter()
        .position(|&b| b as f32 >= pixels_per_tile * 0.75)
        .unwrap_or(BANDS.len() - 1)
}

/// Normalised texture rectangle: x, y, width, height.
pub type Uv = [f32; 4];

/// Padding around each sprite, filled by repeating its edge so linear
/// filtering never picks up a neighbour.
const PAD: u32 = 2;

/// A square RGBA image with sprites packed into shelves.
pub struct Atlas {
    pub size: u32,
    pub pixels: Vec<u8>,
    sprites: HashMap<String, Uv>,
    shelf: (u32, u32, u32), // x, y, height of the current shelf
    /// Goes up with every sprite added, so the GPU copy knows when to refresh.
    pub version: u64,
}

impl Atlas {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            pixels: vec![0; (size * size * 4) as usize],
            sprites: HashMap::new(),
            shelf: (0, 0, 0),
            version: 0,
        }
    }

    /// Paint a `w` × `h` sprite called `name`; `paint(x, y)` returns its RGBA
    /// at each pixel. Returns `None` if the atlas is full.
    pub fn add(
        &mut self,
        name: &str,
        w: u32,
        h: u32,
        paint: impl Fn(u32, u32) -> [u8; 4],
    ) -> Option<Uv> {
        let (pw, ph) = (w + 2 * PAD, h + 2 * PAD);
        let (mut x, mut y, mut shelf_h) = self.shelf;
        if x + pw > self.size {
            (x, y, shelf_h) = (0, y + shelf_h, 0);
        }
        if pw > self.size || y + ph > self.size {
            return None;
        }
        for py in 0..ph {
            for px in 0..pw {
                let sx = px.saturating_sub(PAD).min(w - 1);
                let sy = py.saturating_sub(PAD).min(h - 1);
                let i = (((y + py) * self.size + x + px) * 4) as usize;
                self.pixels[i..i + 4].copy_from_slice(&paint(sx, sy));
            }
        }
        self.shelf = (x + pw, y, shelf_h.max(ph));
        let s = self.size as f32;
        let uv = [
            (x + PAD) as f32 / s,
            (y + PAD) as f32 / s,
            w as f32 / s,
            h as f32 / s,
        ];
        self.sprites.insert(name.to_owned(), uv);
        self.version += 1;
        Some(uv)
    }

    pub fn get(&self, name: &str) -> Option<Uv> {
        self.sprites.get(name).copied()
    }
}

/// Sprites painted here rather than by the art kit.
pub mod sprites {
    pub use crate::look::ORE;

    /// Paint the ore pebbles at `px` pixels a tile. Returns false if the atlas is full.
    /// Four variants, with the chunks laid out differently, so a field of
    /// ore doesn't repeat tile after tile.
    pub fn paint_ore(atlas: &mut super::Atlas, px: u32) -> bool {
        let f = px as f32;
        (0..ORE_VARIANTS).all(|k| {
            atlas
                .add(&ore_name(k), px, px, |x, y| {
                    ore(x as f32 / f, y as f32 / f, k)
                })
                .is_some()
        })
    }

    /// How many kinds of decoration the world lays on the ground
    /// (`mofact_world::Decor`, numbered from 1).
    pub const DECOR_KINDS: u8 = 7;

    /// The name of decoration `kind`.
    pub fn decor_name(kind: u8) -> String {
        format!("decor:{kind}")
    }

    /// Paint the trees, bushes, stones and flowers at `px` pixels a tile, in
    /// their own colours. Returns false if the atlas is full.
    pub fn paint_decor(atlas: &mut super::Atlas, px: u32) -> bool {
        let f = px as f32;
        let aa = 1.2 / f;
        (1..=DECOR_KINDS).all(|k| {
            atlas
                .add(&decor_name(k), px, px, |x, y| {
                    decor(k, (x as f32 + 0.5) / f, (y as f32 + 0.5) / f, aa)
                })
                .is_some()
        })
    }

    use mofact_artkit::palette::{self as pal, Rgba};

    /// Straight-alpha colour being built up, layer over layer.
    #[derive(Clone, Copy)]
    struct Px([f32; 4]);

    impl Px {
        fn over(self, c: Rgba, a: f32) -> Px {
            let a = a.clamp(0.0, 1.0) * f32::from(c[3]) / 255.0;
            let [r, g, b, da] = self.0;
            let out = a + da * (1.0 - a);
            if out <= 0.0 {
                return self;
            }
            let mix = |src: u8, dst: f32| (f32::from(src) * a + dst * da * (1.0 - a)) / out;
            Px([mix(c[0], r), mix(c[1], g), mix(c[2], b), out])
        }

        fn rgba(self) -> [u8; 4] {
            let [r, g, b, a] = self.0;
            [r as u8, g as u8, b as u8, (a * 255.0) as u8]
        }
    }

    /// How much of a pixel lies inside a shape `d` away from its edge
    /// (negative inside), with soft edges `aa` wide.
    fn inside(d: f32, aa: f32) -> f32 {
        (0.5 - d / aa).clamp(0.0, 1.0)
    }

    fn circle(u: f32, v: f32, cx: f32, cy: f32, r: f32) -> f32 {
        ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r
    }

    /// A rounded clump: the union of a few circles, `(x, y, r)`.
    fn clump(u: f32, v: f32, parts: &[(f32, f32, f32)]) -> f32 {
        parts
            .iter()
            .map(|&(x, y, r)| circle(u, v, x, y, r))
            .fold(f32::MAX, f32::min)
    }

    /// Distance from a line segment.
    fn segment(u: f32, v: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
        let (px, py, bx, by) = (u - a.0, v - a.1, b.0 - a.0, b.1 - a.1);
        let t = ((px * bx + py * by) / (bx * bx + by * by)).clamp(0.0, 1.0);
        ((px - bx * t).powi(2) + (py - by * t).powi(2)).sqrt()
    }

    /// A lit, rounded body: dark rim, base colour, lit top left.
    fn lit(p: Px, d: f32, lit_d: f32, base: Rgba, hi: Rgba, aa: f32) -> Px {
        p.over(pal::shade(base, -0.3), inside(d, aa))
            .over(base, inside(d + 0.035, aa))
            .over(hi, inside(lit_d, aa) * inside(d + 0.035, aa))
    }

    fn decor(kind: u8, u: f32, v: f32, aa: f32) -> [u8; 4] {
        let clear = Px([0.0; 4]);
        let shadow = |d: f32| clear.over(pal::SHADOW, inside(d, aa * 6.0) * 1.3);
        let px = match kind {
            // A broadleaf tree: a lumpy round crown.
            1 => {
                const CROWN: [(f32, f32, f32); 5] = [
                    (0.5, 0.5, 0.3),
                    (0.35, 0.46, 0.2),
                    (0.65, 0.53, 0.2),
                    (0.47, 0.34, 0.2),
                    (0.54, 0.65, 0.2),
                ];
                let d = clump(u, v, &CROWN);
                let ds = clump(u - 0.07, v - 0.09, &CROWN);
                let lit_d = clump(u + 0.07, v + 0.08, &CROWN) + 0.06;
                lit(shadow(ds), d, lit_d, pal::LEAF, pal::LEAF_HI, aa)
            }
            // A pine, from above: a star of needle tiers.
            2 => {
                let (dx, dy) = (u - 0.5, v - 0.5);
                let angle = dy.atan2(dx);
                let r = 0.36 + 0.05 * (angle * 7.0).cos();
                let d = (dx * dx + dy * dy).sqrt() - r;
                let (sx, sy) = (dx - 0.07, dy - 0.09);
                let ds = (sx * sx + sy * sy).sqrt() - r;
                let lit_d = circle(u, v, 0.42, 0.4, 0.2);
                let p = lit(shadow(ds), d, lit_d, pal::NEEDLE, pal::NEEDLE_HI, aa);
                p.over(
                    pal::shade(pal::NEEDLE, -0.35),
                    inside(circle(u, v, 0.5, 0.5, 0.05), aa),
                )
            }
            // A bush: a low clump.
            3 => {
                const CLUMP: [(f32, f32, f32); 3] =
                    [(0.42, 0.52, 0.16), (0.6, 0.55, 0.14), (0.5, 0.4, 0.13)];
                let d = clump(u, v, &CLUMP);
                let ds = clump(u - 0.04, v - 0.05, &CLUMP);
                let lit_d = clump(u + 0.05, v + 0.05, &CLUMP) + 0.04;
                let base = pal::shade(pal::LEAF, 0.08);
                lit(shadow(ds), d, lit_d, base, pal::LEAF_HI, aa)
            }
            // A few loose stones.
            4 => {
                let mut p = clear;
                for (x, y, r) in [(0.42, 0.5, 0.12), (0.62, 0.62, 0.08), (0.3, 0.68, 0.06)] {
                    let ds = circle(u - 0.03, v - 0.035, x, y, r);
                    p = p.over(pal::SHADOW, inside(ds, aa * 3.0));
                    let d = circle(u, v * 1.15, x, y * 1.15, r);
                    let lit_d = circle(u, v * 1.15, x - r * 0.3, y * 1.15 - r * 0.35, r * 0.6);
                    p = lit(p, d, lit_d, pal::STONE, pal::STONE_HI, aa);
                }
                p
            }
            // A boulder: one big faceted stone.
            5 => {
                const BODY: [(f32, f32, f32); 3] =
                    [(0.5, 0.52, 0.26), (0.35, 0.6, 0.16), (0.64, 0.42, 0.17)];
                let d = clump(u, v, &BODY);
                let ds = clump(u - 0.06, v - 0.07, &BODY);
                let lit_d = (u - 0.3) + (v - 0.3) - 0.2;
                let p = lit(
                    shadow(ds),
                    d,
                    lit_d.max(d + 0.08),
                    pal::STONE,
                    pal::STONE_HI,
                    aa,
                );
                // A crack across it.
                let crack = segment(u, v, (0.42, 0.36), (0.58, 0.62)) - 0.012;
                p.over(
                    pal::shade(pal::STONE, -0.4),
                    inside(crack, aa) * inside(d + 0.05, aa),
                )
            }
            // Wild flowers in the grass.
            6 => {
                let mut p = clear;
                for (i, (x, y)) in [
                    (0.3, 0.32),
                    (0.62, 0.28),
                    (0.48, 0.55),
                    (0.25, 0.7),
                    (0.72, 0.66),
                    (0.55, 0.8),
                ]
                .into_iter()
                .enumerate()
                {
                    let stem = segment(u, v, (x, y), (x + 0.02, y + 0.08)) - 0.012;
                    p = p.over(pal::BLADE_GRASS, inside(stem, aa));
                    p = p.over(pal::PETALS[i % 3], inside(circle(u, v, x, y, 0.04), aa));
                }
                p
            }
            // A tuft of grass blades.
            _ => {
                let mut p = clear;
                for (x, y) in [(0.5, 0.62), (0.28, 0.4), (0.7, 0.36)] {
                    for (lean, h) in [(-0.07, 0.16), (0.0, 0.2), (0.07, 0.15), (0.12, 0.11)] {
                        let blade = segment(u, v, (x, y), (x + lean, y - h)) - 0.01;
                        p = p.over(pal::BLADE_GRASS, inside(blade, aa) * 0.9);
                    }
                }
                p
            }
        };
        px.rgba()
    }

    pub const ORE_VARIANTS: u32 = 4;

    /// The name of ore variant `k`.
    pub fn ore_name(k: u32) -> String {
        if k == 0 {
            ORE.to_owned()
        } else {
            format!("{ORE}:{k}")
        }
    }

    /// An ore tile: a faint wash of the ore's colour with a seam at its
    /// edge, under four faceted chunks lit from the top left. Painted white
    /// and grey; the tint gives the ore its colour.
    fn ore(u: f32, v: f32, variant: u32) -> [u8; 4] {
        // (centre x, centre y, radius, turn) of each chunk.
        const CHUNKS: [(f32, f32, f32, f32); 4] = [
            (0.3, 0.3, 0.17, 0.3),
            (0.7, 0.32, 0.13, 1.1),
            (0.56, 0.7, 0.18, 0.7),
            (0.22, 0.72, 0.11, 1.6),
        ];
        for (cx, cy, r, turn) in CHUNKS {
            // Variants mirror where the chunks sit, not how they are lit.
            let cx = if variant & 1 == 1 { 1.0 - cx } else { cx };
            let cy = if variant & 2 == 2 { 1.0 - cy } else { cy };
            let (dx, dy) = (u - cx, v - cy);
            let (s, c) = turn.sin_cos();
            let (x, y) = (dx * c - dy * s, dx * s + dy * c);
            // Between a diamond and a square: a cut stone.
            let d = x.abs().max(y.abs()) * 0.55 + (x.abs() + y.abs()) * 0.45;
            if d < r {
                // Facets: the top-left face catches the light, the bottom-right is in shade.
                let face = if dx + dy < -0.02 * r / 0.15 {
                    250.0
                } else if dx - dy > 0.0 {
                    175.0
                } else {
                    140.0
                };
                // A darker rim around each chunk.
                let shade = if d > r * 0.82 { face * 0.72 } else { face };
                let g = shade as u8;
                return [g, g, g, 255];
            }
            // A soft shadow down and to the right of each chunk.
            let (sx, sy) = (x - 0.025, y - 0.03);
            let ds = sx.abs().max(sy.abs()) * 0.55 + (sx.abs() + sy.abs()) * 0.45;
            if ds < r {
                return [40, 40, 40, 140];
            }
        }
        // The wash, inset so neighbouring ore tiles show a seam.
        let inset = (0.04..0.96).contains(&u) && (0.04..0.96).contains(&v);
        if inset {
            [255, 255, 255, 46]
        } else {
            [255, 255, 255, 0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands_only_scale_down_by_a_little() {
        assert_eq!(band_for(8.0), 0);
        assert_eq!(band_for(20.0), 0);
        assert_eq!(band_for(24.0), 1);
        assert_eq!(band_for(48.0), 2);
        assert_eq!(
            band_for(96.0),
            3,
            "a phone's default zoom gets the 128 band"
        );
        assert_eq!(band_for(300.0), 3);
    }

    #[test]
    fn sprites_pack_and_pad_without_overlap() {
        let mut atlas = Atlas::new(64);
        let a = atlas.add("a", 20, 20, |_, _| [255, 0, 0, 255]).unwrap();
        let b = atlas.add("b", 20, 20, |_, _| [0, 255, 0, 255]).unwrap();
        let c = atlas.add("c", 20, 20, |_, _| [0, 0, 255, 255]).unwrap();
        assert!(a[0] + a[2] < b[0], "same shelf, padded apart");
        assert!(c[1] > a[1] + a[3], "third sprite starts a new shelf");
        assert!(atlas.add("too big", 80, 4, |_, _| [0; 4]).is_none());
        // The padding repeats the sprite's edge.
        let at = |x: u32, y: u32| atlas.pixels[((y * 64 + x) * 4) as usize];
        assert_eq!(at(0, 0), 255, "top-left padding is the red sprite's corner");
    }
}
