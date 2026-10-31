//! Input lane: touch, mouse and keyboard mapped to camera gestures and build commands.
//!
//! The platform shell turns its events into [`Pointer`] events in screen
//! pixels, and [`CameraControl`] turns those into a camera: one finger or a
//! mouse drag pans, two fingers pinch-zoom about their midpoint, and the wheel
//! zooms about the cursor (T15). Build tools arrive in T20.
//!
//! Pointer events are plain data, so a session can be recorded and replayed:
//! the replay tests below drive the same code the app does.

pub use mofact_core as core;

use serde::{Deserialize, Serialize};

/// The pointer id the mouse uses; touches use their finger ids.
pub const MOUSE: u64 = u64::MAX;

/// One input event, in screen pixels with y down.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pointer {
    Down {
        id: u64,
        x: f32,
        y: f32,
    },
    Move {
        id: u64,
        x: f32,
        y: f32,
    },
    Up {
        id: u64,
    },
    /// Wheel notches (positive zooms in) with the cursor at `x`, `y`.
    Wheel {
        steps: f32,
        x: f32,
        y: f32,
    },
}

/// Closest and furthest zoom, in screen pixels per tile.
pub const ZOOM: (f32, f32) = (8.0, 128.0);
/// How much one wheel notch zooms.
const WHEEL_STEP: f32 = 1.15;

/// The camera, driven by pointer events.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraControl {
    /// The world point, in tiles, at the middle of the screen.
    pub center: [f32; 2],
    pub pixels_per_tile: f32,
    /// The screen size in pixels.
    viewport: [f32; 2],
    /// The camera centre stays inside this box of tiles: (min, max).
    bounds: ([f32; 2], [f32; 2]),
    /// Pointers that are down: id and last position. At most two count.
    down: Vec<(u64, [f32; 2])>,
}

impl CameraControl {
    pub fn new(center: [f32; 2], pixels_per_tile: f32, viewport: [f32; 2]) -> Self {
        let mut c = Self {
            center,
            pixels_per_tile,
            viewport,
            bounds: ([-5_000.0; 2], [5_000.0; 2]),
            down: Vec::new(),
        };
        c.clamp();
        c
    }

    /// Follow a window resize.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = [width.max(1.0), height.max(1.0)];
    }

    /// Keep the camera centre inside `min`..`max` tiles.
    pub fn set_bounds(&mut self, min: [f32; 2], max: [f32; 2]) {
        self.bounds = (min, max);
        self.clamp();
    }

    /// The world point, in tiles, under screen pixel `p`.
    pub fn world_at(&self, p: [f32; 2]) -> [f32; 2] {
        [
            self.center[0] + (p[0] - self.viewport[0] / 2.0) / self.pixels_per_tile,
            self.center[1] + (p[1] - self.viewport[1] / 2.0) / self.pixels_per_tile,
        ]
    }

    /// Whether a pointer is held: the camera is being moved.
    pub fn busy(&self) -> bool {
        !self.down.is_empty()
    }

    pub fn handle(&mut self, event: Pointer) {
        match event {
            Pointer::Down { id, x, y } => {
                self.down.retain(|(d, _)| *d != id);
                if self.down.len() < 2 {
                    self.down.push((id, [x, y]));
                }
            }
            Pointer::Up { id } => self.down.retain(|(d, _)| *d != id),
            Pointer::Move { id, x, y } => {
                let Some(i) = self.down.iter().position(|(d, _)| *d == id) else {
                    return; // a mouse moving with no button held
                };
                let before = self.down.clone();
                self.down[i].1 = [x, y];
                match (before.as_slice(), self.down.as_slice()) {
                    ([(_, a)], [(_, b)]) => self.pan(*a, *b),
                    ([(_, a0), (_, a1)], [(_, b0), (_, b1)]) => {
                        let (ma, mb) = (mid(*a0, *a1), mid(*b0, *b1));
                        let (da, db) = (dist(*a0, *a1), dist(*b0, *b1));
                        if da > 1.0 {
                            self.zoom_at(db / da, ma);
                        }
                        self.pan(ma, mb);
                    }
                    _ => {}
                }
            }
            Pointer::Wheel { steps, x, y } => self.zoom_at(WHEEL_STEP.powf(steps), [x, y]),
        }
    }

    /// Move the world so the point under screen pixel `from` ends up under `to`.
    fn pan(&mut self, from: [f32; 2], to: [f32; 2]) {
        self.center[0] -= (to[0] - from[0]) / self.pixels_per_tile;
        self.center[1] -= (to[1] - from[1]) / self.pixels_per_tile;
        self.clamp();
    }

    /// Zoom by `factor`, keeping the world point under screen pixel `at` still.
    fn zoom_at(&mut self, factor: f32, at: [f32; 2]) {
        let anchor = self.world_at(at);
        self.pixels_per_tile = (self.pixels_per_tile * factor).clamp(ZOOM.0, ZOOM.1);
        let moved = self.world_at(at);
        self.center[0] += anchor[0] - moved[0];
        self.center[1] += anchor[1] - moved[1];
        self.clamp();
    }

    fn clamp(&mut self) {
        self.pixels_per_tile = self.pixels_per_tile.clamp(ZOOM.0, ZOOM.1);
        for i in 0..2 {
            self.center[i] = self.center[i].clamp(self.bounds.0[i], self.bounds.1[i]);
        }
    }
}

