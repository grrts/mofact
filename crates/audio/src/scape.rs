//! What the player hears of the factory: the machines in view set the hums,
//! and a machine in view that stalls plays a cue.

use crate::synth::{Cue, Hum, Synth, HUMS};
use std::collections::HashMap;

/// What a machine is doing, as far as sound goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Starved,
    Blocked,
    NoPower,
}

/// One machine in or near view.
#[derive(Clone, Copy, Debug)]
pub struct Machine {
    /// Stays the same while the machine exists.
    pub id: u32,
    /// What it sounds like while working; none for silent things.
    pub hum: Option<Hum>,
    pub state: State,
    /// Where it is across the screen, -1 (left edge) to 1 (right edge).
    pub pan: f32,
    /// How much it counts: 1 at the middle of the view, falling to 0 at
    /// its edges and beyond.
    pub near: f32,
}

/// Stall cues come no closer together than this, in seconds, so a whole
/// grid browning out is one sound, not fifty.
const CUE_GAP: f32 = 0.45;

/// Turns the machines in view into hum levels and stall cues.
#[derive(Default)]
pub struct Soundscape {
    last: HashMap<u32, State>,
    /// Seconds since the last stall cue.
    quiet: f32,
}

impl Soundscape {
    pub fn new() -> Self {
        Self {
            last: HashMap::new(),
            quiet: CUE_GAP,
        }
    }

    /// One frame, `dt` seconds after the last: set every hum on `synth`
    /// from the working machines, and play a cue for one that just stalled.
    pub fn update(
        &mut self,
        synth: &mut Synth,
        dt: f32,
        machines: impl IntoIterator<Item = Machine>,
    ) {
        self.quiet += dt;
        let mut weight = [0.0f32; HUMS.len()];
        let mut pan = [0.0f32; HUMS.len()];
        let mut seen = HashMap::with_capacity(self.last.len());
        let mut stall: Option<(Cue, f32, f32)> = None;
        for m in machines {
            let near = m.near.clamp(0.0, 1.0);
            if near <= 0.0 {
                continue;
            }
            if let (Some(hum), State::Working) = (m.hum, m.state) {
                let i = HUMS.iter().position(|&h| h == hum).unwrap_or(0);
                weight[i] += near;
                pan[i] += near * m.pan;
            }
            // Only a machine seen working, now stopped, cues: not one that
            // comes into view already stuck, or one just built.
            let was = self.last.get(&m.id).copied();
            let cue = match (was, m.state) {
                (Some(State::Working), State::Blocked) => Some(Cue::Blocked),
                (Some(State::Working), State::Starved) => Some(Cue::Starved),
                (Some(State::Working), State::NoPower) => Some(Cue::NoPower),
                _ => None,
            };
            if let Some(cue) = cue {
                // The nearest stall speaks for the frame.
                if stall.is_none_or(|s| near > s.2) {
                    stall = Some((cue, m.pan, near));
                }
            }
            seen.insert(m.id, m.state);
        }
        self.last = seen;
        for (i, &hum) in HUMS.iter().enumerate() {
            let w = weight[i];
            // One machine is clearly there; a dozen is a wall of sound,
            // never louder than the hum at full.
            let level = 1.0 - (-w / 3.0).exp();
            let p = if w > 0.0 { pan[i] / w * 0.7 } else { 0.0 };
            synth.set(hum, level, p);
        }
        if let Some((cue, p, _)) = stall {
            if self.quiet >= CUE_GAP {
                synth.play(cue, p * 0.7);
                self.quiet = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: u32, hum: Hum, state: State, near: f32) -> Machine {
        Machine {
            id,
            hum: Some(hum),
            state,
            pan: 0.5,
            near,
        }
    }

    #[test]
    fn more_machines_hum_louder_but_never_past_full() {
        let mut synth = Synth::new(48_000);
        let mut scape = Soundscape::new();
        scape.update(&mut synth, 0.016, [m(1, Hum::Drill, State::Working, 1.0)]);
        let one = synth.level(Hum::Drill);
        let many = (0..40).map(|i| m(i, Hum::Drill, State::Working, 1.0));
        scape.update(&mut synth, 0.016, many);
        let lots = synth.level(Hum::Drill);
        assert!(one > 0.2 && one < lots && lots <= 1.0, "{one} {lots}");
        // Idle machines and ones out of view are quiet.
        scape.update(
            &mut synth,
            0.016,
            [
                m(1, Hum::Drill, State::Idle, 1.0),
                m(2, Hum::Fire, State::Working, 0.0),
            ],
        );
        assert_eq!(synth.level(Hum::Drill), 0.0);
        assert_eq!(synth.level(Hum::Fire), 0.0);
    }

    #[test]
    fn a_machine_that_stalls_in_view_cues_once() {
        let mut synth = Synth::new(48_000);
        let mut scape = Soundscape::new();
        scape.update(&mut synth, 0.5, [m(1, Hum::Assembler, State::Working, 1.0)]);
        assert_eq!(synth.playing(), 0);
        scape.update(&mut synth, 0.5, [m(1, Hum::Assembler, State::Blocked, 1.0)]);
        assert_eq!(synth.playing(), 1);
        scape.update(&mut synth, 0.5, [m(1, Hum::Assembler, State::Blocked, 1.0)]);
        assert_eq!(synth.playing(), 1, "still blocked: no new cue");
    }

    #[test]
    fn a_crowd_stalling_at_once_is_one_cue() {
        let mut synth = Synth::new(48_000);
        let mut scape = Soundscape::new();
        let all = |state| (0..30).map(move |i| m(i, Hum::Assembler, state, 1.0));
        scape.update(&mut synth, 0.5, all(State::Working));
        scape.update(&mut synth, 0.016, all(State::NoPower));
        assert_eq!(synth.playing(), 1);
        // And the next stall soon after waits its turn.
        scape.update(&mut synth, 0.016, all(State::Working));
        scape.update(&mut synth, 0.016, all(State::Starved));
        assert_eq!(synth.playing(), 1);
    }

    #[test]
    fn coming_into_view_stuck_is_silent() {
        let mut synth = Synth::new(48_000);
        let mut scape = Soundscape::new();
        scape.update(&mut synth, 0.5, [m(1, Hum::Drill, State::Working, 0.0)]);
        scape.update(&mut synth, 0.5, [m(1, Hum::Drill, State::Starved, 1.0)]);
        assert_eq!(synth.playing(), 0);
    }
}
