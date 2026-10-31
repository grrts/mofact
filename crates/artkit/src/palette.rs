//! The fixed palette (R68): every colour the art kit and the UI paint with,
//! as sRGB bytes, taken from the prototype. Nothing outside this file names a
//! colour by value.

pub type Rgba = [u8; 4];

pub const GROUND: Rgba = [0x14, 0x18, 0x1d, 0xff];
/// The ground's lighter patches.
pub const GROUND2: Rgba = [0x18, 0x1d, 0x23, 0xff];
pub const BELT: Rgba = [0x2a, 0x31, 0x39, 0xff];
pub const BELT_RAIL: Rgba = [0x3c, 0x46, 0x51, 0xff];
pub const CHEVRON: Rgba = [0x55, 0x61, 0x6e, 0xff];
pub const MACHINE: Rgba = [0x3a, 0x44, 0x4f, 0xff];
pub const MACHINE_HI: Rgba = [0x5b, 0x68, 0x75, 0xff];
pub const MACHINE_EDGE: Rgba = [0x20, 0x26, 0x2d, 0xff];
pub const WHITE: Rgba = [0xff, 0xff, 0xff, 0xff];
pub const ACCENT: Rgba = [0xff, 0xd1, 0x66, 0xff];
pub const STARVE: Rgba = [0xff, 0x5d, 0x5d, 0xff];
pub const INK: Rgba = [0xe9, 0xed, 0xf0, 0xff];
pub const MUTED: Rgba = [0x8d, 0x99, 0xa5, 0xff];

/// Drop shadows under buildings.
pub const SHADOW: Rgba = [0, 0, 0, 82];
/// Belt rollers, under the surface.
pub const ROLLER: Rgba = [0x23, 0x29, 0x30, 0xff];
/// The light top edge of a belt rail.
pub const RAIL_HI: Rgba = [0x56, 0x61, 0x6d, 0xff];
/// Rivets and other dark details.
pub const RIVET: Rgba = [0x1b, 0x20, 0x26, 0xff];
/// Windows and slots into a machine's dark inside.
pub const WINDOW: Rgba = [0x1a, 0x1f, 0x25, 0xff];
pub const SOOT: Rgba = [0x11, 0x15, 0x1a, 0xff];
/// A cold furnace's hearth.
pub const HEARTH: Rgba = [0x20, 0x26, 0x2c, 0xff];
/// The two firebrick tones around a furnace.
pub const BRICK: [Rgba; 2] = [[0x55, 0x43, 0x3d, 0xff], [0x4a, 0x3a, 0x36, 0xff]];
/// A working furnace's flame, outside in.
pub const FLAME: [Rgba; 3] = [
    [0xff, 0x6a, 0x2b, 0xff],
    [0xff, 0xb3, 0x47, 0xff],
    [0xff, 0xf1, 0xc2, 0xff],
];
pub const DRILL_BIT: Rgba = [0xc9, 0xd1, 0xd8, 0xff];
/// The hub's landing pad, its hazard stripes and its unlit lamps.
pub const PAD: Rgba = [0x2b, 0x33, 0x3c, 0xff];
pub const HAZARD_DARK: Rgba = [0x2a, 0x2f, 0x36, 0xff];
pub const LAMP_OFF: Rgba = [0x6b, 0x5a, 0x2a, 0xff];
/// A circuit board's gold pins and dark chip.
pub const PIN: Rgba = [0xe6, 0xc1, 0x5a, 0xff];
pub const CHIP: Rgba = [0x1f, 0x5a, 0x35, 0xff];

/// `c` with its alpha scaled by `a` (0 to 1).
pub fn alpha(c: Rgba, a: f32) -> Rgba {
    [c[0], c[1], c[2], (c[3] as f32 * a) as u8]
}

/// A lighter (`amt` > 0) or darker (`amt` < 0) step of `c`, as the
/// prototype's flat shading does it.
pub fn shade(c: Rgba, amt: f32) -> Rgba {
    let f = |v: u8| {
        let v = v as f32;
        let out = if amt >= 0.0 {
            v + (255.0 - v) * amt
        } else {
            v * (1.0 + amt)
        };
        out.round().clamp(0.0, 255.0) as u8
    };
    [f(c[0]), f(c[1]), f(c[2]), c[3]]
}

/// An item's colour, by the palette key in `data/items.ron`.
pub fn item(key: &str) -> Rgba {
    match key {
        "iron" => [0x8f, 0xb0, 0xd1, 0xff],
        "iron-ore" => [0x4f, 0x6a, 0x86, 0xff],
        "copper" => [0xf0, 0x8a, 0x4b, 0xff],
        "copper-ore" => [0x9a, 0x55, 0x30, 0xff],
        "circuit" => [0x63, 0xd4, 0x8a, 0xff],
        "stone" => [0x8c, 0x84, 0x7a, 0xff],
        "brick" => [0xb8, 0x5a, 0x3c, 0xff],
        "coal" => [0x3a, 0x3d, 0x42, 0xff],
        "steel" => [0xb9, 0xc4, 0xcf, 0xff],
        "motor" => [0x5c, 0x7a, 0x99, 0xff],
        "pack-survey" => [0xe0, 0x4f, 0x5f, 0xff],
        "pack-engineering" => [0x4f, 0xb8, 0x6e, 0xff],
        _ => [0xe9, 0xed, 0xf0, 0xff],
    }
}

/// A tint for a machine body that says which kind it is.
pub fn building(id: &str) -> Rgba {
    match id {
        "drill" => [0xc8, 0xd2, 0xdc, 0xff],
        "furnace" => [0xff, 0xc8, 0xa8, 0xff],
        "assembler" => [0xc0, 0xe8, 0xcc, 0xff],
        _ => WHITE,
    }
}
