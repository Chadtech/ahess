//! Deterministic single-reed / cylindrical-bore clarinet.
//!
//! Original DSP informed by Julius O. Smith's single-reed waveguide and UNSW
//! clarinet acoustics. This is a reduced physical model, not measured samples.
//! The controlled pressure range deliberately avoids overblowing bifurcations.
//! Radiation voicing is informed by John Valentine's CC0 Surge Clarinet preset:
//! soft shaping, fixed body resonances, and breath-dependent spectral balance.
//! No Surge implementation code is incorporated.

const OVERSAMPLE: usize = 4;
const MIN_FREQUENCY: f32 = 20.0;
const TAU: f32 = std::f32::consts::TAU;

pub(crate) struct ClarinetRuntime {
    notes: [ReedBore; 2],
    next: usize,
}

impl ClarinetRuntime {
    pub(crate) fn new(sample_rate: f32) -> Self {
        Self {
            notes: std::array::from_fn(|_| ReedBore::new(sample_rate)),
            next: 0,
        }
    }

    pub(crate) fn trigger(&mut self, frequency: f32, volume: f32, gate_samples: u32) {
        // A wind player articulates one pitch at a time. Retain a short outgoing
        // release to avoid cutting a pressurized bore in a single sample.
        for note in &mut self.notes {
            note.release();
        }
        self.notes[self.next].start(frequency, volume, gate_samples);
        self.next = 1 - self.next;
    }

    pub(crate) fn sample(&mut self) -> (f32, bool) {
        let output = self.notes.iter_mut().map(ReedBore::sample).sum();
        (output, self.notes.iter().any(|note| note.life.is_some()))
    }
}

struct Life {
    age: u64,
    gate: u64,
    end: u64,
    forced_release: Option<u64>,
}

struct ReedBore {
    rate: f32,
    bore: Vec<f32>,
    write: usize,
    delay: usize,
    fraction: f32,
    previous: f32,
    loss: f32,
    loss_pole: f32,
    noise: f32,
    noise_pole: f32,
    rng: u32,
    dc_input: f32,
    dc_output: f32,
    dc_pole: f32,
    radiation: f32,
    square_dc: f32,
    radiation_pole: f32,
    filters: [Biquad; 4],
    volume: f32,
    pressure: f32,
    register: f32,
    attack: f32,
    color: [Biquad; 3],
    breath_color: Biquad,
    articulation: f32,
    articulation_decay: f32,
    expression_seed: u32,
    life: Option<Life>,
}

impl ReedBore {
    fn new(rate: f32) -> Self {
        let internal_rate = rate * OVERSAMPLE as f32;
        Self {
            rate,
            bore: vec![0.0; (internal_rate / (2.0 * MIN_FREQUENCY)).ceil() as usize + 8],
            write: 0,
            delay: 1,
            fraction: 0.0,
            previous: 0.0,
            loss: 0.0,
            loss_pole: (-TAU * 6500.0 / internal_rate).exp(),
            noise: 0.0,
            noise_pole: (-TAU * 1800.0 / internal_rate).exp(),
            rng: 0x6d2b_79f5,
            dc_input: 0.0,
            dc_output: 0.0,
            dc_pole: (-TAU * 12.0 / internal_rate).exp(),
            radiation: 0.0,
            square_dc: 0.0,
            radiation_pole: (-TAU * 950.0 / internal_rate).exp(),
            filters: [0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_6]
                .map(|q| Biquad::new(0.42 * rate / internal_rate, q)),
            volume: 0.0,
            pressure: 0.0,
            register: 0.0,
            attack: 1.0,
            color: [
                Biquad::peak(900.0 / internal_rate, 1.1, 5.0),
                Biquad::peak(1450.0 / internal_rate, 1.5, 3.0),
                Biquad::peak(2650.0 / internal_rate, 1.0, 4.0),
            ],
            breath_color: Biquad::new((5500.0 / internal_rate).min(0.2), 0.707),
            articulation: 1.0,
            articulation_decay: (-1.0 / (0.065 * rate)).exp(),
            expression_seed: 1,
            life: None,
        }
    }

