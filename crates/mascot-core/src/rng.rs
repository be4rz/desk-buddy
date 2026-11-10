//! Seeded xorshift64* (no global RNG, no wall clock: same seed ⇒ same draws).

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // xorshift has a fixed point at 0; mix the seed so nearby seeds differ.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        Rng(if z == 0 { 0x2545_F491_4F6C_DD1D } else { z })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next_u64() as u128 * n as u128) >> 64) as u64
    }

    /// Uniform inclusive `lo..=hi`.
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + self.below((hi - lo) as u64 + 1) as u32
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_and_in_range() {
        let a: Vec<u64> = { let mut r = Rng::new(7); (0..5).map(|_| r.next_u64()).collect() };
        let b: Vec<u64> = { let mut r = Rng::new(7); (0..5).map(|_| r.next_u64()).collect() };
        let c: Vec<u64> = { let mut r = Rng::new(8); (0..5).map(|_| r.next_u64()).collect() };
        assert_eq!(a, b);
        assert_ne!(a, c);
        let mut r = Rng::new(0);
        for _ in 0..1000 {
            let x = r.range(2, 4);
            assert!((2..=4).contains(&x));
            let u = r.unit();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
