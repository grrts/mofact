//! Render lane: wgpu renderer that draws the latest simulation snapshot with instanced batches, texture atlases per zoom band and smoothing between ticks.
//!
//! Skeleton from task T01. The renderer core arrives in T16.

pub use mofact_core as core;

use mofact_artkit as _;
