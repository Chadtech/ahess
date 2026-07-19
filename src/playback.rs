use std::{
    error::Error,
    fmt, io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, FromSample, OutputCallbackInfo, Sample, SampleFormat, SizedSample, Stream,
    StreamConfig, I24, U24,
};

use crate::{
    part::{Part, PartScore},
    pitch_system::FrequencyHz,
    project::{Project, VoiceId, VoiceType},
    seed::{standard_normal, Seed},
};

const MASTER_GAIN: f32 = 0.22;
const TIMING_SEED_DOMAIN: u64 = 0x7469_6d69_6e67_2d31;
const TIMING_STANDARD_DEVIATIONS: f64 = 3.0;

pub struct Playback {
    _stream: Stream,
    shared_loop: Arc<Mutex<PlaybackLoop>>,
    playhead: Arc<AtomicU64>,
}

impl Playback {
    pub fn start(playback_loop: PlaybackLoop) -> Result<Self, PlaybackError> {
        let playhead = Arc::new(AtomicU64::new(playback_loop.first_arrangement_beat));
        let shared_loop = Arc::new(Mutex::new(playback_loop));
        let stream = build_stream(Arc::clone(&shared_loop), Arc::clone(&playhead))?;
        stream.play().map_err(|error| {
            PlaybackError::new(format!("failed to start audio output: {error}"))
        })?;

        Ok(Self {
            _stream: stream,
            shared_loop,
            playhead,
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

    pub fn current_arrangement_beat(&self) -> u64 {
        self.playhead.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Debug)]
pub struct PlaybackLoop {
    beat_length: u32,
    voices: Vec<PlaybackVoice>,
    beat_count: usize,
    first_arrangement_beat: u64,
    version: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BeatRange {
    first: u64,
    last: u64,
}

impl BeatRange {
    pub fn new(first: u64, last: u64, arrangement_beat_count: u64) -> Result<Self, PlaybackError> {
        if arrangement_beat_count == 0 {
            return Err(PlaybackError::new(
                "add at least one part to the arrangement before setting a loop",
            ));
        }
        if first == 0 {
            return Err(PlaybackError::new("from beat must be at least 1"));
        }
        if last < first {
            return Err(PlaybackError::new(
                "to beat must be the same as or later than from beat",
            ));
        }
        if last > arrangement_beat_count {
            return Err(PlaybackError::new(format!(
                "to beat must be no greater than arrangement beat {arrangement_beat_count}",
            )));
        }

        Ok(Self { first, last })
    }

    pub fn first(self) -> u64 {
        self.first
    }

    pub fn last(self) -> u64 {
        self.last
    }
}

impl PlaybackLoop {
    pub fn from_project_arrangement(
        project: &Project,
        arrangement_scores: &[(Part, PartScore)],
        range: BeatRange,
    ) -> Result<Self, PlaybackError> {
        let arrangement_beat_count = project.arrangement_beat_count();
        let range = BeatRange::new(range.first, range.last, arrangement_beat_count)?;
        if arrangement_scores.len() != project.sequence().len() {
            return Err(PlaybackError::new(
                "the available scores do not match the project arrangement",
            ));
        }

        let mut rows = Vec::new();
        let mut occurrence_first = 1_u64;
        for (expected_part_name, (part, score)) in project.sequence().iter().zip(arrangement_scores)
        {
            if !part.name.eq_ignore_ascii_case(expected_part_name) {
                return Err(PlaybackError::new(
                    "the available scores do not match the project arrangement",
                ));
            }

            let occurrence_last = occurrence_first + u64::from(part.length) - 1;
            if range.first <= occurrence_last && range.last >= occurrence_first {
                let resolved_rows = score.resolved_rows(part, project).map_err(|error| {
                    PlaybackError::new(format!("part {:?}: {error}", part.name.as_str()))
                })?;
                let first_row = range.first.saturating_sub(occurrence_first) as usize;
                let last_row = (range.last.min(occurrence_last) - occurrence_first) as usize;
                rows.extend_from_slice(&resolved_rows[first_row..=last_row]);
            }
            occurrence_first = occurrence_last + 1;
        }

        Self::from_rows(project, rows, range.first)
    }

    fn from_rows(
        project: &Project,
        rows: Vec<Vec<Option<FrequencyHz>>>,
        first_arrangement_beat: u64,
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
        if rows.is_empty() {
            return Err(PlaybackError::new("a loop must contain at least one beat"));
        }

        let maximum_delay = project
            .timing_variance
            .min(project.beat_length.saturating_sub(1));
        let voices = project
            .voices()
            .iter()
            .enumerate()
            .map(|(voice_index, voice)| {
                let frequencies = rows.iter().map(|row| row[voice_index]).collect::<Vec<_>>();
                let delays = frequencies
                    .iter()
                    .enumerate()
                    .map(|(beat_index, _)| {
                        let arrangement_beat = first_arrangement_beat + beat_index as u64;
                        let seed = timing_seed(project.seed, arrangement_beat, voice.id());
                        normally_distributed_delay(seed, maximum_delay)
                    })
                    .collect();

                PlaybackVoice {
                    voice_type: voice.voice_type,
                    frequencies,
                    delays,
                }
            })
            .collect();

        Ok(Self {
            beat_length: project.beat_length,
            voices,
            beat_count: rows.len(),
            first_arrangement_beat,
            version: 0,
        })
    }
}

fn timing_seed(project_seed: Seed, arrangement_beat: u64, voice_id: VoiceId) -> Seed {
    project_seed
        .derive(TIMING_SEED_DOMAIN)
        .derive(arrangement_beat)
        .derive(voice_id.value())
}

fn normally_distributed_delay(seed: Seed, maximum_delay: u32) -> u32 {
    if maximum_delay == 0 {
        return 0;
    }

    let mean = f64::from(maximum_delay) / 2.0;
    let standard_deviation = mean / TIMING_STANDARD_DEVIATIONS;
    let (standard_deviation_units, _) = seed.generate(standard_normal());
    let delay = mean + standard_deviation_units * standard_deviation;

    delay.clamp(0.0, f64::from(maximum_delay)).round() as u32
}

#[derive(Clone, Debug)]
struct PlaybackVoice {
    voice_type: VoiceType,
    frequencies: Vec<Option<FrequencyHz>>,
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

fn build_stream(
    shared_loop: Arc<Mutex<PlaybackLoop>>,
    playhead: Arc<AtomicU64>,
) -> Result<Stream, PlaybackError> {
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
        SampleFormat::I8 => build_typed_stream::<i8>(&device, config, shared_loop, playhead),
        SampleFormat::I16 => build_typed_stream::<i16>(&device, config, shared_loop, playhead),
        SampleFormat::I24 => build_typed_stream::<I24>(&device, config, shared_loop, playhead),
        SampleFormat::I32 => build_typed_stream::<i32>(&device, config, shared_loop, playhead),
        SampleFormat::I64 => build_typed_stream::<i64>(&device, config, shared_loop, playhead),
        SampleFormat::U8 => build_typed_stream::<u8>(&device, config, shared_loop, playhead),
        SampleFormat::U16 => build_typed_stream::<u16>(&device, config, shared_loop, playhead),
        SampleFormat::U24 => build_typed_stream::<U24>(&device, config, shared_loop, playhead),
        SampleFormat::U32 => build_typed_stream::<u32>(&device, config, shared_loop, playhead),
        SampleFormat::U64 => build_typed_stream::<u64>(&device, config, shared_loop, playhead),
        SampleFormat::F32 => build_typed_stream::<f32>(&device, config, shared_loop, playhead),
        SampleFormat::F64 => build_typed_stream::<f64>(&device, config, shared_loop, playhead),
        other => Err(PlaybackError::new(format!(
            "unsupported audio output sample format: {other}"
        ))),
    }
}

fn build_typed_stream<T>(
    device: &Device,
    config: StreamConfig,
    shared_loop: Arc<Mutex<PlaybackLoop>>,
    playhead: Arc<AtomicU64>,
) -> Result<Stream, PlaybackError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate as f32;
    let mut engine = AudioEngine::new(sample_rate, shared_loop, playhead);
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
    playhead: Arc<AtomicU64>,
}

impl AudioEngine {
    fn new(
        sample_rate: f32,
        shared_loop: Arc<Mutex<PlaybackLoop>>,
        playhead: Arc<AtomicU64>,
    ) -> Self {
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
            playhead,
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

        let range_changed = playback_loop.first_arrangement_beat
            != self.playback_loop.first_arrangement_beat
            || playback_loop.beat_count != self.playback_loop.beat_count;
        self.playback_loop = playback_loop.clone();
        if range_changed {
            self.beat_index = 0;
            self.sample_in_beat = 0;
            self.publish_playhead();
        } else {
            self.beat_index %= self.playback_loop.beat_count;
            self.sample_in_beat %= self.playback_loop.beat_length;
        }
        self.oscillator_phases
            .resize(self.playback_loop.voices.len(), 0.0);
    }

    fn next_sample(&mut self) -> f32 {
        let mut mixed = 0.0;
        let mut sounding_voice_count = 0_u32;

        for (voice_index, voice) in self.playback_loop.voices.iter().enumerate() {
            let Some(frequency) = voice.frequencies[self.beat_index] else {
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

            self.oscillator_phases[voice_index] =
                (phase + frequency.as_hz_f32() / self.sample_rate).fract();
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
            self.publish_playhead();
        }
    }

    fn publish_playhead(&self) {
        self.playhead.store(
            self.playback_loop.first_arrangement_beat + self.beat_index as u64,
            Ordering::Relaxed,
        );
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    };

    use super::{normally_distributed_delay, timing_seed, AudioEngine, BeatRange, PlaybackLoop};
    use crate::{
        part::{Part, PartScore},
        pitch_system::{FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem, PitchSystem},
        project::{Project, Voice, VoiceId, VoiceType},
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

        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();

        assert_eq!(playback_loop.beat_count, 2);
        assert_eq!(playback_loop.voices.len(), 2);
        assert_eq!(
            playback_loop.voices[0].frequencies[0],
            project.pitch_system().resolve_cell("C4").unwrap()
        );
        assert_eq!(playback_loop.voices[0].frequencies[1], None);
        assert_eq!(
            playback_loop.voices[1].frequencies[1],
            project.pitch_system().resolve_cell("G2").unwrap()
        );
    }

    #[test]
    fn tiny_score_has_visible_deterministic_timing_variation() {
        let project = Project::new("test", 800, 120, Seed::new(7)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let part = Part::new("example", 4);
        let score = PartScore::from_rows(vec![
            vec!["C4".to_string(), "C3".to_string()],
            vec!["D4".to_string(), "D3".to_string()],
            vec!["E4".to_string(), "E3".to_string()],
            vec!["F4".to_string(), "F3".to_string()],
        ]);
        let rows = score.resolved_rows(&part, &project).unwrap();

        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();

        //                         arrangement beat:  1   2   3   4
        assert_eq!(playback_loop.voices[0].delays, vec![36, 78, 77, 11]);
        assert_eq!(playback_loop.voices[1].delays, vec![48, 73, 62, 81]);
    }

    #[test]
    fn timing_delays_are_stable_for_the_same_absolute_beat_in_overlapping_loops() {
        let project = Project::new("test", 800, 25, Seed::new(1)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let part = Part::new("intro", 3);
        let score = PartScore::from_rows(vec![
            vec!["C4".to_string(), "C2".to_string()],
            vec!["D4".to_string(), "D2".to_string()],
            vec!["E4".to_string(), "E2".to_string()],
        ]);
        let rows = score.resolved_rows(&part, &project).unwrap();

        let full_loop = PlaybackLoop::from_rows(&project, rows.clone(), 10).unwrap();
        let overlapping_loop = PlaybackLoop::from_rows(&project, rows[1..].to_vec(), 11).unwrap();

        for voice_index in 0..project.voices().len() {
            assert_eq!(
                &full_loop.voices[voice_index].delays[1..],
                overlapping_loop.voices[voice_index].delays
            );
        }
    }

    #[test]
    fn each_voice_and_arrangement_beat_derives_its_own_timing_seed() {
        let project_seed = Seed::new(1);
        let seeds = (1..=16)
            .flat_map(|beat| {
                (1..=4).map(move |voice_id| timing_seed(project_seed, beat, VoiceId::new(voice_id)))
            })
            .collect::<HashSet<_>>();

        assert_eq!(seeds.len(), 16 * 4);
        assert_ne!(
            timing_seed(Seed::new(1), 3, VoiceId::new(2)),
            timing_seed(Seed::new(2), 3, VoiceId::new(2))
        );
    }

    #[test]
    fn normally_distributed_delays_are_bounded_and_span_the_configured_range() {
        let maximum_delay = 120;
        let delays = (0..10_000)
            .map(|index| normally_distributed_delay(Seed::new(index), maximum_delay))
            .collect::<Vec<_>>();
        let central_count = delays
            .iter()
            .filter(|&&delay| (30..=90).contains(&delay))
            .count();

        assert!(delays.iter().all(|delay| *delay <= maximum_delay));
        assert!(central_count > 8_000);
        assert!(delays.contains(&0));
        assert!(delays.contains(&maximum_delay));
        assert_eq!(normally_distributed_delay(Seed::new(1), 0), 0);
    }

    #[test]
    fn project_tuning_changes_the_frequency_prepared_from_the_same_score_text() {
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec!["11".to_string()]]);
        let project_with = |name: &str, period_numerator: u64, second_degree_numerator: u64| {
            Project::new(name, 800, 0, Seed::new(1))
                .with_pitch_system(PitchSystem::periodic(
                    PeriodicPitchSystem::new(
                        name,
                        FrequencyHz::new(100.0).unwrap(),
                        Interval::ratio(period_numerator, 1).unwrap(),
                        vec![
                            Interval::ratio(1, 1).unwrap(),
                            Interval::ratio(second_degree_numerator, 3).unwrap(),
                        ],
                        PeriodicNotation::radler_digits(10).unwrap(),
                    )
                    .unwrap(),
                ))
                .with_voices(vec![Voice::new(1, "lead", VoiceType::Sin)])
        };
        let first_project = project_with("first", 2, 4);
        let second_project = project_with("second", 3, 5);

        let first = PlaybackLoop::from_rows(
            &first_project,
            score.resolved_rows(&part, &first_project).unwrap(),
            1,
        )
        .unwrap();
        let second = PlaybackLoop::from_rows(
            &second_project,
            score.resolved_rows(&part, &second_project).unwrap(),
            1,
        )
        .unwrap();

        assert!((first.voices[0].frequencies[0].unwrap().as_hz() - (800.0 / 3.0)).abs() < 1e-10);
        assert!((second.voices[0].frequencies[0].unwrap().as_hz() - 500.0).abs() < 1e-10);
        assert_ne!(
            first.voices[0].frequencies[0],
            second.voices[0].frequencies[0]
        );
    }

    #[test]
    fn builds_an_inclusive_loop_across_arrangement_part_boundaries() {
        let first_part = Part::new("first", 2);
        let second_part = Part::new("second", 3);
        let project = Project::new("test", 800, 0, Seed::new(1))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![first_part.clone(), second_part.clone()])
            .with_sequence(vec![
                first_part.name.clone(),
                second_part.name.clone(),
                first_part.name.clone(),
            ]);
        let first_score =
            PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
        let second_score = PartScore::from_rows(vec![
            vec!["E4".to_string()],
            vec!["F4".to_string()],
            vec!["G4".to_string()],
        ]);
        let arrangement_scores = vec![
            (first_part.clone(), first_score.clone()),
            (second_part, second_score),
            (first_part, first_score),
        ];
        let range = BeatRange::new(2, 6, project.arrangement_beat_count()).unwrap();

        let playback_loop =
            PlaybackLoop::from_project_arrangement(&project, &arrangement_scores, range).unwrap();

        assert_eq!(playback_loop.beat_count, 5);
        assert_eq!(playback_loop.first_arrangement_beat, 2);
        assert_eq!(
            playback_loop.voices[0].frequencies,
            ["D4", "E4", "F4", "G4", "C4"]
                .map(|pitch| project.pitch_system().resolve_cell(pitch).unwrap())
                .to_vec()
        );
    }

    #[test]
    fn beat_ranges_must_be_ordered_and_inside_the_arrangement() {
        assert_eq!(
            BeatRange::new(0, 1, 8).unwrap_err().to_string(),
            "from beat must be at least 1"
        );
        assert_eq!(
            BeatRange::new(5, 4, 8).unwrap_err().to_string(),
            "to beat must be the same as or later than from beat"
        );
        assert_eq!(
            BeatRange::new(5, 9, 8).unwrap_err().to_string(),
            "to beat must be no greater than arrangement beat 8"
        );
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
        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();
        let shared = Arc::new(Mutex::new(playback_loop));
        let playhead = Arc::new(AtomicU64::new(1));
        let mut engine = AudioEngine::new(44_100.0, shared, playhead);

        let samples = (0..24).map(|_| engine.next_sample()).collect::<Vec<_>>();

        assert!(samples.iter().all(|sample| (-1.0..=1.0).contains(sample)));
        assert_eq!(engine.beat_index, 0);
        assert_eq!(engine.sample_in_beat, 0);
    }

    #[test]
    fn renderer_publishes_the_current_arrangement_beat() {
        let project = Project::new("test", 2, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Sin,
        )]);
        let part = Part::new("intro", 3);
        let score = PartScore::from_rows(vec![vec![String::new()]; 3]);
        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 4).unwrap();
        let shared = Arc::new(Mutex::new(playback_loop));
        let playhead = Arc::new(AtomicU64::new(4));
        let mut engine = AudioEngine::new(44_100.0, shared, Arc::clone(&playhead));

        engine.next_sample();
        assert_eq!(playhead.load(Ordering::Relaxed), 4);
        engine.next_sample();
        assert_eq!(playhead.load(Ordering::Relaxed), 5);
        engine.next_sample();
        engine.next_sample();
        assert_eq!(playhead.load(Ordering::Relaxed), 6);
        engine.next_sample();
        engine.next_sample();
        assert_eq!(playhead.load(Ordering::Relaxed), 4);
    }
}
