//! Art kit: buildings, belts and items drawn in code with tiny-skia (R71)
//! and baked into atlases, using one fixed palette (R68).
//!
//! [`draw`] is the prototype's canvas drawing code, ported call for call onto
//! the small [`Canvas`] here. The golden images in `golden/` were rendered by
//! the prototype's own code in a browser (`tools/golden/prototype.cjs`), and
//! the tests check that the port draws the same pictures at close zoom.

pub mod canvas;
pub mod draw;
pub mod palette;

pub use canvas::Canvas;
pub use draw::{Pose, Shape};
pub use mofact_core as core;

#[cfg(test)]
mod golden;
