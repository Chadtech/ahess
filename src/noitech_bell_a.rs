const SOURCE_SAMPLE_RATE: f32 = 44_100.0;
const SOURCE_STRIKE_SAMPLES: u32 = 120;
const SOURCE_RAMP_SAMPLES: u32 = 60;
const SOURCE_BODY_SAMPLES: u32 = 5 * 44_100;
const SQUARE_HARMONIC_COUNT: u32 = 8;
const NYQUIST_MARGIN: f32 = 0.98;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Partial {
    pub(crate) frequency_ratio: f32,
    pub(crate) amplitude: f32,
    pub(crate) fade_out_count: i32,
}

// The profile from bells20150804/buildBells.coffee. The amplitude expressions
// retain every group-level `eff.vol`; fade_out_count records which of the seven
// nested full-body fadeOut operations affect each oscillator.
pub(crate) const PARTIALS: [Partial; 16] = [
    Partial {
        frequency_ratio: 6.7,
        amplitude: 0.5 * 0.5 * 0.333 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 6.58,
        amplitude: 0.09 * 0.333 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 6.66,
        amplitude: 0.11 * 0.333 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 5.3,
        amplitude: 0.23 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 5.37,
        amplitude: 0.08 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 5.34,
        amplitude: 0.12 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 5.385,
        amplitude: 0.07 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 5.325,
        amplitude: 0.13 * 0.15 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 7,
    },
    Partial {
        frequency_ratio: 4.1,
        amplitude: 0.3 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 6,
    },
    Partial {
        frequency_ratio: 4.37,
        amplitude: 0.08 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 6,
    },
    Partial {
        frequency_ratio: 4.34,
        amplitude: 0.12 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 6,
    },
    Partial {
        frequency_ratio: 4.385,
        amplitude: 0.07 * 0.15 * 0.6 * 0.7 * 0.5 * 0.6,
        fade_out_count: 6,
    },
    Partial {
        frequency_ratio: 3.0,
        amplitude: 0.3 * 0.7 * 0.5 * 0.6,
        fade_out_count: 5,
    },
    Partial {
        frequency_ratio: 2.0,
        amplitude: 0.3 * 0.5 * 0.6,
        fade_out_count: 4,
    },
    Partial {
        frequency_ratio: 1.0,
        amplitude: 0.3 * 0.6,
        fade_out_count: 3,
    },
    Partial {
        frequency_ratio: 0.5,
        amplitude: 0.3,
        fade_out_count: 2,
    },
];

#[derive(Clone, Copy)]
struct ActiveBell {
    fundamental: f32,
    sample: u32,
}

pub(crate) struct NoitechBellARuntime {
    active: Vec<ActiveBell>,
}

impl NoitechBellARuntime {
    pub(crate) fn new() -> Self {
        Self {
            active: Vec::with_capacity(32),
        }
    }

    pub(crate) fn trigger(&mut self, fundamental: f32) {
        self.active.push(ActiveBell {
            fundamental,
            sample: 0,
        });
    }

    pub(crate) fn sample(&mut self, sample_rate: f32) -> (f32, bool) {
        let mut output = 0.0;
        for bell in &mut self.active {
            output += bell_sample(bell.fundamental, sample_rate, bell.sample);
            bell.sample += 1;
        }
        let duration = total_length(sample_rate);
        self.active.retain(|bell| bell.sample < duration);
        (output, !self.active.is_empty())
    }
}

fn bell_sample(fundamental: f32, sample_rate: f32, sample: u32) -> f32 {
    let strike_length = source_samples_at_rate(SOURCE_STRIKE_SAMPLES, sample_rate);
    if sample < strike_length {
        return strike(fundamental, sample_rate, sample, strike_length);
    }

    let body_sample = sample - strike_length;
    let body_length = source_samples_at_rate(SOURCE_BODY_SAMPLES, sample_rate);
    if body_sample >= body_length {
        return 0.0;
    }

    sine_body(
        fundamental,
        sample_rate,
        body_sample,
        body_length,
        source_samples_at_rate(SOURCE_RAMP_SAMPLES, sample_rate),
    )
}

fn total_length(sample_rate: f32) -> u32 {
    source_samples_at_rate(SOURCE_STRIKE_SAMPLES, sample_rate)
        + source_samples_at_rate(SOURCE_BODY_SAMPLES, sample_rate)
}

fn source_samples_at_rate(source_samples: u32, sample_rate: f32) -> u32 {
    (source_samples as f32 * sample_rate / SOURCE_SAMPLE_RATE)
        .round()
        .max(1.0) as u32
}

