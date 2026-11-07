//! A quick hasher for tile and chunk positions (T60 follow-up): "what is
//! at this tile" runs for every item every tick, and the standard hasher
//! was a fifth of the bot's run. Maps using it are only looked up, or
//! sorted before anything iterates them, so the order never matters.

use std::hash::{BuildHasherDefault, Hasher};

pub type Fast = BuildHasherDefault<PosHasher>;

#[derive(Default)]
pub struct PosHasher(u64);

impl Hasher for PosHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_i32(&mut self, n: i32) {
        self.write_u64(u64::from(n as u32));
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(29) ^ n).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}
