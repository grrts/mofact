//! Frame cost on a big factory (T65): the parts of a frame that run on the
//! main thread besides drawing, timed on the R44 budget base. Run with
//! `cargo test --release -p mofact perf -- --ignored --nocapture`.

use crate::sound::Sound;
use mofact_render::{Camera, Look, Overlay};
use std::time::Instant;

fn ms(t: Instant, n: u32) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0 / f64::from(n)
}

#[test]
#[ignore = "a benchmark: run in release with --ignored --nocapture"]
fn frame_parts_on_the_budget_base() {
    let content = crate::content().unwrap();
    let mut sim = mofact_bench::build(&content, "budget", 200).unwrap();
    for _ in 0..120 {
        sim.step();
    }
    let snap = sim.snapshot();
    let look = Look::new(&content);
    let mut overlay = Overlay::new(&look);
    let size = (1179, 2556);
    let n = 30;
    let mut sound = Sound::new(&content);
    for ppt in [96.0, 24.0] {
        let camera = Camera {
            center: [0.0, 0.0],
            pixels_per_tile: ppt,
        };
        let t = Instant::now();
        for _ in 0..n {
            sound.frame(&snap, &camera, size);
        }
        println!("sound at {ppt}: {:.3} ms", ms(t, n));
    }
    let camera = Camera {
        center: [0.0, 0.0],
        pixels_per_tile: 3.0,
    };
    let t = Instant::now();
    for _ in 0..n {
        overlay.clear();
        crate::worldmap::factory(&mut overlay, &content, &look, &camera, size, 3.0, &snap);
    }
    println!("map factory: {:.3} ms", ms(t, n));
    println!(
        "entities {}, belt items {}",
        snap.entities.len(),
        snap.belt_items.len()
    );
}

/// GPU time for a whole frame of the budget base on a budget phone's screen
/// (720 × 1600), with and without low-power mode. On a software adapter the
/// numbers are slow but scale with the shader work, so they compare.
#[test]
#[ignore = "a benchmark: run in release with --ignored --nocapture"]
fn gpu_frame_on_the_budget_base() {
    use mofact_render::{Gpu, Renderer, Scene};
    let content = crate::content().unwrap();
    let mut sim = mofact_bench::build(&content, "budget", 200).unwrap();
    for _ in 0..120 {
        sim.step();
    }
    let snap = sim.snapshot();
    let look = Look::new(&content);
    let Ok(gpu) = Gpu::headless() else {
        println!("no GPU adapter");
        return;
    };
    println!("adapter: {:?}", gpu.adapter.get_info().name);
    let mut renderer = Renderer::new(&gpu, look.atlases());
    let mut ore = crate::ore::OreMap::new(&content, 42, Vec::new());
    let size = (720, 1600);
    let clear = mofact_render::FLOOR;
    for ppt in [48.0, 24.0, 8.0] {
        for low in [false, true] {
            let camera = Camera {
                center: [0.0, 0.0],
                pixels_per_tile: ppt,
            };
            let mut scene = Scene::default();
            scene.low_power = low;
            let atlases = renderer.atlases();
            scene.build(&look, atlases, &camera, size, &snap, &snap, 0.5);
            scene.animate(&look, atlases, &camera, size, &snap, 1.0);
            ore.fill(&camera, size);
            scene.ore(&look, atlases, &camera, size, ore.tiles());
            scene.decor(atlases, &camera, size, ore.decor(&camera, size));
            scene.terrain(&camera, size, ore.version(), |t, s| ore.terrain(t, s));
            // Once to warm up, then timed.
            renderer.capture(&gpu, size, &camera, &scene, None, clear);
            let n = 5;
            let t = Instant::now();
            for _ in 0..n {
                renderer.capture(&gpu, size, &camera, &scene, None, clear);
            }
            println!(
                "ppt {ppt} low power {low}: {:.1} ms a frame, {} sprites",
                ms(t, n),
                scene.instances.len() + scene.ore.len()
            );
        }
    }
}
