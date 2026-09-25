//! Real picked strings, retaining one coherent transient per note.
use super::sampled_note::{Sample, SampleNote};
use crate::{pitch_system::FrequencyHz, seed::Seed, voice::AttackSharpness};

#[path = "clean_guitar_samples.rs"]
mod samples;

struct GuitarPitch {
    nominal_hz: f64,
    medium: [Sample; 2],
    firm: [Sample; 2],
}

pub(crate) struct CleanGuitarRuntime {
    sample_rate: f64,
    notes: [Option<SampleNote>; 128],
}

impl CleanGuitarRuntime {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: f64::from(sample_rate),
            notes: [None; 128],
        }
    }

    pub(crate) fn trigger(
        &mut self,
        frequency: FrequencyHz,
        volume: f32,
        gate: u32,
        explicit: bool,
        seed: Seed,
    ) {
        let hz = frequency.as_hz();
        if volume <= 0.0 || hz >= self.sample_rate * 0.45 {
            return;
        }
        let Some(slot) = self.notes.iter_mut().find(|n| n.is_none()) else {
            return;
        };
        let pitch = samples::GUITAR
            .iter()
            .min_by(|a, b| {
                (hz / a.nominal_hz)
                    .log2()
                    .abs()
                    .total_cmp(&(hz / b.nominal_hz).log2().abs())
            })
            .expect("embedded guitar bank is nonempty");
        // Select the articulation, never crossfade unrelated pick waveforms.
        let takes = if volume >= 0.65 {
            &pitch.firm
        } else {
            &pitch.medium
        };
        let take = (seed.next_u64().0 >> 63) as usize;
        *slot = Some(SampleNote::new(
            &takes[take],
            self.sample_rate,
            hz,
            volume,
            gate,
            explicit,
            AttackSharpness::default(),
        ));
    }

    pub(crate) fn sample(&mut self) -> (f32, bool) {
        let mut value = 0.0;
        let mut active = false;
        for slot in &mut self.notes {
            if let Some(note) = slot {
                match note.next() {
                    Some(x) => {
                        value += x;
                        active = true;
                    }
                    None => *slot = None,
                }
            }
        }
        (value, active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(hz: f64, volume: f32, seed: u64, rate: u32, explicit: bool) -> Vec<f32> {
        let mut r = CleanGuitarRuntime::new(rate as f32);
        r.trigger(
            FrequencyHz::new(hz).unwrap(),
            volume,
            rate / 2,
            explicit,
            Seed::new(seed),
        );
        (0..rate).map(|_| r.sample().0).collect()
    }

    #[test]
    fn guitar_preserves_pick_attack_and_damps_explicit_notes() {
        for hz in [87.3, 173.1, 271.0, 453.25, 790.0] {
            let x = render(hz, 0.9, 1, 48_000, false);
            let peak = |s: &[f32]| s.iter().map(|v| v.abs()).fold(0.0_f32, f32::max);
            assert!(peak(&x[..2400]) > 0.1, "missing pick attack at {hz}");
            assert!(x.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
            let damped = render(hz, 0.9, 1, 48_000, true);
            assert_eq!(&x[..23_760], &damped[..23_760]);
            assert!(damped[23_999].abs() < 0.00005);
            assert!(damped[24_000..].iter().all(|x| *x == 0.0));
            assert!(peak(&x[24_000..]) > 0.001);
        }
    }

    #[test]
    fn guitar_retunes_real_recordings_to_arbitrary_frequencies() {
        for rate in [44_100, 48_000, 96_000] {
            for hz in [87.3, 173.1, 271.0, 453.25, 790.0] {
                for volume in [0.45, 0.9] {
                    let audio = render(hz, volume, 1, rate, false);
                    let x = &audio[rate as usize / 10..rate as usize / 2];
                    let corr = |lag: usize| {
                        x[..x.len() - lag]
                            .iter()
                            .zip(&x[lag..])
                            .map(|(a, b)| f64::from(*a) * f64::from(*b))
                            .sum::<f64>()
                            / (x.len() - lag) as f64
                    };
                    let lo = (f64::from(rate) / (hz * 1.04)) as usize;
                    let hi = (f64::from(rate) / (hz * 0.96)) as usize;
                    let lag = (lo..=hi)
                        .max_by(|a, b| corr(*a).total_cmp(&corr(*b)))
                        .unwrap();
                    assert!(lag > lo && lag < hi);
                    let (a, b, c) = (corr(lag - 1), corr(lag), corr(lag + 1));
                    let measured =
                        f64::from(rate) / (lag as f64 + 0.5 * (a - c) / (a - 2.0 * b + c));
                    let cents = 1200.0 * (measured / hz).log2();
                    assert!(cents.abs() < 8.0, "{rate}, {hz}, {volume}: {cents} cents");
                }
            }
        }
    }

    #[test]
    fn guitar_takes_are_seeded_and_overlap_independently() {
        let first = render(271.0, 0.9, 1, 48_000, false);
        assert_eq!(first, render(271.0, 0.9, 1, 48_000, false));
        assert!((2..10).any(|seed| render(271.0, 0.9, seed, 48_000, false) != first));
        let mut a = CleanGuitarRuntime::new(48_000.0);
        let mut b = CleanGuitarRuntime::new(48_000.0);
        let mut both = CleanGuitarRuntime::new(48_000.0);
        for r in [&mut a, &mut both] {
            r.trigger(
                FrequencyHz::new(271.0).unwrap(),
                0.45,
                20_000,
                true,
                Seed::new(1),
            );
        }
        for i in 0..48_000 {
            if i == 4000 {
                for r in [&mut b, &mut both] {
                    r.trigger(
                        FrequencyHz::new(453.25).unwrap(),
                        0.9,
                        30_000,
                        true,
                        Seed::new(2),
                    );
                }
            }
            assert_eq!(both.sample().0, a.sample().0 + b.sample().0);
        }
    }
}
