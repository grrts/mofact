//! The screen-space layer on top of the world: panels, icons and text for
//! the UI lane (T21). Everything is in physical pixels, y down.
//!
//! Text uses the prototype's UI face, Saira Semi Condensed (SIL OFL, in
//! `assets/fonts`). Japanese and Chinese fall back to Noto Sans JP and SC,
//! cut down to the characters the game's strings use (T65,
//! `tools/fonts/subset.py`). Glyphs are rasterised the first time each size
//! is used and packed into the overlay atlas alongside the panel and icon
//! sprites.

use crate::atlas::{Atlas, Uv};
use crate::look::Look;
use crate::palette::Rgba;
use crate::scene::Instance;

const FONT: &[u8] = include_bytes!("../../../assets/fonts/SairaSemiCondensed-SemiBold.ttf");
/// The Han script and kana faces: Japanese first, then Simplified Chinese.
const JAPANESE: &[u8] = include_bytes!("../../../assets/fonts/NotoSansJP-SemiBold-subset.ttf");
const CHINESE: &[u8] = include_bytes!("../../../assets/fonts/NotoSansSC-SemiBold-subset.ttf");
/// Icons in the overlay are painted at this many pixels a tile.
const ICON_PX: u32 = 64;
/// The rounded panel sprite: corner radius and size, in pixels.
const PANEL_R: u32 = 12;

/// Panels, icons and text for one frame, plus the atlas they draw from.
pub struct Overlay {
    pub atlas: Atlas,
    pub instances: Vec<Instance>,
    /// Where each run of text went this frame (left, top, width, height),
    /// and how many glyphs found no room in the atlas, all told: for tests
    /// that check words fit their controls in every language.
    pub runs: Vec<([f32; 4], usize)>,
    /// Each panel this frame and the instance it starts at, so a test can
    /// tell which words lie under a later panel.
    pub panels: Vec<([f32; 4], usize)>,
    pub dropped: usize,
    /// Where glyphs start in the atlas, after the panel and icons: when
    /// glyphs fill it, they are cleared from here and rasterised again as
    /// text needs them. Japanese and Chinese have hundreds of characters.
    glyphs_from: (u32, u32, u32),
    /// Saira, then the Japanese and the Chinese face.
    fonts: [fontdue::Font; 3],
    /// Whether a character both Han faces have takes the Chinese shape:
    /// the same character is drawn differently in Japan and in China.
    chinese: bool,
}

/// Which of the overlay's faces draws a character.
const LATIN: usize = 0;

impl Overlay {
    /// An overlay that can show `look`'s buildings and items as icons.
    pub fn new(look: &Look) -> Self {
        // 2,048 is the largest texture every WebGL2 device takes; icons fill
        // about a third of it and glyphs come in as text needs them.
        let mut atlas = Atlas::new(2048);
        let n = PANEL_R * 2 + 1;
        let r = PANEL_R as f32;
        atlas.add("panel", n, n, |x, y| {
            // A white disc of radius PANEL_R, antialiased: the 9-slice source.
            let d = (x as f32 + 0.5 - r - 0.5).hypot(y as f32 + 0.5 - r - 0.5) - r + 0.5;
            [255, 255, 255, ((0.5 - d).clamp(0.0, 1.0) * 255.0) as u8]
        });
        // Panels first: without them there is no HUD, so they must never be
        // what's left out when the icons run out of room.
        let painted = look.paint(&mut atlas, ICON_PX);
        debug_assert!(painted, "the overlay atlas has room for every icon");
        let face = |bytes| {
            fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
                .expect("the bundled font parses")
        };
        Self {
            glyphs_from: atlas.mark(),
            atlas,
            instances: Vec::new(),
            runs: Vec::new(),
            panels: Vec::new(),
            dropped: 0,
            fonts: [face(FONT), face(JAPANESE), face(CHINESE)],
            chinese: false,
        }
    }

    /// Draw Han characters the Chinese way (true) or the Japanese way.
    pub fn prefer_chinese(&mut self, chinese: bool) {
        self.chinese = chinese;
    }

