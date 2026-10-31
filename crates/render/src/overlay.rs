//! The screen-space layer on top of the world: panels, icons and text for
//! the UI lane (T21). Everything is in physical pixels, y down.
//!
//! Text uses the prototype's UI face, Saira Semi Condensed (SIL OFL, in
//! `assets/fonts`). Glyphs are rasterised the first time each size is used
//! and packed into the overlay atlas alongside the panel and icon sprites.

use crate::atlas::{Atlas, Uv};
use crate::look::Look;
use crate::palette::Rgba;
use crate::scene::Instance;

const FONT: &[u8] = include_bytes!("../../../assets/fonts/SairaSemiCondensed-SemiBold.ttf");
/// Icons in the overlay are painted at this many pixels a tile.
const ICON_PX: u32 = 64;
/// The rounded panel sprite: corner radius and size, in pixels.
const PANEL_R: u32 = 12;

/// Panels, icons and text for one frame, plus the atlas they draw from.
pub struct Overlay {
    pub atlas: Atlas,
    pub instances: Vec<Instance>,
    font: fontdue::Font,
}

impl Overlay {
    /// An overlay that can show `look`'s buildings and items as icons.
    pub fn new(look: &Look) -> Self {
        let mut atlas = Atlas::new(1024);
        look.paint(&mut atlas, ICON_PX);
        let n = PANEL_R * 2 + 1;
        let r = PANEL_R as f32;
        atlas.add("panel", n, n, |x, y| {
            // A white disc of radius PANEL_R, antialiased: the 9-slice source.
            let d = (x as f32 + 0.5 - r - 0.5).hypot(y as f32 + 0.5 - r - 0.5) - r + 0.5;
            [255, 255, 255, ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u8]
        });
        let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default())
            .expect("the bundled font parses");
        Self {
            atlas,
            instances: Vec::new(),
            font,
        }
    }

    /// Start a new frame.
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    fn quad(&mut self, pos: [f32; 2], size: [f32; 2], uv: Uv, tint: Rgba, turns: u32) {
        self.instances.push(Instance {
            pos,
            size,
            uv,
            tint,
            turns,
        });
    }

    /// A filled rectangle with corners of `radius` pixels (up to 12).
    pub fn panel(&mut self, pos: [f32; 2], size: [f32; 2], radius: f32, color: Rgba) {
        let Some(uv) = self.atlas.get("panel") else {
            return;
        };
        let r = radius.min(size[0] / 2.0).min(size[1] / 2.0).max(0.0);
        // The source is 25 texels: 12 corner, 1 middle, 12 corner.
        let t = uv[2] / (PANEL_R * 2 + 1) as f32;
        let c = PANEL_R as f32 * t;
        let xs = [pos[0], pos[0] + r, pos[0] + size[0] - r];
        let ys = [pos[1], pos[1] + r, pos[1] + size[1] - r];
        let ws = [r, size[0] - 2.0 * r, r];
        let hs = [r, size[1] - 2.0 * r, r];
        let us = [uv[0], uv[0] + c, uv[0] + c + t];
        let uw = [c, t, c];
        for j in 0..3 {
            for i in 0..3 {
                if ws[i] <= 0.0 || hs[j] <= 0.0 {
                    continue;
                }
                let src = [us[i], uv[1] + (us[j] - uv[0]), uw[i], uw[j]];
                self.quad([xs[i], ys[j]], [ws[i], hs[j]], src, color, 0);
            }
        }
    }

    /// A world sprite (a building or an item shape) as an icon of `size` pixels.
    pub fn icon(&mut self, sprite: &str, pos: [f32; 2], size: f32, tint: Rgba) {
        self.icon_turned(sprite, pos, size, tint, 0);
    }

    /// An icon turned `turns` quarter turns clockwise, as a building facing that way.
    pub fn icon_turned(&mut self, sprite: &str, pos: [f32; 2], size: f32, tint: Rgba, turns: u32) {
        if let Some(uv) = self.atlas.get(sprite) {
            self.quad(pos, [size, size], uv, tint, turns % 4);
        }
    }

    fn glyph(&mut self, c: char, px: u32) -> Option<(fontdue::Metrics, Option<Uv>)> {
        let name = format!("glyph:{px}:{c}");
        let metrics = self.font.metrics(c, px as f32);
        if metrics.width == 0 || metrics.height == 0 {
            return Some((metrics, None));
        }
        if let Some(uv) = self.atlas.get(&name) {
            return Some((metrics, Some(uv)));
        }
        let (m, coverage) = self.font.rasterize(c, px as f32);
        let w = m.width as u32;
        let uv = self.atlas.add(&name, w, m.height as u32, |x, y| {
            [255, 255, 255, coverage[(y * w + x) as usize]]
        })?;
        Some((m, Some(uv)))
    }

    /// How wide `text` is at `px` pixels.
    pub fn measure(&self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| self.font.metrics(c, px.round()).advance_width)
            .sum()
    }

    /// Draw `text` with its baseline at `pos[1]`, starting at `pos[0]`. Returns its width.
    pub fn text(&mut self, text: &str, pos: [f32; 2], px: f32, color: Rgba) -> f32 {
        let px = px.round().max(1.0) as u32;
        let mut x = pos[0].round();
        let y = pos[1].round();
        for c in text.chars() {
            let Some((m, uv)) = self.glyph(c, px) else {
                continue;
            };
            if let Some(uv) = uv {
                let top = y - m.height as f32 - m.ymin as f32;
                self.quad(
                    [(x + m.xmin as f32).round(), top],
                    [m.width as f32, m.height as f32],
                    uv,
                    color,
                    0,
                );
            }
            x += m.advance_width;
        }
        x - pos[0].round()
    }

    /// The distance from the baseline up to the top of capitals, at `px` pixels.
    pub fn cap_height(&self, px: f32) -> f32 {
        let m = self.font.metrics('H', px.round());
        m.height as f32 + m.ymin as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_laid_out_left_to_right_on_its_baseline() {
        let mut o = Overlay::new(&Look::default());
        let w = o.text("Belt 12", [10.0, 40.0], 16.0, [255; 4]);
        assert!((w - o.measure("Belt 12", 16.0)).abs() < 1.0);
        assert!(w > 30.0 && w < 70.0, "{w}");
        assert_eq!(
            o.instances.len(),
            6,
            "a quad per visible glyph, none for the space"
        );
        let xs: Vec<f32> = o.instances.iter().map(|i| i.pos[0]).collect();
        assert!(xs.windows(2).all(|p| p[0] < p[1]), "{xs:?}");
        let bottom_of_b = o.instances[0].pos[1] + o.instances[0].size[1];
        assert!((bottom_of_b - 40.0).abs() <= 1.0, "B sits on the baseline");
    }

    #[test]
    fn glyphs_are_rasterised_once_per_size() {
        let mut o = Overlay::new(&Look::default());
        o.text("aaa", [0.0, 20.0], 14.0, [255; 4]);
        let v = o.atlas.version;
        o.text("aa", [0.0, 40.0], 14.0, [255; 4]);
        assert_eq!(o.atlas.version, v, "cached");
        o.text("a", [0.0, 60.0], 20.0, [255; 4]);
        assert_eq!(o.atlas.version, v + 1, "a new size is a new glyph");
    }

    #[test]
    fn panels_are_nine_slices_that_fill_the_rectangle() {
        let mut o = Overlay::new(&Look::default());
        o.panel([10.0, 20.0], [100.0, 50.0], 8.0, [255; 4]);
        assert_eq!(o.instances.len(), 9);
        let area: f32 = o.instances.iter().map(|i| i.size[0] * i.size[1]).sum();
        assert!((area - 5000.0).abs() < 1e-3);
    }
}
