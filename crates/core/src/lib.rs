//! Contracts between Mofact's lanes.
//!
//! Every type another lane depends on lives here: ids, tile geometry, the
//! commands the main thread sends to the simulation, the snapshots the
//! simulation publishes back, and the schema of the content files in
//! `data/`. A change to this crate is a task of its own, so one lane can't
//! silently break another.

pub mod command;
pub mod content;
pub mod geom;
pub mod ids;
pub mod snapshot;

/// The simulation steps at a fixed rate, independent of frame rate (R43).
pub const TICKS_PER_SECOND: u32 = 60;

/// Length of one simulation tick in seconds.
pub const TICK_SECONDS: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// Side length of a world chunk, in tiles.
pub const CHUNK_SIZE: i32 = 64;

/// A simulation tick count since the world was created.
pub type Tick = u64;
