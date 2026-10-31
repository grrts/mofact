//! A small 2D canvas on tiny-skia with the calls the prototype's drawing
//! code makes on an HTML canvas: a transform stack, a clip, global alpha,
//! filled and stroked paths, rounded rectangles, arcs and polygons.

use crate::palette::Rgba;
use std::f32::consts::{FRAC_PI_2, TAU};
use tiny_skia::{
    FillRule, Mask, Paint, Path, PathBuilder, Pixmap, Rect, Shader, Stroke, Transform,
};

#[derive(Clone)]
struct State {
    transform: Transform,
    clip: Option<Mask>,
    alpha: f32,
}

/// Something to draw into: a pixmap and the drawing state.
pub struct Canvas {
    pub pixmap: Pixmap,
    state: State,
    saved: Vec<State>,
}

impl Canvas {
    /// A transparent canvas `width` × `height` pixels.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixmap: Pixmap::new(width.max(1), height.max(1)).expect("non-zero size"),
            state: State {
                transform: Transform::identity(),
                clip: None,
                alpha: 1.0,
            },
            saved: Vec::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// The pixels as straight (not premultiplied) RGBA bytes.
    pub fn rgba(&self) -> Vec<u8> {
        self.pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect()
    }

    pub fn save(&mut self) {
        self.saved.push(self.state.clone());
    }

    pub fn restore(&mut self) {
        if let Some(s) = self.saved.pop() {
            self.state = s;
        }
    }

    pub fn translate(&mut self, x: f32, y: f32) {
        self.state.transform = self.state.transform.pre_translate(x, y);
    }

    /// Turn clockwise by `radians`, as the canvas's `rotate`.
    pub fn rotate(&mut self, radians: f32) {
        self.state.transform = self.state.transform.pre_rotate(radians.to_degrees());
    }

    /// Multiply the opacity of everything drawn from now on by `a`.
    pub fn set_alpha(&mut self, a: f32) {
        self.state.alpha = a.clamp(0.0, 1.0);
    }

    pub fn alpha(&self) -> f32 {
        self.state.alpha
    }

    /// Only draw inside `path` (intersected with any clip already set).
    pub fn clip(&mut self, path: &Path) {
        let ts = self.state.transform;
        match &mut self.state.clip {
            Some(mask) => mask.intersect_path(path, FillRule::Winding, true, ts),
            None => {
                let mut mask =
                    Mask::new(self.pixmap.width(), self.pixmap.height()).expect("non-zero size");
                mask.fill_path(path, FillRule::Winding, true, ts);
                self.state.clip = Some(mask);
            }
        }
    }

    fn paint(&self, c: Rgba) -> Paint<'static> {
        let a = (c[3] as f32 * self.state.alpha).round().clamp(0.0, 255.0) as u8;
        Paint {
            shader: Shader::SolidColor(tiny_skia::Color::from_rgba8(c[0], c[1], c[2], a)),
            anti_alias: true,
            ..Paint::default()
        }
    }

    pub fn fill(&mut self, path: &Path, c: Rgba) {
        let paint = self.paint(c);
        self.pixmap.fill_path(
            path,
            &paint,
            FillRule::Winding,
            self.state.transform,
            self.state.clip.as_ref(),
        );
    }

    pub fn stroke(&mut self, path: &Path, c: Rgba, width: f32) {
        let paint = self.paint(c);
        let stroke = Stroke {
            width,
            ..Stroke::default()
        };
        self.pixmap.stroke_path(
            path,
            &paint,
            &stroke,
            self.state.transform,
            self.state.clip.as_ref(),
        );
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        if let Some(p) = rect(x, y, w, h) {
            self.fill(&p, c);
        }
    }

    pub fn fill_circle(&mut self, x: f32, y: f32, r: f32, c: Rgba) {
        if let Some(p) = PathBuilder::from_circle(x, y, r) {
            self.fill(&p, c);
        }
    }

    pub fn stroke_circle(&mut self, x: f32, y: f32, r: f32, c: Rgba, width: f32) {
        if let Some(p) = PathBuilder::from_circle(x, y, r) {
            self.stroke(&p, c, width);
        }
    }

    /// Stroke the arc of radius `r` around `(x, y)` from angle `a0` to `a1`
    /// (radians, clockwise from east, as on the canvas).
    pub fn stroke_arc(
        &mut self,
        (x, y): (f32, f32),
        r: f32,
        (a0, a1): (f32, f32),
        c: Rgba,
        width: f32,
    ) {
        let mut pb = PathBuilder::new();
        arc(&mut pb, x, y, r, a0, a1, true);
        if let Some(p) = pb.finish() {
            self.stroke(&p, c, width);
        }
    }

    /// A straight line, stroked.
    pub fn line(&mut self, points: &[(f32, f32)], c: Rgba, width: f32) {
        let mut pb = PathBuilder::new();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if let Some(p) = pb.finish() {
            self.stroke(&p, c, width);
        }
    }
}

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect)
}

/// Add an arc to `pb` as cubic Béziers, starting with a line from the current
/// point (or a move, if `start` is set), as the canvas's `arc` does.
pub fn arc(pb: &mut PathBuilder, x: f32, y: f32, r: f32, a0: f32, a1: f32, start: bool) {
    let sweep = (a1 - a0).clamp(-TAU, TAU);
    let n = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |a: f32| (x + r * a.cos(), y + r * a.sin());
    let (sx, sy) = at(a0);
    if start {
        pb.move_to(sx, sy);
    } else {
        pb.line_to(sx, sy);
    }
    for i in 0..n {
        let (t0, t1) = (a0 + step * i as f32, a0 + step * (i + 1) as f32);
        let (p0, p3) = (at(t0), at(t1));
        let c1 = (p0.0 - k * r * t0.sin(), p0.1 + k * r * t0.cos());
        let c2 = (p3.0 + k * r * t1.sin(), p3.1 - k * r * t1.cos());
        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, p3.0, p3.1);
    }
}

/// A rounded rectangle with corners of radius `r`.
pub fn rr(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    arc(&mut pb, x + w - r, y + r, r, -FRAC_PI_2, 0.0, false);
    arc(&mut pb, x + w - r, y + h - r, r, 0.0, FRAC_PI_2, false);
    arc(
        &mut pb,
        x + r,
        y + h - r,
        r,
        FRAC_PI_2,
        2.0 * FRAC_PI_2,
        false,
    );
    arc(
        &mut pb,
        x + r,
        y + r,
        r,
        2.0 * FRAC_PI_2,
        3.0 * FRAC_PI_2,
        false,
    );
    pb.close();
    pb.finish()
}

/// A closed polygon through `points`, scaled by `s` around (`x`, `y`).
pub fn poly(points: &[(f32, f32)], x: f32, y: f32, s: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, &(a, b)) in points.iter().enumerate() {
        if i == 0 {
            pb.move_to(x + a * s, y + b * s);
        } else {
            pb.line_to(x + a * s, y + b * s);
        }
    }
    pb.close();
    pb.finish()
}

/// An axis-aligned ellipse.
pub fn ellipse(x: f32, y: f32, rx: f32, ry: f32) -> Option<Path> {
    Rect::from_xywh(x - rx, y - ry, 2.0 * rx, 2.0 * ry).map(PathBuilder::from_oval)?
}
