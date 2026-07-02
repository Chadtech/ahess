use std::{
    error::Error,
    fmt,
    io::{self, Write},
    str::FromStr,
    sync::{Arc, Mutex},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream,
    StreamConfig, I24, U24,
};

const STEPS_PER_BEAT: f32 = 4.0;

pub fn run() -> Result<(), Box<dyn Error>> {
    // The command loop edits this patch. The audio callback reads snapshots from it.
    let patch = Arc::new(Mutex::new(Patch::default()));
    let stream = build_stream(Arc::clone(&patch))?;

    stream.play()?;

    println!("CPAL live-loop spike is playing.");
    println!("Edit the loop below while the CPAL output callback keeps rendering audio.");
    print_help();

    command_loop(patch)?;

    drop(stream);
    Ok(())
}

fn build_stream(shared_patch: Arc<Mutex<Patch>>) -> Result<Stream, Box<dyn Error>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no default output device is available",
        )
    })?;
    let supported_config = device.default_output_config()?;
    let sample_format = supported_config.sample_format();
    let config: StreamConfig = supported_config.into();

    println!("Output device: {}", device.id()?);
    println!("Output config: {config:?}, sample format: {sample_format}");

    // CPAL output callbacks are monomorphized by sample type, so the runtime
    // device format chooses which typed stream we build.
    match sample_format {
        SampleFormat::I8 => build_typed_stream::<i8>(&device, config, shared_patch),
        SampleFormat::I16 => build_typed_stream::<i16>(&device, config, shared_patch),
        SampleFormat::I24 => build_typed_stream::<I24>(&device, config, shared_patch),
        SampleFormat::I32 => build_typed_stream::<i32>(&device, config, shared_patch),
        SampleFormat::I64 => build_typed_stream::<i64>(&device, config, shared_patch),
        SampleFormat::U8 => build_typed_stream::<u8>(&device, config, shared_patch),
        SampleFormat::U16 => build_typed_stream::<u16>(&device, config, shared_patch),
        SampleFormat::U24 => build_typed_stream::<U24>(&device, config, shared_patch),
        SampleFormat::U32 => build_typed_stream::<u32>(&device, config, shared_patch),
        SampleFormat::U64 => build_typed_stream::<u64>(&device, config, shared_patch),
        SampleFormat::F32 => build_typed_stream::<f32>(&device, config, shared_patch),
        SampleFormat::F64 => build_typed_stream::<f64>(&device, config, shared_patch),
        other => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported output sample format: {other}"),
        )
        .into()),
    }
}

fn build_typed_stream<T>(
    device: &Device,
    config: StreamConfig,
    shared_patch: Arc<Mutex<Patch>>,
) -> Result<Stream, Box<dyn Error>>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate as f32;
    let mut engine = AudioEngine::new(sample_rate, shared_patch);

    let err_fn = |err| eprintln!("CPAL stream error: {err}");

    let stream = device.build_output_stream(
        config,
        move |output: &mut [T], _: &OutputCallbackInfo| {
            engine.write(output, channels);
        },
        err_fn,
        None,
    )?;

    Ok(stream)
}

fn command_loop(shared_patch: Arc<Mutex<Patch>>) -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();

    loop {
        print!("cpal> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let should_quit = {
            let mut patch = shared_patch.lock().expect("patch mutex was poisoned");
            apply_command(line, &mut patch).unwrap_or_else(|error| {
                eprintln!("{error}");
                eprintln!("type 'help' for commands");
                false
            })
        };

        if should_quit {
            break;
        }
    }

    Ok(())
}

