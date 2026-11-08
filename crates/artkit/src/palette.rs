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
/// Power poles' weathered wood, and wind turbine blades.
pub const POLE: Rgba = [0x8a, 0x6a, 0x48, 0xff];
/// The glazed insulators power cables are tied to.
pub const PORCELAIN: Rgba = [0xe2, 0xdc, 0xcf, 0xff];
pub const BLADE: Rgba = [0xd8, 0xde, 0xe4, 0xff];
/// Pipes' steel, its lit edge, and the empty channel inside.
pub const PIPE: Rgba = [0x6e, 0x7d, 0x8b, 0xff];
pub const PIPE_HI: Rgba = [0x93, 0xa2, 0xb0, 0xff];
pub const PIPE_EMPTY: Rgba = [0x16, 0x1a, 0x1f, 0xff];
/// Solar cells under glass.
pub const SOLAR: Rgba = [0x24, 0x3b, 0x6e, 0xff];
/// A well's concrete pad.
pub const CONCRETE: Rgba = [0x4d, 0x52, 0x57, 0xff];

/// Trim for the faster tiers of a building, so a fast belt or a fast
/// assembler stands out from the first kind: teal for tier 2, violet for 3.
pub const TIER: [Rgba; 2] = [[0x3f, 0xc1, 0xb0, 0xff], [0xa8, 0x7c, 0xf0, 0xff]];
/// A lab's glass dome.
pub const GLASS: Rgba = [0x7f, 0xc8, 0xe6, 0xff];
/// The ground, by the world's terrain number (deep water, water, sand,
/// grass, forest, scrub, rock, cliff, dune, snow, ice, ash, basalt, lava):
/// muted, so the factory stands out on it. The ground shader in
/// `crates/render` uses the same values.
pub const TERRAIN: [Rgba; 14] = [
    [0x12, 0x2c, 0x40, 0xff],
    [0x1c, 0x45, 0x5a, 0xff],
    [0x5a, 0x53, 0x40, 0xff],
    [0x2d, 0x42, 0x2a, 0xff],
    [0x22, 0x35, 0x22, 0xff],
    [0x47, 0x43, 0x30, 0xff],
    [0x3f, 0x42, 0x45, 0xff],
    [0x24, 0x21, 0x1f, 0xff],
    [0x9c, 0x7c, 0x4c, 0xff],
    [0x9a, 0xa5, 0xac, 0xff],
    [0x6f, 0x8f, 0xa0, 0xff],
    [0x3b, 0x36, 0x34, 0xff],
    [0x2a, 0x2a, 0x2e, 0xff],
    [0xc4, 0x47, 0x1c, 0xff],
];
/// Trees: broadleaf canopy and its lit side, pine needles and their lit side.
pub const LEAF: Rgba = [0x34, 0x5e, 0x2f, 0xff];
pub const LEAF_HI: Rgba = [0x4f, 0x80, 0x40, 0xff];
pub const NEEDLE: Rgba = [0x1f, 0x45, 0x2e, 0xff];
pub const NEEDLE_HI: Rgba = [0x31, 0x62, 0x3f, 0xff];
/// Dry grass blades and the trunks under the trees.
pub const BLADE_GRASS: Rgba = [0x6a, 0x7d, 0x45, 0xff];
pub const BARK: Rgba = [0x4a, 0x38, 0x29, 0xff];
/// Loose stones and boulders, and their lit tops.
pub const STONE: Rgba = [0x6a, 0x6c, 0x6d, 0xff];
pub const STONE_HI: Rgba = [0x8e, 0x90, 0x91, 0xff];
/// Wild flowers.
pub const PETALS: [Rgba; 3] = [
    [0xe8, 0xd0, 0x5c, 0xff],
    [0xe0, 0x8c, 0xa8, 0xff],
    [0xe6, 0xe8, 0xea, 0xff],
];
/// Track: the gravel bed, the wooden sleepers, the steel rails and their lit top.
pub const BALLAST: Rgba = [0x2b, 0x2f, 0x33, 0xff];
pub const SLEEPER: Rgba = [0x50, 0x41, 0x34, 0xff];
pub const RAIL: Rgba = [0x8f, 0x9a, 0xa5, 0xff];
pub const RAIL_TOP: Rgba = [0xc9, 0xd1, 0xd8, 0xff];
/// A signal's go lamp; its stop lamp is [`STARVE`].
pub const GO: Rgba = [0x5c, 0xd6, 0x7c, 0xff];
/// A locomotive's paint and its darker roof.
pub const LOCO: Rgba = [0xd9, 0x7a, 0x2e, 0xff];
pub const LOCO_ROOF: Rgba = [0x8c, 0x4a, 0x1d, 0xff];

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
        "bauxite" => [0xc0, 0x6a, 0x3a, 0xff],
        "titanium-ore" => [0x9f, 0xc4, 0xd8, 0xff],
        "rare-earth" => [0xb4, 0x6a, 0xd8, 0xff],
        "steel" => [0xb9, 0xc4, 0xcf, 0xff],
        "motor" => [0x5c, 0x7a, 0x99, 0xff],
        "pack-survey" => [0xe0, 0x4f, 0x5f, 0xff],
        "pack-engineering" => [0x4f, 0xb8, 0x6e, 0xff],
        "water" => [0x3f, 0x9b, 0xe8, 0xff],
        "steam" => [0xd9, 0xe2, 0xea, 0xff],
        "crude-oil" => [0x6a, 0x52, 0x7a, 0xff],
        "light-oil" => [0xf2, 0xc1, 0x4e, 0xff],
        "tar" => [0x6b, 0x47, 0x2e, 0xff],
        "plastic" => [0xf4, 0xf1, 0xea, 0xff],
        "sulfur" => [0xe8, 0xd8, 0x3a, 0xff],
        "sulfuric-acid" => [0xb5, 0xe0, 0x3c, 0xff],
        "battery" => [0x4a, 0x8f, 0xd8, 0xff],
        "advanced-circuit" => [0xd9, 0x4c, 0x4c, 0xff],
        "pack-chemistry" => [0x4f, 0x9c, 0xe8, 0xff],
        "silicon" => [0x6f, 0x76, 0x9a, 0xff],
        "processor" => [0x3b, 0x4a, 0x8c, 0xff],
        "robot-frame" => [0xc9, 0xa2, 0x4a, 0xff],
        "low-density" => [0xd6, 0x9a, 0x6a, 0xff],
        "pack-precision" => [0xa8, 0x7c, 0xf0, 0xff],
        "slag" => [0x5e, 0x55, 0x4e, 0xff],
        "titanium" => [0xd0, 0xd8, 0xe8, 0xff],
        "alumina" => [0xf0, 0xe6, 0xd8, 0xff],
        "aluminium" => [0xa9, 0xc9, 0xc4, 0xff],
        "red-mud" => [0xa8, 0x3e, 0x2a, 0xff],
        "rare-earth-oxide" => [0xe0, 0x9c, 0xe8, 0xff],
        "magnet" => [0xd6, 0x3a, 0x3a, 0xff],
        "electric-engine" => [0x3f, 0xc1, 0xb0, 0xff],
        "pack-aerospace" => [0xf0, 0xa0, 0x3c, 0xff],
        "superconductor" => [0x7f, 0xe0, 0xf0, 0xff],
        "ai-core" => [0x8c, 0x4c, 0xd8, 0xff],
        "drone" => [0x9a, 0xa8, 0xb6, 0xff],
        "pack-orbital" => [0x3c, 0xd0, 0xe0, 0xff],
        "fusion-cell" => [0xf0, 0x5c, 0xb8, 0xff],
        "ion-thruster" => [0x6c, 0x7c, 0xd8, 0xff],
        "satellite" => [0xe8, 0xec, 0xf0, 0xff],
        "pack-stellar" => [0xf4, 0xe0, 0x6c, 0xff],
        "tether" => [0x4c, 0x6c, 0x9c, 0xff],
        "climber" => [0xe0, 0x8c, 0x3c, 0xff],
        "beacon" => [0xff, 0xd1, 0x66, 0xff],
        "graphene" => [0x3a, 0x3f, 0x47, 0xff],
        "nanotube" => [0x5a, 0x63, 0x70, 0xff],
        "nanobot" => [0x7c, 0xe0, 0x9c, 0xff],
        "pack-nano" => [0x8c, 0xe0, 0x5a, 0xff],
        "coolant" => [0x9c, 0xe8, 0xff, 0xff],
        "quantum-chip" => [0x5a, 0x3c, 0xc8, 0xff],
        "quantum-core" => [0xb4, 0x8c, 0xff, 0xff],
        "pack-quantum" => [0xd0, 0x6c, 0xf0, 0xff],
        "plasma" => [0xff, 0x6c, 0xd0, 0xff],
        "exotic-matter" => [0x2c, 0x1c, 0x4c, 0xff],
        "field-coil" => [0x3c, 0x9c, 0xff, 0xff],
        "pack-exotic" => [0xff, 0x5c, 0x8c, 0xff],
        "singularity" => [0x1c, 0x1c, 0x24, 0xff],
        "graviton-lens" => [0xc8, 0xb4, 0xff, 0xff],
        "pack-singularity" => [0xf8, 0xf8, 0xff, 0xff],
        "counterweight" => [0x8c, 0x7c, 0x6c, 0xff],
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

