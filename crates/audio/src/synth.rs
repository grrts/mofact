//! The synthesiser: a loop per kind of machine and one-shot cues, all made
//! from sines, filtered noise and envelopes.

use std::f32::consts::{FRAC_PI_4, TAU};

/// A kind of machine sound, one loop each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hum {
    /// Belts carrying items: a low rumble with a soft tick of rollers.
    Belt,
    /// Drills: a steady thump.
    Drill,
    /// Furnaces, boilers and refineries' heaters: a crackling fire.
    Fire,
    /// Assemblers: a click-clack over a motor's hum.
    Assembler,
    /// Labs: a soft shimmer.
    Lab,
    /// Chemical plants: bubbling.
    Chemistry,
    /// Steam engines: a chuff.
    Engine,
    /// Pumps and wells: a wobbling hum.
    Pump,
    /// Wind turbines: wind.
    Wind,
}

/// Every hum, in the order [`Synth`] holds them.
pub const HUMS: [Hum; 9] = [
    Hum::Belt,
    Hum::Drill,
    Hum::Fire,
    Hum::Assembler,
    Hum::Lab,
    Hum::Chemistry,
    Hum::Engine,
    Hum::Pump,
    Hum::Wind,
];

impl Hum {
    fn index(self) -> usize {
        HUMS.iter().position(|&h| h == self).unwrap_or(0)
    }
}

/// A one-shot sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Something was built: a wooden thock.
    Place,
    /// Something was taken away: a rising pluck.
    Remove,
    /// The game said no: a short low buzz.
    Refused,
    /// A machine's output is full: a two-note clunk down.
    Blocked,
    /// A machine ran out of input: a falling boop.
    Starved,
    /// A machine lost power: a power-down sweep.
    NoPower,
    /// A goal was met: a bright three-note chime.
    Goal,
    /// Research finished: a longer four-note chime.
    Research,
    /// A button was pressed: a tiny click.
    Tap,
}

impl Cue {
    /// How long it sounds, in seconds.
    pub fn seconds(self) -> f32 {
        match self {
            Cue::Place => 0.16,
            Cue::Remove => 0.14,
            Cue::Refused => 0.16,
            Cue::Blocked => 0.24,
            Cue::Starved => 0.28,
            Cue::NoPower => 0.4,
            Cue::Goal => 1.1,
            Cue::Research => 1.6,
            Cue::Tap => 0.03,
        }
    }
}

/// A one-pole low-pass filter.
#[derive(Clone, Copy, Default)]
struct LowPass(f32);

impl LowPass {
    fn run(&mut self, x: f32, a: f32) -> f32 {
        self.0 += a * (x - self.0);
        self.0
    }
}

#[derive(Clone, Copy, Default)]
struct Loop {
    level: f32,
    target: f32,
    pan: f32,
    pan_target: f32,
    /// Seconds it has been sounding.
    t: f32,
    lp: [LowPass; 2],
    /// A random event's envelope (a crackle, a bubble) and its age.
    burst: f32,
    age: f32,
}

#[derive(Clone, Copy)]
struct Voice {
    cue: Cue,
    t: f32,
    pan: f32,
    phase: f32,
}

/// Interleaved stereo synthesis of machine hums and cues.
pub struct Synth {
    rate: f32,
    dt: f32,
    loops: [Loop; HUMS.len()],
    voices: Vec<Voice>,
    seed: u32,
    master: f32,
    muted: bool,
    /// How fast levels glide, per sample.
    glide: f32,
}

/// Cues sounding at once, at most; the oldest gives way.
const VOICES: usize = 12;

