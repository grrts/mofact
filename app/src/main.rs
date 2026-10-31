//! The Mofact app.
//!
//! Skeleton from task T01. The SDL3 + wgpu shell with suspend and resume on
//! iOS, Android and Windows arrives in T03; until then this runs the
//! simulation for one second with no window, to prove the lanes link.

use mofact_input as _;
use mofact_render as _;
use mofact_ui as _;
use mofact_world as _;

fn main() {
    let mut sim = mofact_sim::Sim::new();
    for _ in 0..mofact_core::TICKS_PER_SECOND {
        sim.step();
    }
    println!(
        "mofact {}: ran {} ticks headless (no window until T03)",
        env!("CARGO_PKG_VERSION"),
        sim.tick()
    );
}
