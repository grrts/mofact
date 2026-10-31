//! Machine animations (T41), drawn over the machines each frame from three
//! tinted sprites (a soft dot, a ring and a bar), so they cost no atlas
//! room per machine:
//!
//! - **Working:** smoke rises from furnaces, boilers and refineries, steam
//!   puffs from engines, chips fly from drills and bubbles rise in chemical
//!   plants; anything crafting shows a progress bar along its foot.
//! - **Starved or blocked:** a ring pulses out from its badge, red or amber.
//! - **No power:** the machine dims and brightens, like a failing light.
//!
//! Everything moves with `t`, the seconds the game has been showing, and
//! each machine is offset by its id so a row of them doesn't move in step.

use crate::atlas::Atlas;
use crate::look::{Effect, Look, FX_BAR, FX_DOT, FX_RING};
use crate::palette::Rgba;
use crate::scene::{Camera, Instance, Scene};
use mofact_core::snapshot::{MachineState, Snapshot};
use std::f32::consts::TAU;

const SMOKE: Rgba = [0x9a, 0xa4, 0xae, 0xff];
const STEAM: Rgba = [0xee, 0xf2, 0xf5, 0xff];
const WORKING: Rgba = [0x4c, 0xd9, 0x7b, 0xff];
const TRACK: Rgba = [0x0e, 0x12, 0x16, 0xff];
const STARVED: Rgba = [0xff, 0x5d, 0x5d, 0xff];
const BLOCKED: Rgba = [0xff, 0xd1, 0x66, 0xff];

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    [c[0], c[1], c[2], (255.0 * a.clamp(0.0, 1.0)) as u8]
}

/// A fixed fraction from 0 to 1 for entity `id`, to stagger its animation.
fn stagger(id: u32, salt: u32) -> f32 {
    let h = id.wrapping_mul(0x9e37_79b1) ^ salt.wrapping_mul(0x85eb_ca77);
    (h >> 8) as f32 / (1u32 << 24) as f32
}

