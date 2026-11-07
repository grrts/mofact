//! The world map (T45): what the camera shows once it is zoomed far out.
//! The ground comes from the renderer's far-out terrain; on top of it this
//! draws the factory as a schematic (machines coloured by how they are
//! doing, belts and pipes as thin dim lines), every ore patch as its ore's
//! icon, and the player's pins.

use mofact_core::content::{BuildingRole as Role, Content};
use mofact_core::geom::TilePos;
use mofact_core::ids::ItemId;
use mofact_core::snapshot::{MachineState, Snapshot};
use mofact_render::{Camera, Look, Overlay};
use mofact_world::Patch;

/// Below this many points a tile, the camera is out on the world map.
pub const MAP_ZOOM: f32 = 4.0;

/// How far out the map button zooms, in points a tile.
pub const MAP_OPEN: f32 = 1.0;

/// Whether `camera` is out on the world map at `scale` pixels a point.
pub fn is_map(camera: &Camera, scale: f32) -> bool {
    camera.pixels_per_tile < MAP_ZOOM * scale.max(0.5)
}

/// Screen pixel of the top-left corner of `t`.
fn screen(camera: &Camera, (w, h): (u32, u32), x: f32, y: f32) -> [f32; 2] {
    let ppt = camera.pixels_per_tile;
    [
        (x - camera.center[0]) * ppt + w as f32 / 2.0,
        (y - camera.center[1]) * ppt + h as f32 / 2.0,
    ]
}

const BELT: [u8; 4] = [0x9a, 0xa8, 0xb8, 0xb0];
const IDLE: [u8; 4] = [0xc8, 0xcf, 0xd6, 0xff];
const PIN: [u8; 4] = [0xff, 0x6b, 0xd0, 0xff];
const SHADOW: [u8; 4] = [0x10, 0x14, 0x18, 0xc0];

/// The factory as a schematic, drawn on the map.
pub fn factory(
    o: &mut Overlay,
    content: &Content,
    look: &Look,
    camera: &Camera,
    size: (u32, u32),
    scale: f32,
    snapshot: &Snapshot,
) {
    let ppt = camera.pixels_per_tile;
    let least = 3.0 * scale.max(0.5);
    for e in &snapshot.entities {
        let Some(def) = content.buildings.iter().find(|b| b.id == e.building) else {
            continue;
        };
        let at = screen(camera, size, e.pos.x as f32, e.pos.y as f32);
        let (w, h) = (f32::from(def.size.0) * ppt, f32::from(def.size.1) * ppt);
        if at[0] + w < -least
            || at[1] + h < -least
            || at[0] > size.0 as f32 + least
            || at[1] > size.1 as f32 + least
        {
            continue;
        }
        let carrier = matches!(
            def.role,
            Role::Belt { .. }
                | Role::Splitter
                | Role::Underground { .. }
                | Role::Pipe { .. }
                | Role::UndergroundPipe { .. }
                | Role::Pole { .. }
        );
        if carrier {
            // Lines just under the size of a machine, so the flow reads.
            let s = (ppt * 0.6).max(least * 0.6);
            let c = [at[0] + (ppt - s) / 2.0, at[1] + (ppt - s) / 2.0];
            o.panel(c, [s, s], s * 0.3, BELT);
            continue;
        }
        use mofact_render::palette::Status;
        let status = match e.state {
            MachineState::Working => Status::Working,
            MachineState::Starved | MachineState::NoPower => Status::Starved,
            MachineState::Blocked => Status::Blocked,
            MachineState::Idle => Status::Idle,
        };
        let color = look.status(status);
        // In colour-blind mode a machine in trouble is a ring, not a dot,
        // so its state reads without its colour.
        let hollow = look.colour_blind() && matches!(status, Status::Starved | Status::Blocked);
        let (w, h) = (w.max(least * 1.5), h.max(least * 1.5));
        let mid = [
            at[0] + f32::from(def.size.0) * ppt / 2.0,
            at[1] + f32::from(def.size.1) * ppt / 2.0,
        ];
        let c = [mid[0] - w / 2.0, mid[1] - h / 2.0];
        o.panel(
            [c[0] - 1.0, c[1] - 1.0],
            [w + 2.0, h + 2.0],
            w * 0.3,
            SHADOW,
        );
        o.panel(c, [w, h], w * 0.25, color);
        if hollow {
            let k = 0.5;
            o.panel(
                [c[0] + w * (1.0 - k) / 2.0, c[1] + h * (1.0 - k) / 2.0],
                [w * k, h * k],
                w * k * 0.25,
                SHADOW,
            );
        }
    }
}