fn mid(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use Pointer::*;

    fn camera() -> CameraControl {
        CameraControl::new([0.0, 0.0], 32.0, [800.0, 600.0])
    }

    fn replay(events: &[Pointer]) -> CameraControl {
        let mut c = camera();
        for e in events {
            c.handle(*e);
        }
        c
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn mouse_drag_pans_and_the_world_follows_the_cursor() {
        let c = replay(&[
            Down {
                id: MOUSE,
                x: 400.0,
                y: 300.0,
            },
            Move {
                id: MOUSE,
                x: 432.0,
                y: 300.0,
            },
            Move {
                id: MOUSE,
                x: 464.0,
                y: 236.0,
            },
            Up { id: MOUSE },
            Move {
                id: MOUSE,
                x: 0.0,
                y: 0.0,
            }, // no button: ignored
        ]);
        // Dragged 64 px right and 64 px up at 32 px a tile.
        assert!(near(c.center, [-2.0, 2.0]), "{:?}", c.center);
        assert!(!c.busy());
    }

    #[test]
    fn wheel_zooms_about_the_cursor() {
        let mut c = camera();
        let under = c.world_at([700.0, 100.0]);
        c.handle(Wheel {
            steps: 3.0,
            x: 700.0,
            y: 100.0,
        });
        assert!(c.pixels_per_tile > 32.0 * 1.5);
        assert!(near(c.world_at([700.0, 100.0]), under));
    }

    #[test]
    fn pinch_zooms_about_the_fingers_and_two_finger_drag_pans() {
        let mut c = camera();
        let under = c.world_at([500.0, 300.0]);
        for e in [
            Down {
                id: 1,
                x: 450.0,
                y: 300.0,
            },
            Down {
                id: 2,
                x: 550.0,
                y: 300.0,
            },
            Move {
                id: 1,
                x: 400.0,
                y: 300.0,
            },
            Move {
                id: 2,
                x: 600.0,
                y: 300.0,
            },
        ] {
            c.handle(e);
        }
        // Fingers twice as far apart: twice the zoom, same world point between them.
        assert!(
            (c.pixels_per_tile - 64.0).abs() < 1e-3,
            "{}",
            c.pixels_per_tile
        );
        assert!(near(c.world_at([500.0, 300.0]), under));
        c.handle(Up { id: 2 });
        c.handle(Move {
            id: 1,
            x: 464.0,
            y: 300.0,
        });
        assert!(
            near(c.world_at([564.0, 300.0]), under),
            "one finger left keeps panning"
        );
    }

    #[test]
    fn zoom_and_position_stay_in_bounds() {
        let mut c = camera();
        c.handle(Wheel {
            steps: 100.0,
            x: 400.0,
            y: 300.0,
        });
        assert_eq!(c.pixels_per_tile, ZOOM.1);
        c.handle(Wheel {
            steps: -100.0,
            x: 400.0,
            y: 300.0,
        });
        assert_eq!(c.pixels_per_tile, ZOOM.0);
        c.set_bounds([-10.0, -10.0], [10.0, 10.0]);
        c.handle(Down {
            id: MOUSE,
            x: 0.0,
            y: 0.0,
        });
        c.handle(Move {
            id: MOUSE,
            x: -10_000.0,
            y: 0.0,
        });
        assert_eq!(c.center[0], 10.0);
    }

    #[test]
    fn a_recording_replays_the_same_on_touch_and_mouse() {
        let touch = "[Down(id: 7, x: 400.0, y: 300.0), Move(id: 7, x: 300.0, y: 250.0), Up(id: 7)]";
        let mouse = "[Down(id: 18446744073709551615, x: 400.0, y: 300.0), Move(id: 18446744073709551615, x: 300.0, y: 250.0), Up(id: 18446744073709551615)]";
        let a = replay(&ron::from_str::<Vec<Pointer>>(touch).unwrap());
        let b = replay(&ron::from_str::<Vec<Pointer>>(mouse).unwrap());
        assert_eq!(a, b);
        assert!(near(a.center, [3.125, 1.5625]));
    }
}
