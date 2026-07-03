#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seed(u64);

impl Seed {
    pub const DEFAULT: Self = Self(0x1234_5678_9abc_def0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub fn generate<G>(self, generator: G) -> (G::Output, Self)
    where
        G: Generator,
    {
        generator.generate(self)
    }

    pub fn next_u64(self) -> (u64, Self) {
        let next_seed = Self(self.0.wrapping_add(0x9e37_79b9_7f4a_7c15));
        (mix(next_seed.0), next_seed)
    }
}

impl Default for Seed {
    fn default() -> Self {
        Self::DEFAULT
    }
}

pub trait Generator {
    type Output;

    fn generate(&self, seed: Seed) -> (Self::Output, Seed);
}

impl<T, F> Generator for F
where
    F: Fn(Seed) -> (T, Seed),
{
    type Output = T;

    fn generate(&self, seed: Seed) -> (Self::Output, Seed) {
        self(seed)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct U64;

impl Generator for U64 {
    type Output = u64;

    fn generate(&self, seed: Seed) -> (Self::Output, Seed) {
        seed.next_u64()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct U32Range {
    start: u32,
    end: u32,
}

impl U32Range {
    pub fn new(start: u32, end: u32) -> Self {
        assert!(start <= end, "random range start must not exceed end");

        Self { start, end }
    }
}

impl Generator for U32Range {
    type Output = u32;

    fn generate(&self, seed: Seed) -> (Self::Output, Seed) {
        let (random, next_seed) = seed.next_u64();
        let width = u64::from(self.end) - u64::from(self.start) + 1;
        let offset = random % width;

        (self.start + offset as u32, next_seed)
    }
}

pub fn u64() -> U64 {
    U64
}

pub fn u32_range(start: u32, end: u32) -> U32Range {
    U32Range::new(start, end)
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use super::{u32_range, u64, Seed};

    #[test]
    fn same_seed_generates_same_value_and_next_seed() {
        let seed = Seed::new(19);

        assert_eq!(seed.generate(u64()), seed.generate(u64()));
    }

    #[test]
    fn generated_seed_advances_the_sequence() {
        let seed = Seed::new(19);
        let (first, next_seed) = seed.generate(u64());
        let (second, _) = next_seed.generate(u64());

        assert_ne!(first, second);
    }

    #[test]
    fn u32_range_generator_stays_within_bounds() {
        let seed = Seed::new(19);
        let (value, _) = seed.generate(u32_range(3, 7));

        assert!((3..=7).contains(&value));
    }
}
