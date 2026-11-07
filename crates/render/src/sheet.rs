//! The silhouette sheet (T39): every building drawn 20 pixels across, in
//! colour and as a bare black shape, so you can check each one can be told
//! from the others at the size it has in the build menu and zoomed out.
//!
//! `cargo test -p mofact-render sheet -- --ignored` writes the sheet to
//! `target/silhouettes.png`; the other test checks the shapes stay apart.

use crate::look::Look;
use crate::palette::Rgba;
use mofact_artkit::palette as pal;
use mofact_core::content::Content;
use mofact_core::ids::BuildingId;

/// How many pixels across a building's icon is, the build menu's size.
pub const ICON: u32 = 20;

/// `building` as an icon `ICON` pixels across its longer side, on a square
/// `ICON`×`ICON` canvas, centred.
fn icon(look: &Look, building: &BuildingId) -> Vec<Rgba> {
    let (w, h) = look.size(building).unwrap_or((1, 1));
    let tiles = u32::from(w.max(h));
    // Render big and box-filter down, as the GPU's mipmapped atlas would.
    let up = 8;
    let px = (ICON * up).div_ceil(tiles);
    let (pw, ph, rgba) = look.picture(building, px).expect("a picture");
    let side = ICON * up;
    let mut out = vec![[0u8; 4]; (ICON * ICON) as usize];
    let ox = (side as i64 - pw as i64) / 2;
    let oy = (side as i64 - ph as i64) / 2;
    for y in 0..ICON {
        for x in 0..ICON {
            let mut acc = [0f32; 4];
            for sy in 0..up {
                for sx in 0..up {
                    let px = (x * up + sx) as i64 - ox;
                    let py = (y * up + sy) as i64 - oy;
                    if px < 0 || py < 0 || px >= pw as i64 || py >= ph as i64 {
                        continue;
                    }
                    let i = ((py as u32 * pw + px as u32) * 4) as usize;
                    let a = f32::from(rgba[i + 3]) / 255.0;
                    for (k, v) in acc.iter_mut().enumerate().take(3) {
                        *v += f32::from(rgba[i + k]) * a;
                    }
                    acc[3] += a;
                }
            }
            let n = (up * up) as f32;
            let a = acc[3] / n;
            let c = |k: usize| {
                if acc[3] > 0.0 {
                    (acc[k] / acc[3]).round() as u8
                } else {
                    0
                }
            };
            out[(y * ICON + x) as usize] = [c(0), c(1), c(2), (a * 255.0).round() as u8];
        }
    }
    out
}

/// The shape of an icon: how covered each pixel is, 0 to 1.
fn mask(icon: &[Rgba]) -> Vec<f32> {
    icon.iter().map(|p| f32::from(p[3]) / 255.0).collect()
}

/// An icon laid over the factory floor, in 0–255 per channel.
fn on_ground(icon: &[Rgba]) -> Vec<[f32; 3]> {
    icon.iter()
        .map(|p| {
            let a = f32::from(p[3]) / 255.0;
            let mix = |k: usize| f32::from(p[k]) * a + f32::from(pal::GROUND[k]) * (1.0 - a);
            [mix(0), mix(1), mix(2)]
        })
        .collect()
}

/// How far apart two icons look: the share of pixels whose shape differs,
/// and the average colour difference on the floor, 0 to 1.
fn apart(a: &[Rgba], b: &[Rgba]) -> (f32, f32) {
    let (ma, mb) = (mask(a), mask(b));
    let shape = ma.iter().zip(&mb).map(|(x, y)| (x - y).abs()).sum::<f32>() / ma.len() as f32;
    let (ga, gb) = (on_ground(a), on_ground(b));
    let colour = ga
        .iter()
        .zip(&gb)
        .map(|(x, y)| (0..3).map(|k| (x[k] - y[k]).abs()).sum::<f32>() / (3.0 * 255.0))
        .sum::<f32>()
        / ga.len() as f32;
    (shape, colour)
}

fn content() -> Content {
    crate::look::tests::content()
}

#[test]
fn every_building_is_told_apart_at_twenty_pixels() {
    let content = content();
    let look = Look::new(&content);
    let icons: Vec<(&BuildingId, Vec<Rgba>)> = content
        .buildings
        .iter()
        .map(|b| (&b.id, icon(&look, &b.id)))
        .collect();
    let mut close = Vec::new();
    for (i, (a, ia)) in icons.iter().enumerate() {
        for (b, ib) in &icons[i + 1..] {
            let (shape, colour) = apart(ia, ib);
            // Told apart by shape, or failing that clearly by colour. Belts
            // are all belts; each speed only has its own colour of rails.
            let family = look.is_belt(a) && look.is_belt(b);
            let alike = if family {
                colour < 0.03
            } else {
                shape < 0.06 && colour < 0.05
            };
            if alike {
                close.push(format!(
                    "{} and {}: shape {shape:.3}, colour {colour:.3}",
                    a.as_str(),
                    b.as_str()
                ));
            }
        }
    }
    assert!(
        close.is_empty(),
        "too alike at {ICON} px:\n{}",
        close.join("\n")
    );
}

