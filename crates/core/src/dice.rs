//! The world's only source of chance: a PCG32 generator seeded from the match
//! seed. It lives inside the world state, so every peer rolls the same values
//! (BUILDER-SPEC D2 bans system randomness).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        let mut r = Rng {
            state: 0,
            inc: (seed << 1) | 1,
        };
        r.next_u32();
        r.state = r.state.wrapping_add(seed ^ 0x9E37_79B9_7F4A_7C15);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// A value in `0..n`. `n` must be above zero.
    pub fn below(&mut self, n: u32) -> u32 {
        // Lemire's multiply-shift; the tiny bias is irrelevant for a game.
        ((self.next_u32() as u64 * n as u64) >> 32) as u32
    }

    /// A value in `lo..=hi`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// True with probability `percent` / 100.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.below(100) < percent
    }

    pub fn state(&self) -> u64 {
        self.state ^ self.inc.rotate_left(17)
    }
}

/// FNV-1a, used for checksums and for mixing seeds.
pub fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    let mut h = hash;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub const FNV_START: u64 = 0xCBF2_9CE4_8422_2325;

pub fn fnv_i64(hash: u64, v: i64) -> u64 {
    fnv(hash, &v.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_rolls() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seeds_differ() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let same = (0..100).filter(|_| a.next_u32() == b.next_u32()).count();
        assert!(same < 3);
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut r = Rng::new(7);
        let mut seen = [false; 6];
        for _ in 0..1000 {
            let v = r.below(6) as usize;
            assert!(v < 6);
            seen[v] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }
}
