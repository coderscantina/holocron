//! SplitMix64 random-access draws and exact-count affine selection, as frozen
//! in the manifest's `prng` and `affine_selection` sections.

pub const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
pub const MASTER_SEED: u64 = 0x484F_4C4F_4352_4F4E;

pub fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Output `i` (0-based) of SplitMix64 started at `seed`.
pub fn draw(seed: u64, i: u64) -> u64 {
    mix64(seed.wrapping_add(i.wrapping_add(1).wrapping_mul(GAMMA)))
}

/// `floor(u * n / 2^64)` with a 128-bit product.
pub fn uniform(u: u64, n: u64) -> u64 {
    ((u as u128 * n as u128) >> 64) as u64
}

pub fn permille(u: u64, p: u64) -> bool {
    uniform(u, 1000) < p
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

pub fn stream_seed(name: &str) -> u64 {
    mix64(MASTER_SEED ^ fnv1a64(name.as_bytes()))
}

/// A seeded stream with the manifest's draw index `ordinal * 16 + sub`.
#[derive(Debug, Clone, Copy)]
pub struct Stream(pub u64);

impl Stream {
    pub fn named(name: &str) -> Self {
        Stream(stream_seed(name))
    }

    pub fn at(self, ordinal: u64, sub: u64) -> u64 {
        draw(self.0, ordinal * 16 + sub)
    }
}

/// `v(i) = (a * i + b) mod n`, a bijection on `[0, n)`.
#[derive(Debug, Clone, Copy)]
pub struct Affine {
    pub n: u64,
    pub a: u64,
    pub b: u64,
}

impl Affine {
    /// `a` is the first integer >= floor(n * golden ratio conjugate) coprime to n;
    /// `b = uniform(draw(stream_seed(stream), 0), n)`.
    pub fn new(n: u64, stream: &str) -> Self {
        let mut a = (n as f64 * 0.618_033_988_749_894_9).floor() as u64;
        while gcd(a, n) != 1 {
            a += 1;
        }
        Affine {
            n,
            a,
            b: uniform(draw(stream_seed(stream), 0), n),
        }
    }

    pub fn value(&self, i: u64) -> u64 {
        ((self.a as u128 * i as u128 + self.b as u128) % self.n as u128) as u64
    }

    /// Inverse: the `i` with `value(i) == v`.
    pub fn index(&self, v: u64) -> u64 {
        let inverse = mod_inverse(self.a, self.n);
        (((v + self.n - self.b) % self.n) as u128 * inverse as u128 % self.n as u128) as u64
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn mod_inverse(a: u64, n: u64) -> u64 {
    let (mut t, mut new_t, mut r, mut new_r) = (0i128, 1i128, n as i128, a as i128);
    while new_r != 0 {
        let q = r / new_r;
        (t, new_t) = (new_t, t - q * new_t);
        (r, new_r) = (new_r, r - q * new_r);
    }
    t.rem_euclid(n as i128) as u64
}

/// Cumulative integer weights; `pick` returns the least index whose cumulative weight exceeds `x`.
#[derive(Debug, Clone)]
pub struct Weights {
    cumulative: Vec<u64>,
}

impl Weights {
    pub fn list(weights: &[u64]) -> Self {
        let mut total = 0;
        Weights {
            cumulative: weights
                .iter()
                .map(|w| {
                    total += w;
                    total
                })
                .collect(),
        }
    }

    /// Zipf exponent 1: `w_k = floor(2^40 / (k + 1))`.
    pub fn zipf(n: usize) -> Self {
        let mut total = 0;
        Weights {
            cumulative: (0..n as u64)
                .map(|k| {
                    total += (1u64 << 40) / (k + 1);
                    total
                })
                .collect(),
        }
    }

    pub fn pick(&self, u: u64) -> usize {
        let x = uniform(u, *self.cumulative.last().expect("non-empty weights"));
        self.cumulative.partition_point(|c| *c <= x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_test_vectors() {
        assert_eq!(mix64(0), 0);
        assert_eq!(mix64(1), 0x5692_161D_100B_05E5);
        let seed = stream_seed("group.start_time");
        assert_eq!(seed, 0x8C7C_B312_75F4_AEE8);
        assert_eq!(draw(seed, 0), 0xFDB8_6A2D_6FA0_BC22);
        assert_eq!(draw(seed, 1), 0xC471_6329_B1B7_F173);
        assert_eq!(draw(seed, 160_000_000), 0xD3C5_DD68_D04B_188D);
    }

    #[test]
    fn affine_instances_match_the_manifest() {
        let day = Affine::new(1_000_000, "day.placement");
        assert_eq!((day.a, day.b), (618_033, 997_833));
        let pageviews = Affine::new(90_000_000, "select.pageview_properties");
        assert_eq!((pageviews.a, pageviews.b), (55_623_059, 56_020_635));
        let revenue = Affine::new(24_000_000, "select.revenue");
        assert_eq!((revenue.a, revenue.b), (14_832_817, 23_161_986));
        assert_eq!(day.index(day.value(123_456)), 123_456);
    }
}