/// What a machine (or a train) is doing, as a colour says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Working,
    /// Waiting for input, or for power.
    Starved,
    /// Its output is full.
    Blocked,
    Idle,
}

/// The colour for `status`; with `safe`, from a set that stays apart for
/// red-green and blue-yellow colour blindness (T58), after Okabe and Ito.
pub fn status(status: Status, safe: bool) -> Rgba {
    match (status, safe) {
        (Status::Working, false) => [0x63, 0xd4, 0x8a, 0xff],
        (Status::Starved, false) => STARVE,
        (Status::Blocked, false) => ACCENT,
        (Status::Working, true) => [0x56, 0xb4, 0xe9, 0xff],
        (Status::Starved, true) => [0xd5, 0x5e, 0x00, 0xff],
        (Status::Blocked, true) => [0xf0, 0xe4, 0x42, 0xff],
        (Status::Idle, _) => [0xc8, 0xcf, 0xd6, 0xff],
    }
}

/// An item's colour in colour-blind mode (T58): the ores, which lie on
/// the ground as nothing but their colour, are moved apart in lightness
/// and along blue-yellow, which every common colour blindness still sees.
/// Everything else keeps its colour, as its icon shape tells it apart.
pub fn item_safe(key: &str) -> Rgba {
    match key {
        "iron-ore" => [0x3f, 0x6f, 0xd8, 0xff],
        "copper-ore" => [0xe8, 0x90, 0x2c, 0xff],
        "stone" => [0xb0, 0xa8, 0x98, 0xff],
        "coal" => [0x2c, 0x2e, 0x33, 0xff],
        "bauxite" => [0x8a, 0x3c, 0x1c, 0xff],
        "titanium-ore" => [0xa8, 0xe4, 0xf4, 0xff],
        "rare-earth" => [0xd8, 0x78, 0xc8, 0xff],
        other => item(other),
    }
}

