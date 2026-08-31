use crate::voice::VoiceType;

const SOURCE_SAMPLE_RATE: f32 = 44_100.0;
const SOURCE_RAMP_SAMPLES: f32 = 60.0;
const NYQUIST_MARGIN: f32 = 0.98;

#[derive(Clone, Copy)]
struct ActiveNote {
    fundamental: f32,
    sample: u32,
}

#[derive(Clone, Copy)]
struct Partial {
    ratio: f32,
    amplitude: f32,
    duration: f32,
    fade_power: i32,
    phase: f32,
}

impl Partial {
    const fn new(ratio: f32, amplitude: f32, duration: f32) -> Self {
        Self {
            ratio,
            amplitude,
            duration,
            fade_power: 1,
            phase: 0.0,
        }
    }

    const fn shaped(mut self, fade_power: i32, phase: f32) -> Self {
        self.fade_power = fade_power;
        self.phase = phase;
        self
    }
}

pub(crate) struct RecoveredVoiceRuntime {
    voice_type: VoiceType,
    active: Vec<ActiveNote>,
}

impl RecoveredVoiceRuntime {
    pub(crate) fn new(voice_type: VoiceType) -> Self {
        debug_assert!(voice_type.uses_recovered_runtime());
        Self {
            voice_type,
            active: Vec::with_capacity(32),
        }
    }

    pub(crate) fn voice_type(&self) -> VoiceType {
        self.voice_type
    }

    pub(crate) fn trigger(&mut self, fundamental: f32) {
        self.active.push(ActiveNote {
            fundamental,
            sample: 0,
        });
    }

    pub(crate) fn sample(&mut self, sample_rate: f32) -> (f32, bool) {
        let voice_type = self.voice_type;
        let mut output = 0.0;
        for note in &mut self.active {
            output += voice_sample(voice_type, note.fundamental, note.sample, sample_rate);
            note.sample += 1;
        }
        let duration = duration_samples(voice_type, sample_rate);
        self.active.retain(|note| note.sample < duration);
        (output, !self.active.is_empty())
    }
}

