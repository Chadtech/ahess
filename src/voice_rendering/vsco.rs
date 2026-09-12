//! Embedded VSCO 2 CE recordings, independently retuned per note.
//! Asset provenance and preparation live in assets/samples/vsco2/README.md.
use crate::{pitch_system::FrequencyHz, seed::Seed, voice::VoiceType};

#[path = "vsco_samples.rs"]
mod samples;

const SOURCE_RATE: f64 = 44_100.0;
const MAX_NOTES: usize = 128;

struct Sample {
    reference_hz: f64,
    // Successive levels are low-pass filtered and decimated by powers of two.
    levels: &'static [&'static [u8]],
    sustain: Option<SampleLoop>,
}

#[derive(Clone, Copy)]
struct SampleLoop {
    start: f64,
    end: f64,
    crossfade: f64,
}

impl SampleLoop {
    fn scaled(self, factor: f64) -> Self {
        Self {
            start: self.start / factor,
            end: self.end / factor,
            crossfade: self.crossfade / factor,
        }
    }
}

pub(crate) struct VscoRuntime {
    voice_type: VoiceType,
    samples: &'static [Sample],
    sample_rate: f64,
    notes: [Option<BlendedNote>; MAX_NOTES],
}

impl VscoRuntime {
    pub(crate) fn new(voice_type: VoiceType, sample_rate: f32) -> Self {
        let samples = match voice_type {
            VoiceType::VscoCello => samples::CELLO,
            VoiceType::VscoFlute => samples::FLUTE,
            VoiceType::VscoClarinet => samples::CLARINET,
            VoiceType::VscoHarp => samples::HARP,
            _ => unreachable!("VSCO runtime requires a VSCO instrument"),
        };
        Self {
            voice_type,
            samples,
            sample_rate: f64::from(sample_rate),
            notes: [None; MAX_NOTES],
        }
    }

    pub(crate) fn voice_type(&self) -> VoiceType {
        self.voice_type
    }

    #[cfg(test)]
    fn trigger(&mut self, frequency: FrequencyHz, volume: f32, gate: u32, explicit: bool) {
        self.trigger_seeded(frequency, volume, gate, explicit, Seed::DEFAULT);
    }

    #[cfg(test)]
    pub(crate) fn trigger_seeded(
        &mut self,
        frequency: FrequencyHz,
        volume: f32,
        gate: u32,
        explicit: bool,
        seed: Seed,
    ) {
        self.trigger_with_attack(
            frequency,
            volume,
            gate,
            explicit,
            seed,
            crate::voice::AttackSharpness::default(),
        );
    }

    pub(crate) fn trigger_with_attack(
        &mut self,
        frequency: FrequencyHz,
        volume: f32,
        gate: u32,
        explicit: bool,
        seed: Seed,
        attack: crate::voice::AttackSharpness,
    ) {
        let hz = frequency.as_hz();
        if volume <= 0.0 || hz >= self.sample_rate * 0.45 {
            return;
        }
        let Some(slot) = self.notes.iter_mut().find(|n| n.is_none()) else {
            return;
        };
        let (pair, weights) = blend(self.samples, hz, seed);
        *slot = Some(BlendedNote {
            layers: std::array::from_fn(|i| {
                Some(SampleNote::new(
                    pair[i],
                    self.sample_rate,
                    hz,
                    volume * weights[i],
                    gate,
                    explicit,
                    attack,
                ))
            }),
        });
    }

    pub(crate) fn sample(&mut self) -> (f32, bool) {
        let mut value = 0.0;
        let mut active = false;
        for slot in &mut self.notes {
            if let Some(note) = slot {
                if let Some(sample) = note.next() {
                    value += sample;
                    active = true;
                } else {
                    *slot = None;
                }
            }
        }
        (value, active)
    }
}

// Always two distinct neighboring recordings, including at the bank edges.
// Independent random gains are proximity-weighted and normalized to sum to one.
fn blend(bank: &'static [Sample], hz: f64, seed: Seed) -> ([&'static Sample; 2], [f32; 2]) {
    let upper = bank
        .partition_point(|s| s.reference_hz < hz)
        .clamp(1, bank.len() - 1);
    let pair = [&bank[upper - 1], &bank[upper]];
    let mut rng = seed;
    let mut weights = [0.0; 2];
    for i in 0..2 {
        let (bits, next) = rng.next_u64();
        rng = next;
        let random = (bits >> 40) as f64 / (1_u64 << 24) as f64;
        let semitones = 12.0 * (hz / pair[i].reference_hz).log2().abs();
        weights[i] = ((0.2 + 0.8 * random) / (1.0 + semitones / 3.0)) as f32;
    }
    let sum = weights[0] + weights[1];
    weights[0] /= sum;
    weights[1] /= sum;
    (pair, weights)
}

