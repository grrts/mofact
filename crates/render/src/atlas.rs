//! Texture atlases, one per zoom band (R66).
//!
//! Every sprite is painted at the band's pixel size, so a building is never
//! drawn more than about 1.5 times bigger or smaller than it was painted. The
//! sprites come from the art kit, through [`crate::Look::paint`].

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

/// Sprites painted here rather than by the art kit.
pub mod sprites {
    pub use crate::look::ORE;

    /// Paint the ore pebbles at `px` pixels a tile. Returns false if the atlas is full.
    pub fn paint_ore(atlas: &mut super::Atlas, px: u32) -> bool {
        let f = px as f32;
        atlas
            .add(ORE, px, px, |x, y| ore(x as f32 / f, y as f32 / f))
            .is_some()
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
}