fn voice_sample(voice_type: VoiceType, fundamental: f32, sample: u32, sample_rate: f32) -> f32 {
    let duration_samples = duration_samples(voice_type, sample_rate);
    if sample >= duration_samples {
        return 0.0;
    }
    let seconds = sample as f32 / sample_rate;
    let body_seconds = duration_seconds(voice_type);
    let mut signal = 0.0;

    match voice_type {
        VoiceType::NoitechBellG => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BELL_G,
            );
        }
        VoiceType::NoitechBellH => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BELL_H,
            );
        }
        VoiceType::NoitechBellI => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BELL_I,
            );
        }
        VoiceType::NoitechBellJ => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BELL_J,
            );
        }
        VoiceType::NoitechBellK => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BELL_K,
            );
        }
        VoiceType::NoitechBellL => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BELL_L,
            );
        }
        VoiceType::NoitechBellM => {
            signal += 0.5
                * partial_bank(
                    fundamental,
                    sample_rate,
                    seconds,
                    sample,
                    body_seconds,
                    &BELL_M,
                );
        }
        VoiceType::IconoclastBellG => {
            let detuned = fundamental * deterministic_detuning(fundamental);
            signal += partial_bank(detuned, sample_rate, seconds, sample, body_seconds, &ICON_G);
        }
        VoiceType::IconoclastBellH => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &ICON_H,
            );
        }
        VoiceType::IconoclastIndustrialBar => {
            signal += partial_bank(
                fundamental,
                sample_rate,
                seconds,
                sample,
                body_seconds,
                &BAR_R,
            );
        }
        VoiceType::CtpianoHiSaw => signal += saw_bank(fundamental, 60, sample_rate, seconds),
        VoiceType::CtpianoLoSaw => signal += saw_bank(fundamental, 15, sample_rate, seconds),
        VoiceType::CtpianoDkSquare | VoiceType::CtpianoLoSquare => {
            signal += square_bank(fundamental, 15, sample_rate, seconds)
        }
        VoiceType::CtpianoTriangleDrop => {
            signal += triangle_bank(fundamental * 0.5, 30, sample_rate, seconds) * 0.2;
            signal += triangle_bank(fundamental * 0.25, 30, sample_rate, seconds) * 0.2;
            signal += triangle_bank(fundamental, 30, sample_rate, seconds) * 0.4;
            signal += sine(fundamental * 0.25, 0.2, 0.0, sample_rate, seconds);
        }
        VoiceType::CtpianoEmphaenharm => {
            let source_sample = seconds * SOURCE_SAMPLE_RATE;
            signal += triangle_bank(fundamental * 0.5, 30, sample_rate, seconds)
                * 0.2
                * 0.9995_f32.powf(source_sample);
            signal += triangle_bank(fundamental * 0.25, 30, sample_rate, seconds)
                * 0.2
                * 0.99935_f32.powf(source_sample);
            signal += triangle_bank(fundamental, 30, sample_rate, seconds)
                * 0.2
                * 0.9996_f32.powf(source_sample);
            signal +=
                sine(fundamental, 0.4, 0.0, sample_rate, seconds) * 0.9997_f32.powf(source_sample);
        }
        VoiceType::CtpianoBars => {
            signal += ctpiano_bars(fundamental, sample_rate, seconds);
        }
        VoiceType::LegacyNoitechEnharmonic => {
            signal += legacy_noitech_enharmonic(fundamental, sample_rate, seconds);
        }
        VoiceType::Sin
        | VoiceType::Saw
        | VoiceType::HarmonicSaw
        | VoiceType::NoitechBellA
        | VoiceType::NoitechBellB
        | VoiceType::RadlerDullSaw
        | VoiceType::RadlerHarmonics
        | VoiceType::SurgeXtPiano
        | VoiceType::SurgeXtDistortedElectricGuitar
        | VoiceType::SurgeXtClarinet => unreachable!("voice does not use the recovered runtime"),
    }

    let source_ramp = matches!(
        voice_type,
        VoiceType::NoitechBellG
            | VoiceType::NoitechBellH
            | VoiceType::NoitechBellI
            | VoiceType::NoitechBellJ
            | VoiceType::NoitechBellK
            | VoiceType::NoitechBellL
            | VoiceType::NoitechBellM
            | VoiceType::IconoclastBellG
            | VoiceType::IconoclastBellH
            | VoiceType::IconoclastIndustrialBar
    );
    if source_ramp {
        signal * outer_ramp(sample, duration_samples, sample_rate)
    } else {
        signal
    }
}

fn partial_bank(
    fundamental: f32,
    sample_rate: f32,
    seconds: f32,
    sample: u32,
    body_seconds: f32,
    partials: &[Partial],
) -> f32 {
    partials
        .iter()
        .filter_map(|partial| {
            let partial_samples = (body_seconds * partial.duration * sample_rate).round() as u32;
            let frequency = fundamental * partial.ratio;
            (sample < partial_samples && frequency < sample_rate * 0.5 * NYQUIST_MARGIN).then(
                || {
                    let fade = (1.0 - sample as f32 / partial_samples.max(1) as f32)
                        .powi(partial.fade_power);
                    sine(
                        frequency,
                        partial.amplitude * fade,
                        partial.phase,
                        sample_rate,
                        seconds,
                    )
                },
            )
        })
        .sum()
}

fn saw_bank(fundamental: f32, count: u32, sample_rate: f32, seconds: f32) -> f32 {
    (1..=count)
        .map(|harmonic| {
            sine(
                fundamental * harmonic as f32,
                1.0 / harmonic as f32,
                0.0,
                sample_rate,
                seconds,
            )
        })
        .sum::<f32>()
        / 2.0
}

fn square_bank(fundamental: f32, count: u32, sample_rate: f32, seconds: f32) -> f32 {
    (0..count)
        .map(|index| 2 * index + 1)
        .map(|harmonic| {
            sine(
                fundamental * harmonic as f32,
                1.0 / harmonic as f32,
                0.0,
                sample_rate,
                seconds,
            )
        })
        .sum::<f32>()
        * 0.5
}