impl Scene {
    /// Add the animations for `latest` at `t` seconds, over what
    /// [`Scene::build`] laid out. Call right after it.
    pub fn animate(
        &mut self,
        look: &Look,
        atlases: &[Atlas],
        camera: &Camera,
        (width, height): (u32, u32),
        latest: &Snapshot,
        t: f32,
    ) {
        let atlas = &atlases[self.band.min(atlases.len().saturating_sub(1))];
        let uv = |name: &str| atlas.get(name).unwrap_or([0.0; 4]);
        let (dot, ring, bar) = (uv(FX_DOT), uv(FX_RING), uv(FX_BAR));
        let (lo, hi) = camera.visible(width, height);
        // As Scene::build sizes the badges.
        let badge = (7.0 / camera.pixels_per_tile).max(0.2) * 2.0;
        let mut put = |pos: [f32; 2], size: [f32; 2], uv, tint| {
            self.instances.push(Instance {
                pos,
                size,
                uv,
                tint,
                turns: 0,
            });
        };
        // A dot `d` tiles across centred on `c`.
        let blob = |c: [f32; 2], d: f32| ([c[0] - d / 2.0, c[1] - d / 2.0], [d, d]);

        for e in &latest.entities {
            let Some((w, h)) = look.size(&e.building) else {
                continue;
            };
            let (x, y, w, h) = (e.pos.x as f32, e.pos.y as f32, f32::from(w), f32::from(h));
            if x + w < lo[0] || y + h < lo[1] || x > hi[0] || y > hi[1] {
                continue;
            }
            let id = e.id.0;
            let mid = [x + w / 2.0, y + h / 2.0];
            match e.state {
                MachineState::Working => {
                    match look.effect(&e.building) {
                        Effect::Smoke { big } => {
                            let scale = if big { 1.6 } else { 1.0 };
                            // From the back corner, where a chimney would be.
                            let at = [x + w * 0.3, y + h * 0.3];
                            for k in 0..3 {
                                let p = (t * 0.45 + k as f32 / 3.0 + stagger(id, 1)).fract();
                                let drift = (t * 1.3 + k as f32 * 2.0).sin() * 0.08 + p * 0.15;
                                let d = (0.22 + 0.45 * p) * scale;
                                let c = [at[0] + drift, at[1] - p * 0.9 * scale];
                                let a = (1.0 - p) * (p * 6.0).min(1.0) * 0.55;
                                let (pos, size) = blob(c, d);
                                put(pos, size, dot, with_alpha(SMOKE, a));
                            }
                        }
                        Effect::Steam => {
                            let at = [x + w * 0.25, y + h * 0.45];
                            for k in 0..2 {
                                let p = (t * 0.9 + k as f32 / 2.0 + stagger(id, 2)).fract();
                                let d = 0.3 + 0.6 * p;
                                let c = [at[0] - p * 0.2, at[1] - p * 1.1];
                                let a = (1.0 - p) * (p * 8.0).min(1.0) * 0.7;
                                let (pos, size) = blob(c, d);
                                put(pos, size, dot, with_alpha(STEAM, a));
                            }
                        }
                        Effect::Dust => {
                            let ore = e
                                .ore
                                .as_ref()
                                .and_then(|o| look.item_color(o))
                                .unwrap_or(SMOKE);
                            // Lit chips, so dark ores still show on the drill.
                            let ore = ore.map(|v| ((u16::from(v) + 255) / 2) as u8);
                            for k in 0..4 {
                                let p = (t * 1.8 + k as f32 / 4.0 + stagger(id, 3)).fract();
                                let a = (k as f32 / 4.0 + stagger(id, 4)) * TAU;
                                let r = 0.12 + 0.32 * p;
                                // A little hop: up, then down.
                                let hop = (p * std::f32::consts::PI).sin() * 0.12;
                                let c = [mid[0] + a.cos() * r, mid[1] + a.sin() * r - hop];
                                let (pos, size) = blob(c, 0.14);
                                put(pos, size, dot, with_alpha(ore, (1.0 - p) * 1.0));
                            }
                        }
                        Effect::Bubbles => {
                            for k in 0..3 {
                                let p = (t * 0.8 + k as f32 / 3.0 + stagger(id, 5)).fract();
                                let side = (stagger(id, 6 + k) - 0.5) * 0.3;
                                let c = [mid[0] + side, mid[1] + 0.15 - p * 0.35];
                                let (pos, size) = blob(c, 0.08 + 0.06 * p);
                                let a = (1.0 - p) * (p * 5.0).min(1.0) * 0.9;
                                put(pos, size, dot, with_alpha(STEAM, a));
                            }
                        }
                        Effect::Busy | Effect::None => {}
                    }
                    // A progress bar along its foot, for anything mid-craft.
                    if e.progress > 0.0 && look.effect(&e.building) != Effect::None {
                        let inset = 0.16 * w.min(h);
                        let (bx, by) = (x + inset, y + h - inset - 0.06);
                        let bw = w - 2.0 * inset;
                        put([bx, by], [bw, 0.07], bar, with_alpha(TRACK, 0.7));
                        let done = e.progress.clamp(0.0, 1.0);
                        put([bx, by], [bw * done, 0.07], bar, with_alpha(WORKING, 0.95));
                    }
                }
                MachineState::Starved | MachineState::Blocked | MachineState::NoPower
                    if look.has_badge(&e.building) =>
                {
                    if e.state == MachineState::NoPower {
                        // Dimming and brightening, like a failing light.
                        let a = 0.25 + 0.15 * (t * 3.0 + stagger(id, 7) * TAU).sin();
                        put([x, y], [w, h], bar, with_alpha([0, 0, 0, 255], a));
                    }
                    // A ring pulsing out from the badge, where Scene::build put it.
                    let r = badge / 2.0;
                    let c = [x + w - r * 0.3, y + r * 0.3];
                    let p = (t * 0.8 + stagger(id, 8)).fract();
                    let d = badge * (1.0 + 1.4 * p);
                    let color = if e.state == MachineState::Blocked {
                        BLOCKED
                    } else {
                        STARVED
                    };
                    let (pos, size) = blob(c, d);
                    put(pos, size, ring, with_alpha(color, (1.0 - p) * 0.8));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofact_core::content::Content;
    use mofact_core::geom::{Dir, TilePos};
    use mofact_core::ids::EntityId;
    use mofact_core::snapshot::EntityView;

    fn content() -> Content {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap();
        Content {
            items: ron::from_str(&read("items.ron")).unwrap(),
            recipes: ron::from_str(&read("recipes.ron")).unwrap(),
            buildings: ron::from_str(&read("buildings.ron")).unwrap(),
            research: Vec::new(),
            goals: Vec::new(),
        }
    }

    fn machine(
        id: u32,
        building: &str,
        x: i32,
        y: i32,
        state: MachineState,
        progress: f32,
    ) -> EntityView {
        EntityView {
            id: EntityId(id),
            building: building.into(),
            pos: TilePos::new(x, y),
            dir: Dir::East,
            recipe: None,
            ore: None,
            grid: None,
            exit: false,
            fluid: None,
            links: 0,
            state,
            progress,
        }
    }

    fn uv_of(atlases: &[Atlas], band: usize, name: &str) -> [f32; 4] {
        atlases[band].get(name).unwrap()
    }

    #[test]
    fn working_machines_move_and_stalled_ones_pulse() {
        let look = Look::new(&content());
        let atlases = look.atlases();
        let camera = Camera {
            center: [5.0, 5.0],
            pixels_per_tile: 64.0,
        };
        let snap = Snapshot {
            entities: vec![
                machine(1, "furnace", 3, 3, MachineState::Working, 0.5),
                machine(2, "assembler", 6, 3, MachineState::Starved, 0.0),
                machine(3, "assembler", 8, 3, MachineState::Idle, 0.0),
            ],
            ..Snapshot::default()
        };
        let frame = |t: f32| {
            let mut scene = Scene::default();
            scene.build(&look, &atlases, &camera, (800, 600), &snap, &snap, 1.0);
            let before = scene.instances.len();
            scene.animate(&look, &atlases, &camera, (800, 600), &snap, t);
            (scene.band, scene.instances.split_off(before))
        };
        let (band, a) = frame(0.0);
        let (_, b) = frame(0.5);
        let dots: Vec<_> = a
            .iter()
            .filter(|i| i.uv == uv_of(&atlases, band, FX_DOT))
            .collect();
        assert_eq!(dots.len(), 3, "three puffs of smoke");
        let rings = a
            .iter()
            .filter(|i| i.uv == uv_of(&atlases, band, FX_RING))
            .count();
        assert_eq!(
            rings, 1,
            "the starved assembler pulses, the idle one doesn't"
        );
        let bars = a
            .iter()
            .filter(|i| i.uv == uv_of(&atlases, band, FX_BAR))
            .count();
        assert_eq!(bars, 2, "the furnace's progress bar and its track");
        // Half a second on, the smoke has moved.
        assert_ne!(a[0].pos, b[0].pos);
        assert_eq!(a.len(), b.len());
    }
}
