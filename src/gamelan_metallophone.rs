//! A sine-only model of a paired Balinese-style bronze metallophone.
//!
//! This is intentionally an instrument model rather than a general additive
//! synthesizer. Its mode ratios combine measured gangsa and gender spectra,
//! while the paired bars create the characteristic ombak beating.

const DURATION_SECONDS: f32 = 5.5;
const OMBAK_HZ: f32 = 4.0;
const CENTER_BAR_MIX: f32 = 0.80;
const OMBAK_BAR_MIX: f32 = (1.0 - CENTER_BAR_MIX) * 0.5;
const NYQUIST_MARGIN: f32 = 0.98;
const TAU: f32 = std::f32::consts::TAU;

#[derive(Clone, Copy)]
struct ActiveNote {
    frequency: f32,
    sample: u32,
    volume: f32,
    cutoff_samples: Option<u32>,
}

#[derive(Clone, Copy)]
struct Mode {
    ratio: f32,
    amplitude: f32,
    decay_seconds: f32,
    attack_seconds: f32,
    phase_turns: f32,
}

impl Mode {
    const fn new(
        ratio: f32,
        amplitude: f32,
        decay_seconds: f32,
        attack_seconds: f32,
        phase_turns: f32,
    ) -> Self {
        Self {
            ratio,
            amplitude,
            decay_seconds,
            attack_seconds,
            phase_turns,
        }
    }
}

// Ratios near 2.7 and 5.3 are characteristic flexural bar modes. The quieter
// near-harmonic modes represent the tuned/resonated components reported for
// gender, and give the note a definite pitch without turning it into a bell.
const MODES: [Mode; 8] = [
    Mode::new(1.000, 0.54, 2.80, 0.000_35, 0.00),
    Mode::new(2.010, 0.13, 1.35, 0.000_25, 0.25),
    Mode::new(2.706, 0.31, 1.05, 0.000_18, 0.00),
    Mode::new(4.050, 0.075, 0.62, 0.000_14, 0.25),
    Mode::new(4.800, 0.085, 0.48, 0.000_12, 0.00),
    Mode::new(5.199, 0.12, 0.39, 0.000_10, 0.25),
    Mode::new(5.502, 0.095, 0.31, 0.000_09, 0.00),
    Mode::new(6.970, 0.028, 0.20, 0.000_08, 0.25),
];

pub(crate) struct GamelanMetallophoneRuntime {
    active: Vec<ActiveNote>,
}

impl GamelanMetallophoneRuntime {
    pub(crate) fn new() -> Self {
        Self {
            active: Vec::with_capacity(32),
        }
    }

    #[cfg(test)]
    pub(crate) fn trigger(&mut self, frequency: f32) {
        self.trigger_with_volume_and_cutoff(frequency, 1.0, None);
    }

    pub(crate) fn trigger_with_volume_and_cutoff(
        &mut self,
        frequency: f32,
        volume: f32,
        cutoff_samples: Option<u32>,
    ) {
        self.active.push(ActiveNote {
            frequency,
            sample: 0,
            volume,
            cutoff_samples,
        });
    }

    pub(crate) fn sample(&mut self, sample_rate: f32) -> (f32, bool) {
        let mut output = 0.0;
        for note in &mut self.active {
            output += note_sample(note.frequency, note.sample, sample_rate) * note.volume;
            note.sample += 1;
        }

        let duration = duration_samples(sample_rate);
        self.active.retain(|note| {
            note.sample < duration
                && note
                    .cutoff_samples
                    .is_none_or(|cutoff_samples| note.sample < cutoff_samples)
        });
        (output, !self.active.is_empty())
    }
}

fn note_sample(frequency: f32, sample: u32, sample_rate: f32) -> f32 {
    let duration = duration_samples(sample_rate);
    if sample >= duration {
        return 0.0;
    }

    let seconds = sample as f32 / sample_rate;
    let low_bar = (frequency - OMBAK_HZ * 0.5).max(frequency * 0.97);
    let high_bar = frequency + OMBAK_HZ * 0.5;
    let body = MODES
        .iter()
        .map(|mode| {
            mode_sample(*mode, frequency, seconds, sample_rate) * CENTER_BAR_MIX
                + mode_sample(*mode, low_bar, seconds, sample_rate) * OMBAK_BAR_MIX
                + mode_sample(*mode, high_bar, seconds, sample_rate) * OMBAK_BAR_MIX
        })
        .sum::<f32>();

    // A padded mallet still injects a brief broad-band pulse. This deterministic
    // cluster is all sine waves and follows a smooth, click-free impulse shape.
    let strike = strike_sample(frequency, seconds, sample_rate);
    let final_taper = (1.0 - sample as f32 / duration as f32)
        .clamp(0.0, 1.0)
        .powi(2);
    (body + strike) * final_taper * 0.72
}

