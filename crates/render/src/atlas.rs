//! Texture atlases, one per zoom band (R66).
//!
//! Every sprite is painted at the band's pixel size, so a building is never
//! drawn more than about 1.5 times bigger or smaller than it was painted. The
//! sprites here are placeholders in the prototype's palette until the art kit
//! (T17, T18) supplies the real ones through the same [`Atlas::add`] call.

use std::collections::HashMap;

/// Pixels per tile for each zoom band, smallest first.
pub const BANDS: [u32; 3] = [16, 32, 64];

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

/// The placeholder sprites. Items are painted white and tinted per item when drawn.
pub mod sprites {
    use crate::palette::{self, BELT_RAIL, CHEVRON, MACHINE_EDGE, MACHINE_HI};

    /// Lets the ground show through.
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    pub const BELT: &str = "belt";
    pub const MACHINE: &str = "machine";
    pub const HUB: &str = "hub";
    /// Ore lying on a tile, painted white and tinted per ore.
    pub const ORE: &str = "ore-tile";
    /// Item shapes, in [`mofact_core::content::IconShape`] order.
    pub const SHAPES: [&str; 6] = ["ore", "plate", "gear", "wire", "circuit", "droplet"];

    /// Paint every placeholder sprite at `px` pixels per tile into `atlas`.
    pub fn paint_all(atlas: &mut super::Atlas, px: u32) {
        let f = px as f32;
        atlas.add(BELT, px, px, |x, y| belt(x as f32 / f, y as f32 / f));
        atlas.add(MACHINE, px, px, |x, y| {
            machine(x as f32 / f, y as f32 / f, 1.0)
        });
        atlas.add(HUB, px * 3, px * 3, |x, y| {
            machine(x as f32 / f, y as f32 / f, 3.0)
        });
        atlas.add(ORE, px, px, |x, y| ore(x as f32 / f, y as f32 / f));
        let item = (px / 2).max(4);
        let fi = item as f32;
        for (i, name) in SHAPES.iter().enumerate() {
            atlas.add(name, item, item, |x, y| {
                shape(i, (x as f32 + 0.5) / fi, (y as f32 + 0.5) / fi)
            });
        }
    }

    /// A belt running east; the shader rotates it for other directions.
    fn belt(u: f32, v: f32) -> [u8; 4] {
        if !(0.08..0.92).contains(&v) {
            return CLEAR;
        }
        if !(0.16..0.84).contains(&v) {
            return BELT_RAIL;
        }
        // A chevron pointing the way the belt runs.
        let d = (u - 0.65 + (v - 0.5).abs() * 0.6).abs();
        if d < 0.07 {
            CHEVRON
        } else {
            palette::BELT
        }
    }

    /// Five pebbles scattered over the tile, the same on every tile.
    fn ore(u: f32, v: f32) -> [u8; 4] {
        const PEBBLES: [(f32, f32, f32); 5] = [
            (0.25, 0.28, 0.14),
            (0.68, 0.22, 0.11),
            (0.52, 0.55, 0.16),
            (0.2, 0.74, 0.1),
            (0.78, 0.76, 0.12),
        ];
        let lit = PEBBLES
            .iter()
            .find(|(x, y, r)| (u - x).hypot(v - y) < *r)
            .map(|(x, y, r)| ((x - u) + (y - v)) / r);
        match lit {
            // Lighter on the top-left of each pebble.
            Some(l) => {
                let shade = (200.0 + 55.0 * l.clamp(-1.0, 1.0)) as u8;
                [shade, shade, shade, 235]
            }
            None => [255, 255, 255, 0],
        }
    }

    /// A machine body `tiles` tiles across: rounded, bevelled, with a dark edge.
    fn machine(u: f32, v: f32, tiles: f32) -> [u8; 4] {
        let inset = 0.08 / tiles;
        let (du, dv) = (
            (u / tiles - 0.5).abs() - (0.5 - inset),
            (v / tiles - 0.5).abs() - (0.5 - inset),
        );
        let r = 0.12 / tiles;
        let outside = (du + r).max(0.0).hypot((dv + r).max(0.0)) - r;
        if outside > 0.0 {
            CLEAR
        } else if outside > -0.04 / tiles {
            MACHINE_EDGE
        } else if v / tiles < 0.3 {
            MACHINE_HI
        } else {
            palette::MACHINE
        }
    }

    /// Item shape `i` as white with alpha.
    fn shape(i: usize, u: f32, v: f32) -> [u8; 4] {
        let (x, y) = (u - 0.5, v - 0.5);
        let r = x.hypot(y);
        let inside = match i {
            0 => r < 0.42 - 0.05 * (y * 9.0).sin(), // ore: a lumpy rock
            1 => x.abs() < 0.38 && y.abs() < 0.3,   // plate
            2 => r < 0.24 || (r < 0.44 && (y.atan2(x) * 4.0).cos() > -0.2), // gear
            3 => (y - 0.18 * (x * 9.0).sin()).abs() < 0.09 && x.abs() < 0.45, // wire
            4 => x.abs() < 0.4 && y.abs() < 0.4 && !(x.abs() < 0.14 && y.abs() < 0.14), // circuit
            _ => r < 0.32 || (y < 0.0 && x.abs() < 0.3 * (y + 0.5) * 1.2), // droplet
        };
        if inside {
            [255, 255, 255, 255]
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
        assert_eq!(band_for(300.0), 2);
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

    #[test]
    fn every_band_fits_its_sprites() {
        for px in BANDS {
            let mut atlas = Atlas::new(px * 8);
            sprites::paint_all(&mut atlas, px);
            for name in [sprites::BELT, sprites::MACHINE, sprites::HUB, sprites::ORE]
                .into_iter()
                .chain(sprites::SHAPES)
            {
                assert!(atlas.get(name).is_some(), "{name} at {px}px");
            }
        }
    }
}
