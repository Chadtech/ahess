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
    acoustics::{AcousticScene, Point3Meters, StereoFrame, VoiceSpatializer},
    gamelan_metallophone::GamelanMetallophoneRuntime,
    noitech_bell_a::NoitechBellARuntime,
    noitech_bell_b::NoitechBellBRuntime,
    part::{Part, PartScore},
    pitch_system::{FrequencyHz, Strike, StrikeDuration},
    project::{BeatDurationMillis, FrequencyVariance, Project, VoiceId, VoiceType},
    recovered_voice::RecoveredVoiceRuntime,
    seed::{standard_normal, Seed},
};
#[cfg(target_os = "macos")]
use crate::{
    mts_esp::{MtsEspMaster, MtsNoteAddress},
    surge_xt::{SurgeXt, SurgeXtPatch},
};

const MASTER_GAIN: f32 = 0.22;
const MIX_GAIN_RAMP_SAMPLES: u32 = 64;
const TIMING_SEED_DOMAIN: u64 = 0x7469_6d69_6e67_2d31;
const FREQUENCY_VARIANCE_SEED_DOMAIN: u64 = 0x6672_6571_2d76_6172;
const TIMING_STANDARD_DEVIATIONS: f64 = 3.0;
const FREQUENCY_STANDARD_DEVIATIONS: f64 = 3.0;
const HARMONIC_SAW_PARTIAL_COUNT: usize = 32;
const HARMONIC_SAW_INHARMONICITY: f32 = 0.000_016;
const HARMONIC_SAW_SPECTRAL_SLOPE: f32 = 1.12;
const HARMONIC_SAW_NYQUIST_MARGIN: f32 = 0.98;
const HARMONIC_SAW_RENORMALIZE_INTERVAL: u32 = 1_024;
#[cfg(target_os = "macos")]
const SURGE_RENDER_BLOCK_FRAMES: usize = 512;
#[cfg(target_os = "macos")]
const SURGE_SILENCE_THRESHOLD: f32 = 0.000_001;
#[cfg(target_os = "macos")]
const SURGE_PIANO_GAIN: f32 = 8.0;

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
    beat_duration_millis: BeatDurationMillis,
    timing_variance: u32,
    voices: Vec<PlaybackVoice>,
    acoustic_scene: AcousticScene,
    beat_count: usize,
    first_arrangement_beat: u64,
    version: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PreparedTimingOffset {
    pub(crate) applied_samples: u32,
    pub(crate) maximum_samples: u32,
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
    pub fn from_part(
        project: &Project,
        part: &Part,
        score: &PartScore,
    ) -> Result<Self, PlaybackError> {
        let rows = score.resolved_strikes(part, project).map_err(|error| {
            PlaybackError::new(format!("part {:?}: {error}", part.name.as_str()))
        })?;
        Self::from_rows(project, rows, 1)
    }

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
                let resolved_rows = score.resolved_strikes(part, project).map_err(|error| {
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

    fn from_rows<S: PlaybackStrikeSpec>(
        project: &Project,
        rows: Vec<Vec<Option<S>>>,
        first_arrangement_beat: u64,
    ) -> Result<Self, PlaybackError> {
        if project.voices().is_empty() {
            return Err(PlaybackError::new(
                "add a sin or saw voice before starting playback",
            ));
        }
        if rows.is_empty() {
            return Err(PlaybackError::new("a loop must contain at least one beat"));
        }
        validate_external_voice_support(project)?;

        let maximum_delay = project.timing_variance;
        let voices = project
            .voices()
            .iter()
            .enumerate()
            .map(|(voice_index, voice)| {
                project
                    .acoustic_scene()
                    .validate_source(voice.position())
                    .map_err(|error| PlaybackError::new(error.to_string()))?;
                let strikes = rows
                    .iter()
                    .enumerate()
                    .map(|(beat_index, row)| {
                        let arrangement_beat = first_arrangement_beat + beat_index as u64;
                        let seed =
                            frequency_variance_seed(project.seed, arrangement_beat, voice.id());
                        varied_strike(row[voice_index], seed, project.frequency_variance())
                    })
                    .collect::<Result<Vec<_>, PlaybackError>>()?;
                let delays = strikes
                    .iter()
                    .enumerate()
                    .map(|(beat_index, _)| {
                        let arrangement_beat = first_arrangement_beat + beat_index as u64;
                        let seed = timing_seed(project.seed, arrangement_beat, voice.id());
                        normally_distributed_delay(seed, maximum_delay)
                    })
                    .collect();

                Ok(PlaybackVoice {
                    id: voice.id(),
                    voice_type: voice.voice_type,
                    position: voice.position(),
                    volume_multiplier: voice
                        .volume_adjustment()
                        .map_or(1.0, |adjustment| adjustment.multiplier()),
                    frequencies: strikes
                        .iter()
                        .map(|strike| strike.map(|strike| strike.frequency))
                        .collect(),
                    strikes,
                    delays,
                })
            })
            .collect::<Result<Vec<_>, PlaybackError>>()?;

        Ok(Self {
            beat_duration_millis: project.beat_duration_millis,
            timing_variance: project.timing_variance,
            voices,
            acoustic_scene: project.acoustic_scene().clone(),
            beat_count: rows.len(),
            first_arrangement_beat,
            version: 0,
        })
    }

    pub(crate) fn prepared_timing_offset(
        &self,
        voice_index: usize,
        arrangement_beat: u64,
        sample_rate: u32,
    ) -> Option<PreparedTimingOffset> {
        let beat_index = arrangement_beat.checked_sub(self.first_arrangement_beat)? as usize;
        let voice = self.voices.get(voice_index)?;
        voice.strikes.get(beat_index)?.as_ref()?;
        let beat_length = beat_length_samples(self.beat_duration_millis, sample_rate as f32);
        let maximum_samples = self.timing_variance.min(beat_length.saturating_sub(1));
        let applied_samples = (*voice.delays.get(beat_index)?).min(maximum_samples);
        Some(PreparedTimingOffset {
            applied_samples,
            maximum_samples,
        })
    }

    pub(crate) fn beat_length_samples_at(&self, sample_rate: u32) -> u32 {
        beat_length_samples(self.beat_duration_millis, sample_rate as f32)
    }
}

fn timing_seed(project_seed: Seed, arrangement_beat: u64, voice_id: VoiceId) -> Seed {
    project_seed
        .derive(TIMING_SEED_DOMAIN)
        .derive(arrangement_beat)
        .derive(voice_id.value())
}

fn frequency_variance_seed(project_seed: Seed, arrangement_beat: u64, voice_id: VoiceId) -> Seed {
    project_seed
        .derive(FREQUENCY_VARIANCE_SEED_DOMAIN)
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

fn varied_frequency(
    frequency: Option<FrequencyHz>,
    seed: Seed,
    maximum_variance: FrequencyVariance,
) -> Result<Option<FrequencyHz>, PlaybackError> {
    let Some(frequency) = frequency else {
        return Ok(None);
    };
    let maximum_ratio = maximum_variance.ratio();
    if maximum_ratio == 0.0 {
        return Ok(Some(frequency));
    }

    let standard_deviation = maximum_ratio / FREQUENCY_STANDARD_DEVIATIONS;
    let (standard_deviation_units, _) = seed.generate(standard_normal());
    let ratio =
        (standard_deviation_units * standard_deviation).clamp(-maximum_ratio, maximum_ratio);
    FrequencyHz::new(frequency.as_hz() * (1.0 + ratio))
        .map(Some)
        .map_err(|error| {
            PlaybackError::new(format!(
                "frequency variation produced an unsupported frequency: {error}"
            ))
        })
}

fn varied_strike<S: PlaybackStrikeSpec>(
    strike: Option<S>,
    seed: Seed,
    maximum_variance: FrequencyVariance,
) -> Result<Option<PlaybackStrike>, PlaybackError> {
    let Some(strike) = strike else {
        return Ok(None);
    };
    Ok(
        varied_frequency(Some(strike.frequency()), seed, maximum_variance)?.map(|frequency| {
            PlaybackStrike {
                frequency,
                duration: strike.duration(),
                volume: strike.volume(),
            }
        }),
    )
}

trait PlaybackStrikeSpec: Copy {
    fn frequency(self) -> FrequencyHz;
    fn duration(self) -> StrikeDuration;
    fn volume(self) -> f32;
}

impl PlaybackStrikeSpec for Strike {
    fn frequency(self) -> FrequencyHz {
        self.frequency()
    }

    fn duration(self) -> StrikeDuration {
        self.duration()
    }

    fn volume(self) -> f32 {
        self.volume()
    }
}

impl PlaybackStrikeSpec for FrequencyHz {
    fn frequency(self) -> FrequencyHz {
        self
    }

    fn duration(self) -> StrikeDuration {
        StrikeDuration::VoiceDefault
    }

    fn volume(self) -> f32 {
        1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PlaybackStrike {
    frequency: FrequencyHz,
    duration: StrikeDuration,
    volume: f32,
}

#[derive(Clone, Debug)]
struct PlaybackVoice {
    id: VoiceId,
    voice_type: VoiceType,
    position: Point3Meters,
    volume_multiplier: f32,
    frequencies: Vec<Option<FrequencyHz>>,
    strikes: Vec<Option<PlaybackStrike>>,
    delays: Vec<u32>,
}

#[derive(Debug)]
pub struct PlaybackError {
    message: String,
    recovery: Option<PlaybackRecovery>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlaybackRecovery {
    ResetMtsEsp,
}

impl PlaybackError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            recovery: None,
        }
    }

    #[cfg(target_os = "macos")]
    fn from_mts_esp(error: crate::mts_esp::MtsEspError) -> Self {
        let recovery = error
            .is_master_already_active()
            .then_some(PlaybackRecovery::ResetMtsEsp);
        Self {
            message: error.to_string(),
            recovery,
        }
    }

    pub fn can_reset_mts_esp(&self) -> bool {
        self.recovery == Some(PlaybackRecovery::ResetMtsEsp)
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
    let mut engine = AudioEngine::new(sample_rate, shared_loop, playhead)?;
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
    beat_length_samples: u32,
    voice_runtimes: Vec<VoiceRuntime>,
    #[cfg(target_os = "macos")]
    mts_master: Option<Arc<MtsEspMaster>>,
    mix_gain: GainRamp,
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
    ) -> Result<Self, PlaybackError> {
        let playback_loop = shared_loop
            .lock()
            .expect("playback loop mutex was poisoned")
            .clone();
        #[cfg(target_os = "macos")]
        let mts_master = prepare_mts_master(&playback_loop)?;
        let voice_runtimes = playback_loop
            .voices
            .iter()
            .enumerate()
            .map(|(voice_index, voice)| {
                VoiceRuntime::new(
                    voice,
                    voice_index,
                    playback_loop.voices.len(),
                    &playback_loop.acoustic_scene,
                    sample_rate,
                    #[cfg(target_os = "macos")]
                    mts_master.as_ref(),
                )
            })
            .collect::<Result<Vec<_>, PlaybackError>>()?;
        let beat_length_samples =
            beat_length_samples(playback_loop.beat_duration_millis, sample_rate);

        Ok(Self {
            sample_rate,
            beat_length_samples,
            voice_runtimes,
            #[cfg(target_os = "macos")]
            mts_master,
            mix_gain: GainRamp::new(MASTER_GAIN),
            beat_index: 0,
            sample_in_beat: 0,
            playback_loop,
            shared_loop,
            playhead,
        })
    }

    fn write<T>(&mut self, output: &mut [T], channels: usize)
    where
        T: Sample + FromSample<f32>,
    {
        if channels == 0 {
            return;
        }
        self.refresh_loop_snapshot();
        for frame in output.chunks_mut(channels) {
            write_device_frame(frame, self.next_frame());
        }
    }

    fn refresh_loop_snapshot(&mut self) {
        let playback_loop = {
            let Ok(playback_loop) = self.shared_loop.try_lock() else {
                return;
            };
            if playback_loop.version == self.playback_loop.version {
                return;
            }
            playback_loop.clone()
        };

        let range_changed = playback_loop.first_arrangement_beat
            != self.playback_loop.first_arrangement_beat
            || playback_loop.beat_count != self.playback_loop.beat_count;
        let scene_changed = playback_loop.acoustic_scene != self.playback_loop.acoustic_scene;
        #[cfg(target_os = "macos")]
        if self.mts_master.is_none()
            && playback_loop
                .voices
                .iter()
                .any(|voice| voice.voice_type.uses_surge_xt())
        {
            match prepare_mts_master(&playback_loop) {
                Ok(master) => self.mts_master = master,
                Err(error) => {
                    eprintln!("cannot update playback with Surge XT: {error}");
                    return;
                }
            }
        }
        let mut previous_runtimes = std::mem::take(&mut self.voice_runtimes);
        self.voice_runtimes = playback_loop
            .voices
            .iter()
            .enumerate()
            .map(|(voice_index, voice)| {
                let Some(index) = previous_runtimes
                    .iter()
                    .position(|runtime| runtime.id == voice.id)
                else {
                    return VoiceRuntime::new(
                        voice,
                        voice_index,
                        playback_loop.voices.len(),
                        &playback_loop.acoustic_scene,
                        self.sample_rate,
                        #[cfg(target_os = "macos")]
                        self.mts_master.as_ref(),
                    );
                };
                let mut runtime = previous_runtimes.swap_remove(index);
                if scene_changed || runtime.position != voice.position {
                    runtime.position = voice.position;
                    runtime.spatializer = VoiceSpatializer::new(
                        &playback_loop.acoustic_scene,
                        voice.position,
                        f64::from(self.sample_rate),
                    );
                }
                runtime.reconcile_voice_type(
                    voice.voice_type,
                    voice_index,
                    playback_loop.voices.len(),
                    self.sample_rate,
                    #[cfg(target_os = "macos")]
                    self.mts_master.as_ref(),
                )?;
                Ok(runtime)
            })
            .collect::<Result<Vec<_>, PlaybackError>>()
            .expect("validated playback update could not create its voice instruments");
        self.playback_loop = playback_loop;
        self.beat_length_samples =
            beat_length_samples(self.playback_loop.beat_duration_millis, self.sample_rate);
        if range_changed {
            self.beat_index = 0;
            self.sample_in_beat = 0;
            self.publish_playhead();
        } else {
            self.beat_index %= self.playback_loop.beat_count;
            self.sample_in_beat %= self.beat_length_samples;
        }
    }

    fn next_frame(&mut self) -> StereoFrame {
        let mut mixed = StereoFrame::SILENCE;
        let mut sounding_voice_count = 0_u32;

        for (voice_index, voice) in self.playback_loop.voices.iter().enumerate() {
            let runtime = &mut self.voice_runtimes[voice_index];
            let (contribution, acoustically_active) = runtime.render(
                voice,
                Some(self.beat_index),
                self.sample_in_beat,
                self.beat_length_samples,
                self.sample_rate,
            );
            mixed.add(contribution);
            if acoustically_active {
                sounding_voice_count += 1;
            }
        }

        let mix_gain = self.mix_gain.next(mix_gain_target(sounding_voice_count));

        self.advance_playhead();
        if sounding_voice_count == 0 {
            StereoFrame::SILENCE
        } else {
            mixed.scale(mix_gain).clamp()
        }
    }

    fn advance_playhead(&mut self) {
        self.sample_in_beat += 1;
        if self.sample_in_beat >= self.beat_length_samples {
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

pub(crate) struct OfflineRenderer {
    sample_rate: f32,
    beat_length_samples: u32,
    voice_runtimes: Vec<VoiceRuntime>,
    voice_frames: Vec<StereoFrame>,
    mix_gain: GainRamp,
    beat_index: usize,
    sample_in_beat: u32,
    playback_loop: PlaybackLoop,
}

impl OfflineRenderer {
    pub(crate) fn new(
        playback_loop: PlaybackLoop,
        sample_rate: u32,
    ) -> Result<Self, PlaybackError> {
        let sample_rate = sample_rate as f32;
        #[cfg(target_os = "macos")]
        let mts_master = prepare_mts_master(&playback_loop)?;
        let voice_runtimes = playback_loop
            .voices
            .iter()
            .enumerate()
            .map(|(voice_index, voice)| {
                VoiceRuntime::new(
                    voice,
                    voice_index,
                    playback_loop.voices.len(),
                    &playback_loop.acoustic_scene,
                    sample_rate,
                    #[cfg(target_os = "macos")]
                    mts_master.as_ref(),
                )
            })
            .collect::<Result<Vec<_>, PlaybackError>>()?;
        let voice_frames = vec![StereoFrame::SILENCE; playback_loop.voices.len()];
        let beat_length_samples =
            beat_length_samples(playback_loop.beat_duration_millis, sample_rate);

        Ok(Self {
            sample_rate,
            beat_length_samples,
            voice_runtimes,
            voice_frames,
            mix_gain: GainRamp::new(MASTER_GAIN),
            beat_index: 0,
            sample_in_beat: 0,
            playback_loop,
        })
    }

    pub(crate) fn voice_count(&self) -> usize {
        self.voice_frames.len()
    }

    pub(crate) fn score_frame_count(&self) -> u64 {
        self.playback_loop.beat_count as u64 * u64::from(self.beat_length_samples)
    }

    pub(crate) fn next_frame(&mut self) -> Option<(StereoFrame, &[StereoFrame])> {
        let score_is_active = self.beat_index < self.playback_loop.beat_count;
        let beat_index = score_is_active.then_some(self.beat_index);
        let mut sounding_voice_count = 0_u32;

        for (voice_index, voice) in self.playback_loop.voices.iter().enumerate() {
            let (contribution, acoustically_active) = self.voice_runtimes[voice_index].render(
                voice,
                beat_index,
                self.sample_in_beat,
                self.beat_length_samples,
                self.sample_rate,
            );
            self.voice_frames[voice_index] = contribution;
            if acoustically_active {
                sounding_voice_count += 1;
            }
        }

        if !score_is_active && sounding_voice_count == 0 {
            return None;
        }

        let mix_gain = self.mix_gain.next(mix_gain_target(sounding_voice_count));
        let mut mixed = StereoFrame::SILENCE;
        for frame in &mut self.voice_frames {
            *frame = frame.scale(mix_gain);
            mixed.add(*frame);
        }
        self.advance_score();

        Some((mixed.clamp(), &self.voice_frames))
    }

    fn advance_score(&mut self) {
        if self.beat_index >= self.playback_loop.beat_count {
            return;
        }

        self.sample_in_beat += 1;
        if self.sample_in_beat >= self.beat_length_samples {
            self.sample_in_beat = 0;
            self.beat_index += 1;
        }
    }
}

fn beat_length_samples(duration: BeatDurationMillis, sample_rate: f32) -> u32 {
    let sample_rate = sample_rate.round().max(1.0) as u64;
    let samples = (u64::from(duration.get()) * sample_rate + 500) / 1_000;
    u32::try_from(samples.max(1)).unwrap_or(u32::MAX)
}

fn mix_gain_target(sounding_voice_count: u32) -> f32 {
    if sounding_voice_count == 0 {
        MASTER_GAIN
    } else {
        MASTER_GAIN / (sounding_voice_count as f32).sqrt()
    }
}

struct GainRamp {
    current: f32,
    target: f32,
    samples_remaining: u32,
}

impl GainRamp {
    fn new(gain: f32) -> Self {
        Self {
            current: gain,
            target: gain,
            samples_remaining: 0,
        }
    }

    fn next(&mut self, target: f32) -> f32 {
        if target != self.target {
            self.target = target;
            self.samples_remaining = MIX_GAIN_RAMP_SAMPLES;
        }

        if self.samples_remaining > 0 {
            self.current += (self.target - self.current) / self.samples_remaining as f32;
            self.samples_remaining -= 1;
        } else {
            self.current = self.target;
        }

        self.current
    }
}

struct VoiceRuntime {
    id: VoiceId,
    position: Point3Meters,
    instrument: InstrumentRuntime,
    spatializer: VoiceSpatializer,
}

impl VoiceRuntime {
    fn new(
        voice: &PlaybackVoice,
        voice_index: usize,
        voice_count: usize,
        scene: &AcousticScene,
        sample_rate: f32,
        #[cfg(target_os = "macos")] mts_master: Option<&Arc<MtsEspMaster>>,
    ) -> Result<Self, PlaybackError> {
        Ok(Self {
            id: voice.id,
            position: voice.position,
            instrument: InstrumentRuntime::new(
                voice.voice_type,
                voice_index,
                voice_count,
                sample_rate,
                #[cfg(target_os = "macos")]
                mts_master,
            )?,
            spatializer: VoiceSpatializer::new(scene, voice.position, f64::from(sample_rate)),
        })
    }

    fn reconcile_voice_type(
        &mut self,
        voice_type: VoiceType,
        voice_index: usize,
        voice_count: usize,
        sample_rate: f32,
        #[cfg(target_os = "macos")] mts_master: Option<&Arc<MtsEspMaster>>,
    ) -> Result<(), PlaybackError> {
        if !self
            .instrument
            .matches(voice_type, voice_index, voice_count)
        {
            self.instrument = InstrumentRuntime::new(
                voice_type,
                voice_index,
                voice_count,
                sample_rate,
                #[cfg(target_os = "macos")]
                mts_master,
            )?;
        }
        Ok(())
    }

    fn render(
        &mut self,
        voice: &PlaybackVoice,
        beat_index: Option<usize>,
        sample_in_beat: u32,
        beat_length: u32,
        sample_rate: f32,
    ) -> (StereoFrame, bool) {
        let (source_sample, source_is_active) =
            self.instrument
                .sample(voice, beat_index, sample_in_beat, beat_length, sample_rate);

        self.spatializer
            .process(source_sample * voice.volume_multiplier, source_is_active)
    }
}

enum InstrumentRuntime {
    BuiltIn(OscillatorRuntime),
    GamelanMetallophone(GamelanMetallophoneRuntime),
    NoitechBellA(NoitechBellARuntime),
    NoitechBellB(NoitechBellBRuntime),
    Recovered(RecoveredVoiceRuntime),
    #[cfg(target_os = "macos")]
    SurgeXt(SurgeXtRuntime),
}

impl InstrumentRuntime {
    fn new(
        voice_type: VoiceType,
        voice_index: usize,
        voice_count: usize,
        sample_rate: f32,
        #[cfg(target_os = "macos")] mts_master: Option<&Arc<MtsEspMaster>>,
    ) -> Result<Self, PlaybackError> {
        if voice_type.uses_recovered_runtime() {
            return Ok(Self::Recovered(RecoveredVoiceRuntime::new(
                voice_type,
                sample_rate,
            )));
        }
        match voice_type {
            VoiceType::Sin
            | VoiceType::Saw
            | VoiceType::HarmonicSaw
            | VoiceType::RadlerDullSaw
            | VoiceType::RadlerHarmonics => Ok(Self::BuiltIn(OscillatorRuntime::new(voice_type))),
            VoiceType::GamelanMetallophone => {
                Ok(Self::GamelanMetallophone(GamelanMetallophoneRuntime::new()))
            }
            VoiceType::NoitechBellA => Ok(Self::NoitechBellA(NoitechBellARuntime::new())),
            VoiceType::NoitechBellB => Ok(Self::NoitechBellB(NoitechBellBRuntime::new())),
            VoiceType::SurgeXtPiano
            | VoiceType::SurgeXtDistortedElectricGuitar
            | VoiceType::SurgeXtClarinet => {
                #[cfg(target_os = "macos")]
                {
                    let master = mts_master.ok_or_else(|| {
                        PlaybackError::new(format!(
                            "{} requires the MTS-ESP exact-frequency master",
                            voice_type.label()
                        ))
                    })?;
                    return SurgeXtRuntime::new(
                        voice_type,
                        voice_index,
                        voice_count,
                        sample_rate,
                        Arc::clone(master),
                    )
                    .map(Self::SurgeXt);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Err(PlaybackError::new(format!(
                        "{} voices are currently supported only on macOS",
                        voice_type.label()
                    )))
                }
            }
            _ => unreachable!("recovered voice was handled before the instrument match"),
        }
    }

    fn matches(&self, voice_type: VoiceType, voice_index: usize, voice_count: usize) -> bool {
        match self {
            Self::BuiltIn(oscillator) => oscillator.voice_type() == voice_type,
            Self::GamelanMetallophone(_) => voice_type == VoiceType::GamelanMetallophone,
            Self::NoitechBellA(_) => voice_type == VoiceType::NoitechBellA,
            Self::NoitechBellB(_) => voice_type == VoiceType::NoitechBellB,
            Self::Recovered(runtime) => runtime.voice_type() == voice_type,
            #[cfg(target_os = "macos")]
            Self::SurgeXt(runtime) => {
                voice_type == runtime.voice_type
                    && runtime.voice_index == voice_index
                    && runtime.voice_count == voice_count
            }
        }
    }

    fn sample(
        &mut self,
        voice: &PlaybackVoice,
        beat_index: Option<usize>,
        sample_in_beat: u32,
        beat_length: u32,
        sample_rate: f32,
    ) -> (f32, bool) {
        match self {
            Self::BuiltIn(oscillator) => {
                let Some(beat_index) = beat_index else {
                    return (0.0, false);
                };
                let Some((strike_beat, strike)) = active_strike(voice, beat_index) else {
                    return (0.0, false);
                };
                let delay = voice.delays[strike_beat].min(beat_length.saturating_sub(1));
                if beat_index == strike_beat && sample_in_beat < delay {
                    return (0.0, false);
                }
                let elapsed_beats = beat_index - strike_beat;
                let note_sample = (elapsed_beats as u32)
                    .saturating_mul(beat_length)
                    .saturating_add(sample_in_beat)
                    .saturating_sub(delay);
                let note_length = u32::from(strike.duration.beats_or_one())
                    .saturating_mul(beat_length)
                    .saturating_sub(delay);
                (
                    oscillator.sample(strike.frequency.as_hz_f32(), sample_rate)
                        * envelope(note_sample, note_length)
                        * strike.volume,
                    true,
                )
            }
            Self::NoitechBellA(runtime) => {
                if let Some(beat_index) = beat_index {
                    if let Some(strike) = voice.strikes[beat_index] {
                        let delay = voice.delays[beat_index].min(beat_length.saturating_sub(1));
                        if sample_in_beat == delay {
                            runtime.trigger_with_volume_and_cutoff(
                                strike.frequency.as_hz_f32(),
                                strike.volume,
                                explicit_cutoff_samples(strike, beat_length, delay),
                            );
                        }
                    }
                }
                runtime.sample(sample_rate)
            }
            Self::NoitechBellB(runtime) => {
                if let Some(beat_index) = beat_index {
                    if let Some(strike) = voice.strikes[beat_index] {
                        let delay = voice.delays[beat_index].min(beat_length.saturating_sub(1));
                        if sample_in_beat == delay {
                            runtime.trigger_with_volume_and_cutoff(
                                strike.frequency.as_hz_f32(),
                                strike.volume,
                                explicit_cutoff_samples(strike, beat_length, delay),
                            );
                        }
                    }
                }
                runtime.sample(sample_rate)
            }
            Self::GamelanMetallophone(runtime) => {
                if let Some(beat_index) = beat_index {
                    if let Some(strike) = voice.strikes[beat_index] {
                        let delay = voice.delays[beat_index].min(beat_length.saturating_sub(1));
                        if sample_in_beat == delay {
                            runtime.trigger_with_volume_and_cutoff(
                                strike.frequency.as_hz_f32(),
                                strike.volume,
                                explicit_cutoff_samples(strike, beat_length, delay),
                            );
                        }
                    }
                }
                runtime.sample(sample_rate)
            }
            Self::Recovered(runtime) => {
                if let Some(beat_index) = beat_index {
                    if let Some(strike) = voice.strikes[beat_index] {
                        let delay = voice.delays[beat_index].min(beat_length.saturating_sub(1));
                        if sample_in_beat == delay {
                            runtime.trigger_with_volume_and_cutoff(
                                strike.frequency.as_hz_f32(),
                                strike.volume,
                                explicit_cutoff_samples(strike, beat_length, delay),
                            );
                        }
                    }
                }
                runtime.sample(sample_rate)
            }
            #[cfg(target_os = "macos")]
            Self::SurgeXt(runtime) => {
                runtime.sample(voice, beat_index, sample_in_beat, beat_length)
            }
        }
    }
}

fn active_strike(voice: &PlaybackVoice, beat_index: usize) -> Option<(usize, PlaybackStrike)> {
    voice.strikes[..=beat_index]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(strike_beat, strike)| strike.map(|strike| (strike_beat, strike)))
        .filter(|(strike_beat, strike)| {
            beat_index - strike_beat < usize::from(strike.duration.beats_or_one())
        })
}

fn explicit_cutoff_samples(strike: PlaybackStrike, beat_length: u32, delay: u32) -> Option<u32> {
    strike.duration.explicit_beats().map(|duration_beats| {
        u32::from(duration_beats)
            .saturating_mul(beat_length)
            .saturating_sub(delay)
            .max(1)
    })
}

#[cfg(target_os = "macos")]
struct SurgeXtRuntime {
    synth: SurgeXt,
    master: Arc<MtsEspMaster>,
    voice_type: VoiceType,
    gain: f32,
    voice_index: usize,
    voice_count: usize,
    active_note: Option<MtsNoteAddress>,
    stereo_buffer: Vec<f32>,
    buffer_frame: usize,
    buffered_frames: usize,
    buffer_has_audio: bool,
    failed: bool,
}

#[cfg(target_os = "macos")]
impl SurgeXtRuntime {
    fn new(
        voice_type: VoiceType,
        voice_index: usize,
        voice_count: usize,
        sample_rate: f32,
        master: Arc<MtsEspMaster>,
    ) -> Result<Self, PlaybackError> {
        if voice_index >= 16 {
            return Err(PlaybackError::new(
                "Surge XT exact-frequency playback supports at most 16 project voices",
            ));
        }
        let (patch, gain) = match voice_type {
            VoiceType::SurgeXtPiano => (SurgeXtPatch::GrandPiano, SURGE_PIANO_GAIN),
            VoiceType::SurgeXtDistortedElectricGuitar => {
                (SurgeXtPatch::DistortedElectricGuitar, 1.0)
            }
            VoiceType::SurgeXtClarinet => (SurgeXtPatch::Clarinet, 1.0),
            VoiceType::Sin
            | VoiceType::Saw
            | VoiceType::HarmonicSaw
            | VoiceType::GamelanMetallophone
            | VoiceType::NoitechBellA
            | VoiceType::NoitechBellB
            | VoiceType::NoitechBellG
            | VoiceType::NoitechBellH
            | VoiceType::NoitechBellI
            | VoiceType::NoitechBellJ
            | VoiceType::NoitechBellK
            | VoiceType::NoitechBellL
            | VoiceType::NoitechBellM
            | VoiceType::IconoclastBellG
            | VoiceType::IconoclastBellH
            | VoiceType::IconoclastIndustrialBar
            | VoiceType::CtpianoBars
            | VoiceType::CtpianoDkSquare
            | VoiceType::CtpianoEmphaenharm
            | VoiceType::CtpianoHiSaw
            | VoiceType::CtpianoLoSaw
            | VoiceType::CtpianoLoSquare
            | VoiceType::CtpianoTriangleDrop
            | VoiceType::RadlerDullSaw
            | VoiceType::RadlerHarmonics
            | VoiceType::LegacyNoitechEnharmonic => {
                unreachable!("built-in voices do not use Surge XT")
            }
        };
        let synth = SurgeXt::new_with_patch(f64::from(sample_rate), patch).map_err(|error| {
            PlaybackError::new(format!(
                "failed to prepare {} voice: {error}",
                voice_type.label()
            ))
        })?;
        Ok(Self {
            synth,
            master,
            voice_type,
            gain,
            voice_index,
            voice_count,
            active_note: None,
            stereo_buffer: vec![0.0; SURGE_RENDER_BLOCK_FRAMES * 2],
            buffer_frame: 0,
            buffered_frames: 0,
            buffer_has_audio: false,
            failed: false,
        })
    }

    fn sample(
        &mut self,
        voice: &PlaybackVoice,
        beat_index: Option<usize>,
        sample_in_beat: u32,
        beat_length: u32,
    ) -> (f32, bool) {
        if self.failed {
            return (0.0, false);
        }
        if self.buffer_frame >= self.buffered_frames {
            if let Err(error) = self.fill_buffer(voice, beat_index, sample_in_beat, beat_length) {
                eprintln!("Surge XT render stopped: {error}");
                self.failed = true;
                return (0.0, false);
            }
        }

        let offset = self.buffer_frame * 2;
        let sample =
            (self.stereo_buffer[offset] + self.stereo_buffer[offset + 1]) * 0.5 * self.gain;
        self.buffer_frame += 1;
        (sample, self.active_note.is_some() || self.buffer_has_audio)
    }

    fn fill_buffer(
        &mut self,
        voice: &PlaybackVoice,
        beat_index: Option<usize>,
        sample_in_beat: u32,
        beat_length: u32,
    ) -> Result<(), PlaybackError> {
        let continuing_strike = beat_index
            .and_then(|beat_index| active_strike(voice, beat_index))
            .is_some_and(|(strike_beat, _)| strike_beat < beat_index.unwrap());
        if sample_in_beat == 0 && !continuing_strike {
            if let Some(address) = self.active_note.take() {
                self.synth
                    .note_off(address.channel, address.note, 0)
                    .map_err(|error| PlaybackError::new(error.to_string()))?;
            }
        }

        let next_boundary = if let Some(beat_index) = beat_index {
            let delay = voice.delays[beat_index].min(beat_length.saturating_sub(1));
            if sample_in_beat == delay {
                if let Some(strike) = voice.strikes[beat_index] {
                    if let Some(address) = self.active_note.take() {
                        self.synth
                            .note_off(address.channel, address.note, 0)
                            .map_err(|error| PlaybackError::new(error.to_string()))?;
                    }
                    let frequency = strike.frequency;
                    let address = MtsNoteAddress {
                        // Surge's default channel-2/channel-3 scene behavior does
                        // not reliably apply MTS retuning outside MIDI channel 1.
                        // Instances are already isolated; disjoint note lanes
                        // prevent collisions in the shared general tuning table.
                        channel: 0,
                        note: collision_free_midi_note(
                            frequency,
                            self.voice_index,
                            self.voice_count,
                        ),
                    };
                    self.master.set_frequency(address, frequency.as_hz());
                    self.synth
                        .note_on(
                            address.channel,
                            address.note,
                            (strike.volume * 127.0).round() as u8,
                            0,
                        )
                        .map_err(|error| PlaybackError::new(error.to_string()))?;
                    self.active_note = Some(address);
                }
            }
            if sample_in_beat < delay && voice.strikes[beat_index].is_some() {
                delay
            } else {
                beat_length
            }
        } else {
            if let Some(address) = self.active_note.take() {
                self.synth
                    .note_off(address.channel, address.note, 0)
                    .map_err(|error| PlaybackError::new(error.to_string()))?;
            }
            sample_in_beat.saturating_add(SURGE_RENDER_BLOCK_FRAMES as u32)
        };
        let frames_until_boundary = next_boundary.saturating_sub(sample_in_beat).max(1) as usize;
        self.buffered_frames = frames_until_boundary.min(SURGE_RENDER_BLOCK_FRAMES);
        self.buffer_frame = 0;
        let samples = self.buffered_frames * 2;
        self.stereo_buffer[..samples].fill(0.0);
        self.synth
            .render(&mut self.stereo_buffer[..samples])
            .map_err(|error| PlaybackError::new(error.to_string()))?;
        self.buffer_has_audio = self.stereo_buffer[..samples]
            .iter()
            .any(|sample| sample.abs() > SURGE_SILENCE_THRESHOLD);
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn collision_free_midi_note(frequency: FrequencyHz, voice_index: usize, voice_count: usize) -> u8 {
    let ideal_note = 69.0 + 12.0 * (frequency.as_hz() / 440.0).log2();
    (0_u8..=127)
        .filter(|note| usize::from(*note) % voice_count == voice_index)
        .min_by(|left, right| {
            (f64::from(*left) - ideal_note)
                .abs()
                .total_cmp(&(f64::from(*right) - ideal_note).abs())
        })
        .expect("a validated project voice owns at least one MIDI note")
}

fn validate_external_voice_support(project: &Project) -> Result<(), PlaybackError> {
    let surge_voice_count = project
        .voices()
        .iter()
        .filter(|voice| voice.voice_type.uses_surge_xt())
        .count();
    if surge_voice_count == 0 {
        return Ok(());
    }
    if project.voices().len() > 16 {
        return Err(PlaybackError::new(
            "Surge XT exact-frequency playback supports at most 16 project voices",
        ));
    }

    #[cfg(target_os = "macos")]
    {
        if !SurgeXt::is_available() {
            return Err(PlaybackError::new(
                "Surge XT Audio Unit aumu/SgXT/VmbA is not installed",
            ));
        }
        if !MtsEspMaster::is_available() {
            return Err(PlaybackError::new(
                "Surge XT exact-frequency playback requires the free MTS-ESP middleware",
            ));
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(PlaybackError::new(
            "Surge XT voices are currently supported only on macOS",
        ))
    }
}

#[cfg(target_os = "macos")]
fn prepare_mts_master(
    playback_loop: &PlaybackLoop,
) -> Result<Option<Arc<MtsEspMaster>>, PlaybackError> {
    if !playback_loop
        .voices
        .iter()
        .any(|voice| voice.voice_type.uses_surge_xt())
    {
        return Ok(None);
    }
    MtsEspMaster::new()
        .map(Arc::new)
        .map(Some)
        .map_err(PlaybackError::from_mts_esp)
}

pub fn reset_mts_esp_master() -> Result<(), PlaybackError> {
    #[cfg(target_os = "macos")]
    {
        MtsEspMaster::reinitialize().map_err(PlaybackError::from_mts_esp)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(PlaybackError::new(
            "MTS-ESP master recovery is supported only on macOS",
        ))
    }
}

enum OscillatorRuntime {
    Sin { phase: f32 },
    Saw { phase: f32 },
    HarmonicSaw(HarmonicSawRuntime),
    RadlerDullSaw { phase: f32 },
    RadlerHarmonics { phase: f32 },
}

impl OscillatorRuntime {
    fn new(voice_type: VoiceType) -> Self {
        match voice_type {
            VoiceType::Sin => Self::Sin { phase: 0.0 },
            VoiceType::Saw => Self::Saw { phase: 0.0 },
            VoiceType::HarmonicSaw => Self::HarmonicSaw(HarmonicSawRuntime::new()),
            VoiceType::RadlerDullSaw => Self::RadlerDullSaw { phase: 0.0 },
            VoiceType::RadlerHarmonics => Self::RadlerHarmonics { phase: 0.0 },
            VoiceType::NoitechBellA => unreachable!("Noitech Bell A has a tail-aware runtime"),
            VoiceType::NoitechBellB => unreachable!("Noitech Bell B has a tail-aware runtime"),
            VoiceType::GamelanMetallophone => {
                unreachable!("gamelan metallophone has a tail-aware runtime")
            }
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
            | VoiceType::CtpianoBars
            | VoiceType::CtpianoDkSquare
            | VoiceType::CtpianoEmphaenharm
            | VoiceType::CtpianoHiSaw
            | VoiceType::CtpianoLoSaw
            | VoiceType::CtpianoLoSquare
            | VoiceType::CtpianoTriangleDrop
            | VoiceType::LegacyNoitechEnharmonic => {
                unreachable!("recovered voice has a tail-aware runtime")
            }
            VoiceType::SurgeXtPiano
            | VoiceType::SurgeXtDistortedElectricGuitar
            | VoiceType::SurgeXtClarinet => {
                unreachable!("external instruments are not oscillators")
            }
        }
    }

    fn voice_type(&self) -> VoiceType {
        match self {
            Self::Sin { .. } => VoiceType::Sin,
            Self::Saw { .. } => VoiceType::Saw,
            Self::HarmonicSaw(_) => VoiceType::HarmonicSaw,
            Self::RadlerDullSaw { .. } => VoiceType::RadlerDullSaw,
            Self::RadlerHarmonics { .. } => VoiceType::RadlerHarmonics,
        }
    }

    fn sample(&mut self, frequency: f32, sample_rate: f32) -> f32 {
        match self {
            Self::Sin { phase } => {
                let sample = (*phase * std::f32::consts::TAU).sin();
                advance_phase(phase, frequency, sample_rate);
                sample
            }
            Self::Saw { phase } => {
                let sample = (*phase * 2.0) - 1.0;
                advance_phase(phase, frequency, sample_rate);
                sample
            }
            Self::RadlerDullSaw { phase } => {
                let sample = radler_dull_saw(*phase, frequency, sample_rate);
                advance_phase(phase, frequency, sample_rate);
                sample
            }
            Self::RadlerHarmonics { phase } => {
                let sample = (std::f32::consts::TAU * *phase).sin()
                    + 0.5 * (std::f32::consts::TAU * *phase * 2.0).sin()
                    + 0.2 * (std::f32::consts::TAU * *phase * 3.0).sin();
                advance_phase(phase, frequency, sample_rate);
                sample / 1.7
            }
            Self::HarmonicSaw(runtime) => runtime.sample(frequency, sample_rate),
        }
    }
}

fn advance_phase(phase: &mut f32, frequency: f32, sample_rate: f32) {
    *phase = (*phase + frequency / sample_rate).fract();
}

fn radler_dull_saw(phase: f32, fundamental: f32, sample_rate: f32) -> f32 {
    let normalization = binomial(20, 10) as f32;
    (1_u32..=10)
        .filter(|harmonic| {
            fundamental * (*harmonic as f32) < sample_rate * 0.5 * HARMONIC_SAW_NYQUIST_MARGIN
        })
        .map(|harmonic| {
            let weight = binomial(20, 10 - harmonic) as f32 / normalization / harmonic as f32;
            (std::f32::consts::TAU * phase * harmonic as f32).sin() * weight
        })
        .sum()
}

const fn binomial(n: u32, k: u32) -> u64 {
    let k = if k < n - k { k } else { n - k };
    let mut result = 1_u64;
    let mut index = 0;
    while index < k {
        result = result * (n - index) as u64 / (index + 1) as u64;
        index += 1;
    }
    result
}

struct HarmonicSawRuntime {
    partials: [HarmonicPartial; HARMONIC_SAW_PARTIAL_COUNT],
    prepared: Option<PreparedHarmonicSaw>,
    samples_since_normalization: u32,
}

impl HarmonicSawRuntime {
    fn new() -> Self {
        Self {
            partials: std::array::from_fn(|index| HarmonicPartial::new(index + 1)),
            prepared: None,
            samples_since_normalization: 0,
        }
    }

    fn sample(&mut self, fundamental: f32, sample_rate: f32) -> f32 {
        let active_partial_count = match self.prepared {
            Some(prepared)
                if prepared.fundamental == fundamental && prepared.sample_rate == sample_rate =>
            {
                prepared.active_partial_count
            }
            _ => self.prepare(fundamental, sample_rate),
        };
        let mut sample = 0.0;
        for partial in &mut self.partials[..active_partial_count] {
            sample -= partial.sin_phase * partial.amplitude;
            partial.advance();
        }

        self.samples_since_normalization += 1;
        if self.samples_since_normalization >= HARMONIC_SAW_RENORMALIZE_INTERVAL {
            for partial in &mut self.partials[..active_partial_count] {
                partial.normalize_phase();
            }
            self.samples_since_normalization = 0;
        }

        sample
    }

    fn prepare(&mut self, fundamental: f32, sample_rate: f32) -> usize {
        let maximum_frequency = sample_rate * 0.5 * HARMONIC_SAW_NYQUIST_MARGIN;
        let mut active_partial_count = 0;

        for partial in &mut self.partials {
            let frequency = fundamental * partial.frequency_ratio;
            if frequency >= maximum_frequency {
                break;
            }
            partial.set_frequency(frequency, sample_rate);
            active_partial_count += 1;
        }

        self.prepared = Some(PreparedHarmonicSaw {
            fundamental,
            sample_rate,
            active_partial_count,
        });
        active_partial_count
    }
}

#[derive(Clone, Copy)]
struct PreparedHarmonicSaw {
    fundamental: f32,
    sample_rate: f32,
    active_partial_count: usize,
}

struct HarmonicPartial {
    sin_phase: f32,
    cos_phase: f32,
    sin_step: f32,
    cos_step: f32,
    frequency_ratio: f32,
    amplitude: f32,
}

impl HarmonicPartial {
    fn new(number: usize) -> Self {
        let number = number as f32;
        let stretch = ((1.0 + HARMONIC_SAW_INHARMONICITY * number * number)
            / (1.0 + HARMONIC_SAW_INHARMONICITY))
            .sqrt();

        Self {
            sin_phase: 0.0,
            cos_phase: 1.0,
            sin_step: 0.0,
            cos_step: 1.0,
            frequency_ratio: number * stretch,
            amplitude: (2.0 / std::f32::consts::PI) / number.powf(HARMONIC_SAW_SPECTRAL_SLOPE),
        }
    }

    fn set_frequency(&mut self, frequency: f32, sample_rate: f32) {
        let phase_step = std::f32::consts::TAU * frequency / sample_rate;
        (self.sin_step, self.cos_step) = phase_step.sin_cos();
    }

    fn advance(&mut self) {
        let sin_phase = self.sin_phase * self.cos_step + self.cos_phase * self.sin_step;
        let cos_phase = self.cos_phase * self.cos_step - self.sin_phase * self.sin_step;
        self.sin_phase = sin_phase;
        self.cos_phase = cos_phase;
    }

    fn normalize_phase(&mut self) {
        let magnitude = (self.sin_phase * self.sin_phase + self.cos_phase * self.cos_phase).sqrt();
        if magnitude > 0.0 {
            self.sin_phase /= magnitude;
            self.cos_phase /= magnitude;
        }
    }
}

fn write_device_frame<T>(device_frame: &mut [T], frame: StereoFrame)
where
    T: Sample + FromSample<f32>,
{
    match device_frame {
        [] => {}
        [mono] => *mono = T::from_sample((frame.left + frame.right) * 0.5),
        [left, right, remaining @ ..] => {
            *left = T::from_sample(frame.left);
            *right = T::from_sample(frame.right);
            for sample in remaining {
                *sample = T::from_sample(0.0);
            }
        }
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
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    };

    #[cfg(target_os = "macos")]
    use super::collision_free_midi_note;
    use super::{
        beat_length_samples, frequency_variance_seed, normally_distributed_delay, timing_seed,
        varied_frequency, write_device_frame, AudioEngine, BeatRange, GainRamp, HarmonicPartial,
        HarmonicSawRuntime, InstrumentRuntime, OfflineRenderer, PlaybackLoop,
        HARMONIC_SAW_PARTIAL_COUNT, MIX_GAIN_RAMP_SAMPLES,
    };
    #[cfg(target_os = "macos")]
    use crate::mts_esp::MtsEspTuningProbe;
    use crate::{
        acoustics::{Point3Meters, StereoFrame},
        part::{Part, PartScore},
        pitch_system::{
            ExplicitPitchSystem, FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem,
            PitchSystem,
        },
        project::{FrequencyVariance, Project, Voice, VoiceId, VoiceType, VoiceVolumeAdjustment},
        seed::Seed,
    };

    #[test]
    fn harmonic_saw_keeps_its_fundamental_and_gently_stretches_every_upper_partial() {
        let partials = (1..=HARMONIC_SAW_PARTIAL_COUNT)
            .map(HarmonicPartial::new)
            .collect::<Vec<_>>();

        assert_eq!(partials[0].frequency_ratio, 1.0);
        for (index, partial) in partials.iter().enumerate().skip(1) {
            let harmonic_number = (index + 1) as f32;
            assert!(partial.frequency_ratio > harmonic_number);
            assert!(partial.frequency_ratio < harmonic_number + 1.0);
            assert!(partial.amplitude < partials[index - 1].amplitude);
        }
        assert!(
            partials[1].amplitude < (2.0 / std::f32::consts::PI) / 2.0,
            "upper partials should roll off faster than an ideal saw"
        );
    }

    #[test]
    fn harmonic_saw_omits_partials_too_close_to_nyquist() {
        let mut runtime = HarmonicSawRuntime::new();

        assert_eq!(runtime.sample(1_000.0, 4_000.0), 0.0);
        assert!((runtime.partials[0].sin_phase - 1.0).abs() < 1e-6);
        assert_eq!(runtime.partials[1].sin_phase, 0.0);
        assert_eq!(runtime.prepared.unwrap().active_partial_count, 1);

        let sample = runtime.sample(1_000.0, 4_000.0);
        let expected_fundamental_peak = -(2.0 / std::f32::consts::PI);
        assert!((sample - expected_fundamental_peak).abs() < 1e-6);
    }

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
    fn voice_volume_adjustment_scales_the_source_in_offline_rendering() {
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec!["A4".to_string()]]);
        let base_project = Project::new("test", 8, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Sin,
        )]);
        let adjusted_project =
            Project::new("test", 8, 0, Seed::new(1)).with_voices(vec![Voice::new(
                1,
                "lead",
                VoiceType::Sin,
            )
            .with_volume_adjustment(Some(VoiceVolumeAdjustment::new(1.5).unwrap()))]);
        let base_rows = score.resolved_rows(&part, &base_project).unwrap();
        let adjusted_rows = score.resolved_rows(&part, &adjusted_project).unwrap();
        let mut base = OfflineRenderer::new(
            PlaybackLoop::from_rows(&base_project, base_rows, 1).unwrap(),
            48_000,
        )
        .unwrap();
        let mut adjusted = OfflineRenderer::new(
            PlaybackLoop::from_rows(&adjusted_project, adjusted_rows, 1).unwrap(),
            48_000,
        )
        .unwrap();

        let mut compared_nonzero_frame = false;
        for _ in 0..200 {
            let (_, base_voices) = base.next_frame().unwrap();
            let base_frame = base_voices[0];
            let (_, adjusted_voices) = adjusted.next_frame().unwrap();
            let adjusted_frame = adjusted_voices[0];
            if base_frame.left != 0.0 || base_frame.right != 0.0 {
                compared_nonzero_frame = true;
                assert!((adjusted_frame.left - base_frame.left * 1.5).abs() < 1e-6);
                assert!((adjusted_frame.right - base_frame.right * 1.5).abs() < 1e-6);
            }
        }
        assert!(compared_nonzero_frame);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn surge_voices_use_collision_free_keys_on_the_common_tuning_channel() {
        let clustered_frequency = FrequencyHz::new(261.625_565).unwrap();
        let notes = (0..4)
            .map(|voice_index| collision_free_midi_note(clustered_frequency, voice_index, 4))
            .collect::<Vec<_>>();
        assert_eq!(notes, vec![60, 61, 58, 59]);
        assert_eq!(notes.iter().copied().collect::<HashSet<_>>().len(), 4);

        assert_eq!(
            collision_free_midi_note(FrequencyHz::new(64.0).unwrap(), 0, 1),
            36
        );
        assert_eq!(
            collision_free_midi_note(FrequencyHz::new(440.0).unwrap(), 0, 1),
            69
        );
        assert_eq!(
            collision_free_midi_note(FrequencyHz::new(512.0).unwrap(), 0, 1),
            72
        );
    }

    #[test]
    fn isolated_part_loops_use_the_complete_part_and_part_local_beats() {
        let project = Project::new("test", 800, 25, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )]);
        let part = Part::new("not arranged", 2);
        let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);

        let playback_loop = PlaybackLoop::from_part(&project, &part, &score).unwrap();

        assert_eq!(playback_loop.first_arrangement_beat, 1);
        assert_eq!(playback_loop.beat_count, 2);
        assert_eq!(
            playback_loop.voices[0].frequencies,
            [
                project.pitch_system().resolve_cell("C4").unwrap(),
                project.pitch_system().resolve_cell("D4").unwrap(),
            ]
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
    fn project_frequency_variance_detunes_each_voice_and_beat_around_its_target() {
        let maximum_variance = FrequencyVariance::new(0.05).unwrap();
        let project = Project::new("test", 800, 0, Seed::new(7))
            .with_frequency_variance(maximum_variance)
            .with_voices(vec![
                Voice::new(1, "lead", VoiceType::Saw),
                Voice::new(2, "bass", VoiceType::Sin),
            ]);
        let part = Part::new("example", 4);
        let score = PartScore::from_rows(vec![
            vec!["A4".to_string(), "A3".to_string()],
            vec!["A4".to_string(), "A3".to_string()],
            vec!["A4".to_string(), "A3".to_string()],
            vec!["A4".to_string(), "A3".to_string()],
        ]);
        let rows = score.resolved_rows(&part, &project).unwrap();

        let first = PlaybackLoop::from_rows(&project, rows.clone(), 1).unwrap();
        let second = PlaybackLoop::from_rows(&project, rows, 1).unwrap();

        assert_eq!(first.voices[0].frequencies, second.voices[0].frequencies);
        assert_eq!(first.voices[1].frequencies, second.voices[1].frequencies);
        let lead_target = project.pitch_system().resolve_cell("A4").unwrap().unwrap();
        let offsets = first.voices[0]
            .frequencies
            .iter()
            .map(|frequency| (frequency.unwrap().as_hz() / lead_target.as_hz()) - 1.0)
            .collect::<Vec<_>>();
        assert!(offsets
            .iter()
            .all(|offset| offset.abs() <= maximum_variance.ratio() + 1e-12));
        assert!(offsets.iter().any(|offset| *offset < 0.0));
        assert!(offsets.iter().any(|offset| *offset > 0.0));
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
    fn varied_frequencies_are_stable_for_the_same_absolute_beat_in_overlapping_loops() {
        let project = Project::new("test", 800, 0, Seed::new(1))
            .with_frequency_variance(FrequencyVariance::new(0.025).unwrap())
            .with_voices(vec![
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
                &full_loop.voices[voice_index].frequencies[1..],
                overlapping_loop.voices[voice_index].frequencies
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
    fn frequency_variation_uses_a_separate_seed_per_voice_and_arrangement_beat() {
        let project_seed = Seed::new(1);
        let seeds = (1..=16)
            .flat_map(|beat| {
                (1..=4).map(move |voice_id| {
                    frequency_variance_seed(project_seed, beat, VoiceId::new(voice_id))
                })
            })
            .collect::<HashSet<_>>();

        assert_eq!(seeds.len(), 16 * 4);
        assert_ne!(
            frequency_variance_seed(Seed::new(1), 3, VoiceId::new(2)),
            frequency_variance_seed(Seed::new(2), 3, VoiceId::new(2))
        );
        assert_ne!(
            frequency_variance_seed(project_seed, 3, VoiceId::new(2)),
            timing_seed(project_seed, 3, VoiceId::new(2))
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
    fn normally_distributed_frequency_variation_is_bounded_and_centered_on_the_target() {
        let target = FrequencyHz::new(440.0).unwrap();
        let maximum_variance = FrequencyVariance::new(0.05).unwrap();
        let offsets = (0..10_000)
            .map(|index| {
                let varied = varied_frequency(Some(target), Seed::new(index), maximum_variance)
                    .unwrap()
                    .unwrap();
                (varied.as_hz() / target.as_hz()) - 1.0
            })
            .collect::<Vec<_>>();
        let central_count = offsets
            .iter()
            .filter(|&&offset| (-0.025..=0.025).contains(&offset))
            .count();

        assert!(offsets.iter().all(|offset| offset.abs() <= 0.05 + 1e-12));
        assert!(central_count > 8_000);
        assert!(offsets.iter().any(|offset| *offset <= -0.049_999));
        assert!(offsets.iter().any(|offset| *offset >= 0.049_999));
        assert_eq!(
            varied_frequency(Some(target), Seed::new(1), FrequencyVariance::default()).unwrap(),
            Some(target)
        );
        assert_eq!(
            varied_frequency(None, Seed::new(1), maximum_variance).unwrap(),
            None
        );
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
        let mut engine = AudioEngine::new(1_000.0, shared, playhead).unwrap();

        let frames = (0..24).map(|_| engine.next_frame()).collect::<Vec<_>>();

        assert!(frames.iter().all(|frame| {
            (-1.0..=1.0).contains(&frame.left) && (-1.0..=1.0).contains(&frame.right)
        }));
        assert_eq!(engine.beat_index, 0);
        assert_eq!(engine.sample_in_beat, 0);
    }

    #[test]
    fn explicit_duration_stops_bell_h_body_but_preserves_its_short_response_tail() {
        let pitch_system = PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "bell test",
                FrequencyHz::new(55.0).unwrap(),
                Interval::cents(1_200.0).unwrap(),
                (0..7)
                    .map(|degree| Interval::cents(f64::from(degree) * 100.0).unwrap())
                    .collect(),
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        );
        let project = Project::new("test", 100, 0, Seed::new(1))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, "bell", VoiceType::NoitechBellH)]);
        let part = Part::new("intro", 1);
        let beat_length = beat_length_samples(project.beat_duration_millis, 1_000.0);

        let active_samples_after_one_beat = |notation: &str| {
            let score = PartScore::from_rows(vec![vec![notation.to_string()]]);
            let playback_loop = PlaybackLoop::from_part(&project, &part, &score).unwrap();
            let voice = &playback_loop.voices[0];
            #[cfg(target_os = "macos")]
            let mut runtime =
                InstrumentRuntime::new(voice.voice_type, 0, 1, 1_000.0, None).unwrap();
            #[cfg(not(target_os = "macos"))]
            let mut runtime = InstrumentRuntime::new(voice.voice_type, 0, 1, 1_000.0).unwrap();

            for sample_in_beat in 0..beat_length {
                runtime.sample(voice, Some(0), sample_in_beat, beat_length, 1_000.0);
            }
            let mut active_samples = 0;
            while runtime.sample(voice, None, 0, beat_length, 1_000.0).1 {
                active_samples += 1;
                assert!(active_samples < 5_000, "bell response must terminate");
            }
            active_samples
        };

        assert!(active_samples_after_one_beat("60") > 3_000);
        assert!((1..=10).contains(&active_samples_after_one_beat("6001ff")));
    }

    #[test]
    fn every_voice_type_matches_between_live_and_offline_rendering() {
        for voice_type in VoiceType::BUILT_IN {
            let project = Project::new("test", 8, 0, Seed::new(1))
                .with_voices(vec![Voice::new(1, "lead", voice_type)]);
            let part = Part::new("intro", 1);
            let score = PartScore::from_rows(vec![vec!["A4".to_string()]]);
            let rows = score.resolved_rows(&part, &project).unwrap();
            let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();
            let shared = Arc::new(Mutex::new(playback_loop.clone()));
            let playhead = Arc::new(AtomicU64::new(1));
            let mut live = AudioEngine::new(48_000.0, shared, playhead).unwrap();
            let mut offline = OfflineRenderer::new(playback_loop, 48_000).unwrap();

            for _ in 0..384 {
                let live_frame = live.next_frame();
                let (offline_frame, voice_frames) = offline.next_frame().unwrap();
                assert_eq!(offline_frame, live_frame);
                assert_eq!(voice_frames.len(), 1);
                assert_eq!(voice_frames[0], live_frame);
            }
            let tail_bounds = match voice_type {
                VoiceType::NoitechBellA => Some((200_000, 250_000)),
                VoiceType::NoitechBellB => Some((160_000, 200_000)),
                VoiceType::NoitechBellG
                | VoiceType::NoitechBellH
                | VoiceType::NoitechBellI
                | VoiceType::NoitechBellJ
                | VoiceType::NoitechBellK
                | VoiceType::IconoclastBellG
                | VoiceType::IconoclastBellH => Some((180_000, 195_000)),
                VoiceType::NoitechBellL | VoiceType::NoitechBellM => Some((205_000, 212_000)),
                VoiceType::IconoclastIndustrialBar => Some((135_000, 148_000)),
                VoiceType::GamelanMetallophone => Some((250_000, 266_000)),
                VoiceType::CtpianoDkSquare => Some((1_400, 2_000)),
                VoiceType::CtpianoBars
                | VoiceType::CtpianoEmphaenharm
                | VoiceType::CtpianoHiSaw
                | VoiceType::CtpianoLoSaw
                | VoiceType::CtpianoLoSquare
                | VoiceType::CtpianoTriangleDrop
                | VoiceType::LegacyNoitechEnharmonic => Some((90_000, 100_000)),
                _ => None,
            };
            if let Some((minimum, maximum)) = tail_bounds {
                let mut tail_frames = 0;
                while offline.next_frame().is_some() {
                    tail_frames += 1;
                    assert!(tail_frames < maximum, "voice tail must terminate");
                }
                assert!(
                    tail_frames > minimum,
                    "voice must retain its source duration"
                );
            } else {
                assert!(offline.next_frame().is_none());
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires installed Surge XT and MTS-ESP"]
    fn surge_xt_piano_renders_simultaneous_clustered_exact_frequencies() {
        let frequencies = [255.0, 259.0, 263.0, 267.0].map(|hz| FrequencyHz::new(hz).unwrap());
        let pitch_system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "exact test",
                BTreeMap::from([
                    ("first".to_string(), frequencies[0]),
                    ("second".to_string(), frequencies[1]),
                    ("third".to_string(), frequencies[2]),
                    ("fourth".to_string(), frequencies[3]),
                ]),
            )
            .unwrap(),
        );
        let project = Project::new("test", 100, 0, Seed::new(1))
            .with_pitch_system(pitch_system)
            .with_voices(vec![
                Voice::new(1, "first", VoiceType::SurgeXtPiano),
                Voice::new(2, "second", VoiceType::SurgeXtPiano),
                Voice::new(3, "third", VoiceType::SurgeXtPiano),
                Voice::new(4, "fourth", VoiceType::SurgeXtPiano),
            ]);
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
            "fourth".to_string(),
        ]]);
        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();

        assert_eq!(
            playback_loop
                .voices
                .iter()
                .map(|voice| voice.frequencies[0].unwrap().as_hz())
                .collect::<Vec<_>>(),
            frequencies.map(FrequencyHz::as_hz)
        );

        let mut renderer = OfflineRenderer::new(playback_loop, 48_000).unwrap();
        let (_, first_voice_frames) = renderer.next_frame().unwrap();
        assert_eq!(first_voice_frames.len(), 4);

        let tuning = MtsEspTuningProbe::new().unwrap();
        for (voice_index, frequency) in frequencies.into_iter().enumerate() {
            let note = collision_free_midi_note(frequency, voice_index, 4);
            let published_frequency = tuning.frequency(note);
            assert!(
                (published_frequency - frequency.as_hz()).abs() < 1.0e-9,
                "voice {voice_index} requested {} Hz but the MTS general table contains {published_frequency} Hz",
                frequency.as_hz()
            );
        }

        let mut energy = [0.0_f32; 4];
        let mut voice_samples: [Vec<f32>; 4] = std::array::from_fn(|_| Vec::new());
        for _ in 0..48_000 {
            let Some((_, voice_frames)) = renderer.next_frame() else {
                break;
            };
            for ((voice_energy, samples), frame) in
                energy.iter_mut().zip(&mut voice_samples).zip(voice_frames)
            {
                *voice_energy += frame.left.abs() + frame.right.abs();
                samples.push((frame.left + frame.right) * 0.5);
            }
        }
        assert!(
            energy.into_iter().all(|voice_energy| voice_energy > 0.01),
            "a Surge XT Piano voice rendered silence"
        );
        for (voice_index, (samples, expected)) in voice_samples.iter().zip(frequencies).enumerate()
        {
            let measured = strongest_frequency_near(samples, expected.as_hz(), 48_000.0);
            let error_cents = 1_200.0 * (measured / expected.as_hz()).log2();
            assert!(
                error_cents.abs() < 40.0,
                "voice {voice_index} requested {} Hz but rendered {measured} Hz ({error_cents} cents)",
                expected.as_hz()
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires installed Surge XT and MTS-ESP"]
    fn surge_xt_distorted_electric_guitar_renders_an_exact_frequency() {
        assert_surge_voice_renders_an_exact_frequency(
            VoiceType::SurgeXtDistortedElectricGuitar,
            "guitar",
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires installed Surge XT and MTS-ESP"]
    fn surge_xt_clarinet_renders_an_exact_frequency() {
        assert_surge_voice_renders_an_exact_frequency(VoiceType::SurgeXtClarinet, "clarinet");
    }

    #[cfg(target_os = "macos")]
    fn assert_surge_voice_renders_an_exact_frequency(voice_type: VoiceType, voice_name: &str) {
        let frequency = FrequencyHz::new(259.0).unwrap();
        let pitch_system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "exact Surge XT test",
                BTreeMap::from([("exact-note".to_string(), frequency)]),
            )
            .unwrap(),
        );
        let project = Project::new("test", 100, 0, Seed::new(1))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, voice_name, voice_type)]);
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec!["exact-note".to_string()]]);
        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();
        let mut renderer = OfflineRenderer::new(playback_loop, 48_000).unwrap();

        renderer.next_frame().unwrap();
        let tuning = MtsEspTuningProbe::new().unwrap();
        let note = collision_free_midi_note(frequency, 0, 1);
        let published_frequency = tuning.frequency(note);
        assert!(
            (published_frequency - frequency.as_hz()).abs() < 1.0e-9,
            "{voice_name} requested {} Hz but the MTS general table contains {published_frequency} Hz",
            frequency.as_hz()
        );

        let mut energy = 0.0_f32;
        let mut samples = Vec::new();
        for _ in 1..48_000 {
            let Some((_, voice_frames)) = renderer.next_frame() else {
                break;
            };
            let frame = voice_frames[0];
            energy += frame.left.abs() + frame.right.abs();
            samples.push((frame.left + frame.right) * 0.5);
        }
        assert!(energy > 0.01, "Surge XT {voice_name} rendered silence");

        let measured = strongest_frequency_near(&samples, frequency.as_hz(), 48_000.0);
        let error_cents = 1_200.0 * (measured / frequency.as_hz()).log2();
        assert!(
            error_cents.abs() < 40.0,
            "{voice_name} requested {} Hz but rendered {measured} Hz ({error_cents} cents)",
            frequency.as_hz()
        );
    }

    #[cfg(target_os = "macos")]
    fn strongest_frequency_near(samples: &[f32], expected: f64, sample_rate: f64) -> f64 {
        let start = 2_400.min(samples.len());
        let end = 12_000.min(samples.len());
        let sample_count = end.saturating_sub(start);
        assert!(sample_count > 1);
        let windowed = samples[start..end]
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let phase = std::f64::consts::TAU * index as f64 / (sample_count - 1) as f64;
                f64::from(*sample) * (0.5 - 0.5 * phase.cos())
            })
            .collect::<Vec<_>>();

        let minimum = expected * 0.85;
        let maximum = expected * 1.15;
        let step = 0.05;
        let candidate_count = ((maximum - minimum) / step).ceil() as usize;
        (0..=candidate_count)
            .map(|candidate| minimum + candidate as f64 * step)
            .map(|frequency| {
                let angle = std::f64::consts::TAU * frequency / sample_rate;
                let (step_sin, step_cos) = angle.sin_cos();
                let (mut oscillator_sin, mut oscillator_cos) = (0.0, 1.0);
                let (mut imaginary, mut real) = (0.0, 0.0);
                for sample in &windowed {
                    real += sample * oscillator_cos;
                    imaginary -= sample * oscillator_sin;
                    let next_cos = oscillator_cos * step_cos - oscillator_sin * step_sin;
                    oscillator_sin = oscillator_sin * step_cos + oscillator_cos * step_sin;
                    oscillator_cos = next_cos;
                }
                (frequency, real * real + imaginary * imaginary)
            })
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .unwrap()
            .0
    }

    #[test]
    fn offline_renderer_finishes_after_emitting_acoustic_tails() {
        let project = Project::new("test", 8, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )
        .with_position(Point3Meters::new(1.0, 0.0, 0.0).unwrap())]);
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec!["A4".to_string()]]);
        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();
        let mut offline = OfflineRenderer::new(playback_loop, 48_000).unwrap();
        let mut rendered_frame_count = 0;

        while offline.next_frame().is_some() {
            rendered_frame_count += 1;
        }

        assert!(rendered_frame_count > 8);
        assert!(rendered_frame_count < 1_000);
        assert!(offline.next_frame().is_none());
    }

    #[test]
    fn positioned_voice_produces_distinct_left_and_right_channels() {
        let project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )
        .with_position(Point3Meters::new(1.0, 0.0, 0.0).unwrap())]);
        let part = Part::new("intro", 1);
        let score = PartScore::from_rows(vec![vec!["A4".to_string()]]);
        let rows = score.resolved_rows(&part, &project).unwrap();
        let playback_loop = PlaybackLoop::from_rows(&project, rows, 1).unwrap();
        let shared = Arc::new(Mutex::new(playback_loop));
        let playhead = Arc::new(AtomicU64::new(1));
        let mut engine = AudioEngine::new(44_100.0, shared, playhead).unwrap();

        let frames = (0..600).map(|_| engine.next_frame()).collect::<Vec<_>>();
        let left_energy = frames.iter().map(|frame| frame.left.abs()).sum::<f32>();
        let right_energy = frames.iter().map(|frame| frame.right.abs()).sum::<f32>();

        assert!(left_energy > 0.0);
        assert!(right_energy > left_energy);
        assert!(right_energy < left_energy * 2.0);
    }

    #[test]
    fn device_mapper_preserves_stereo_downmixes_mono_and_silences_extra_channels() {
        let frame = StereoFrame {
            left: 0.25,
            right: -0.5,
        };
        let mut mono = [0.0_f32; 1];
        let mut stereo = [0.0_f32; 2];
        let mut surround = [1.0_f32; 6];

        write_device_frame(&mut mono, frame);
        write_device_frame(&mut stereo, frame);
        write_device_frame(&mut surround, frame);

        assert_eq!(mono, [-0.125]);
        assert_eq!(stereo, [0.25, -0.5]);
        assert_eq!(surround, [0.25, -0.5, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn mix_gain_changes_are_ramped_instead_of_applied_in_one_sample() {
        let mut ramp = GainRamp::new(1.0);

        let first = ramp.next(0.5);
        let mut last = first;
        for _ in 1..MIX_GAIN_RAMP_SAMPLES {
            last = ramp.next(0.5);
        }

        assert!(first < 1.0);
        assert!(first > 0.5);
        assert_eq!(last, 0.5);
        assert_eq!(ramp.next(0.5), 0.5);
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
        let mut engine = AudioEngine::new(1_000.0, shared, Arc::clone(&playhead)).unwrap();

        engine.next_frame();
        assert_eq!(playhead.load(Ordering::Relaxed), 4);
        engine.next_frame();
        assert_eq!(playhead.load(Ordering::Relaxed), 5);
        engine.next_frame();
        engine.next_frame();
        assert_eq!(playhead.load(Ordering::Relaxed), 6);
        engine.next_frame();
        engine.next_frame();
        assert_eq!(playhead.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn beat_duration_keeps_the_same_time_at_different_sample_rates() {
        let duration = crate::project::BeatDurationMillis::new(250).unwrap();

        assert_eq!(beat_length_samples(duration, 44_100.0), 11_025);
        assert_eq!(beat_length_samples(duration, 48_000.0), 12_000);
        assert_eq!(beat_length_samples(duration, 96_000.0), 24_000);
    }
}
