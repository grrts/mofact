//! The game's sound (T41), made rather than recorded: every hum and cue is
//! synthesised here, so the web build ships no audio files and every
//! platform sounds the same.
//!
//! - [`Synth`] renders interleaved stereo `f32` samples. It holds one loop
//!   per kind of machine ([`Hum`]), each with a level and a pan, and plays
//!   one-shot [`Cue`]s over them.
//! - [`Soundscape`] turns what's on screen into those levels and cues: the
//!   more working drills in view, the louder the drills, panned to where
//!   they are; a machine in view that stalls plays a cue for how it stalled.
//! - [`Audio`] is the synth behind a lock, shared between the game, which
//!   sets levels each frame, and the platform's audio callback, which pulls
//!   samples (SDL's audio thread, or Web Audio on the page's thread).
//!
//! Nothing here knows about the simulation; the game describes each machine
//! as a [`Machine`].

mod scape;
mod synth;

pub use scape::{Machine, Soundscape, State};
pub use synth::{Cue, Hum, Synth, HUMS};

use std::sync::{Arc, Mutex, MutexGuard};

/// Samples a second, per channel. Platforms resample if they must.
pub const RATE: u32 = 48_000;

/// The synth, shared between the game and the platform's audio callback.
#[derive(Clone)]
pub struct Audio(Arc<Mutex<Synth>>);

impl Default for Audio {
    fn default() -> Self {
        Self::new(RATE)
    }
}

impl Audio {
    pub fn new(rate: u32) -> Self {
        Self(Arc::new(Mutex::new(Synth::new(rate))))
    }

    /// The synth, to set levels or play cues. A panic on the audio thread
    /// mid-render leaves it usable: the next render starts clean.
    pub fn lock(&self) -> MutexGuard<'_, Synth> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Fill `out` with interleaved stereo samples.
    pub fn render(&self, out: &mut [f32]) {
        self.lock().render(out);
    }
}