fn mode_sample(mode: Mode, bar_frequency: f32, seconds: f32, sample_rate: f32) -> f32 {
    let mode_frequency = bar_frequency * mode.ratio;
    if mode_frequency >= sample_rate * 0.5 * NYQUIST_MARGIN {
        return 0.0;
    }

    let attack = sine_ramp(seconds / mode.attack_seconds);
    let fade = (-seconds / mode.decay_seconds).exp();
    (TAU * (mode_frequency * seconds + mode.phase_turns)).sin() * mode.amplitude * attack * fade
}

fn strike_sample(frequency: f32, seconds: f32, sample_rate: f32) -> f32 {
    const STRIKE_SECONDS: f32 = 0.014;
    const RATIOS: [f32; 20] = [
        0.43, 0.79, 1.17, 1.63, 2.11, 2.71, 3.34, 4.08, 4.80, 5.20, 5.89, 6.97, 7.83, 8.90, 10.07,
        11.35, 12.68, 14.20, 16.03, 18.31,
    ];
    const OFFSETS_HZ: [f32; 20] = [
        113.0, 521.0, 89.0, 997.0, 307.0, 1_423.0, 661.0, 1_861.0, 191.0, 2_299.0, 809.0, 2_777.0,
        419.0, 3_197.0, 1_103.0, 3_701.0, 1_619.0, 4_213.0, 2_033.0, 4_759.0,
    ];
    if seconds >= STRIKE_SECONDS {
        return 0.0;
    }

    let position = seconds / STRIKE_SECONDS;
    let shape = sine_ramp(position / 0.08) * (1.0 - position).powi(3);
    RATIOS
        .iter()
        .enumerate()
        .filter_map(|(index, ratio)| {
            let component_frequency = frequency * ratio + OFFSETS_HZ[index];
            (component_frequency < sample_rate * 0.5 * NYQUIST_MARGIN).then(|| {
                let amplitude = 0.32 / (1.0 + index as f32 * 0.28).sqrt();
                let phase = (index as f32 * 0.381_966).fract();
                (TAU * (component_frequency * seconds + phase)).sin() * amplitude
            })
        })
        .sum::<f32>()
        * shape
}

fn sine_ramp(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    (std::f32::consts::FRAC_PI_2 * position).sin().powi(2)
}

fn duration_samples(sample_rate: f32) -> u32 {
    (DURATION_SECONDS * sample_rate).round().max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{duration_samples, note_sample, GamelanMetallophoneRuntime, MODES, OMBAK_HZ};

    #[test]
    fn measured_bar_modes_are_inharmonic_and_bronze_bright() {
        assert_eq!(MODES[0].ratio, 1.0);
        assert!((MODES[2].ratio - 2.706).abs() < 0.000_1);
        assert!(MODES[5].decay_seconds < MODES[2].decay_seconds);
        assert!(MODES[2].decay_seconds < MODES[0].decay_seconds);
    }

    #[test]
    fn paired_bars_have_a_gentle_four_hertz_ombak_interval() {
        let frequency = 440.0_f32;
        let low = frequency - OMBAK_HZ * 0.5;
        let high = frequency + OMBAK_HZ * 0.5;
        assert!((high - low - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn voice_is_click_free_audible_and_finishes() {
        let sample_rate = 44_100.0;
        assert_eq!(note_sample(220.0, 0, sample_rate), 0.0);

        let mut runtime = GamelanMetallophoneRuntime::new();
        runtime.trigger(220.0);
        let mut peak = 0.0_f32;
        let mut active = true;
        for _ in 0..=duration_samples(sample_rate) {
            let (sample, is_active) = runtime.sample(sample_rate);
            assert!(sample.is_finite());
            peak = peak.max(sample.abs());
            active = is_active;
        }
        assert!(peak > 0.1);
        assert!(!active);
    }
}
