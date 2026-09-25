//! Shared allocation-free, frequency-native PCM note playback.
use crate::voice::AttackSharpness;
const SOURCE_RATE: f64 = 44_100.0;

pub(super) struct Sample {
    pub(super) reference_hz: f64,
    // Successive levels are low-pass filtered and decimated by powers of two.
    pub(super) levels: &'static [&'static [u8]],
    pub(super) sustain: Option<SampleLoop>,
}

#[derive(Clone, Copy)]
pub(super) struct SampleLoop {
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) crossfade: f64,
}

impl SampleLoop {
    pub(super) fn scaled(self, factor: f64) -> Self {
        Self {
            start: self.start / factor,
            end: self.end / factor,
            crossfade: self.crossfade / factor,
        }
    }
}

#[derive(Clone, Copy)]
enum Release {
    Natural,
    Sustain { gate: u64, length: u64 },
    Cutoff { end: u64, fade: u64 },
}

#[derive(Clone, Copy)]
pub(super) struct SampleNote {
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
    pub(super) fn new(
        sample: &'static Sample,
        sample_rate: f64,
        hz: f64,
        volume: f32,
        gate: u32,
        explicit: bool,
        attack: AttackSharpness,
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

    pub(super) fn next(&mut self) -> Option<f32> {
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