/// Writes the sheet: for each building its name, its icon at 20 pixels
/// (shown four times over, pixel for pixel), the same as a black shape, and
/// the icon at its real size.
#[test]
#[ignore = "writes target/silhouettes.png"]
fn sheet() {
    let content = content();
    let look = Look::new(&content);
    let font = fontdue::Font::from_bytes(
        include_bytes!("../../../assets/fonts/SairaSemiCondensed-SemiBold.ttf") as &[u8],
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let zoom = 4;
    let big = ICON * zoom;
    let (cell_w, cell_h) = (2 * big + ICON + 48, big + 40);
    let cols = 4;
    let n = content.buildings.len() as u32;
    let rows = n.div_ceil(cols);
    let (w, h) = (cols * cell_w + 16, rows * cell_h + 16);
    let mut img = vec![[0x0e, 0x12, 0x16, 0xff]; (w * h) as usize];
    let put = |img: &mut Vec<Rgba>, x: u32, y: u32, c: Rgba| {
        if x < w && y < h {
            let d = &mut img[(y * w + x) as usize];
            let a = f32::from(c[3]) / 255.0;
            for k in 0..3 {
                d[k] = (f32::from(c[k]) * a + f32::from(d[k]) * (1.0 - a)).round() as u8;
            }
        }
    };
    for (i, b) in content.buildings.iter().enumerate() {
        let (cx, cy) = (
            8 + (i as u32 % cols) * cell_w,
            8 + (i as u32 / cols) * cell_h,
        );
        let px = icon(&look, &b.id);
        for y in 0..big {
            for x in 0..big {
                let p = px[((y / zoom) * ICON + x / zoom) as usize];
                put(&mut img, cx + x, cy + y, pal::GROUND);
                put(&mut img, cx + x, cy + y, p);
                put(&mut img, cx + big + 8 + x, cy + y, [0xe9, 0xed, 0xf0, 0xff]);
                put(&mut img, cx + big + 8 + x, cy + y, [0, 0, 0, p[3]]);
            }
        }
        let (rx, ry) = (cx + 2 * big + 16, cy + (big - ICON) / 2);
        for y in 0..ICON {
            for x in 0..ICON {
                put(&mut img, rx + x, ry + y, pal::GROUND);
                put(&mut img, rx + x, ry + y, px[(y * ICON + x) as usize]);
            }
        }
        // The name under it.
        let mut pen = cx as f32;
        for ch in b.name.chars() {
            let (m, bitmap) = font.rasterize(ch, 15.0);
            let top = cy as i32 + big as i32 + 26 - m.height as i32 - m.ymin;
            for gy in 0..m.height {
                for gx in 0..m.width {
                    let a = bitmap[gy * m.width + gx];
                    let (x, y) = (pen as i32 + m.xmin + gx as i32, top + gy as i32);
                    if x >= 0 && y >= 0 {
                        put(&mut img, x as u32, y as u32, [0xe9, 0xed, 0xf0, a]);
                    }
                }
            }
            pen += m.advance_width;
        }
    }
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/silhouettes.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let flat: Vec<u8> = img.into_iter().flatten().collect();
    enc.write_header().unwrap().write_image_data(&flat).unwrap();
    println!("wrote {}", path.display());
}

/// Writes the buildings at close zoom, 96 pixels a tile, to
/// `target/closeups.png`, to check their detail.
#[test]
#[ignore = "writes target/closeups.png"]
fn closeups() {
    let content = content();
    let look = Look::new(&content);
    let px = 96;
    // Room for the largest building.
    let tiles = content
        .buildings
        .iter()
        .map(|b| b.size.0.max(b.size.1))
        .max()
        .unwrap_or(1);
    let cell = u32::from(tiles) * px + 12;
    let cols = 6;
    let n = content.buildings.len() as u32;
    let (w, h) = (cols * cell, n.div_ceil(cols) * cell);
    let mut img = vec![pal::GROUND; (w * h) as usize];
    for (i, b) in content.buildings.iter().enumerate() {
        let (ox, oy) = ((i as u32 % cols) * cell + 6, (i as u32 / cols) * cell + 6);
        let (pw, ph, rgba) = look.picture(&b.id, px).unwrap();
        for y in 0..ph {
            for x in 0..pw {
                let s = ((y * pw + x) * 4) as usize;
                let a = f32::from(rgba[s + 3]) / 255.0;
                let d = &mut img[((oy + y) * w + ox + x) as usize];
                for k in 0..3 {
                    d[k] = (f32::from(rgba[s + k]) * a + f32::from(d[k]) * (1.0 - a)).round() as u8;
                }
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/closeups.png");
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let flat: Vec<u8> = img.into_iter().flatten().collect();
    enc.write_header().unwrap().write_image_data(&flat).unwrap();
}