    fn start(&mut self, frequency: f32, volume: f32, gate_samples: u32) {
        self.life = None;
        // Bound memory and avoid folding ultrasonic input into audible notes.
        if !frequency.is_finite()
            || !(MIN_FREQUENCY..self.rate * 0.4).contains(&frequency)
            || !volume.is_finite()
            || volume <= 0.0
        {
            return;
        }
        self.write = 0;
        self.previous = 0.0;
        self.loss = 0.0;
        self.noise = 0.0;
        self.rng = 0x6d2b_79f5; // Repeatable turbulence, independent of history.
        self.dc_input = 0.0;
        self.dc_output = 0.0;
        self.radiation = 0.0;
        self.square_dc = 0.0;
        for filter in &mut self.filters {
            filter.clear();
        }
        self.volume = volume.clamp(0.0, 1.0);
        self.pressure = 0.58 + 0.22 * self.volume.sqrt();
        self.register = ((frequency / 390.0).log2() / 1.5).clamp(0.0, 1.0);
        self.expression_seed = frequency.to_bits() ^ 0x41c6_4e6d;
        self.articulation = 1.0;
        let internal_rate = self.rate * OVERSAMPLE as f32;
        // Like JV's Bell Aah controls, shape radiation at fixed acoustic
        // frequencies rather than moving every feature with the fundamental.
        // These deliberately broad, restrained peaks are original voicing.
        let strength = self.volume.sqrt();
        self.color = [
            Biquad::peak((900.0 / internal_rate).min(0.2), 1.1, 3.0 + 4.0 * strength),
            Biquad::peak((1450.0 / internal_rate).min(0.2), 1.5, 1.0 + 3.0 * strength),
            Biquad::peak((2650.0 / internal_rate).min(0.2), 1.0, 1.0 + 5.0 * strength),
        ];
        self.breath_color.clear();
        // Tonguing releases an already supported air column. The audible ramp
        // is independent of the self-oscillator's much slower startup growth.
        self.attack = (self.rate * (0.005 + 0.003 * (1.0 - strength)))
            .min(gate_samples.max(1) as f32 * 0.2)
            .max(1.0);
        // The sign-inverting round trip takes half a period. Compensate the
        // actual loss-filter phase, including the half-sample averaging delay.
        let omega = TAU * frequency / (self.rate * OVERSAMPLE as f32);
        let filter_phase = (self.loss_pole * omega.sin()).atan2(1.0 - self.loss_pole * omega.cos());
        let delay = (std::f32::consts::PI / omega - 0.5 - filter_phase / omega).max(1.0);
        self.delay = delay.floor() as usize;
        self.fraction = delay.fract();
        // Seed the fundamental coherently through the delay history. A quiet,
        // unseeded loop can otherwise take hundreds of milliseconds to speak.
        // This bounded initial condition excites the requested mode without
        // pre-rendering audio or allocating on note-on.
        let amplitude = 0.35 + 0.30 * strength;
        let length = self.bore.len();
        for (i, sample) in self.bore.iter_mut().enumerate() {
            *sample = amplitude * (omega * (i as f32 - length as f32)).sin();
        }
        let gate = u64::from(gate_samples.max(1));
        self.life = Some(Life {
            age: 0,
            gate,
            end: gate + (self.rate * 0.080) as u64,
            forced_release: None,
        });
    }

    fn release(&mut self) {
        if let Some(life) = &mut self.life {
            if life.forced_release.is_none() {
                life.forced_release = Some(life.age);
                life.end = life.end.min(life.age + (self.rate * 0.012) as u64);
            }
        }
    }