impl Synth {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(8000) as f32;
        Self {
            rate,
            dt: 1.0 / rate,
            loops: [Loop::default(); HUMS.len()],
            voices: Vec::with_capacity(VOICES),
            seed: 0x9e37_79b9,
            master: 0.7,
            muted: false,
            glide: 1.0 - (-1.0 / (0.2 * rate)).exp(),
        }
    }

    pub fn rate(&self) -> u32 {
        self.rate as u32
    }

    /// Set a hum's loudness (0 to 1) and where it sits (-1 left to 1 right).
    /// Both glide there over a fifth of a second.
    pub fn set(&mut self, hum: Hum, level: f32, pan: f32) {
        let l = &mut self.loops[hum.index()];
        l.target = level.clamp(0.0, 1.0);
        l.pan_target = pan.clamp(-1.0, 1.0);
    }

    /// A hum's loudness as last set.
    pub fn level(&self, hum: Hum) -> f32 {
        self.loops[hum.index()].target
    }

    /// Play a cue, `pan` from -1 (left) to 1 (right).
    pub fn play(&mut self, cue: Cue, pan: f32) {
        if self.voices.len() >= VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice {
            cue,
            t: 0.0,
            pan: pan.clamp(-1.0, 1.0),
            phase: 0.0,
        });
    }

    /// Cues sounding now.
    pub fn playing(&self) -> usize {
        self.voices.len()
    }

    /// Overall loudness, 0 to 1.
    pub fn set_master(&mut self, master: f32) {
        self.master = master.clamp(0.0, 1.0);
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    fn noise(&mut self) -> f32 {
        // xorshift32: cheap, and the same on every platform.
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// The low-pass coefficient for a cutoff of `hz`.
    fn cut(&self, hz: f32) -> f32 {
        1.0 - (-TAU * hz / self.rate).exp()
    }

    /// Fill `out` with interleaved stereo samples. Silent while muted, but
    /// time still passes, so unmuting picks up where the loops are.
    pub fn render(&mut self, out: &mut [f32]) {
        for frame in out.chunks_mut(2) {
            let (mut l, mut r) = (0.0, 0.0);
            for i in 0..HUMS.len() {
                let lp = &mut self.loops[i];
                lp.level += (lp.target - lp.level) * self.glide;
                lp.pan += (lp.pan_target - lp.pan) * self.glide;
                if lp.level < 1e-3 && lp.target == 0.0 {
                    lp.level = 0.0;
                    continue;
                }
                let (level, pan) = (lp.level, lp.pan);
                let s = self.hum(i) * level;
                let (gl, gr) = pan_gains(pan);
                l += s * gl;
                r += s * gr;
            }
            let mut k = 0;
            while k < self.voices.len() {
                let mut v = self.voices[k];
                let s = self.cue(&mut v);
                let (gl, gr) = pan_gains(v.pan);
                l += s * gl;
                r += s * gr;
                v.t += self.dt;
                if v.t >= v.cue.seconds() {
                    self.voices.remove(k);
                } else {
                    self.voices[k] = v;
                    k += 1;
                }
            }
            let g = if self.muted { 0.0 } else { self.master };
            // A soft clip, so a crowd of cues never cracks.
            let (l, r) = ((l * g).tanh(), (r * g).tanh());
            frame[0] = l;
            if let Some(right) = frame.get_mut(1) {
                *right = r;
            }
        }
    }

    /// One mono sample of hum `i`, and its clock moved on.
    fn hum(&mut self, i: usize) -> f32 {
        let n = self.noise();
        let (a200, a400, a900, a2k) = (
            self.cut(200.0),
            self.cut(400.0),
            self.cut(900.0),
            self.cut(2500.0),
        );
        let spark = self.noise() * 0.5 + 0.5;
        let dt = self.dt;
        let rate = self.rate;
        let lp = &mut self.loops[i];
        let t = lp.t;
        lp.t += dt;
        // Keep the clock small, so phases stay exact after hours.
        if lp.t > 600.0 {
            lp.t -= 600.0;
        }
        let sine = |hz: f32| (TAU * hz * t).sin();
        // A pulse every 1/hz seconds, decaying at `fall`.
        let pulse = |hz: f32, fall: f32| (-(t * hz).fract() * fall).exp();
        match HUMS[i] {
            Hum::Belt => {
                let rumble = lp.lp[0].run(n, a200) * 2.2;
                let tick = lp.lp[1].run(n, a2k) * pulse(6.0, 90.0);
                (rumble + tick * 0.6) * 0.35
            }
            Hum::Drill => {
                let env = pulse(4.0, 10.0);
                let body = sine(62.0 + 30.0 * env) * env;
                let grit = lp.lp[0].run(n, a900) * (0.3 + 0.7 * env);
                (body * 0.8 + grit * 0.5) * 0.5
            }
            Hum::Fire => {
                let roar = lp.lp[0].run(n, a400) * 1.8;
                // Random crackles, a few a second.
                if spark < 9.0 / rate {
                    lp.burst = 1.0;
                }
                lp.burst *= 0.993;
                let crackle = lp.lp[1].run(n, a2k) * lp.burst;
                (roar * 0.6 + crackle * 0.9) * 0.4
            }
            Hum::Assembler => {
                let x = (t / 0.8).fract();
                let click =
                    (-x * 70.0).exp() + (-(x - 0.4).max(0.0) * 70.0).exp() * f32::from(x >= 0.4);
                let clack = lp.lp[0].run(n, a2k) * click;
                let motor = sine(150.0) * 0.5 + sine(300.0) * 0.15;
                (clack * 0.9 + motor * 0.25) * 0.4
            }
            Hum::Lab => {
                let trem = 0.6 + 0.4 * sine(0.7);
                (sine(523.25) + sine(783.99) * 0.5 + sine(1046.5) * 0.15) * trem * 0.08
            }
            Hum::Chemistry => {
                if spark < 7.0 / rate {
                    lp.burst = 1.0;
                    lp.age = 0.0;
                }
                lp.age += dt;
                lp.burst *= 0.9992;
                // A bubble: a quick upward chirp.
                let hz = 280.0 + 2400.0 * lp.age;
                let bubble = (TAU * hz * lp.age).sin() * lp.burst * (-lp.age * 30.0).exp();
                let hum = sine(90.0) * 0.25;
                (bubble * 0.8 + hum) * 0.4
            }
            Hum::Engine => {
                let env = pulse(2.5, 7.0);
                let chuff = lp.lp[0].run(n, a900) * env * 1.6;
                let body = sine(48.0) * 0.6 + sine(96.0) * 0.2;
                (chuff + body * (0.6 + 0.4 * env)) * 0.4
            }
            Hum::Pump => {
                let wob = 0.75 + 0.25 * sine(3.0);
                (sine(110.0) + sine(220.0) * 0.35 + sine(330.0) * 0.1) * wob * 0.22
            }
            Hum::Wind => {
                let swell = 0.55 + 0.45 * sine(0.13);
                let air = lp.lp[0].run(n, a400 * (0.6 + 0.6 * swell));
                lp.lp[1].run(air, a900) * swell * 1.6
            }
        }
    }

    /// One mono sample of a cue at its time `v.t`.
    fn cue(&mut self, v: &mut Voice) -> f32 {
        let t = v.t;
        let n = self.noise();
        // Gliding notes keep their own phase, so sweeps don't chirp.
        let mut glide = |hz: f32| {
            v.phase = (v.phase + hz * self.dt).fract();
            (TAU * v.phase).sin()
        };
        let bell = |hz: f32, at: f32| {
            let t = t - at;
            if t < 0.0 {
                return 0.0;
            }
            let env = (-t * 4.5).exp() * (t * 400.0).min(1.0);
            ((TAU * hz * t).sin()
                + (TAU * hz * 2.0 * t).sin() * 0.3
                + (TAU * hz * 3.01 * t).sin() * 0.08)
                * env
        };
        // A short attack, so nothing clicks on.
        let attack = (t * 600.0).min(1.0);
        attack
            * match v.cue {
                Cue::Place => {
                    let env = (-t * 22.0).exp();
                    let hz = 120.0 + 160.0 * (-t * 30.0).exp();
                    (glide(hz) * env + n * (-t * 160.0).exp() * 0.3) * 0.7
                }
                Cue::Remove => {
                    let env = (-t * 26.0).exp();
                    glide(380.0 + 900.0 * t) * env * 0.45
                }
                Cue::Refused => {
                    let env = ((Cue::Refused.seconds() - t) * 30.0).min(1.0);
                    (glide(110.0) * 3.0).tanh() * env * 0.25
                }
                Cue::Blocked => {
                    let (hz, x) = if t < 0.1 {
                        (330.0, t)
                    } else {
                        (247.0, t - 0.1)
                    };
                    let env = (-x * 18.0).exp();
                    (glide(hz) * 1.8).tanh() * env * 0.35
                }
                Cue::Starved => {
                    let env = (-t * 10.0).exp();
                    glide(200.0 - 160.0 * t) * env * 0.45
                }
                Cue::NoPower => {
                    let env = ((Cue::NoPower.seconds() - t) * 12.0).min(1.0);
                    let hz = 440.0 * (-t * 5.5).exp() + 50.0;
                    (glide(hz) * 1.5).tanh() * env * 0.3
                }
                Cue::Goal => (bell(523.25, 0.0) + bell(659.25, 0.09) + bell(783.99, 0.18)) * 0.22,
                Cue::Research => {
                    (bell(392.0, 0.0) + bell(523.25, 0.1) + bell(659.25, 0.2) + bell(1046.5, 0.32))
                        * 0.2
                }
                Cue::Tap => n * (-t * 300.0).exp() * 0.25,
            }
    }
}

/// Equal-power left and right gains for `pan` from -1 to 1.
fn pan_gains(pan: f32) -> (f32, f32) {
    let a = (pan + 1.0) * FRAC_PI_4;
    (a.cos(), a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    #[test]
    fn silence_until_something_sounds() {
        let mut s = Synth::new(48_000);
        let mut out = vec![1.0; 4800];
        s.render(&mut out);
        assert!(out.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn every_hum_and_cue_is_audible_and_never_clips() {
        for hum in HUMS {
            let mut s = Synth::new(48_000);
            s.set(hum, 1.0, 0.0);
            let mut out = vec![0.0; 48_000 * 2];
            s.render(&mut out);
            let loud = rms(&out[48_000..]);
            assert!(loud > 0.01, "{hum:?} is too quiet: {loud}");
            assert!(
                out.iter().all(|x| x.is_finite() && x.abs() <= 1.0),
                "{hum:?}"
            );
        }
        for cue in [
            Cue::Place,
            Cue::Remove,
            Cue::Refused,
            Cue::Blocked,
            Cue::Starved,
            Cue::NoPower,
            Cue::Goal,
            Cue::Research,
            Cue::Tap,
        ] {
            let mut s = Synth::new(48_000);
            s.play(cue, 0.0);
            let mut out = vec![0.0; (cue.seconds() * 48_000.0) as usize * 2];
            s.render(&mut out);
            assert!(rms(&out) > 0.005, "{cue:?} is too quiet");
            assert!(
                out.iter().all(|x| x.is_finite() && x.abs() <= 1.0),
                "{cue:?}"
            );
            // And it ends.
            let mut tail = vec![0.0; 256];
            s.render(&mut tail);
            assert_eq!(s.playing(), 0, "{cue:?} still playing");
        }
    }

    #[test]
    fn pan_puts_a_hum_on_one_side() {
        let mut s = Synth::new(48_000);
        s.set(Hum::Pump, 1.0, -1.0);
        let mut out = vec![0.0; 240_000];
        s.render(&mut out);
        // Once the pan has glided there.
        let left: Vec<f32> = out[192_000..].chunks(2).map(|f| f[0]).collect();
        let right: Vec<f32> = out[192_000..].chunks(2).map(|f| f[1]).collect();
        assert!(rms(&left) > rms(&right) * 20.0);
    }

    #[test]
    fn muting_silences_and_levels_glide() {
        let mut s = Synth::new(48_000);
        s.set(Hum::Drill, 1.0, 0.0);
        s.set_muted(true);
        let mut out = vec![0.0; 9600];
        s.render(&mut out);
        assert!(out.iter().all(|&x| x == 0.0));
        s.set_muted(false);
        s.set(Hum::Drill, 0.0, 0.0);
        let mut out = vec![0.0; 192_000];
        s.render(&mut out);
        // It fades out rather than stopping dead.
        assert!(rms(&out[..2400]) > 0.0);
        assert_eq!(rms(&out[180_000..]), 0.0);
    }
}