fn apply_command(line: &str, patch: &mut Patch) -> Result<bool, String> {
    let mut parts = line.split_whitespace();
    let command = parts.next().unwrap_or_default().to_ascii_lowercase();

    match command.as_str() {
        "help" | "?" => {
            print_help();
            Ok(false)
        }
        "show" => {
            print_patch(patch);
            Ok(false)
        }
        "bpm" | "tempo" => {
            let bpm = parse_next::<f32>(&mut parts, "bpm")?;
            require_range("bpm", bpm, 30.0, 260.0)?;
            patch.bpm = bpm;
            patch.changed();
            println!("tempo set to {bpm:.1} bpm");
            Ok(false)
        }
        "wave" => {
            let waveform = parse_next::<Waveform>(&mut parts, "waveform")?;
            patch.waveform = waveform;
            patch.changed();
            println!("waveform set to {waveform}");
            Ok(false)
        }
        "gain" => {
            let gain = parse_next::<f32>(&mut parts, "gain")?;
            require_range("gain", gain, 0.0, 1.0)?;
            patch.master_gain = gain;
            patch.changed();
            println!("master gain set to {gain:.2}");
            Ok(false)
        }
        "transpose" => {
            let semitones = parse_next::<i32>(&mut parts, "semitones")?;
            require_range("transpose", semitones, -36, 36)?;
            patch.transpose = semitones;
            patch.changed();
            println!("transpose set to {semitones:+} semitones");
            Ok(false)
        }
        "set" => {
            let index = parse_step_index(&mut parts, patch)?;
            let note = parse_next::<i32>(&mut parts, "midi note")?;
            require_range("midi note", note, 0, 127)?;
            let velocity = match parts.next() {
                Some(value) => parse_value::<f32>(value, "velocity")?,
                None => patch.steps[index].velocity,
            };
            require_range("velocity", velocity, 0.0, 1.0)?;

            patch.steps[index] = Step {
                note: Some(note),
                velocity,
            };
            patch.changed();
            println!(
                "step {index} set to {} at velocity {velocity:.2}",
                note_label(note)
            );
            Ok(false)
        }
        "rest" | "mute" => {
            let index = parse_step_index(&mut parts, patch)?;
            patch.steps[index].note = None;
            patch.changed();
            println!("step {index} muted");
            Ok(false)
        }
        "vel" | "velocity" => {
            let index = parse_step_index(&mut parts, patch)?;
            let velocity = parse_next::<f32>(&mut parts, "velocity")?;
            require_range("velocity", velocity, 0.0, 1.0)?;
            patch.steps[index].velocity = velocity;
            patch.changed();
            println!("step {index} velocity set to {velocity:.2}");
            Ok(false)
        }
        "quit" | "exit" => Ok(true),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn print_help() {
    println!(
        "\
Commands:
  show                         print the current 16-step loop
  set <step> <midi> [velocity] set a step, for example: set 0 72 0.8
  rest <step>                  mute a step
  vel <step> <velocity>        change one step's velocity, 0.0 to 1.0
  bpm <value>                  change loop tempo, 30 to 260
  wave sine|square|saw|tri     change oscillator waveform
  transpose <semitones>        transpose the whole loop, -36 to +36
  gain <value>                 set master gain, 0.0 to 1.0
  quit                         stop playback"
    );
}

fn print_patch(patch: &Patch) {
    println!(
        "bpm: {:.1}, waveform: {}, transpose: {:+}, master gain: {:.2}",
        patch.bpm, patch.waveform, patch.transpose, patch.master_gain
    );

    for (index, step) in patch.steps.iter().enumerate() {
        println!("{index:02}: {}", step_label(*step));
    }
}

fn parse_step_index<'a>(
    parts: &mut impl Iterator<Item = &'a str>,
    patch: &Patch,
) -> Result<usize, String> {
    let index = parse_next::<usize>(parts, "step")?;
    if index >= patch.steps.len() {
        return Err(format!(
            "step must be between 0 and {}",
            patch.steps.len() - 1
        ));
    }
    Ok(index)
}

fn parse_next<'a, T>(parts: &mut impl Iterator<Item = &'a str>, name: &str) -> Result<T, String>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    let value = parts.next().ok_or_else(|| format!("missing {name}"))?;
    parse_value(value, name)
}

fn parse_value<T>(value: &str, name: &str) -> Result<T, String>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| format!("invalid {name} '{value}': {error}"))
}

