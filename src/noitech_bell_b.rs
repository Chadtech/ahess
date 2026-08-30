const SOURCE_SAMPLE_RATE: f32 = 44_100.0;
const SOURCE_BODY_SAMPLES: u32 = 4 * 44_100;
const SOURCE_RAMP_SAMPLES: u32 = 60;
const NYQUIST_MARGIN: f32 = 0.98;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Partial {
    pub(crate) frequency_ratio: f32,
    pub(crate) amplitude: f32,
    pub(crate) source_duration_samples: u32,
}

// The seven audible sine arrays from bells20150804/buildBellsB.coffee. Each is
// faded over its own array and then multiplied by 0.7 before the final mix.
// The source's fourteen `enharmonics` pass 0.03..0.13 directly as sample counts;
// generate.sine therefore emits one sample at phase zero for each, contributing
// silence to the rendered waveform.
pub(crate) const PARTIALS: [Partial; 7] = [
    Partial {
        frequency_ratio: 1.0,
        amplitude: 0.5 * 0.7,
        source_duration_samples: 105_840,
    },
    Partial {
        frequency_ratio: 2.0,
        amplitude: 0.25 * 0.7,
        source_duration_samples: 79_380,
    },
    Partial {
        frequency_ratio: 0.5,
        amplitude: 0.25 * 0.7,
        source_duration_samples: SOURCE_BODY_SAMPLES,
    },
    Partial {
        frequency_ratio: 3.0,
        amplitude: 0.125 * 0.7,
        source_duration_samples: 70_560,
    },
    Partial {
        frequency_ratio: 4.26,
        amplitude: 0.0625 * 0.7,
        source_duration_samples: 67_032,
    },
    Partial {
        frequency_ratio: 5.55,
        amplitude: 0.03125 * 0.7,
        source_duration_samples: 56_448,
    },
    Partial {
        frequency_ratio: 7.02,
        amplitude: 0.015625 * 0.7,
        source_duration_samples: 52_920,
    },
];

#[derive(Clone, Copy)]
struct ActiveBell {
    fundamental: f32,
    sample: u32,
}

pub(crate) struct NoitechBellBRuntime {
    active: Vec<ActiveBell>,
}

impl NoitechBellBRuntime {
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
        let duration = source_samples_at_rate(SOURCE_BODY_SAMPLES, sample_rate);
        self.active.retain(|bell| bell.sample < duration);
        (output, !self.active.is_empty())
    }
}

fn bell_sample(fundamental: f32, sample_rate: f32, sample: u32) -> f32 {
    let body_length = source_samples_at_rate(SOURCE_BODY_SAMPLES, sample_rate);
    if sample >= body_length {
        return 0.0;
    }

    let maximum_frequency = sample_rate * 0.5 * NYQUIST_MARGIN;
    let seconds = sample as f32 / sample_rate;
    let signal = PARTIALS
        .iter()
        .filter_map(|partial| {
            let duration = source_samples_at_rate(partial.source_duration_samples, sample_rate);
            let frequency = fundamental * partial.frequency_ratio;
            (sample < duration && frequency < maximum_frequency).then(|| {
                let fade_out = 1.0 - sample as f32 / duration as f32;
                (std::f32::consts::TAU * frequency * seconds).sin() * partial.amplitude * fade_out
            })
        })
        .sum::<f32>();

    signal * outer_ramp(sample, body_length, sample_rate)
}

fn outer_ramp(sample: u32, length: u32, sample_rate: f32) -> f32 {
    let ramp_length = source_samples_at_rate(SOURCE_RAMP_SAMPLES, sample_rate)
        .min(length.saturating_div(2).max(1));
    let ramp_in = (sample as f32 / ramp_length as f32).min(1.0);
    let ramp_out_start = length.saturating_sub(ramp_length);
    let ramp_out = if sample < ramp_out_start {
        1.0
    } else {
        1.0 - (sample - ramp_out_start) as f32 / ramp_length as f32
    };
    ramp_in * ramp_out
}

fn source_samples_at_rate(source_samples: u32, sample_rate: f32) -> u32 {
    (source_samples as f32 * sample_rate / SOURCE_SAMPLE_RATE)
        .round()
        .max(1.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{bell_sample, NoitechBellBRuntime, Partial, PARTIALS, SOURCE_BODY_SAMPLES};

    #[test]
    fn profile_retains_the_coffeescript_ratios_volumes_and_durations() {
        let expected = [
            (1.0, 0.35, 105_840),
            (2.0, 0.175, 79_380),
            (0.5, 0.175, 176_400),
            (3.0, 0.0875, 70_560),
            (4.26, 0.04375, 67_032),
            (5.55, 0.021875, 56_448),
            (7.02, 0.0109375, 52_920),
        ]
        .map(
            |(frequency_ratio, amplitude, source_duration_samples)| Partial {
                frequency_ratio,
                amplitude,
                source_duration_samples,
            },
        );

        assert_eq!(PARTIALS, expected);
    }

    #[test]
    fn body_keeps_the_four_second_source_duration_and_outer_ramp() {
        let sample_rate = 44_100.0;
        assert_eq!(bell_sample(103.0, sample_rate, 0), 0.0);
        assert_ne!(bell_sample(103.0, sample_rate, 60), 0.0);
        assert_eq!(bell_sample(103.0, sample_rate, SOURCE_BODY_SAMPLES), 0.0);
    }

    #[test]
    fn successive_triggers_overlap_instead_of_cutting_off_the_decay() {
        let mut runtime = NoitechBellBRuntime::new();
        runtime.trigger(103.0);
        runtime.sample(44_100.0);
        runtime.trigger(137.0);

        assert_eq!(runtime.active.len(), 2);
        let (_, active) = runtime.sample(44_100.0);
        assert!(active);
    }
}
