//! Input lane: touch, mouse and keyboard mapped to camera gestures and build commands.
//!
//! The platform shell turns its events into [`Pointer`] events in screen
//! pixels, and [`CameraControl`] turns those into a camera: one finger or a
//! mouse drag pans, two fingers pinch-zoom about their midpoint, and the wheel
//! zooms about the cursor (T15). [`build`] turns the rest into build
//! commands (T20).
//!
//! Pointer events are plain data, so a session can be recorded and replayed:
//! the replay tests below drive the same code the app does.

pub mod build;

pub use build::{Builder, Ghost, Ground, Occupant, Tool};
pub use mofact_core as core;

use mofact_core::command::Command;
use mofact_core::geom::TilePos;
use serde::{Deserialize, Serialize};

/// The pointer id the mouse's left button uses; touches use their finger ids.
pub const MOUSE: u64 = u64::MAX;
/// The right or middle mouse button: always moves the camera, whatever the tool.
pub const MOUSE_PAN: u64 = u64::MAX - 1;

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

/// Closest and furthest zoom, in points per tile (screen pixels at scale 1).
pub const ZOOM: (f32, f32) = (8.0, 128.0);
/// How much one wheel notch zooms.
const WHEEL_STEP: f32 = 1.15;

/// The camera, driven by pointer events.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraControl {
    /// The world point, in tiles, at the middle of the screen.
    pub center: [f32; 2],
    pub pixels_per_tile: f32,
    /// Screen pixels per point: 3 on most phones, so a tile looks the same
    /// size on every screen.
    scale: f32,
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
            scale: 1.0,
            viewport,
            bounds: ([-5_000.0; 2], [5_000.0; 2]),
            down: Vec::new(),
        };
        c.clamp();
        c
    }

    /// Follow the display's pixels per point, keeping the view the same size.
    pub fn set_scale(&mut self, scale: f32) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        if scale != self.scale {
            self.pixels_per_tile *= scale / self.scale;
            self.scale = scale;
            self.clamp();
        }
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
        self.pixels_per_tile *= factor;
        self.clamp();
        let moved = self.world_at(at);
        self.center[0] += anchor[0] - moved[0];
        self.center[1] += anchor[1] - moved[1];
        self.clamp();
    }

    fn clamp(&mut self) {
        self.pixels_per_tile = self
            .pixels_per_tile
            .clamp(ZOOM.0 * self.scale, ZOOM.1 * self.scale);
        for i in 0..2 {
            self.center[i] = self.center[i].clamp(self.bounds.0[i], self.bounds.1[i]);
        }
    }
}

/// Routes pointers to the camera or the build tools. With the hand,
/// everything moves the camera. With the belt or remove tool, one finger (or
/// the left button) draws and two fingers pan and zoom. With a building in
/// hand, a tap places it and a drag pans, so looking around never builds by
/// accident. A second finger always drops what the first one started.
#[derive(Debug)]
pub struct Controls {
    pub camera: CameraControl,
    pub builder: Builder,
    /// How far, in pixels, a finger may wander and still be tapping. The game
    /// sets it from the display scale.
    pub slop: f32,
    /// The pointer building right now, where it went down, and where it last was.
    building: Option<(u64, [f32; 2], [f32; 2])>,
}

impl Controls {
    pub fn new(camera: CameraControl) -> Self {
        Self {
            camera,
            builder: Builder::default(),
            slop: 12.0,
            building: None,
        }
    }

    fn tile(&self, x: f32, y: f32) -> TilePos {
        let [wx, wy] = self.camera.world_at([x, y]);
        TilePos::new(wx.floor() as i32, wy.floor() as i32)
    }