fn require_range<T>(name: &str, value: T, min: T, max: T) -> Result<(), String>
where
    T: PartialOrd + fmt::Display + Copy,
{
    if value < min || value > max {
        return Err(format!("{name} must be between {min} and {max}"));
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct Patch {
    bpm: f32,
    waveform: Waveform,
    transpose: i32,
    master_gain: f32,
    steps: Vec<Step>,
    version: u64,
}

impl Patch {
    fn changed(&mut self) {
        self.version = self.version.wrapping_add(1);
    }
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            bpm: 104.0,
            waveform: Waveform::Sine,
            transpose: 0,
            master_gain: 0.20,
            steps: vec![
                Step::note(60, 0.90),
                Step::rest(),
                Step::note(63, 0.65),
                Step::rest(),
                Step::note(67, 0.80),
                Step::rest(),
                Step::note(70, 0.60),
                Step::rest(),
                Step::note(72, 0.90),
                Step::note(70, 0.55),
                Step::note(67, 0.70),
                Step::note(63, 0.55),
                Step::note(65, 0.80),
                Step::rest(),
                Step::note(67, 0.70),
                Step::rest(),
            ],
            version: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Step {
    note: Option<i32>,
    velocity: f32,
}

impl Step {
    fn note(note: i32, velocity: f32) -> Self {
        Self {
            note: Some(note),
            velocity,
        }
    }

    fn rest() -> Self {
        Self {
            note: None,
            velocity: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Waveform {
    Sine,
    Square,
    Saw,
    Triangle,
}

impl Waveform {
    fn sample(self, phase: f32) -> f32 {
        match self {
            Self::Sine => (phase * std::f32::consts::TAU).sin(),
            Self::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Self::Saw => (phase * 2.0) - 1.0,
            Self::Triangle => 1.0 - (4.0 * (phase - 0.5).abs()),
        }
    }
}

impl fmt::Display for Waveform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Sine => "sine",
            Self::Square => "square",
            Self::Saw => "saw",
            Self::Triangle => "triangle",
        };

        formatter.write_str(label)
    }
}

impl FromStr for Waveform {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "sine" => Ok(Self::Sine),
            "square" | "sq" => Ok(Self::Square),
            "saw" => Ok(Self::Saw),
            "triangle" | "tri" => Ok(Self::Triangle),
            _ => Err(format!(
                "expected sine, square, saw, or triangle; got {value}"
            )),
        }
    }
}

struct AudioEngine {
    sample_rate: f32,
    oscillator_phase: f32,
    playhead_step: f32,
    patch: Patch,
    shared_patch: Arc<Mutex<Patch>>,
}

impl AudioEngine {
    fn new(sample_rate: f32, shared_patch: Arc<Mutex<Patch>>) -> Self {
        let patch = shared_patch
            .lock()
            .expect("patch mutex was poisoned")
            .clone();

        Self {
            sample_rate,
            oscillator_phase: 0.0,
            playhead_step: 0.0,
            patch,
            shared_patch,
        }
    }

    fn write<T>(&mut self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        self.refresh_patch_snapshot();

        for frame in output.chunks_mut(channels) {
            let value = T::from_sample(self.next_sample());

            for sample in frame {
                *sample = value;
            }
        }
    }

    fn refresh_patch_snapshot(&mut self) {
        // Audio callbacks should avoid blocking. If the editor owns the mutex
        // for this buffer, keep rendering the previous patch snapshot.
        let Ok(patch) = self.shared_patch.try_lock() else {
            return;
        };

        if patch.version == self.patch.version {
            return;
        }

        self.patch = patch.clone();
        let step_count = self.patch.steps.len() as f32;
        if step_count > 0.0 {
            self.playhead_step %= step_count;
        }
    }

    fn next_sample(&mut self) -> f32 {
        if self.patch.steps.is_empty() {
            return 0.0;
        }

        let step_index = self.playhead_step.floor() as usize % self.patch.steps.len();
        let step = self.patch.steps[step_index];
        let step_phase = self.playhead_step.fract();

        let sample = match step.note {
            Some(note) => {
                let transposed_note = (note + self.patch.transpose).clamp(0, 127);
                let frequency = midi_note_frequency(transposed_note);
                let envelope = step_envelope(step_phase);
                let oscillator = self.patch.waveform.sample(self.oscillator_phase);

                self.advance_oscillator(frequency);

                oscillator * envelope * step.velocity * self.patch.master_gain
            }
            None => 0.0,
        };

        self.advance_playhead();
        sample.clamp(-1.0, 1.0)
    }

    fn advance_oscillator(&mut self, frequency: f32) {
        self.oscillator_phase += frequency / self.sample_rate;
        self.oscillator_phase = self.oscillator_phase.fract();
    }

    fn advance_playhead(&mut self) {
        let steps_per_second = (self.patch.bpm / 60.0) * STEPS_PER_BEAT;
        self.playhead_step += steps_per_second / self.sample_rate;

        let loop_len = self.patch.steps.len() as f32;
        while self.playhead_step >= loop_len {
            self.playhead_step -= loop_len;
        }
    }
}

fn step_envelope(step_phase: f32) -> f32 {
    let attack = (step_phase / 0.08).min(1.0);
    let release = if step_phase > 0.82 {
        (1.0 - step_phase) / 0.18
    } else {
        1.0
    };

    attack.min(release).clamp(0.0, 1.0)
}

fn midi_note_frequency(note: i32) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

fn step_label(step: Step) -> String {
    match step.note {
        Some(note) => format!("{} velocity {:.2}", note_label(note), step.velocity),
        None => "rest".to_string(),
    }
}

fn note_label(note: i32) -> String {
    const NOTE_NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];

    let octave = (note / 12) - 1;
    let name = NOTE_NAMES[note.rem_euclid(12) as usize];
    format!("{name}{octave} ({note})")
}
