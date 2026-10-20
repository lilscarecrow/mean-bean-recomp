//! The original's random number generator (`Random`, `RandomBound`).

/// Seed the original falls back to when its seed is zero (e.g. right after power-on).
pub const DEFAULT_SEED: u32 = 0x2A6D_365A;

/// 32-bit multiply-and-fold generator. `seed` is exactly the game's `rng_seed` long.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    pub seed: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng { seed }
    }

    /// `Random`: returns the 16-bit result the game reads from `d0.w`.
    ///
    /// seed * 41, then the result's high and low words are added; the sum becomes the
    /// output and the new seed's high word, with the old low word below it.
    pub fn next(&mut self) -> u16 {
        let seed = if self.seed == 0 { DEFAULT_SEED } else { self.seed };
        let t = seed.wrapping_mul(41);
        let lo = t as u16;
        let sum = lo.wrapping_add((t >> 16) as u16);
        self.seed = (u32::from(sum) << 16) | u32::from(lo);
        sum
    }

    /// `RandomBound`: a value in `0..bound`, as the high word of `next() * bound`.
    pub fn below(&mut self, bound: u16) -> u16 {
        ((u32::from(self.next()) * u32::from(bound)) >> 16) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_seed_uses_default() {
        let mut a = Rng::new(0);
        let mut b = Rng::new(DEFAULT_SEED);
        assert_eq!(a.next(), b.next());
        assert_eq!(a, b);
    }

    #[test]
    fn one_step_by_hand() {
        // 0x2A6D365A * 41 = 0x6_CB7D_B46A; low long 0xCB7DB46A; 0xB46A + 0xCB7D = 0x7FE7 (carry dropped)
        let mut rng = Rng::new(DEFAULT_SEED);
        assert_eq!(rng.next(), 0x7FE7);
        assert_eq!(rng.seed, 0x7FE7_B46A);
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::new(1234);
        for bound in [1u16, 5, 0xF8, 0xFFFF] {
            for _ in 0..1000 {
                assert!(rng.below(bound) < bound);
            }
        }
    }
}