fn triangle_bank(fundamental: f32, count: u32, sample_rate: f32, seconds: f32) -> f32 {
    (0..count)
        .map(|index| 2 * index + 1)
        .enumerate()
        .map(|(index, harmonic)| {
            let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
            let ratio = harmonic as f32 * (1.0 + index as f32 * 0.000_07);
            sine(
                fundamental * ratio,
                sign / (harmonic * harmonic) as f32,
                0.0,
                sample_rate,
                seconds,
            )
        })
        .sum::<f32>()
}

fn ctpiano_bars(fundamental: f32, sample_rate: f32, seconds: f32) -> f32 {
    let source_sample = seconds * SOURCE_SAMPLE_RATE;
    let approximate_key = (12.0 * (fundamental / 25.0).log2()).clamp(0.0, 127.0);
    let register_gain = approximate_key / 140.0;
    let decaying =
        triangle_bank_with_decay(fundamental, 60, source_sample, false, sample_rate, seconds)
            * 0.15
            * register_gain;
    let high_four = triangle_bank_with_decay(
        fundamental * 4.0,
        60,
        source_sample,
        true,
        sample_rate,
        seconds,
    ) * 0.025
        * register_gain;
    let high_eight = triangle_bank_with_decay(
        fundamental * 8.0,
        30,
        source_sample,
        true,
        sample_rate,
        seconds,
    ) * 0.025
        * register_gain;
    let high_three = triangle_bank_with_decay(
        fundamental * 3.0,
        30,
        source_sample,
        true,
        sample_rate,
        seconds,
    ) * 0.075
        * register_gain;
    decaying
        + high_four
        + high_eight
        + high_three
        + sine(fundamental, 0.6, 0.5, sample_rate, seconds)
}

fn triangle_bank_with_decay(
    fundamental: f32,
    count: u32,
    source_sample: f32,
    decay_on_fundamental: bool,
    sample_rate: f32,
    seconds: f32,
) -> f32 {
    (0..count)
        .map(|index| {
            let harmonic = 2 * index + 1;
            let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
            let ratio = harmonic as f32 * (1.0 + index as f32 * 0.000_07);
            let decay = if index == 0 && !decay_on_fundamental {
                1.0 - 8_481.0 / (8_481.0 + count as f32 * 3.0 * source_sample)
            } else {
                8_481.0 / (481.0 + source_sample * count as f32) * 2.0
            };
            sine(
                fundamental * ratio,
                sign * decay / (harmonic * harmonic) as f32,
                0.0,
                sample_rate,
                seconds,
            )
        })
        .sum()
}

fn legacy_noitech_enharmonic(fundamental: f32, sample_rate: f32, seconds: f32) -> f32 {
    const HARMONIC_COUNT: u32 = 24;
    let source_sample = seconds * SOURCE_SAMPLE_RATE;
    let decay = 4_410.0 / (4_410.0 + source_sample * HARMONIC_COUNT as f32);
    (0..HARMONIC_COUNT)
        .map(|index| {
            let harmonic = 2 * index + 1;
            let sign = if index % 2 == 0 { 1.0 } else { -1.0 };
            let ratio = harmonic as f32 * (1.0 + index as f32 * 0.0013);
            sine(
                fundamental * ratio,
                sign * decay / (harmonic * harmonic) as f32,
                0.0,
                sample_rate,
                seconds,
            )
        })
        .sum()
}

fn sine(frequency: f32, amplitude: f32, phase: f32, sample_rate: f32, seconds: f32) -> f32 {
    if frequency >= sample_rate * 0.5 * NYQUIST_MARGIN {
        0.0
    } else {
        (std::f32::consts::TAU * (frequency * seconds + phase)).sin() * amplitude
    }
}

fn duration_seconds(voice_type: VoiceType) -> f32 {
    match voice_type {
        VoiceType::NoitechBellG
        | VoiceType::NoitechBellH
        | VoiceType::NoitechBellI
        | VoiceType::NoitechBellJ
        | VoiceType::NoitechBellK
        | VoiceType::NoitechBellL
        | VoiceType::NoitechBellM
        | VoiceType::IconoclastBellG
        | VoiceType::IconoclastBellH => 4.0,
        VoiceType::IconoclastIndustrialBar => 3.0,
        VoiceType::CtpianoDkSquare => 0.04,
        VoiceType::CtpianoBars
        | VoiceType::CtpianoEmphaenharm
        | VoiceType::CtpianoHiSaw
        | VoiceType::CtpianoLoSaw
        | VoiceType::CtpianoLoSquare
        | VoiceType::CtpianoTriangleDrop
        | VoiceType::LegacyNoitechEnharmonic => 2.0,
        _ => unreachable!("voice does not use the recovered runtime"),
    }
}

