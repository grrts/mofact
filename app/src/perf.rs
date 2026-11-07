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