    /// The face that draws `c`: Saira when it has it, then the Han face of
    /// the language shown, then the other one.
    fn face(&self, c: char) -> usize {
        let order = if self.chinese { [0, 2, 1] } else { [0, 1, 2] };
        order
            .into_iter()
            .find(|&f| self.fonts[f].lookup_glyph_index(c) != 0)
            .unwrap_or(LATIN)
    }

    /// Whether any face has a glyph for `c`.
    pub fn has_glyph(&self, c: char) -> bool {
        c.is_whitespace() || self.fonts.iter().any(|f| f.lookup_glyph_index(c) != 0)
    }

    /// Start a new frame.
    pub fn clear(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.panels.clear();
        // Between frames no quad points at a glyph, so they can go: room
        // for the next frame's, which come back as they are drawn.
        if self.atlas.filled() > 0.75 {
            self.atlas.rewind(self.glyphs_from, "glyph:");
        }
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
        self.panels
            .push(([pos[0], pos[1], size[0], size[1]], self.instances.len()));
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
        let f = self.face(c);
        let name = format!("glyph:{f}:{px}:{c}");
        let metrics = self.fonts[f].metrics(c, px as f32);
        if metrics.width == 0 || metrics.height == 0 {
            return Some((metrics, None));
        }
        if let Some(uv) = self.atlas.get(&name) {
            return Some((metrics, Some(uv)));
        }
        let (m, coverage) = self.fonts[f].rasterize(c, px as f32);
        let w = m.width as u32;
        let Some(uv) = self.atlas.add(&name, w, m.height as u32, |x, y| {
            [255, 255, 255, coverage[(y * w + x) as usize]]
        }) else {
            self.dropped += 1;
            return None;
        };
        Some((m, Some(uv)))
    }

    /// How wide `text` is at `px` pixels.
    pub fn measure(&self, text: &str, px: f32) -> f32 {
        text.chars()
            .map(|c| {
                self.fonts[self.face(c)]
                    .metrics(c, px.round())
                    .advance_width
            })
            .sum()
    }

    /// Draw `text` with its baseline at `pos[1]`, starting at `pos[0]`. Returns its width.
    pub fn text(&mut self, text: &str, pos: [f32; 2], px: f32, color: Rgba) -> f32 {
        let first = self.instances.len();
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
        let w = x - pos[0].round();
        if w > 0.0 {
            let top = y - self.cap_height(px as f32);
            self.runs.push(([pos[0].round(), top, w, px as f32], first));
        }
        w
    }

    /// The distance from the baseline up to the top of capitals, at `px` pixels.
    pub fn cap_height(&self, px: f32) -> f32 {
        let m = self.fonts[LATIN].metrics('H', px.round());
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
    fn han_text_draws_and_a_full_atlas_makes_room_between_frames() {
        let mut o = Overlay::new(&Look::default());
        assert!(o.has_glyph('鉄') && o.has_glyph('铁') && o.has_glyph('é'));
        o.text("鉄板", [0.0, 40.0], 16.0, [255; 4]);
        assert_eq!(o.instances.len(), 2, "both drawn by the Japanese face");
        let mut frames = 0;
        while o.atlas.filled() <= 0.75 {
            o.clear();
            o.text(
                &format!("鉄板{frames}"),
                [0.0, 40.0],
                20.0 + frames as f32,
                [255; 4],
            );
            frames += 1;
        }
        o.clear();
        assert!(o.atlas.filled() < 0.75, "glyphs cleared");
        assert!(o.atlas.get("panel").is_some(), "but not the panel");
        o.text("鉄", [0.0, 40.0], 16.0, [255; 4]);
        assert_eq!((o.instances.len(), o.dropped), (1, 0));
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
    fn every_icon_and_the_panel_fit() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        let content = mofact_core::content::Content {
            items: ron::from_str(&read("items.ron")).unwrap(),
            recipes: ron::from_str(&read("recipes.ron")).unwrap(),
            buildings: ron::from_str(&read("buildings.ron")).unwrap(),
            research: Vec::new(),
            goals: Vec::new(),
        };
        let look = Look::new(&content);
        let overlay = Overlay::new(&look);
        assert!(overlay.atlas.get("panel").is_some());
        let mut atlas = Atlas::new(2048);
        assert!(look.paint(&mut atlas, ICON_PX), "icons fit");
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
