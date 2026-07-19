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

    pub fn derive(self, discriminator: u64) -> Self {
        Self(mix(self.0 ^ mix(discriminator)))
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
pub struct StandardNormal;

impl Generator for StandardNormal {
    type Output = f64;

    fn generate(&self, seed: Seed) -> (Self::Output, Seed) {
        let (radius_source, seed) = seed.next_u64();
        let (angle_source, next_seed) = seed.next_u64();
        let radius = (-2.0 * open_unit_interval(radius_source).ln()).sqrt();
        let angle = std::f64::consts::TAU * open_unit_interval(angle_source);

        (radius * angle.cos(), next_seed)
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

pub fn standard_normal() -> StandardNormal {
    StandardNormal
}

pub fn u32_range(start: u32, end: u32) -> U32Range {
    U32Range::new(start, end)
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn open_unit_interval(value: u64) -> f64 {
    const F64_SIGNIFICAND_VALUES: f64 = (1_u64 << 53) as f64;

    ((value >> 11) as f64 + 0.5) / F64_SIGNIFICAND_VALUES
}

#[cfg(test)]
mod tests {
    use super::{standard_normal, u32_range, u64, Seed};

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
    fn derived_seeds_are_deterministic_and_separate_discriminators() {
        let seed = Seed::new(19);

        assert_eq!(seed.derive(3), seed.derive(3));
        assert_ne!(seed.derive(3), seed.derive(4));
        assert_ne!(Seed::new(20).derive(3), seed.derive(3));
    }

    #[test]
    fn standard_normal_generator_is_deterministic_and_finite() {
        let seed = Seed::new(19);
        let (first, first_next_seed) = seed.generate(standard_normal());
        let (second, second_next_seed) = seed.generate(standard_normal());

        assert_eq!(first, second);
        assert_eq!(first_next_seed, second_next_seed);
        assert!(first.is_finite());
    }

    #[test]
    fn u32_range_generator_stays_within_bounds() {
        let seed = Seed::new(19);
        let (value, _) = seed.generate(u32_range(3, 7));

        assert!((3..=7).contains(&value));
    }
}