    /// Handle one pointer event; returns the commands it builds.
    pub fn handle(&mut self, event: Pointer, ground: &dyn Ground) -> Vec<Command> {
        match (event, self.building) {
            (Pointer::Down { id, x, y }, None)
                if !self.camera.busy() && id != MOUSE_PAN && self.builder.tool != Tool::Hand =>
            {
                self.building = Some((id, [x, y], [x, y]));
                let tile = self.tile(x, y);
                return self.builder.start(tile, ground);
            }
            (Pointer::Down { .. }, Some((first, _, [fx, fy]))) => {
                // A second finger: stop building and hand both to the camera.
                self.builder.cancel();
                self.building = None;
                self.camera.handle(Pointer::Down {
                    id: first,
                    x: fx,
                    y: fy,
                });
                self.camera.handle(event);
            }
            (Pointer::Move { id, x, y }, Some((b, start, _))) if id == b => {
                let taps = matches!(self.builder.tool, Tool::Place(_));
                if taps && dist(start, [x, y]) > self.slop {
                    // The finger is travelling: a pan, not a placement.
                    self.builder.cancel();
                    self.building = None;
                    self.camera.handle(Pointer::Down {
                        id,
                        x: start[0],
                        y: start[1],
                    });
                    self.camera.handle(event);
                    return Vec::new();
                }
                self.building = Some((id, start, [x, y]));
                if taps {
                    return Vec::new(); // the ghost stays where the tap began
                }
                let tile = self.tile(x, y);
                return self.builder.drag(tile, ground);
            }
            (Pointer::Up { id }, Some((b, _, _))) if id == b => {
                self.building = None;
                return self.builder.end(ground);
            }
            _ => self.camera.handle(event),
        }
        Vec::new()
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
    fn one_finger_builds_two_fingers_pan_and_the_hand_only_pans() {
        struct Empty;
        impl Ground for Empty {
            fn at(&self, _: TilePos) -> Option<build::Occupant> {
                None
            }
        }
        let mut c = Controls::new(camera());
        c.builder.set_tool(Tool::Belt);
        // Screen centre is world (0, 0); 32 px a tile.
        c.handle(
            Down {
                id: 1,
                x: 400.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Move {
                id: 1,
                x: 496.0,
                y: 300.0,
            },
            &Empty,
        );
        let built = c.handle(Up { id: 1 }, &Empty);
        assert_eq!(built.len(), 4, "tiles 0 to 3: {built:?}");
        assert_eq!(c.camera.center, [0.0, 0.0], "building doesn't pan");

        c.handle(
            Down {
                id: 1,
                x: 400.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Down {
                id: 2,
                x: 500.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Move {
                id: 2,
                x: 600.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(Up { id: 2 }, &Empty);
        assert!(
            c.handle(Up { id: 1 }, &Empty).is_empty(),
            "a pinch builds nothing"
        );
        assert!(c.camera.pixels_per_tile > 32.0);

        let before = c.camera.center;
        c.handle(
            Down {
                id: MOUSE_PAN,
                x: 400.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Move {
                id: MOUSE_PAN,
                x: 300.0,
                y: 300.0,
            },
            &Empty,
        );
        assert!(c.handle(Up { id: MOUSE_PAN }, &Empty).is_empty());
        assert!(
            c.camera.center[0] > before[0],
            "right drag pans even with a tool"
        );
    }

    #[test]
    fn with_a_building_in_hand_a_tap_places_and_a_drag_pans() {
        struct Empty;
        impl Ground for Empty {
            fn at(&self, _: TilePos) -> Option<build::Occupant> {
                None
            }
        }
        let mut c = Controls::new(camera());
        c.builder.set_tool(Tool::Place("furnace".into()));
        // A tap with a little wobble places at the tile where it began.
        c.handle(
            Down {
                id: 1,
                x: 400.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Move {
                id: 1,
                x: 405.0,
                y: 304.0,
            },
            &Empty,
        );
        let placed = c.handle(Up { id: 1 }, &Empty);
        assert!(
            matches!(&placed[..], [Command::Place { pos, .. }] if *pos == TilePos::new(0, 0)),
            "{placed:?}"
        );
        // A drag moves the camera and builds nothing.
        c.handle(
            Down {
                id: 1,
                x: 400.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Move {
                id: 1,
                x: 340.0,
                y: 300.0,
            },
            &Empty,
        );
        c.handle(
            Move {
                id: 1,
                x: 272.0,
                y: 300.0,
            },
            &Empty,
        );
        assert!(c.builder.ghosts().is_empty(), "the ghost goes once it pans");
        assert!(c.handle(Up { id: 1 }, &Empty).is_empty());
        assert!(near(c.camera.center, [4.0, 0.0]), "{:?}", c.camera.center);
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

    #[test]
    fn a_tile_looks_the_same_size_at_any_scale() {
        let mut c = CameraControl::new([0.0, 0.0], 32.0, [1170.0, 2532.0]);
        c.set_scale(3.0);
        assert_eq!(c.pixels_per_tile, 96.0);
        c.handle(Pointer::Wheel {
            steps: 100.0,
            x: 0.0,
            y: 0.0,
        });
        assert_eq!(
            c.pixels_per_tile,
            ZOOM.1 * 3.0,
            "zooms in as far, in points"
        );
        c.set_scale(1.0);
        assert_eq!(c.pixels_per_tile, ZOOM.1);
    }
}