/// Each ore patch in view as its ore's icon on a dark disc, sized by the patch.
pub fn patches(
    o: &mut Overlay,
    look: &Look,
    camera: &Camera,
    size: (u32, u32),
    scale: f32,
    patches: &[(Patch, ItemId)],
) {
    let u = scale.max(0.5);
    let ppt = camera.pixels_per_tile;
    for (p, item) in patches {
        let Some((sprite, tint)) = look.item(item) else {
            continue;
        };
        let d = (p.radius * 2.0 * ppt).min(26.0 * u);
        let mid = screen(
            camera,
            size,
            p.center.x as f32 + 0.5,
            p.center.y as f32 + 0.5,
        );
        if mid[0] < -d || mid[1] < -d || mid[0] > size.0 as f32 + d || mid[1] > size.1 as f32 + d {
            continue;
        }
        let ring = look.item_color(item).unwrap_or(IDLE);
        if d < 12.0 * u {
            // Too far out for an icon: a dot of the ore's colour.
            let d = d.max(5.0 * u);
            let dot = [mid[0] - d / 2.0, mid[1] - d / 2.0];
            o.panel(
                [dot[0] - u, dot[1] - u],
                [d + 2.0 * u, d + 2.0 * u],
                d,
                SHADOW,
            );
            o.panel(dot, [d, d], d, ring);
            continue;
        }
        o.panel(
            [mid[0] - d / 2.0 - 2.0 * u, mid[1] - d / 2.0 - 2.0 * u],
            [d + 4.0 * u, d + 4.0 * u],
            d,
            [ring[0], ring[1], ring[2], 0xe0],
        );
        o.panel([mid[0] - d / 2.0, mid[1] - d / 2.0], [d, d], d, SHADOW);
        let s = d * 0.8;
        o.icon(sprite, [mid[0] - s / 2.0, mid[1] - s / 2.0], s, tint);
    }
}

/// The player's pins: a drop-shaped marker standing on its tile, at any zoom.
pub fn pins(o: &mut Overlay, camera: &Camera, size: (u32, u32), scale: f32, pins: &[TilePos]) {
    let u = scale.max(0.5);
    let ppt = camera.pixels_per_tile;
    for t in pins {
        let foot = screen(camera, size, t.x as f32 + 0.5, t.y as f32 + 0.5);
        let _ = ppt;
        let d = 18.0 * u;
        let head = [foot[0] - d / 2.0, foot[1] - d - 8.0 * u];
        // The stem, then the head with a dark eye.
        o.panel(
            [foot[0] - 1.5 * u, head[1] + d / 2.0],
            [3.0 * u, d / 2.0 + 8.0 * u],
            1.5 * u,
            PIN,
        );
        o.panel(
            [head[0] - 1.5 * u, head[1] - 1.5 * u],
            [d + 3.0 * u, d + 3.0 * u],
            d,
            SHADOW,
        );
        o.panel(head, [d, d], d, PIN);
        let e = d * 0.4;
        o.panel(
            [foot[0] - e / 2.0, head[1] + (d - e) / 2.0],
            [e, e],
            e,
            SHADOW,
        );
        o.panel(
            [foot[0] - 3.0 * u, foot[1] - 1.5 * u],
            [6.0 * u, 3.0 * u],
            1.5 * u,
            SHADOW,
        );
    }
}

/// The pin, if any, under screen point `at`: within a finger of its head.
pub fn pin_at(
    camera: &Camera,
    size: (u32, u32),
    scale: f32,
    pins: &[TilePos],
    at: [f32; 2],
) -> Option<TilePos> {
    let u = scale.max(0.5);
    pins.iter()
        .copied()
        .filter(|t| {
            let foot = screen(camera, size, t.x as f32 + 0.5, t.y as f32 + 0.5);
            let head = [foot[0], foot[1] - 17.0 * u];
            (head[0] - at[0]).hypot(head[1] - at[1]) < 22.0 * u
                || (foot[0] - at[0]).hypot(foot[1] - at[1]) < 14.0 * u
        })
        .min_by(|a, b| {
            let d = |t: &TilePos| {
                let f = screen(camera, size, t.x as f32 + 0.5, t.y as f32 + 0.5);
                (f[0] - at[0]).hypot(f[1] - at[1])
            };
            d(a).total_cmp(&d(b))
        })
}
