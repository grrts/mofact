//! The port against the prototype: each sprite drawn here must match the
//! prototype's own drawing of it (rendered by `tools/golden/prototype.cjs`)
//! at 64 pixels a tile, the close zoom.
//!
//! Antialiasing differs a little between tiny-skia and a browser's canvas, so
//! the check allows small differences along edges, but not a missing or
//! misplaced part: on the factory floor, the average difference stays under
//! 2 levels in 255, fewer than 1% of pixels differ by more than 48,
//! and no 4×4 block differs on average by more than 12, so a missing bolt
//! or rivet fails.

use crate::draw::{self, Pose, Shape};
use crate::palette::{self as pal, Rgba};
use crate::Canvas;
use std::path::{Path, PathBuf};

const PX: f32 = 64.0;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("golden")
}

fn load(name: &str) -> (u32, u32, Vec<u8>) {
    let path = golden_dir().join(format!("{name}.png"));
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut decoder = png::Decoder::new(file);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().expect("a PNG");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("decodes");
    assert_eq!(info.color_type, png::ColorType::Rgba, "{name}: RGBA");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// `rgba` laid over the factory floor.
fn on_ground(rgba: &[u8]) -> Vec<f32> {
    rgba.chunks(4)
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            (0..3).map(move |i| p[i] as f32 * a + pal::GROUND[i] as f32 * (1.0 - a))
        })
        .collect()
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("writable");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(rgba))
        .expect("writes");
}

/// The largest average difference over any 4×4 block of pixels.
fn blocks(diffs: &[f32], w: usize, h: usize) -> f32 {
    let mut worst = 0.0f32;
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(4) {
            let mut sum = 0.0;
            let mut n = 0;
            for y in by..(by + 4).min(h) {
                for x in bx..(bx + 4).min(w) {
                    sum += diffs[(y * w + x) * 3..][..3].iter().sum::<f32>() / 3.0;
                    n += 1;
                }
            }
            worst = worst.max(sum / n as f32);
        }
    }
    worst
}

fn check(name: &str, paint: impl Fn(&mut Canvas)) {
    let (w, h, want) = load(name);
    let mut c = Canvas::new(w, h);
    paint(&mut c);
    let got = c.rgba();
    let (a, b) = (on_ground(&want), on_ground(&got));
    let diffs: Vec<f32> = a.iter().zip(&b).map(|(x, y)| (x - y).abs()).collect();
    let mean = diffs.iter().sum::<f32>() / diffs.len() as f32;
    let bad = diffs
        .chunks(3)
        .filter(|d| d.iter().any(|&v| v > 48.0))
        .count();
    let share = bad as f32 / (w * h) as f32;
    let block = blocks(&diffs, w as usize, h as usize);
    if mean >= 2.0 || share >= 0.01 || block > 12.0 {
        // Leave the port's drawing next to the build for a look.
        let dir = std::env::temp_dir();
        write_png(&dir.join(format!("{name}.port.png")), w, h, &got);
        panic!(
            "{name}: mean difference {mean:.2}, {:.2}% of pixels off, worst block {block:.1}; see {}",
            share * 100.0,
            dir.join(format!("{name}.port.png")).display()
        );
    }
}

const IRON_ORE: Rgba = [0x4f, 0x6a, 0x86, 0xff];
const COPPER_ORE: Rgba = [0x9a, 0x55, 0x30, 0xff];
const IRON: Rgba = [0x8f, 0xb0, 0xd1, 0xff];
const COPPER: Rgba = [0xf0, 0x8a, 0x4b, 0xff];
const CIRCUIT: Rgba = [0x63, 0xd4, 0x8a, 0xff];

fn still() -> Pose {
    Pose::default()
}

#[test]
fn belts_match_the_prototype() {
    check("belt", |c| draw::belt(c, PX, true, 0.0, 2.0));
    check("belt-mid", |c| draw::belt(c, PX, false, 0.0, 2.0));
}

#[test]
fn drills_match_the_prototype() {
    check("drill", |c| draw::drill(c, PX, IRON_ORE, true, &still()));
    check("drill-copper", |c| {
        draw::drill(c, PX, COPPER_ORE, true, &still())
    });
}

#[test]
fn furnaces_match_the_prototype() {
    check("furnace", |c| draw::furnace(c, PX, None, true, &still()));
    let lit = Pose {
        working: true,
        ..still()
    };
    // The prototype's flame flickers with the entity id; at id 1 and t = 0,
    // sin(1) sets its size.
    let flicker = Pose {
        t: 1.0 / 11.0,
        ..lit
    };
    check("furnace-iron", |c| {
        draw::furnace(c, PX, Some(IRON), true, &flicker)
    });
}

#[test]
fn assemblers_match_the_prototype() {
    check("assembler-gear", |c| {
        draw::assembler(c, PX, Some((Shape::Gear, IRON)), true, &still())
    });
    check("assembler-circuit", |c| {
        draw::assembler(c, PX, Some((Shape::Circuit, CIRCUIT)), true, &still())
    });
}

#[test]
fn the_hub_matches_the_prototype() {
    check("hub", |c| draw::hub(c, PX, true, 0.0));
}

#[test]
fn items_match_the_prototype() {
    let items = [
        ("iron-ore", Shape::Ore, IRON_ORE),
        ("iron-plate", Shape::Plate, IRON),
        ("gear", Shape::Gear, IRON),
        ("wire", Shape::Wire, COPPER),
        ("circuit", Shape::Circuit, CIRCUIT),
    ];
    for (name, shape, color) in items {
        check(&format!("item-{name}"), |c| {
            draw::item(c, shape, color, PX / 2.0, PX / 2.0, PX * 0.6, false)
        });
    }
}

#[test]
fn a_machine_turns_its_output_with_its_direction() {
    let draw_facing = |dir| {
        let mut c = Canvas::new(64, 64);
        draw::furnace(&mut c, PX, None, true, &Pose { dir, ..still() });
        c.rgba()
    };
    let px = |img: &[u8], x: usize, y: usize| img[(y * 64 + x) * 4..][..4].to_vec();
    // The output notch is yellow on the side the machine faces.
    let (east, south) = (draw_facing(0), draw_facing(1));
    assert_eq!(px(&east, 61, 32)[..3], pal::ACCENT[..3]);
    assert_eq!(px(&south, 32, 61)[..3], pal::ACCENT[..3]);
    assert_ne!(px(&south, 61, 32)[..3], pal::ACCENT[..3]);
}
