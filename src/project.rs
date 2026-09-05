//! Project domain model. File operations live in the focused modules below.

pub mod parts;
pub mod storage;
pub mod transaction;
pub mod voices;

// Preserve the existing project API while implementations have a single, discoverable home.
pub use parts::{edit_part_rows, EditPartRowsError};
pub use storage::{
    create_project, duplicate_project, list_projects, load_project, project_directory_name,
    save_project, save_project_with_voice_convolution, CreateProjectError, DuplicateProjectError,
    ListProjectsError, LoadProjectError, SaveProjectError, VoiceConvolutionChange,
};
pub use transaction::{restore_project_state, ProjectTransactionError, RestoreProjectStateError};
pub use voices::{
    add_voice, add_voice_at, add_voice_with_adjustment_at, delete_voice, edit_voice, edit_voice_at,
    edit_voice_with_adjustment_at, VoiceChangeError,
};

use std::{
    error::Error,
    fmt::{self, Write as _},
    num::NonZeroU32,
    path::PathBuf,
};

use serde::Deserialize;

pub use crate::voice::{Voice, VoiceId, VoiceType, VoiceVolumeAdjustment};

use crate::{
    acoustics::{self, AcousticError, AcousticScene, Point3Meters, RectangularRoom},
    convolution::VoiceConvolutionSpec,
    part::{Part, PartName},
    pitch_system::PitchSystem,
    seed::Seed,
    tuning_system::{TuningSystem, TuningSystemId},
    voice_name::VoiceName,
};

pub const PROJECTS_DIRECTORY: &str = "projects";
pub const PROJECT_CONFIG_FILE: &str = "project.toml";
const LEGACY_BEAT_SAMPLE_RATE_HZ: u64 = 48_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BeatDurationMillis(NonZeroU32);

impl BeatDurationMillis {
    pub fn new(milliseconds: u32) -> Result<Self, BeatDurationError> {
        NonZeroU32::new(milliseconds)
            .map(Self)
            .ok_or(BeatDurationError)
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    fn from_legacy_samples(samples: u32) -> Result<Self, BeatDurationError> {
        if samples == 0 {
            return Err(BeatDurationError);
        }
        let milliseconds = (u64::from(samples) * 1_000 + LEGACY_BEAT_SAMPLE_RATE_HZ / 2)
            / LEGACY_BEAT_SAMPLE_RATE_HZ;
        let milliseconds = u32::try_from(milliseconds.max(1)).map_err(|_| BeatDurationError)?;
        Self::new(milliseconds)
    }
}

impl From<u32> for BeatDurationMillis {
    fn from(milliseconds: u32) -> Self {
        Self::new(milliseconds).expect("beat duration must be at least one millisecond")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BeatDurationError;

impl fmt::Display for BeatDurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("beat duration must be at least one millisecond")
    }
}

impl Error for BeatDurationError {}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, PartialOrd)]
#[serde(try_from = "f64")]
pub struct FrequencyVariance(f64);

impl FrequencyVariance {
    pub fn new(ratio: f64) -> Result<Self, FrequencyVarianceError> {
        if !ratio.is_finite() || !(0.0..1.0).contains(&ratio) {
            return Err(FrequencyVarianceError);
        }

        Ok(Self(ratio))
    }

    pub const fn ratio(self) -> f64 {
        self.0
    }
}

// `FrequencyVariance::new` excludes NaN, so equality is reflexive.
impl Eq for FrequencyVariance {}

impl TryFrom<f64> for FrequencyVariance {
    type Error = FrequencyVarianceError;

    fn try_from(ratio: f64) -> Result<Self, Self::Error> {
        Self::new(ratio)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrequencyVarianceError;

impl fmt::Display for FrequencyVarianceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("frequency variance must be a decimal from 0 up to but not including 1")
    }
}

impl Error for FrequencyVarianceError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub beat_duration_millis: BeatDurationMillis,
    pub timing_variance: u32,
    frequency_variance: FrequencyVariance,
    mix_normalization_enabled: bool,
    pub seed: Seed,
    pub description: String,
    tuning_system_id: Option<TuningSystemId>,
    pitch_system: PitchSystem,
    voice_convolution: Option<VoiceConvolutionSpec>,
    acoustic_scene: AcousticScene,
    next_voice_id: u64,
    voices: Vec<Voice>,
    pub parts: Vec<Part>,
    sequence: Vec<PartName>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArrangementOccurrence {
    index: usize,
    part_name: PartName,
    length: u32,
    first_beat: u64,
    last_beat: u64,
}

impl ArrangementOccurrence {
    pub fn index(&self) -> usize {
        self.index
    }

    pub fn part_name(&self) -> &PartName {
        &self.part_name
    }

    pub fn length(&self) -> u32 {
        self.length
    }

    pub fn first_beat(&self) -> u64 {
        self.first_beat
    }

    pub fn last_beat(&self) -> u64 {
        self.last_beat
    }
}