    fn sample(&mut self) -> f32 {
        let Some(life) = &mut self.life else {
            return 0.0;
        };
        if life.age >= life.end {
            self.life = None;
            return 0.0;
        }
        let age = life.age as f32;
        let attack = smooth(age / self.attack);
        let release_length = (self.rate * 0.025).min(life.gate as f32 * 0.3).max(1.0);
        let seconds = age / self.rate;
        // Smooth, aperiodic breath expression can be deterministic. Avoid a
        // conspicuous periodic vibrato or independently detuned oscillators.
        let breath_motion = 0.016 * smooth_noise(seconds * 2.3, self.expression_seed)
            + 0.008 * smooth_noise(seconds * 7.1, self.expression_seed ^ 0x9e37_79b9);
        let breath = smooth((life.gate as f32 - age) / release_length);
        let support = 1.0 + breath_motion + 0.025 * self.articulation * attack;
        let noise_strength = 0.010 + 0.010 * (1.0 - self.volume.sqrt()) + 0.035 * self.articulation;
        self.articulation *= self.articulation_decay;
        let outgoing = life.forced_release.map_or(1.0, |start| {
            1.0 - smooth((life.age - start) as f32 / (life.end - start).max(1) as f32)
        });
        let tail = smooth((life.end - life.age) as f32 / (self.rate * 0.010));
        life.age += 1;
        let mut output = 0.0;
        for _ in 0..OVERSAMPLE {
            let read = (self.write + self.bore.len() - self.delay) % self.bore.len();
            let older = (read + self.bore.len() - 1) % self.bore.len();
            let wave = self.bore[read] * (1.0 - self.fraction) + self.bore[older] * self.fraction;
            let averaged = 0.5 * (wave + self.previous);
            self.previous = wave;
            self.loss = averaged + self.loss_pole * (self.loss - averaged);
            let reflected = -0.97 * self.loss;
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 17;
            self.rng ^= self.rng << 5;
            let white = self.rng as f32 / u32::MAX as f32 * 2.0 - 1.0;
            self.noise = white + self.noise_pole * (self.noise - white);
            let mouth = self.pressure * breath * outgoing * (1.0 + 0.006 * self.noise);
            let difference = reflected - mouth;
            // A bounded closing-reed reflection curve: the mouth supplies
            // energy, while the pressure-dependent aperture limits oscillation.
            let reed = (0.7 - 0.3 * difference).clamp(-1.0, 1.0);
            self.bore[self.write] = mouth + difference * reed;
            self.write = (self.write + 1) % self.bore.len();
            let ac = wave - self.dc_input + self.dc_pole * self.dc_output;
            self.dc_input = wave;
            self.dc_output = ac;
            // A bounded reed-flow shaper supplies upper-register harmonics that
            // the lossy bore alone underproduces. The drive grows with register
            // and blowing strength, inspired by the Surge patch's soft shaper.
            // Keeping it outside feedback cannot trigger a different bore mode.
            let drive = 1.0 + self.register * (1.0 + self.volume.sqrt());
            let voiced = (ac * drive).tanh() / drive.sqrt();
            self.radiation = voiced + self.radiation_pole * (self.radiation - voiced);
            // Tone-hole radiation brightens the bore signal. The upper register
            // has more even-harmonic energy; this weak asymmetric term is an
            // empirical voicing approximation, not a register-hole simulation.
            self.square_dc = voiced * voiced + self.dc_pole * (self.square_dc - voiced * voiced);
            let radiated = 0.45 * voiced
                + 0.55 * (voiced - self.radiation)
                + (0.04 + 0.65 * self.register) * (voiced * voiced - self.square_dc)
                + noise_strength
                    * breath
                    * self.breath_color.sample(white - self.noise)
                    * (0.65 + 0.35 * (1.0 - reed.abs()));
            output = radiated * support;
            for filter in &mut self.color {
                output = filter.sample(output);
            }
            for filter in &mut self.filters {
                output = filter.sample(output);
            }
        }
        output * self.volume * attack * outgoing * tail * 0.36
    }
}