/// A kind of colour blindness, to check colours against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cvd {
    /// No red cones.
    Protanopia,
    /// No green cones: the most common.
    Deuteranopia,
    /// No blue cones.
    Tritanopia,
}

pub const CVDS: [Cvd; 3] = [Cvd::Protanopia, Cvd::Deuteranopia, Cvd::Tritanopia];

fn linear(v: u8) -> f32 {
    let v = f32::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// `c` as someone with `cvd` sees it, in linear RGB (Machado, Oliveira and
/// Fernandes 2009, full severity).
pub fn seen(c: Rgba, cvd: Option<Cvd>) -> [f32; 3] {
    let m = match cvd {
        None => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        Some(Cvd::Protanopia) => [
            [0.152_286, 1.052_583, -0.204_868],
            [0.114_503, 0.786_281, 0.099_216],
            [-0.003_882, -0.048_116, 1.051_998],
        ],
        Some(Cvd::Deuteranopia) => [
            [0.367_322, 0.860_646, -0.227_968],
            [0.280_085, 0.672_501, 0.047_413],
            [-0.011_820, 0.042_940, 0.968_881],
        ],
        Some(Cvd::Tritanopia) => [
            [1.255_528, -0.076_749, -0.178_779],
            [-0.078_411, 0.930_809, 0.147_602],
            [0.004_733, 0.691_367, 0.303_900],
        ],
    };
    let l = [linear(c[0]), linear(c[1]), linear(c[2])];
    m.map(|row| (row[0] * l[0] + row[1] * l[1] + row[2] * l[2]).clamp(0.0, 1.0))
}

/// CIE L*a*b* of a linear RGB colour.
fn lab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.950_47;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.088_83;
    let f = |t: f32| {
        if t > 0.008_856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    [
        116.0 * f(y) - 16.0,
        500.0 * (f(x) - f(y)),
        200.0 * (f(y) - f(z)),
    ]
}

/// How far apart `a` and `b` look to someone with `cvd` (or normal sight),
/// as CIE76 ΔE: about 2 is just noticeable, 20 is plainly another colour.
pub fn difference(a: Rgba, b: Rgba, cvd: Option<Cvd>) -> f32 {
    let (a, b) = (lab(seen(a, cvd)), lab(seen(b, cvd)));
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Colours that must tell things apart differ by at least this much.
    const APART: f32 = 20.0;

    fn worst(colours: &[(&str, Rgba)], cvd: Option<Cvd>) -> (f32, String) {
        let mut worst = (f32::MAX, String::new());
        for (i, (a, ca)) in colours.iter().enumerate() {
            for (b, cb) in &colours[i + 1..] {
                let d = difference(*ca, *cb, cvd);
                if d < worst.0 {
                    worst = (d, format!("{a} and {b}"));
                }
            }
        }
        worst
    }

    #[test]
    fn colour_blind_machine_states_stay_apart_for_every_colour_blindness() {
        let states = [
            Status::Working,
            Status::Starved,
            Status::Blocked,
            Status::Idle,
        ]
        .map(|s| {
            (
                ["working", "starved", "blocked", "idle"][s as usize],
                status(s, true),
            )
        });
        for cvd in [
            None,
            Some(Cvd::Protanopia),
            Some(Cvd::Deuteranopia),
            Some(Cvd::Tritanopia),
        ] {
            let (d, which) = worst(&states, cvd);
            assert!(d >= APART, "{which} are only {d:.1} apart under {cvd:?}");
        }
    }

    #[test]
    fn colour_blind_ores_on_the_ground_stay_apart_for_every_colour_blindness() {
        let ores = [
            "iron-ore",
            "copper-ore",
            "stone",
            "coal",
            "bauxite",
            "titanium-ore",
            "rare-earth",
        ]
        .map(|k| (k, item_safe(k)));
        for cvd in [
            None,
            Some(Cvd::Protanopia),
            Some(Cvd::Deuteranopia),
            Some(Cvd::Tritanopia),
        ] {
            let (d, which) = worst(&ores, cvd);
            assert!(d >= APART, "{which} are only {d:.1} apart under {cvd:?}");
        }
        // Without the mode, two ores nearly match even for full colour sight.
        let plain = ores.map(|(k, _)| (k, item(k)));
        assert!(worst(&plain, Some(Cvd::Deuteranopia)).0 < APART);
    }

    #[test]
    fn simulation_keeps_grey_grey_and_merges_red_and_green_for_deuteranopes() {
        let grey = [0x80, 0x80, 0x80, 0xff];
        for cvd in CVDS {
            assert!(difference(grey, grey, Some(cvd)) < 0.01);
            let [r, g, b] = seen(grey, Some(cvd));
            assert!((r - g).abs() < 0.02 && (g - b).abs() < 0.02, "{cvd:?}");
        }
        let (red, green) = ([0xd0, 0x40, 0x40, 0xff], [0x50, 0x90, 0x40, 0xff]);
        assert!(
            difference(red, green, None) > 2.0 * difference(red, green, Some(Cvd::Deuteranopia))
        );
    }
}
