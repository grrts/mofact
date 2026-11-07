//! Frame cost on the main thread (T65): building the scene, its animations
//! and the HUD for the R44 budget base on a phone-sized screen, at four
//! zooms. Run with `cargo run --release -p mofact-bench --example frame`.

use mofact_bench::build;
use mofact_render::{Camera, Look, Overlay, Scene};
use mofact_ui::{Hud, View};
use std::time::Instant;

fn main() {
    let content = mofact_content::load_dir(std::path::Path::new("data")).unwrap();
    let mut sim = build(&content, "budget", 200).unwrap();
    for _ in 0..120 {
        sim.step();
    }
    let prev = sim.snapshot();
    sim.step();
    let latest = sim.snapshot();
    let look = Look::new(&content);
    let atlases = look.atlases();
    let mut scene = Scene::default();
    let mut overlay = Overlay::new(&look);
    let mut hud = Hud::new();
    let size = (1179u32, 2556u32);
    let tool = mofact_input::Tool::Hand;
    let belt: mofact_core::ids::BuildingId = "belt".into();
    for ppt in [96.0f32, 48.0, 24.0, 6.0] {
        let camera = Camera {
            center: [0.0, 0.0],
            pixels_per_tile: ppt,
        };
        let n = 30;
        let (mut b, mut a, mut h) = (0.0, 0.0, 0.0);
        for _ in 0..n {
            let t = Instant::now();
            scene.build(&look, &atlases, &camera, size, &prev, &latest, 0.5);
            b += t.elapsed().as_secs_f64();
            let t = Instant::now();
            scene.animate(&look, &atlases, &camera, size, &latest, 1.0);
            a += t.elapsed().as_secs_f64();
            let t = Instant::now();
            overlay.clear();
            let view = View {
                size,
                scale: 3.0,
                content: &content,
                look: &look,
                snapshot: &latest,
                tool: &tool,
                dir: mofact_core::geom::Dir::East,
                recipe: None,
                belt: &belt,
                history: (false, false),
                safe: [0.0; 4],
                nudge: None,
                slots: &[],
                store: &mofact_ui::StoreView::default(),
                crash: false,
                selection: 0,
                blueprints: &[],
            };
            hud.draw(&view, &mut overlay);
            h += t.elapsed().as_secs_f64();
        }
        let f = 1000.0 / n as f64;
        println!(
            "ppt {ppt}: build {:.2} ms, animate {:.2} ms, hud {:.2} ms, instances {}",
            b * f,
            a * f,
            h * f,
            scene.instances.len()
        );
    }
    let t = Instant::now();
    for _ in 0..30 {
        std::hint::black_box(sim.snapshot());
    }
    println!(
        "snapshot {:.2} ms, entities {}",
        t.elapsed().as_secs_f64() * 1000.0 / 30.0,
        latest.entities.len()
    );
}