#[derive(Clone, Copy)]
struct BlendedNote {
    layers: [Option<SampleNote>; 2],
}

impl BlendedNote {
    fn next(&mut self) -> Option<f32> {
        let mut output = 0.0;
        let mut active = false;
        for layer in &mut self.layers {
            if let Some(note) = layer {
                if let Some(value) = note.next() {
                    output += value;
                    active = true;
                } else {
                    *layer = None;
                }
            }
        }
        active.then_some(output)
    }
}

#[derive(Clone, Copy)]
enum Release {
    Natural,
    Sustain { gate: u64, length: u64 },
    Cutoff { end: u64, fade: u64 },
}

#[derive(Clone, Copy)]
struct SampleNote {
    pcm: &'static [u8],
    position: f64,
    step: f64,
    sustain: Option<SampleLoop>,
    age: u64,
    release: Release,
    attack_fade: u64,
    volume: f32,
}

impl SampleNote {
    fn new(
        sample: &'static Sample,
        sample_rate: f64,
        hz: f64,
        volume: f32,
        gate: u32,
        explicit: bool,
        attack: crate::voice::AttackSharpness,
    ) -> Self {
        let source_step = SOURCE_RATE / sample_rate * hz / sample.reference_hz;
        let level = source_step.log2().ceil().max(0.0) as usize;
        let level = level.min(sample.levels.len() - 1);
        let factor = (1_u32 << level) as f64;
        let release = if sample.sustain.is_some() {
            Release::Sustain {
                gate: u64::from(gate),
                length: (sample_rate * 0.12).round().max(1.0) as u64,
            }
        } else if explicit {
            Release::Cutoff {
                end: u64::from(gate),
                fade: (sample_rate * 0.005).round().max(1.0).min(f64::from(gate)) as u64,
            }
        } else {
            Release::Natural
        };
        Self {
            pcm: sample.levels[level],
            position: SOURCE_RATE * 0.5 * f64::from(attack.percent()) / 100.0 / factor,
            step: source_step / factor,
            sustain: sample.sustain.map(|s| s.scaled(factor)),
            age: 0,
            release,
            attack_fade: if attack.percent() == 0 {
                0
            } else {
                (sample_rate * 0.005).round().max(1.0) as u64
            },
            volume,
        }
    }

    fn next(&mut self) -> Option<f32> {
        let gain = match self.release {
            Release::Natural => 1.0,
            Release::Sustain { gate, length } => {
                if self.age >= gate + length {
                    return None;
                }
                fade_out(self.age.saturating_sub(gate) as f64 / length as f64)
            }
            Release::Cutoff { end, fade } => {
                if self.age >= end {
                    return None;
                }
                fade_out(self.age.saturating_sub(end - fade) as f64 / fade.max(1) as f64)
            }
        };
        if self.position >= (self.pcm.len() / 2) as f64 {
            return None;
        }
        let mut output = interpolate(self.pcm, self.position);
        if let Some(lp) = self.sustain {
            if self.position >= lp.end - lp.crossfade {
                let t = (self.position - (lp.end - lp.crossfade)) / lp.crossfade;
                let other =
                    interpolate(self.pcm, lp.start + self.position - (lp.end - lp.crossfade));
                output = output * (1.0 - t as f32) + other * t as f32;
            }
            self.position += self.step;
            if self.position >= lp.end {
                self.position = lp.start
                    + lp.crossfade
                    + (self.position - lp.end) % (lp.end - lp.start - lp.crossfade);
            }
        } else {
            self.position += self.step;
        }
        let attack_gain = if self.attack_fade == 0 {
            1.0
        } else {
            1.0 - fade_out(self.age as f64 / self.attack_fade as f64)
        };
        self.age += 1;
        Some(output * self.volume * gain * attack_gain)
    }
}

fn fade_out(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - t * t * (3.0 - 2.0 * t)) as f32
}

fn pcm_at(pcm: &[u8], frame: isize) -> f32 {
    if frame < 0 || frame as usize >= pcm.len() / 2 {
        return 0.0;
    }
    let i = frame as usize * 2;
    f32::from(i16::from_le_bytes([pcm[i], pcm[i + 1]])) / 32768.0
}