fn duration_samples(voice_type: VoiceType, sample_rate: f32) -> u32 {
    (duration_seconds(voice_type) * sample_rate)
        .round()
        .max(1.0) as u32
}

fn outer_ramp(sample: u32, length: u32, sample_rate: f32) -> f32 {
    let ramp = (SOURCE_RAMP_SAMPLES * sample_rate / SOURCE_SAMPLE_RATE)
        .round()
        .max(1.0) as u32;
    let ramp = ramp.min(length.saturating_div(2).max(1));
    let ramp_in = (sample as f32 / ramp as f32).min(1.0);
    let ramp_out = ((length.saturating_sub(sample)) as f32 / ramp as f32).min(1.0);
    ramp_in * ramp_out
}

fn deterministic_detuning(fundamental: f32) -> f32 {
    let mixed = fundamental
        .to_bits()
        .wrapping_mul(0x9e37_79b9)
        .rotate_left(13);
    let unit = (mixed % 10_001) as f32 / 10_000.0;
    1.0 + (unit - 0.5) / 250.0
}

const BELL_G: [Partial; 14] = [
    Partial::new(1.0, 0.30, 0.60).shaped(2, 0.0),
    Partial::new(2.0, 0.15, 0.25).shaped(2, 0.0),
    Partial::new(0.5, 0.15, 1.00).shaped(2, 0.0),
    Partial::new(3.0, 0.075, 0.20).shaped(2, 0.0),
    Partial::new(4.26, 0.0375, 0.30).shaped(3, 0.0),
    Partial::new(5.55, 0.01875, 0.26).shaped(3, 0.0),
    Partial::new(7.02, 0.0094, 0.15).shaped(3, 0.0),
    Partial::new(8.1, 0.00375, 0.14).shaped(3, 0.0),
    Partial::new(9.2, 0.0015, 0.12).shaped(4, 0.0),
    Partial::new(9.2, 0.0006, 0.12).shaped(4, 0.0),
    Partial::new(10.5, 0.00024, 0.10).shaped(4, 0.0),
    Partial::new(11.7, 0.000096, 0.10).shaped(4, 0.0),
    Partial::new(12.42, 0.000038, 0.10).shaped(4, 0.0),
    Partial::new(16.0, 0.000015, 0.10).shaped(4, 0.0),
];
const BELL_H: [Partial; 9] = [
    Partial::new(1.0, 0.30, 0.6).shaped(2, 0.0),
    Partial::new(2.0, 0.15, 0.15).shaped(2, 0.5),
    Partial::new(0.5, 0.15, 1.0).shaped(2, 0.5),
    Partial::new(3.0, 0.075, 0.2).shaped(2, 0.0),
    Partial::new(4.26, 0.0375, 0.3).shaped(3, 0.5),
    Partial::new(5.55, 0.01875, 0.26).shaped(3, 0.0),
    Partial::new(7.02, 0.0094, 0.15).shaped(3, 0.0),
    Partial::new(8.1, 0.00375, 0.14).shaped(4, 0.5),
    Partial::new(9.2, 0.0015, 0.14).shaped(4, 0.0),
];
const BELL_I: [Partial; 8] = [
    BELL_H[0], BELL_H[1], BELL_H[2], BELL_H[3], BELL_H[4], BELL_H[5], BELL_H[6], BELL_H[7],
];
const BELL_J: [Partial; 14] = [
    Partial::new(0.5, 0.25, 1.0),
    Partial::new(1.0, 0.5, 1.0),
    Partial::new(2.0, 0.25, 1.0),
    Partial::new(3.0, 0.125, 1.0),
    Partial::new(4.26, 0.0625, 1.0),
    Partial::new(5.55, 0.03125, 1.0),
    Partial::new(7.02, 0.0156, 1.0),
    Partial::new(8.1, 0.00625, 1.0),
    Partial::new(9.2, 0.0025, 1.0),
    Partial::new(9.2, 0.001, 1.0),
    Partial::new(10.5, 0.0004, 1.0),
    Partial::new(11.7, 0.00016, 1.0),
    Partial::new(12.42, 0.000064, 1.0),
    Partial::new(16.0, 0.000026, 1.0),
];
const BELL_K: [Partial; 7] = [
    Partial::new(1.0, 0.5, 1.0),
    Partial::new(2.0, 0.167, 0.75),
    Partial::new(3.0, 0.056, 0.6),
    Partial::new(4.15, 0.0185, 0.5),
    Partial::new(5.2, 0.0062, 0.4),
    Partial::new(7.02, 0.0021, 0.3),
    Partial::new(8.1, 0.0007, 0.25),
];
const BELL_L: [Partial; 3] = [
    Partial::new(1.0, 0.5, 1.0),
    Partial::new(2.01, 0.125, 0.75),
    Partial::new(4.04, 0.03125, 0.5),
];
const BELL_M: [Partial; 7] = [
    Partial::new(1.0, 0.5, 1.0),
    Partial::new(2.0, 0.1667, 0.8),
    Partial::new(3.01, 0.0556, 0.65),
    Partial::new(4.02, 0.0185, 0.5),
    Partial::new(5.04, 0.00617, 0.4),
    Partial::new(7.1, 0.00206, 0.3),
    Partial::new(8.2, 0.000514, 0.25),
];
const ICON_G: [Partial; 13] = [
    Partial::new(1.0, 1.0, 0.6).shaped(2, 0.0),
    Partial::new(2.0, 0.5, 0.15).shaped(2, 0.0),
    Partial::new(0.5, 0.5, 1.0).shaped(2, 0.0),
    Partial::new(3.0, 0.26, 0.2).shaped(2, 0.0),
    Partial::new(4.26, 0.14, 0.3).shaped(3, 0.0),
    Partial::new(5.55, 0.07, 0.26).shaped(3, 0.0),
    Partial::new(7.02, 0.066, 0.15).shaped(3, 0.0),
    Partial::new(8.1, 0.028, 0.14).shaped(4, 0.0),
    Partial::new(9.2, 0.02, 0.14).shaped(4, 0.0),
    Partial::new(10.5, 0.006, 0.11).shaped(4, 0.0),
    Partial::new(11.6, 0.002, 0.1).shaped(4, 0.0),
    Partial::new(12.7, 0.002, 0.1).shaped(4, 0.0),
    Partial::new(16.3, 0.002, 0.1).shaped(4, 0.0),
];
const ICON_H: [Partial; 9] = BELL_H;
const BAR_R: [Partial; 9] = [
    Partial::new(0.5, 0.5, 0.02),
    Partial::new(1.0, 1.0, 0.2),
    Partial::new(2.0, 0.5, 0.3),
    Partial::new(3.0, 0.26, 0.3).shaped(1, 0.5),
    Partial::new(4.2, 0.14, 0.6),
    Partial::new(5.17, 0.07, 0.2).shaped(1, 0.5),
    Partial::new(7.4, 0.066, 0.4).shaped(1, 0.5),
    Partial::new(8.1, 0.028, 1.0),
    Partial::new(9.2, 0.02, 0.3).shaped(1, 0.5),
];

#[cfg(test)]
mod tests {
    use super::RecoveredVoiceRuntime;
    use crate::voice::VoiceType;

    #[test]
    fn every_recovered_voice_sounds_and_finishes() {
        for voice_type in VoiceType::BUILT_IN {
            if !voice_type.uses_recovered_runtime() {
                continue;
            }
            let mut runtime = RecoveredVoiceRuntime::new(voice_type);
            runtime.trigger(110.0);
            let mut peak = 0.0_f32;
            let mut active = true;
            for _ in 0..200_000 {
                let (sample, is_active) = runtime.sample(44_100.0);
                assert!(sample.is_finite());
                peak = peak.max(sample.abs());
                active = is_active;
                if !active {
                    break;
                }
            }
            assert!(peak > 0.000_001, "{} was silent", voice_type.label());
            assert!(!active, "{} did not finish", voice_type.label());
        }
    }
}
