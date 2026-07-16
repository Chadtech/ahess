use std::{
    error::Error,
    fmt, io,
    sync::{Arc, Mutex},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream,
    StreamConfig, I24, U24,
};

use crate::{
    note::Note,
    part::{Part, PartScore},
    project::{Project, VoiceType},
};

const MASTER_GAIN: f32 = 0.22;

pub struct Playback {
    _stream: Stream,
    shared_loop: Arc<Mutex<PlaybackLoop>>,
}

impl Playback {
    pub fn start(playback_loop: PlaybackLoop) -> Result<Self, PlaybackError> {
        let shared_loop = Arc::new(Mutex::new(playback_loop));
        let stream = build_stream(Arc::clone(&shared_loop))?;
        stream.play().map_err(|error| {
            PlaybackError::new(format!("failed to start audio output: {error}"))
        })?;

        Ok(Self {
            _stream: stream,
            shared_loop,
        })
    }

    pub fn update(&self, mut playback_loop: PlaybackLoop) {
        let mut current = self
            .shared_loop
            .lock()
            .expect("playback loop mutex was poisoned");
        playback_loop.version = current.version.wrapping_add(1);
        *current = playback_loop;
    }
}

#[derive(Clone, Debug)]
pub struct PlaybackLoop {
    beat_length: u32,
    voices: Vec<PlaybackVoice>,
    beat_count: usize,
    version: u64,
}

impl PlaybackLoop {
    pub fn from_project_score(
        project: &Project,
        part: &Part,
        score: &PartScore,
    ) -> Result<Self, PlaybackError> {
        if project.beat_length == 0 {
            return Err(PlaybackError::new(
                "beat length must be at least one sample",
            ));
        }
        if project.voices().is_empty() {
            return Err(PlaybackError::new(
                "add a sin or saw voice before starting playback",
            ));
        }

        let rows = score
            .parsed_rows(part, project.voices())
            .map_err(|error| PlaybackError::new(error.to_string()))?;
        if rows.is_empty() {
            return Err(PlaybackError::new("a loop must contain at least one beat"));
        }

        let mut random_seed = project.seed;
        let maximum_delay = project
            .timing_variance
            .min(project.beat_length.saturating_sub(1));
        let voices = project
            .voices()
            .iter()
            .enumerate()
            .map(|(voice_index, voice)| {
                let notes = rows.iter().map(|row| row[voice_index]).collect::<Vec<_>>();
                let delays = notes
                    .iter()
                    .map(|_| {
                        let (random, next_seed) = random_seed.next_u64();
                        random_seed = next_seed;
                        if maximum_delay == 0 {
                            0
                        } else {
                            (random % (u64::from(maximum_delay) + 1)) as u32
                        }
                    })
                    .collect();

                PlaybackVoice {
                    voice_type: voice.voice_type,
                    notes,
                    delays,
                }
            })
            .collect();

        Ok(Self {
            beat_length: project.beat_length,
            voices,
            beat_count: rows.len(),
            version: 0,
        })
    }
}

#[derive(Clone, Debug)]
struct PlaybackVoice {
    voice_type: VoiceType,
    notes: Vec<Option<Note>>,
    delays: Vec<u32>,
}

#[derive(Debug)]
pub struct PlaybackError {
    message: String,
}

impl PlaybackError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PlaybackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PlaybackError {}

fn build_stream(shared_loop: Arc<Mutex<PlaybackLoop>>) -> Result<Stream, PlaybackError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| {
        PlaybackError::new(
            io::Error::new(
                io::ErrorKind::NotFound,
                "no default audio output device is available",
            )
            .to_string(),
        )
    })?;
    let supported_config = device.default_output_config().map_err(|error| {
        PlaybackError::new(format!("failed to read audio output settings: {error}"))
    })?;
    let sample_format = supported_config.sample_format();
    let config: StreamConfig = supported_config.into();

    match sample_format {
        SampleFormat::I8 => build_typed_stream::<i8>(&device, config, shared_loop),
        SampleFormat::I16 => build_typed_stream::<i16>(&device, config, shared_loop),
        SampleFormat::I24 => build_typed_stream::<I24>(&device, config, shared_loop),
        SampleFormat::I32 => build_typed_stream::<i32>(&device, config, shared_loop),
        SampleFormat::I64 => build_typed_stream::<i64>(&device, config, shared_loop),
        SampleFormat::U8 => build_typed_stream::<u8>(&device, config, shared_loop),
        SampleFormat::U16 => build_typed_stream::<u16>(&device, config, shared_loop),
        SampleFormat::U24 => build_typed_stream::<U24>(&device, config, shared_loop),
        SampleFormat::U32 => build_typed_stream::<u32>(&device, config, shared_loop),
        SampleFormat::U64 => build_typed_stream::<u64>(&device, config, shared_loop),
        SampleFormat::F32 => build_typed_stream::<f32>(&device, config, shared_loop),
        SampleFormat::F64 => build_typed_stream::<f64>(&device, config, shared_loop),
        other => Err(PlaybackError::new(format!(
            "unsupported audio output sample format: {other}"
        ))),
    }
}

fn build_typed_stream<T>(
    device: &Device,
    config: StreamConfig,
    shared_loop: Arc<Mutex<PlaybackLoop>>,
) -> Result<Stream, PlaybackError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate as f32;
    let mut engine = AudioEngine::new(sample_rate, shared_loop);
    let error_callback = |error| eprintln!("audio stream error: {error}");

    device
        .build_output_stream(
            config,
            move |output: &mut [T], _: &OutputCallbackInfo| engine.write(output, channels),
            error_callback,
            None,
        )
        .map_err(|error| PlaybackError::new(format!("failed to open audio output: {error}")))
}