fn interpolate(pcm: &[u8], position: f64) -> f32 {
    let i = position.floor() as isize;
    let t = (position - i as f64) as f32;
    let a = pcm_at(pcm, i - 1);
    let b = pcm_at(pcm, i);
    let c = pcm_at(pcm, i + 1);
    let d = pcm_at(pcm, i + 2);
    b + 0.5 * t * (c - a + t * (2.0 * a - 5.0 * b + 4.0 * c - d + t * (3.0 * (b - c) + d - a)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(kind: VoiceType, hz: f64, seconds: f64, rate: u32) -> Vec<f32> {
        let mut runtime = VscoRuntime::new(kind, rate as f32);
        runtime.trigger(FrequencyHz::new(hz).unwrap(), 0.8, rate * 10, false);
        (0..(seconds * f64::from(rate)) as usize)
            .map(|_| runtime.sample().0)
            .collect()
    }

    fn measured_frequency(x: &[f32], rate: u32, target: f64) -> f64 {
        let lo = (f64::from(rate) / (target * 1.04)).floor() as usize;
        let hi = (f64::from(rate) / (target * 0.96)).ceil() as usize;
        let correlation = |lag: usize| -> f64 {
            x[..x.len() - lag]
                .iter()
                .zip(&x[lag..])
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum::<f64>()
                / (x.len() - lag) as f64
        };
        let lag = (lo..=hi)
            .max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)))
            .unwrap();
        assert!(lag > lo && lag < hi);
        let a = correlation(lag - 1);
        let b = correlation(lag);
        let c = correlation(lag + 1);
        f64::from(rate) / (lag as f64 + 0.5 * (a - c) / (a - 2.0 * b + c))
    }

    #[test]
    fn sharp_attacks_start_promptly_keep_tuning_and_release_cleanly() {
        for (kind, hz) in [
            (VoiceType::VscoFlute, 453.25),
            (VoiceType::VscoClarinet, 388.5),
        ] {
            let render = |sharpness| {
                let mut runtime = VscoRuntime::new(kind, 48_000.0);
                runtime.trigger_with_attack(
                    FrequencyHz::new(hz).unwrap(),
                    0.8,
                    24_000,
                    true,
                    Seed::new(42),
                    crate::voice::AttackSharpness::new(sharpness).unwrap(),
                );
                (0..36_000).map(|_| runtime.sample().0).collect::<Vec<_>>()
            };
            let natural = render(0);
            let sharp = render(100);
            let energy =
                |x: &[f32]| x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64;
            let early_natural = energy(&natural[..2400]);
            let early_sharp = energy(&sharp[..2400]);
            println!("{kind:?}: first 50ms energy natural={early_natural}, sharp={early_sharp}, gain={}x", early_sharp/early_natural);
            assert!(early_sharp > early_natural * 2.0);
            assert_eq!(sharp[0], 0.0);
            assert!(sharp[..24].iter().all(|v| v.abs() < 0.02));
            assert!(sharp[30_000..].iter().all(|v| *v == 0.0));
            let measured = measured_frequency(&sharp[6000..24000], 48_000, hz);
            assert!(
                (1200.0 * (measured / hz).log2()).abs() < 8.0,
                "{kind:?}: {measured}"
            );
        }
    }

    #[test]
    fn bundled_vsco_recordings_follow_non_equal_tempered_frequencies() {
        for (kind, hz) in [
            (VoiceType::VscoCello, 259.0),
            (VoiceType::VscoFlute, 453.0),
            (VoiceType::VscoClarinet, 383.0),
            (VoiceType::VscoHarp, 271.0),
        ] {
            for rate in [44_100, 48_000, 96_000] {
                let audio = render(kind, hz, 1.0, rate);
                let start = rate as usize / 4;
                let measured =
                    measured_frequency(&audio[start..start + rate as usize / 2], rate, hz);
                let cents = 1200.0 * (measured / hz).log2();
                assert!(
                    cents.abs() < 8.0,
                    "{kind:?} at {rate}: target {hz}, measured {measured} ({cents} cents)"
                );
                assert!(audio.iter().all(|s| s.is_finite()));
                assert!(audio.iter().any(|s| s.abs() > 0.01));
            }
        }
    }

    #[test]
    fn vsco_overlapping_notes_keep_independent_pitch_and_volume() {
        let mut combined = VscoRuntime::new(VoiceType::VscoFlute, 48_000.0);
        let mut first = VscoRuntime::new(VoiceType::VscoFlute, 48_000.0);
        let mut second = VscoRuntime::new(VoiceType::VscoFlute, 48_000.0);
        let a = FrequencyHz::new(453.0).unwrap();
        let b = FrequencyHz::new(471.25).unwrap();
        combined.trigger(a, 0.3, 48_000, false);
        first.trigger(a, 0.3, 48_000, false);
        for i in 0..60_000 {
            if i == 8_000 {
                combined.trigger(b, 0.6, 48_000, false);
                second.trigger(b, 0.6, 48_000, false);
            }
            assert_eq!(combined.sample().0, first.sample().0 + second.sample().0);
        }
    }

    #[test]
    fn vsco_loops_hold_and_release_and_harp_obeys_explicit_cutoff() {
        for kind in [
            VoiceType::VscoCello,
            VoiceType::VscoFlute,
            VoiceType::VscoClarinet,
        ] {
            let mut runtime = VscoRuntime::new(kind, 48_000.0);
            runtime.trigger(FrequencyHz::new(453.0).unwrap(), 0.8, 48_000 * 5, true);
            let mut late_energy = 0.0;
            for i in 0..48_000 * 5 {
                let (x, active) = runtime.sample();
                assert!(active);
                if i > 48_000 * 4 {
                    late_energy += x * x;
                }
            }
            assert!(late_energy > 1.0);
            for _ in 0..5_760 {
                assert!(runtime.sample().1);
            }
            assert_eq!(runtime.sample(), (0.0, false));
        }
        let mut harp = VscoRuntime::new(VoiceType::VscoHarp, 48_000.0);
        harp.trigger(FrequencyHz::new(271.0).unwrap(), 1.0, 4_800, true);
        let mut last = 0.0;
        for _ in 0..4_800 {
            last = harp.sample().0;
        }
        assert!(last.abs() < 0.0001);
        assert_eq!(harp.sample(), (0.0, false));
        harp.trigger(FrequencyHz::new(271.0).unwrap(), 1.0, 4_800, false);
        for _ in 0..4_801 {
            assert!(harp.sample().1);
        }
    }

    #[test]
    fn vsco_random_blends_change_the_waveform_and_repeat_with_the_same_seed() {
        for kind in [
            VoiceType::VscoCello,
            VoiceType::VscoFlute,
            VoiceType::VscoClarinet,
            VoiceType::VscoHarp,
        ] {
            let render_seed = |seed| {
                let mut r = VscoRuntime::new(kind, 48_000.0);
                r.trigger_seeded(
                    FrequencyHz::new(453.0).unwrap(),
                    0.8,
                    48_000,
                    false,
                    Seed::new(seed),
                );
                (0..24_000).map(|_| r.sample().0).collect::<Vec<_>>()
            };
            let first = render_seed(1);
            assert_eq!(first, render_seed(1));
            for seed in 2..6 {
                let other = render_seed(seed);
                let difference: f64 = first
                    .iter()
                    .zip(&other)
                    .map(|(a, b)| f64::from((a - b) * (a - b)))
                    .sum();
                assert!(difference > 0.001, "{kind:?} did not vary with seed {seed}");
                let measured = measured_frequency(&other[6_000..], 48_000, 453.0);
                assert!(
                    (1200.0 * (measured / 453.0).log2()).abs() < 8.0,
                    "{kind:?}: {measured}"
                );
            }
        }
    }

    #[test]
    fn vsco_blends_remain_distinct_and_normalized_at_recordings_and_bank_edges() {
        for hz in [20.0, samples::FLUTE[0].reference_hz, 453.0, 20_000.0] {
            let mut seen = std::collections::HashSet::new();
            for seed in 0..100 {
                let (pair, weights) = blend(samples::FLUTE, hz, Seed::new(seed));
                assert!(pair[0].reference_hz < pair[1].reference_hz);
                assert!(weights.iter().all(|w| *w > 0.0 && *w < 1.0));
                assert!((weights[0] + weights[1] - 1.0).abs() < 1e-6);
                seen.insert(weights[0].to_bits());
            }
            assert_eq!(seen.len(), 100);
        }
    }

    #[test]
    fn vsco_assets_have_valid_loops_and_antialias_levels() {
        for bank in [
            samples::CELLO,
            samples::FLUTE,
            samples::CLARINET,
            samples::HARP,
        ] {
            assert!(bank.len() >= 2);
            assert!(bank
                .windows(2)
                .all(|w| w[0].reference_hz < w[1].reference_hz));
            for sample in bank {
                assert!(sample.reference_hz.is_finite() && sample.reference_hz > 20.0);
                for (i, pcm) in sample.levels.iter().enumerate() {
                    assert!(!pcm.is_empty() && pcm.len() % 2 == 0);
                    if let Some(lp) = sample.sustain {
                        let lp = lp.scaled((1_u32 << i) as f64);
                        assert!(
                            lp.start > 0.0
                                && lp.crossfade > 0.0
                                && lp.end > lp.start + lp.crossfade
                        );
                        assert!(lp.end + 2.0 < (pcm.len() / 2) as f64);
                    }
                }
            }
        }
    }
}