// Interpolated deterministic noise for breathing gestures. Evaluate in seconds
// so the motion has the same bandwidth at every output sample rate.
fn smooth_noise(position: f32, seed: u32) -> f32 {
    let index = position.floor() as u32;
    let hash = |index: u32| {
        let mut x = index.wrapping_add(seed);
        x = (x ^ (x >> 16)).wrapping_mul(0x7feb_352d);
        x = (x ^ (x >> 15)).wrapping_mul(0x846c_a68b);
        ((x ^ (x >> 16)) as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let a = hash(index);
    a + (hash(index.wrapping_add(1)) - a) * smooth(position.fract())
}

fn smooth(value: f32) -> f32 {
    let x = value.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}
impl Biquad {
    // Standard peaking-EQ biquad (gain in dB), independently implemented.
    fn peak(cycles: f32, q: f32, db: f32) -> Self {
        let omega = TAU * cycles;
        let a = 10.0_f32.powf(db / 40.0);
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * omega.cos() / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * omega.cos() / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn new(cycles: f32, q: f32) -> Self {
        let omega = TAU * cycles;
        let cosine = omega.cos();
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cosine) / (2.0 * a0),
            b1: (1.0 - cosine) / a0,
            b2: (1.0 - cosine) / (2.0 * a0),
            a1: -2.0 * cosine / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }
    fn clear(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
    fn sample(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(rate: f32, frequency: f32, volume: f32, seconds: f32) -> Vec<f32> {
        let mut runtime = ClarinetRuntime::new(rate);
        runtime.trigger(frequency, volume, (rate * seconds) as u32);
        (0..(rate * (seconds + 0.1)) as usize)
            .map(|_| runtime.sample().0)
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn tuning_stays_in_register_at_all_build_rates_and_dynamics() {
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            for frequency in [146.83, 256.0, 259.0, 384.0, 480.0, 880.0, 1568.0] {
                for volume in [1.0 / 255.0, 0.25, 1.0] {
                    let samples = render(rate, frequency, volume, 1.3);
                    assert!(samples.iter().all(|x| x.is_finite() && x.abs() < 1.0));
                    let sustain = &samples[(rate * 0.7) as usize..(rate * 1.2) as usize];
                    assert!(
                        rms(sustain) > volume * 0.05,
                        "silent {rate} {frequency} {volume}"
                    );
                    // Isolate the fundamental before counting crossings: realistic
                    // loud low notes can have several crossings per period from
                    // strong upper partials. Require substantial fundamental energy
                    // so an overblown note cannot pass via tiny residual noise.
                    let mut filters =
                        [0.541_196_1, 1.306_563].map(|q| Biquad::new(frequency * 1.05 / rate, q));
                    let filtered: Vec<_> = samples
                        .iter()
                        .map(|&x| filters.iter_mut().fold(x, |x, filter| filter.sample(x)))
                        .collect();
                    let fundamental = &filtered[(rate * 0.7) as usize..(rate * 1.2) as usize];
                    assert!(
                        rms(fundamental) > rms(sustain) * 0.15,
                        "missing fundamental {rate} {frequency} {volume}: {}",
                        rms(fundamental) / rms(sustain)
                    );
                    let crossings: Vec<f64> = fundamental
                        .windows(2)
                        .enumerate()
                        .filter(|(_, pair)| pair[0] <= 0.0 && pair[1] > 0.0)
                        .map(|(i, pair)| {
                            i as f64 - f64::from(pair[0]) / f64::from(pair[1] - pair[0])
                        })
                        .collect();
                    assert!(crossings.len() > 20);
                    let measured = f64::from(rate) * (crossings.len() - 1) as f64
                        / (crossings.last().unwrap() - crossings[0]);
                    let cents = 1200.0 * (measured / f64::from(frequency)).log2();
                    assert!(
                        cents.abs() < 3.0,
                        "{rate} Hz / {frequency} / {volume}: {cents} cents"
                    );
                }
            }
        }
    }

    #[test]
    fn retriggers_and_complete_renders_are_repeatable_and_release_finishes() {
        assert_eq!(
            render(48_000.0, 259.0, 0.6, 0.5),
            render(48_000.0, 259.0, 0.6, 0.5)
        );
        let mut runtime = ClarinetRuntime::new(48_000.0);
        let mut sequences = Vec::new();
        for _ in 0..2 {
            let mut samples = Vec::new();
            for (frequency, gate) in [(480.0, 7000), (384.0, 200), (256.0, 4800)] {
                runtime.trigger(frequency, 0.6, gate);
                samples.extend((0..gate).map(|_| runtime.sample().0));
            }
            samples.extend((0..5000).map(|_| runtime.sample().0));
            assert_eq!(runtime.sample(), (0.0, false));
            assert!(samples
                .windows(2)
                .all(|pair| (pair[1] - pair[0]).abs() < 0.12));
            sequences.push(samples);
        }
        assert_eq!(sequences[0], sequences[1]);
    }

    #[test]
    fn silence_short_gates_and_extreme_pitches_are_bounded() {
        for frequency in [f32::MIN_POSITIVE, 19.0, 20.0, 8000.0, 19000.0, f32::MAX] {
            for volume in [0.0, 1.0] {
                let samples = render(48_000.0, frequency, volume, 0.001);
                assert!(samples.iter().all(|x| x.is_finite() && x.abs() < 1.0));
                assert_eq!(*samples.last().unwrap(), 0.0);
                if volume == 0.0 {
                    assert!(samples.iter().all(|x| *x == 0.0));
                }
            }
        }
    }

    #[test]
    fn staccato_notes_speak_before_their_gate_closes() {
        // Includes concert 128 Hz (Radler 30 in music-in-parts-2), whose old
        // unseeded startup exceeded the project's 313-ms one-beat duration.
        for rate in [44_100.0, 48_000.0, 96_000.0] {
            for frequency in [128.0, 160.0, 256.0, 384.0, 480.0, 880.0, 1568.0] {
                for volume in [4.0 / 255.0, 0.25, 1.0] {
                    let reference = render(rate, frequency, volume, 0.8);
                    let steady = rms(&reference[(rate * 0.5) as usize..(rate * 0.7) as usize]);
                    for seconds in [0.03, 0.05, 0.1, 0.313] {
                        let short = render(rate, frequency, volume, seconds);
                        let onset = &short[(rate * 0.01) as usize..(rate * 0.025) as usize];
                        assert!(
                            rms(onset) > steady * 0.35,
                            "slow onset: {rate} Hz, {frequency} Hz, volume {volume}, {seconds}s"
                        );
                        assert_eq!(short[0], 0.0, "the seeded bore must still fade in");
                        assert!(short.iter().all(|x| x.is_finite() && x.abs() < 1.0));
                        assert_eq!(*short.last().unwrap(), 0.0);
                    }
                }
            }
        }
    }

    #[test]
    fn upper_register_has_audible_even_and_odd_overtones() {
        for frequency in [880.0, 1568.0] {
            let samples = render(48_000.0, frequency, 1.0, 1.0);
            let sustain = &samples[24000..32192];
            // Search a narrow pitch tolerance, then compare harmonic amplitudes.
            // A single FFT bin would confound timbre with tuning/bin alignment.
            let amplitude = |harmonic: f64| {
                (-4..=4)
                    .map(|cents| {
                        let hz = f64::from(frequency)
                            * harmonic
                            * 2.0_f64.powf(f64::from(cents) / 1200.0);
                        let (mut real, mut imaginary) = (0.0, 0.0);
                        for (i, &sample) in sustain.iter().enumerate() {
                            let phase = std::f64::consts::TAU * hz * i as f64 / 48000.0;
                            let window = 0.5
                                - 0.5
                                    * (std::f64::consts::TAU * i as f64 / sustain.len() as f64)
                                        .cos();
                            real += f64::from(sample) * window * phase.cos();
                            imaginary += f64::from(sample) * window * phase.sin();
                        }
                        real.hypot(imaginary)
                    })
                    .fold(0.0_f64, f64::max)
            };
            let fundamental = amplitude(1.0);
            assert!(
                amplitude(2.0) / fundamental > 0.065,
                "weak even harmonics at {frequency}"
            );
            assert!(
                amplitude(3.0) / fundamental > 0.08,
                "sine-like tone at {frequency}"
            );
        }
    }

    #[test]
    fn dynamics_change_tone_as_well_as_gain() {
        let quiet = render(48_000.0, 259.0, 0.05, 1.0);
        let loud = render(48_000.0, 259.0, 1.0, 1.0);
        let range = 24000..44000;
        let brightness = |samples: &[f32]| {
            let differences: Vec<_> = samples.windows(2).map(|x| x[1] - x[0]).collect();
            rms(&differences) / rms(samples)
        };
        assert!(rms(&loud[range.clone()]) > rms(&quiet[range.clone()]) * 20.0);
        assert!(brightness(&loud[range.clone()]) > brightness(&quiet[range]) * 1.05);
    }
}
