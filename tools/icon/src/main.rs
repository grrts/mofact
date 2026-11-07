//! `mofact-icon [ROOT]`: draw the app icon (`mofact_artkit::icon`, T68) at
//! every size the platforms and stores take, into the repository at ROOT
//! (default `.`). The files are committed; run this again after changing
//! the drawing. `--check` writes nothing and fails if a committed file is
//! missing or differs, which CI runs.

use mofact_artkit::icon::Layer;
use mofact_artkit::Canvas;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Where each icon goes: path from the repository root, size in pixels, layer.
const ICONS: &[(&str, u32, Layer)] = &[
    // Android launcher icons, for launchers without adaptive icons.
    (
        "platform/android/app/src/main/res/mipmap-mdpi/ic_launcher.png",
        48,
        Layer::Full,
    ),
    (
        "platform/android/app/src/main/res/mipmap-hdpi/ic_launcher.png",
        72,
        Layer::Full,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xhdpi/ic_launcher.png",
        96,
        Layer::Full,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xxhdpi/ic_launcher.png",
        144,
        Layer::Full,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xxxhdpi/ic_launcher.png",
        192,
        Layer::Full,
    ),
    // Android adaptive layers: 108 dp squares.
    (
        "platform/android/app/src/main/res/mipmap-mdpi/ic_launcher_foreground.png",
        108,
        Layer::Foreground,
    ),
    (
        "platform/android/app/src/main/res/mipmap-hdpi/ic_launcher_foreground.png",
        162,
        Layer::Foreground,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xhdpi/ic_launcher_foreground.png",
        216,
        Layer::Foreground,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xxhdpi/ic_launcher_foreground.png",
        324,
        Layer::Foreground,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xxxhdpi/ic_launcher_foreground.png",
        432,
        Layer::Foreground,
    ),
    (
        "platform/android/app/src/main/res/mipmap-mdpi/ic_launcher_background.png",
        108,
        Layer::Background,
    ),
    (
        "platform/android/app/src/main/res/mipmap-hdpi/ic_launcher_background.png",
        162,
        Layer::Background,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xhdpi/ic_launcher_background.png",
        216,
        Layer::Background,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xxhdpi/ic_launcher_background.png",
        324,
        Layer::Background,
    ),
    (
        "platform/android/app/src/main/res/mipmap-xxxhdpi/ic_launcher_background.png",
        432,
        Layer::Background,
    ),
    // iOS, named as Info.plist's CFBundleIcons finds them in the bundle.
    ("platform/ios/icons/AppIcon60x60@2x.png", 120, Layer::Full),
    ("platform/ios/icons/AppIcon60x60@3x.png", 180, Layer::Full),
    (
        "platform/ios/icons/AppIcon76x76@2x~ipad.png",
        152,
        Layer::Full,
    ),
    (
        "platform/ios/icons/AppIcon83.5x83.5@2x~ipad.png",
        167,
        Layer::Full,
    ),
    // The web link: tab, home screen and install icons.
    ("web/favicon.png", 32, Layer::Full),
    ("web/apple-touch-icon.png", 180, Layer::Full),
    ("web/icon-192.png", 192, Layer::Full),
    ("web/icon-512.png", 512, Layer::Full),
    // The stores (docs/store).
    ("docs/store/art/play-icon-512x512.png", 512, Layer::Full),
    (
        "docs/store/art/app-store-icon-1024x1024.png",
        1024,
        Layer::Full,
    ),
    (
        "docs/store/art/steam-community-icon-184x184.png",
        184,
        Layer::Full,
    ),
];

/// The Windows icon (Steam's client icon and the .exe), with these sizes inside.
const ICO: (&str, &[u32]) = ("platform/steam/mofact.ico", &[16, 24, 32, 48, 64, 256]);

fn png(size: u32, layer: Layer) -> Result<Vec<u8>, String> {
    let mut c = Canvas::new(size, size);
    mofact_artkit::icon::icon(&mut c, size as f32, layer);
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, size, size);
    // Store icons must be opaque; only the adaptive foreground has alpha.
    let rgba = c.rgba();
    if layer == Layer::Foreground {
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&rgba).map_err(|e| e.to_string())?;
    } else {
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut w = e.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// An .ico holding a PNG for each size, as Windows Vista and later read.
fn ico(sizes: &[u32]) -> Result<Vec<u8>, String> {
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&s| png(s, Layer::Full).map(|p| (s, p)))
        .collect::<Result<_, _>>()?;
    let n = images.len() as u16;
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&n.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(n);
    for (s, data) in &images {
        let side = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in images {
        out.extend_from_slice(&data);
    }
    Ok(out)
}

fn main() -> ExitCode {
    let mut root = PathBuf::from(".");
    let mut check = false;
    for a in std::env::args().skip(1) {
        if a == "--check" {
            check = true;
        } else {
            root = PathBuf::from(a);
        }
    }
    let mut files: Vec<(&str, Result<Vec<u8>, String>)> = ICONS
        .iter()
        .map(|&(path, size, layer)| (path, png(size, layer)))
        .collect();
    files.push((ICO.0, ico(ICO.1)));
    let mut bad = 0;
    for (path, bytes) in files {
        let bytes = match bytes {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let file = root.join(path);
        if check {
            if std::fs::read(&file).ok().as_deref() != Some(&bytes[..]) {
                eprintln!("{path} is out of date: run `cargo run -p mofact-icon`");
                bad += 1;
            }
        } else if let Err(e) = write(&file, &bytes) {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        } else {
            println!("{path}");
        }
    }
    if bad > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn write(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, bytes)
}
