//! Fixed-step timing: how many ticks to run for the real time that has passed.

use mofact_core::TICKS_PER_SECOND;
use std::time::Duration;

/// Converts real elapsed time into whole simulation ticks.
///
/// If the simulation falls far behind (the app was suspended, or a tick was
/// very slow) it runs at most [`MAX_CATCH_UP`](Self::MAX_CATCH_UP) ticks at once and
/// drops the rest, so it slows down instead of spiralling.
#[derive(Debug, Default)]
pub struct Stepper {
    owed: Duration,
}

impl Stepper {
    pub const TICK: Duration = Duration::from_nanos(1_000_000_000 / TICKS_PER_SECOND as u64);
    pub const MAX_CATCH_UP: u32 = 6;

    pub fn new() -> Self {
        Self::default()
    }

    /// Record `elapsed` real time and return how many ticks to run now.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.owed += elapsed;
        let mut n = 0;
        while self.owed >= Self::TICK && n < Self::MAX_CATCH_UP {
            self.owed -= Self::TICK;
            n += 1;
        }
        if n == Self::MAX_CATCH_UP && self.owed >= Self::TICK {
            self.owed = Duration::ZERO;
        }
        n
    }

    /// How long until the next tick is due.
    pub fn until_next(&self) -> Duration {
        Self::TICK.saturating_sub(self.owed)
    }

    /// Forget owed time, for resuming after the app was in the background.
    pub fn reset(&mut self) {
        self.owed = Duration::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_is_sixty_ticks_however_it_is_sliced() {
        for slice_ms in [1u64, 7, 16, 33, 50, 100] {
            let mut s = Stepper::new();
            let mut ticks = 0;
            let mut t = 0;
            while t < 1000 {
                ticks += s.advance(Duration::from_millis(slice_ms));
                t += slice_ms;
            }
            // Slices that don't divide 1000 ms overshoot by up to one slice.
            let expected = (t * 60 / 1000) as u32;
            assert!(
                ticks.abs_diff(expected) <= 1,
                "{slice_ms} ms slices: {ticks} ticks, expected {expected}"
            );
        }
    }

    #[test]
    fn long_stalls_are_dropped_not_replayed() {
        let mut s = Stepper::new();
        assert_eq!(s.advance(Duration::from_secs(5)), Stepper::MAX_CATCH_UP);
        assert_eq!(
            s.advance(Duration::ZERO),
            0,
            "the rest of the stall is forgotten"
        );
    }
}
