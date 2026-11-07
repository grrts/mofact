//! The fog over land the factory hasn't charted yet (S3).
//!
//! Fog is only a look: unexplored ground is dark, and the world map hides
//! its ore patches. Building there isn't blocked.

use mofact_core::geom::{FogCell, TilePos};
use mofact_core::FOG_CELL;
use mofact_render::{Camera, Overlay};

/// The fog's colour: the night sky, nearly opaque.
const FOG: [u8; 4] = [0x0d, 0x12, 0x1c, 0xe4];
/// The soft edge, from the fog outwards.
const EDGE: [u8; 3] = [0x90, 0x50, 0x20];

/// Whether `t` is on charted ground.
pub fn charted(explored: &[FogCell], t: TilePos) -> bool {
    explored.binary_search(&t.fog()).is_ok()
}

/// Darken the uncharted cells in view.
pub fn draw(o: &mut Overlay, camera: &Camera, size: (u32, u32), explored: &[FogCell]) {
    let ppt = camera.pixels_per_tile;
    let cell = FOG_CELL as f32 * ppt;
    let (lo, hi) = camera.visible(size.0, size.1);
    let to_cell = |v: f32| (v.floor() as i32).div_euclid(FOG_CELL);
    let (x0, y0) = (to_cell(lo[0]), to_cell(lo[1]));
    let (x1, y1) = (to_cell(hi[0]), to_cell(hi[1]));
    let px = |x: i32, y: i32| {
        [
            (x as f32 * FOG_CELL as f32 - camera.center[0]) * ppt + size.0 as f32 / 2.0,
            (y as f32 * FOG_CELL as f32 - camera.center[1]) * ppt + size.1 as f32 / 2.0,
        ]
    };
    let dark = |x: i32, y: i32| explored.binary_search(&FogCell::new(x, y)).is_err();
    // The edge fades over a few tiles into charted land, never past a
    // third of a cell, and stays a few pixels wide on the world map.
    let band = (ppt * 1.5).max(1.5).min(cell / 9.0);
    for y in y0..=y1 {
        // Runs of fog along the row, as one panel each.
        let mut x = x0;
        while x <= x1 {
            if !dark(x, y) {
                x += 1;
                continue;
            }
            let start = x;
            while x <= x1 && dark(x, y) {
                x += 1;
            }
            let at = px(start, y);
            o.panel(at, [(x - start) as f32 * cell, cell], 0.0, FOG);
        }
        // Soft edges on charted cells that touch the fog.
        for x in x0..=x1 {
            if dark(x, y) {
                continue;
            }
            let at = px(x, y);
            for (i, a) in EDGE.iter().enumerate() {
                let tint = [FOG[0], FOG[1], FOG[2], *a];
                let d = band * i as f32;
                if dark(x - 1, y) {
                    o.panel([at[0] + d, at[1]], [band, cell], 0.0, tint);
                }
                if dark(x + 1, y) {
                    o.panel([at[0] + cell - d - band, at[1]], [band, cell], 0.0, tint);
                }
                if dark(x, y - 1) {
                    o.panel([at[0], at[1] + d], [cell, band], 0.0, tint);
                }
                if dark(x, y + 1) {
                    o.panel([at[0], at[1] + cell - d - band], [cell, band], 0.0, tint);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tile_is_charted_by_its_cell() {
        let explored = vec![FogCell::new(-1, 0), FogCell::new(0, 0)];
        assert!(charted(&explored, TilePos::new(0, 0)));
        assert!(charted(&explored, TilePos::new(31, 31)));
        assert!(charted(&explored, TilePos::new(-1, 5)));
        assert!(!charted(&explored, TilePos::new(32, 0)));
        assert!(!charted(&explored, TilePos::new(0, -1)));
    }
}