fn strike(fundamental: f32, sample_rate: f32, sample: u32, strike_length: u32) -> f32 {
    let ramp_length = source_samples_at_rate(SOURCE_RAMP_SAMPLES, sample_rate)
        .min(strike_length.saturating_div(2).max(1));
    let ramp_in = (sample as f32 / ramp_length as f32).min(1.0);
    let ramp_out_start = strike_length.saturating_sub(ramp_length);
    let ramp_out = if sample < ramp_out_start {
        1.0
    } else {
        1.0 - (sample - ramp_out_start) as f32 / ramp_length as f32
    };
    let harmonic_adjustment = (4 * (SQUARE_HARMONIC_COUNT - 1)) as f32
        / (((SQUARE_HARMONIC_COUNT - 1).pow(2) + 1) as f32).sqrt()
        / std::f32::consts::PI;
    let normalization = 1.0 - harmonic_adjustment;
    let maximum_frequency = sample_rate * 0.5 * NYQUIST_MARGIN;
    let seconds = sample as f32 / sample_rate;
    let square = (1..=SQUARE_HARMONIC_COUNT)
        .filter_map(|harmonic| {
            let odd = (harmonic * 2 - 1) as f32;
            let frequency = fundamental / 3.0 * odd;
            (frequency < maximum_frequency)
                .then(|| (std::f32::consts::TAU * frequency * seconds).sin() / odd)
        })
        .sum::<f32>();

    square * normalization * ramp_in * ramp_out
}

fn sine_body(
    fundamental: f32,
    sample_rate: f32,
    sample: u32,
    length: u32,
    fade_in_length: u32,
) -> f32 {
    let fade_in = (sample as f32 / fade_in_length as f32).min(1.0);
    let fade_out = 1.0 - sample as f32 / length as f32;
    let maximum_frequency = sample_rate * 0.5 * NYQUIST_MARGIN;
    let seconds = sample as f32 / sample_rate;

    PARTIALS
        .iter()
        .filter_map(|partial| {
            let frequency = fundamental * partial.frequency_ratio;
            (frequency < maximum_frequency).then(|| {
                (std::f32::consts::TAU * frequency * seconds).sin()
                    * partial.amplitude
                    * fade_out.powi(partial.fade_out_count)
            })
        })
        .sum::<f32>()
        * fade_in
}

#[cfg(test)]
mod tests {
    use super::{
        bell_sample, source_samples_at_rate, strike, total_length, NoitechBellARuntime, Partial,
        PARTIALS, SOURCE_BODY_SAMPLES, SOURCE_RAMP_SAMPLES, SOURCE_STRIKE_SAMPLES,
    };

    #[test]
    fn profile_retains_the_coffeescript_sine_ratios_volumes_and_fades() {
        let expected = [
            (6.7, 0.000_236_013_75, 7),
            (6.58, 0.000_084_964_95, 7),
            (6.66, 0.000_103_846_05, 7),
            (5.3, 0.000_652_05, 7),
            (5.37, 0.000_226_8, 7),
            (5.34, 0.000_340_2, 7),
            (5.385, 0.000_198_45, 7),
            (5.325, 0.000_368_55, 7),
            (4.1, 0.005_67, 6),
            (4.37, 0.001_512, 6),
            (4.34, 0.002_268, 6),
            (4.385, 0.001_323, 6),
            (3.0, 0.063, 5),
            (2.0, 0.09, 4),
            (1.0, 0.18, 3),
            (0.5, 0.3, 2),
        ]
        .map(|(frequency_ratio, amplitude, fade_out_count)| Partial {
            frequency_ratio,
            amplitude,
            fade_out_count,
        });

        for (actual, expected) in PARTIALS.iter().zip(expected) {
            assert_eq!(actual.frequency_ratio, expected.frequency_ratio);
            assert!((actual.amplitude - expected.amplitude).abs() < 1e-8);
            assert_eq!(actual.fade_out_count, expected.fade_out_count);
        }
    }

    #[test]
    fn strike_is_the_ramped_eight_harmonic_coffeescript_square() {
        let sample_rate = 44_100.0;
        let strike_length = source_samples_at_rate(SOURCE_STRIKE_SAMPLES, sample_rate);
        assert_eq!(strike(277.0, sample_rate, 0, strike_length), 0.0);
        assert_ne!(strike(277.0, sample_rate, 30, strike_length), 0.0);
        assert_ne!(strike(277.0, sample_rate, 60, strike_length), 0.0);
        assert_ne!(strike(277.0, sample_rate, 90, strike_length), 0.0);
    }

    #[test]
    fn body_keeps_the_five_second_source_duration_and_fade_in_timing() {
        let sample_rate = 44_100.0;
        let strike_length = source_samples_at_rate(SOURCE_STRIKE_SAMPLES, sample_rate);
        let fade_in_length = source_samples_at_rate(SOURCE_RAMP_SAMPLES, sample_rate);
        assert_eq!(total_length(sample_rate), 120 + SOURCE_BODY_SAMPLES);
        assert_eq!(bell_sample(103.0, sample_rate, strike_length), 0.0);
        assert_ne!(
            bell_sample(103.0, sample_rate, strike_length + fade_in_length),
            0.0
        );
    }

    #[test]
    fn successive_triggers_overlap_instead_of_cutting_off_the_decay() {
        let mut runtime = NoitechBellARuntime::new();
        runtime.trigger(103.0);
        runtime.sample(44_100.0);
        runtime.trigger(137.0);

        assert_eq!(runtime.active.len(), 2);
        let (_, active) = runtime.sample(44_100.0);
        assert!(active);
    }
}