impl Project {
    pub fn new(
        name: impl Into<String>,
        beat_duration_millis: impl Into<BeatDurationMillis>,
        timing_variance: u32,
        seed: Seed,
    ) -> Self {
        Self {
            name: name.into(),
            beat_duration_millis: beat_duration_millis.into(),
            timing_variance,
            frequency_variance: FrequencyVariance::default(),
            mix_normalization_enabled: true,
            seed,
            description: String::new(),
            tuning_system_id: Some(TuningSystemId::default_western()),
            pitch_system: PitchSystem::default(),
            voice_convolution: None,
            acoustic_scene: AcousticScene::default(),
            next_voice_id: 1,
            voices: Vec::new(),
            parts: Vec::new(),
            sequence: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_frequency_variance(mut self, variance: FrequencyVariance) -> Self {
        self.frequency_variance = variance;
        self
    }

    pub fn frequency_variance(&self) -> FrequencyVariance {
        self.frequency_variance
    }

    pub fn set_frequency_variance(&mut self, variance: FrequencyVariance) {
        self.frequency_variance = variance;
    }

    pub fn mix_normalization_enabled(&self) -> bool {
        self.mix_normalization_enabled
    }

    pub fn set_mix_normalization_enabled(&mut self, enabled: bool) {
        self.mix_normalization_enabled = enabled;
    }

    pub fn with_pitch_system(mut self, pitch_system: PitchSystem) -> Self {
        self.tuning_system_id = None;
        self.pitch_system = pitch_system;
        self
    }

    pub fn with_tuning_system(mut self, tuning_system: &TuningSystem) -> Self {
        self.tuning_system_id = Some(tuning_system.id().clone());
        self.pitch_system = tuning_system.pitch_system().clone();
        self
    }

    pub fn set_tuning_system(&mut self, tuning_system: &TuningSystem) {
        self.tuning_system_id = Some(tuning_system.id().clone());
        self.pitch_system = tuning_system.pitch_system().clone();
    }

    pub fn tuning_system_id(&self) -> Option<&TuningSystemId> {
        self.tuning_system_id.as_ref()
    }

    pub fn pitch_system(&self) -> &PitchSystem {
        &self.pitch_system
    }

    pub fn voice_convolution(&self) -> Option<&VoiceConvolutionSpec> {
        self.voice_convolution.as_ref()
    }

    fn set_voice_convolution(&mut self, spec: Option<VoiceConvolutionSpec>) {
        self.voice_convolution = spec;
    }

    pub fn acoustic_scene(&self) -> &AcousticScene {
        &self.acoustic_scene
    }

    pub fn set_acoustic_scene(&mut self, scene: AcousticScene) -> Result<(), AcousticError> {
        scene.validate()?;
        for voice in &self.voices {
            scene.validate_source(voice.position())?;
        }
        self.acoustic_scene = scene;
        Ok(())
    }

    pub fn set_centered_room(
        &mut self,
        room: Option<RectangularRoom>,
    ) -> Result<(), AcousticError> {
        let old_listener = self.acoustic_scene.listener();
        let new_listener = room.map_or(Point3Meters::origin(), RectangularRoom::center);
        let delta_x = new_listener.x() - old_listener.x();
        let delta_y = new_listener.y() - old_listener.y();
        let delta_z = new_listener.z() - old_listener.z();
        let scene = AcousticScene::new(new_listener, room)?;
        let translated_voices = self
            .voices
            .iter()
            .map(|voice| {
                let position = voice.position();
                let translated = Point3Meters::new(
                    position.x() + delta_x,
                    position.y() + delta_y,
                    position.z() + delta_z,
                )?;
                scene.validate_source(translated)?;
                Ok(voice.clone().with_position(translated))
            })
            .collect::<Result<Vec<_>, AcousticError>>()?;

        self.acoustic_scene = scene;
        self.voices = translated_voices;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn with_voices(mut self, voices: Vec<Voice>) -> Self {
        self.next_voice_id = voices
            .iter()
            .map(|voice| voice.id().value())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.voices = voices;
        self
    }

    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    #[cfg(test)]
    pub(crate) fn with_parts(mut self, parts: Vec<Part>) -> Self {
        self.sequence = parts.iter().map(|part| part.name.clone()).collect();
        self.parts = parts;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_sequence(mut self, sequence: Vec<PartName>) -> Self {
        self.sequence = sequence;
        self
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    pub fn sequence(&self) -> &[PartName] {
        &self.sequence
    }

    pub fn arrangement_beat_count(&self) -> u64 {
        self.sequence
            .iter()
            .filter_map(|name| self.part(name))
            .map(|part| u64::from(part.length))
            .sum()
    }

    pub fn arrangement_occurrences(&self) -> Vec<ArrangementOccurrence> {
        let mut next_beat = 1_u64;
        self.sequence
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                let part = self.part(name)?;
                if part.length == 0 {
                    return None;
                }
                let first_beat = next_beat;
                let last_beat = first_beat + u64::from(part.length) - 1;
                next_beat = last_beat + 1;
                Some(ArrangementOccurrence {
                    index,
                    part_name: part.name.clone(),
                    length: part.length,
                    first_beat,
                    last_beat,
                })
            })
            .collect()
    }

    pub fn set_sequence(&mut self, sequence: Vec<PartName>) {
        self.sequence = sequence;
    }

    pub fn add_part(&mut self, part: Part) {
        self.parts.push(part);
    }

    pub fn voice(&self, name: &VoiceName) -> Option<&Voice> {
        self.voices
            .iter()
            .find(|voice| voice.name.eq_ignore_ascii_case(name))
    }

    pub fn part(&self, name: &PartName) -> Option<&Part> {
        self.parts
            .iter()
            .find(|part| part.name.eq_ignore_ascii_case(name))
    }

    pub fn remove_part(&mut self, name: &PartName) -> Option<Part> {
        let index = self
            .parts
            .iter()
            .position(|part| part.name.eq_ignore_ascii_case(name))?;
        Some(self.parts.remove(index))
    }

    pub fn config_file_contents(&self) -> String {
        let mut contents = format!(
            "name = {}\ndescription = {}\nbeat_duration_millis = {}\ntiming_variance = {}\nfrequency_variance = {}\nmix_normalization = {}\nseed = {}\nnext_voice_id = {}\nsequence = [",
            toml_string(&self.name),
            toml_string(&self.description),
            self.beat_duration_millis.get(),
            self.timing_variance,
            self.frequency_variance.ratio(),
            self.mix_normalization_enabled,
            self.seed.value(),
            self.next_voice_id
        );

        for (index, part_name) in self.sequence.iter().enumerate() {
            if index > 0 {
                contents.push_str(", ");
            }
            contents.push_str(&toml_string(part_name.as_str()));
        }
        contents.push_str("]\n");
        match &self.tuning_system_id {
            Some(id) => {
                contents.push_str("tuning_system_id = ");
                contents.push_str(&toml_string(id.as_str()));
                contents.push('\n');
            }
            None => self.pitch_system.append_config(&mut contents),
        }
        if let Some(convolution) = &self.voice_convolution {
            contents.push_str("\n[voice_convolution]\nfile = ");
            contents.push_str(&toml_string(convolution.file_config_value()));
            contents.push_str("\nname = ");
            contents.push_str(&toml_string(convolution.file_name()));
            contents.push('\n');
        }
        self.acoustic_scene.append_config(&mut contents);

        for voice in &self.voices {
            contents.push_str("\n[[voices]]\n");
            contents.push_str("id = ");
            contents.push_str(&voice.id().value().to_string());
            contents.push('\n');
            contents.push_str("name = ");
            contents.push_str(&toml_string(voice.name.as_str()));
            contents.push_str("\nvoice_type = ");
            contents.push_str(&toml_string(voice.voice_type.config_value()));
            contents.push('\n');
            if let Some(adjustment) = voice.volume_adjustment() {
                contents.push_str("volume_adjustment = ");
                contents.push_str(&adjustment.multiplier().to_string());
                contents.push('\n');
            }
            if voice.position() != Point3Meters::origin() {
                contents.push_str("position = ");
                acoustics::append_point(&mut contents, voice.position());
                contents.push('\n');
            }
        }

        for part in &self.parts {
            contents.push_str("\n[[parts]]\n");
            contents.push_str("name = ");
            contents.push_str(&toml_string(part.name.as_str()));
            contents.push_str("\nlength = ");
            contents.push_str(&part.length.to_string());
            contents.push('\n');
            if let Some(pattern) = part.subdivision_pattern() {
                contents.push_str("subdivision_pattern = [");
                for (index, subdivision) in pattern.subdivisions().enumerate() {
                    if index > 0 {
                        contents.push_str(", ");
                    }
                    contents.push_str(&subdivision.to_string());
                }
                contents.push_str("]\n");
            }
            if let Some(major_subdivision) = part.major_subdivision() {
                contents.push_str("major_subdivision = ");
                contents.push_str(&major_subdivision.to_string());
                contents.push('\n');
            }
        }

        contents
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectEntry {
    pub project: Project,
    pub project_directory: PathBuf,
}

/// Identifies a project to open; the receiving app still needs to load it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenProjectRequest {
    pub project_name: String,
    pub project_directory: PathBuf,
}

fn toml_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');

    for ch in value.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            ch if ch.is_control() && u32::from(ch) <= 0xffff => {
                write!(output, "\\u{:04x}", u32::from(ch))
                    .expect("writing to a String cannot fail");
            }
            ch if ch.is_control() => {
                write!(output, "\\U{:08x}", u32::from(ch))
                    .expect("writing to a String cannot fail");
            }
            ch => output.push(ch),
        }
    }

    output.push('"');
    output
}

#[cfg(test)]
mod tests;