struct AudioEngine {
    sample_rate: f32,
    oscillator_phases: Vec<f32>,
    beat_index: usize,
    sample_in_beat: u32,
    playback_loop: PlaybackLoop,
    shared_loop: Arc<Mutex<PlaybackLoop>>,
}

impl AudioEngine {
    fn new(sample_rate: f32, shared_loop: Arc<Mutex<PlaybackLoop>>) -> Self {
        let playback_loop = shared_loop
            .lock()
            .expect("playback loop mutex was poisoned")
            .clone();

        Self {
            sample_rate,
            oscillator_phases: vec![0.0; playback_loop.voices.len()],
            beat_index: 0,
            sample_in_beat: 0,
            playback_loop,
            shared_loop,
        }
    }

    fn write<T>(&mut self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        self.refresh_loop_snapshot();
        for frame in output.chunks_mut(channels) {
            let value = T::from_sample(self.next_sample());
            for sample in frame {
                *sample = value;
            }
        }
    }

    fn refresh_loop_snapshot(&mut self) {
        let Ok(playback_loop) = self.shared_loop.try_lock() else {
            return;
        };
        if playback_loop.version == self.playback_loop.version {
            return;
        }

        self.playback_loop = playback_loop.clone();
        self.beat_index %= self.playback_loop.beat_count;
        self.sample_in_beat %= self.playback_loop.beat_length;
        self.oscillator_phases
            .resize(self.playback_loop.voices.len(), 0.0);
    }

    fn next_sample(&mut self) -> f32 {
        let mut mixed = 0.0;
        let mut sounding_voice_count = 0_u32;

        for (voice_index, voice) in self.playback_loop.voices.iter().enumerate() {
            let Some(note) = voice.notes[self.beat_index] else {
                continue;
            };
            let delay = voice.delays[self.beat_index];
            if self.sample_in_beat < delay {
                continue;
            }

            let note_sample = self.sample_in_beat - delay;
            let note_length = self.playback_loop.beat_length - delay;
            let envelope = envelope(note_sample, note_length);
            let phase = self.oscillator_phases[voice_index];
            mixed += waveform_sample(voice.voice_type, phase) * envelope;
            sounding_voice_count += 1;

            let frequency = midi_note_frequency(note.midi());
            self.oscillator_phases[voice_index] = (phase + frequency / self.sample_rate).fract();
        }

        self.advance_playhead();
        if sounding_voice_count == 0 {
            0.0
        } else {
            (mixed * MASTER_GAIN / (sounding_voice_count as f32).sqrt()).clamp(-1.0, 1.0)
        }
    }

    fn advance_playhead(&mut self) {
        self.sample_in_beat += 1;
        if self.sample_in_beat >= self.playback_loop.beat_length {
            self.sample_in_beat = 0;
            self.beat_index = (self.beat_index + 1) % self.playback_loop.beat_count;
        }
    }
}

fn waveform_sample(voice_type: VoiceType, phase: f32) -> f32 {
    match voice_type {
        VoiceType::Sin => (phase * std::f32::consts::TAU).sin(),
        VoiceType::Saw => (phase * 2.0) - 1.0,
    }
}

fn envelope(sample: u32, length: u32) -> f32 {
    let attack_length = (length / 8).clamp(1, 64);
    let release_length = (length / 6).clamp(1, 256);
    let attack = sample as f32 / attack_length as f32;
    let remaining = length.saturating_sub(sample + 1);
    let release = remaining as f32 / release_length as f32;
    attack.min(release).clamp(0.0, 1.0)
}

fn midi_note_frequency(note: u8) -> f32 {
    440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{AudioEngine, PlaybackLoop};
    use crate::{
        note::Note,
        part::{Part, PartScore},
        project::{Project, Voice, VoiceType},
        seed::Seed,
    };

    #[test]
    fn builds_a_two_voice_loop_from_score_rows() {
        let project = Project::new("test", 800, 25, Seed::new(1)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let part = Part::new("intro", 2);
        let score = PartScore::from_rows(vec![
            vec!["C4".to_string(), "C2".to_string()],
            vec![String::new(), "G2".to_string()],
        ]);

        let playback_loop = PlaybackLoop::from_project_score(&project, &part, &score).unwrap();

        assert_eq!(playback_loop.beat_count, 2);
        assert_eq!(playback_loop.voices.len(), 2);
        assert_eq!(playback_loop.voices[0].notes[0], Some(Note::from_midi(60)));
        assert_eq!(playback_loop.voices[0].notes[1], None);
        assert_eq!(playback_loop.voices[1].notes[1], Some(Note::from_midi(43)));
    }

    #[test]
    fn renderer_loops_and_emits_bounded_samples_without_an_audio_device() {
        let project = Project::new("test", 8, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Sin,
        )]);
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec!["A4".to_string()]]);
        let playback_loop = PlaybackLoop::from_project_score(&project, &part, &score).unwrap();
        let shared = Arc::new(Mutex::new(playback_loop));
        let mut engine = AudioEngine::new(44_100.0, shared);

        let samples = (0..24).map(|_| engine.next_sample()).collect::<Vec<_>>();

        assert!(samples.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        assert_eq!(engine.beat_index, 0);
        assert_eq!(engine.sample_in_beat, 0);
    }
}
