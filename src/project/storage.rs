//! Project discovery, creation, loading, saving, and persisted-format compatibility.

use super::transaction::{recover_project_transaction, ProjectTransactionError};
use super::{
    BeatDurationMillis, FrequencyVariance, Project, ProjectEntry, Voice, VoiceType,
    VoiceVolumeAdjustment, PROJECTS_DIRECTORY, PROJECT_CONFIG_FILE,
};
use crate::{
    acoustics::{AcousticScene, Point3Meters},
    convolution::{self, ImpulseResponseError, VoiceConvolutionSpec},
    part::{self, Part, PartName},
    pitch_system::PitchSystem,
    seed::Seed,
    tuning_system::{self, TuningLibraryError, TuningSystem, TuningSystemId},
    voice_name::VoiceName,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum CreateProjectError {
    EmptyProjectName,
    ProjectAlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for CreateProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyProjectName => write!(f, "project name must contain a letter or number"),
            Self::ProjectAlreadyExists(path) => {
                write!(f, "project already exists at {}", path.display())
            }
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {}", path.display(), source)
            }
        }
    }
}

impl Error for CreateProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn create_project(
    workspace_root: impl AsRef<Path>,
    project: &Project,
) -> Result<PathBuf, CreateProjectError> {
    let project_directory_name =
        project_directory_name(&project.name).ok_or(CreateProjectError::EmptyProjectName)?;
    let projects_directory = workspace_root.as_ref().join(PROJECTS_DIRECTORY);
    let project_directory = projects_directory.join(project_directory_name);

    fs::create_dir_all(&projects_directory).map_err(|source| CreateProjectError::Io {
        path: projects_directory.clone(),
        source,
    })?;

    match fs::create_dir(&project_directory) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Err(CreateProjectError::ProjectAlreadyExists(project_directory));
        }
        Err(source) => {
            return Err(CreateProjectError::Io {
                path: project_directory,
                source,
            });
        }
    }

    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    fs::write(&config_path, project.config_file_contents()).map_err(|source| {
        CreateProjectError::Io {
            path: config_path,
            source,
        }
    })?;

    Ok(project_directory)
}

#[derive(Debug)]
pub enum DuplicateProjectError {
    Create(CreateProjectError),
    InvalidPartName {
        name: String,
        source: part::InvalidPartName,
    },
    CopyPart {
        source_path: PathBuf,
        destination_path: PathBuf,
        source: io::Error,
        cleanup_error: Option<io::Error>,
    },
    CopyImpulseResponse {
        source_path: PathBuf,
        destination_path: PathBuf,
        source: io::Error,
        cleanup_error: Option<io::Error>,
    },
}

impl fmt::Display for DuplicateProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Create(error) => write!(f, "{error}"),
            Self::InvalidPartName { name, .. } => {
                write!(f, "part {name:?} cannot be used as a filename")
            }
            Self::CopyPart {
                source_path,
                destination_path,
                source,
                cleanup_error: None,
            } => write!(
                f,
                "failed to copy {} to {}: {source}",
                source_path.display(),
                destination_path.display()
            ),
            Self::CopyPart {
                source_path,
                destination_path,
                source,
                cleanup_error: Some(cleanup_error),
            } => write!(
                f,
                "failed to copy {} to {}: {source}; also failed to remove the incomplete copy: {cleanup_error}",
                source_path.display(),
                destination_path.display()
            ),
            Self::CopyImpulseResponse {
                source_path,
                destination_path,
                source,
                cleanup_error: None,
            } => write!(
                f,
                "failed to copy impulse response {} to {}: {source}",
                source_path.display(),
                destination_path.display()
            ),
            Self::CopyImpulseResponse {
                source_path,
                destination_path,
                source,
                cleanup_error: Some(cleanup_error),
            } => write!(
                f,
                "failed to copy impulse response {} to {}: {source}; also failed to remove the incomplete copy: {cleanup_error}",
                source_path.display(),
                destination_path.display()
            ),
        }
    }
}

impl Error for DuplicateProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Create(error) => Some(error),
            Self::InvalidPartName { source, .. } => Some(source),
            Self::CopyPart { source, .. } | Self::CopyImpulseResponse { source, .. } => {
                Some(source)
            }
        }
    }
}

impl From<CreateProjectError> for DuplicateProjectError {
    fn from(error: CreateProjectError) -> Self {
        Self::Create(error)
    }
}

pub fn duplicate_project(
    workspace_root: impl AsRef<Path>,
    source: &ProjectEntry,
    new_name: &str,
) -> Result<ProjectEntry, DuplicateProjectError> {
    let mut project = source.project.clone();
    project.name = new_name.trim().to_string();
    let part_file_names = project
        .parts
        .iter()
        .map(|project_part| {
            part::csv_file_name(&project_part.name).map_err(|source| {
                DuplicateProjectError::InvalidPartName {
                    name: project_part.name.as_str().to_string(),
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let project_directory = create_project(workspace_root, &project)?;

    for file_name in part_file_names {
        let source_path = source.project_directory.join(&file_name);
        let destination_path = project_directory.join(file_name);
        if let Err(copy_error) = fs::copy(&source_path, &destination_path) {
            let cleanup_error = fs::remove_dir_all(&project_directory).err();
            return Err(DuplicateProjectError::CopyPart {
                source_path,
                destination_path,
                source: copy_error,
                cleanup_error,
            });
        }
    }

    if let Some(convolution) = project.voice_convolution() {
        let source_path = source.project_directory.join(convolution.file());
        let destination_path = project_directory.join(convolution.file());
        let destination_parent = destination_path
            .parent()
            .expect("an impulse response asset always has a parent directory");
        let copy_result = fs::create_dir_all(destination_parent)
            .and_then(|_| fs::copy(&source_path, &destination_path).map(|_| ()));
        if let Err(copy_error) = copy_result {
            let cleanup_error = fs::remove_dir_all(&project_directory).err();
            return Err(DuplicateProjectError::CopyImpulseResponse {
                source_path,
                destination_path,
                source: copy_error,
                cleanup_error,
            });
        }
    }

    Ok(ProjectEntry {
        project,
        project_directory,
    })
}

#[derive(Debug)]
pub enum LoadProjectError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidConfig {
        path: PathBuf,
        source: toml::de::Error,
    },
    InvalidSequence {
        path: PathBuf,
        message: String,
    },
    InvalidPart(part::PartFileError),
    InvalidImpulseResponse(ImpulseResponseError),
    TuningLibrary(Box<TuningLibraryError>),
    Recovery(ProjectTransactionError),
}

#[derive(Debug)]
pub enum SaveProjectError {
    Recovery(ProjectTransactionError),
    ImpulseResponse(ImpulseResponseError),
    Write { path: PathBuf, source: io::Error },
    Replace { path: PathBuf, source: io::Error },
}

impl fmt::Display for SaveProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
            Self::ImpulseResponse(error) => write!(f, "{error}"),
            Self::Write { path, source } => {
                write!(f, "filesystem error writing {}: {}", path.display(), source)
            }
            Self::Replace { path, source } => {
                write!(
                    f,
                    "filesystem error replacing {}: {}",
                    path.display(),
                    source
                )
            }
        }
    }
}

impl Error for SaveProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::ImpulseResponse(error) => Some(error),
            Self::Write { source, .. } | Self::Replace { source, .. } => Some(source),
        }
    }
}

impl fmt::Display for LoadProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {}", path.display(), source)
            }
            Self::InvalidConfig { path, source } => {
                write!(
                    f,
                    "invalid project config at {}: {}",
                    path.display(),
                    source
                )
            }
            Self::InvalidSequence { path, message } => {
                write!(
                    f,
                    "invalid project config at {}: {}",
                    path.display(),
                    message
                )
            }
            Self::InvalidPart(error) => write!(f, "{error}"),
            Self::InvalidImpulseResponse(error) => write!(f, "{error}"),
            Self::TuningLibrary(error) => write!(f, "failed to load tuning systems: {error}"),
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
        }
    }
}

impl Error for LoadProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidConfig { source, .. } => Some(source),
            Self::InvalidSequence { .. } => None,
            Self::InvalidPart(error) => Some(error),
            Self::InvalidImpulseResponse(error) => Some(error),
            Self::TuningLibrary(error) => Some(error),
            Self::Recovery(error) => Some(error),
        }
    }
}

#[derive(Debug)]
pub enum ListProjectsError {
    Io { path: PathBuf, source: io::Error },
    LoadProject(LoadProjectError),
}

impl fmt::Display for ListProjectsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {}", path.display(), source)
            }
            Self::LoadProject(error) => write!(f, "{error}"),
        }
    }
}

impl Error for ListProjectsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::LoadProject(error) => Some(error),
        }
    }
}

pub fn list_projects(
    workspace_root: impl AsRef<Path>,
) -> Result<Vec<ProjectEntry>, ListProjectsError> {
    let projects_directory = workspace_root.as_ref().join(PROJECTS_DIRECTORY);
    let entries = match fs::read_dir(&projects_directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(ListProjectsError::Io {
                path: projects_directory,
                source,
            });
        }
    };

    let mut projects = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|source| ListProjectsError::Io {
            path: projects_directory.clone(),
            source,
        })?;
        let path = entry.path();

        if !path.is_dir() || !path.join(PROJECT_CONFIG_FILE).is_file() {
            continue;
        }

        projects.push(load_project(&path).map_err(ListProjectsError::LoadProject)?);
    }

    projects.sort_by_key(|entry| entry.project.name.to_ascii_lowercase());
    Ok(projects)
}

pub fn load_project(project_directory: impl AsRef<Path>) -> Result<ProjectEntry, LoadProjectError> {
    let project_directory = project_directory.as_ref();
    recover_project_transaction(project_directory).map_err(LoadProjectError::Recovery)?;
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let config = fs::read_to_string(&config_path).map_err(|source| LoadProjectError::Io {
        path: config_path.clone(),
        source,
    })?;
    let project_config = toml::from_str::<ProjectConfig>(&config).map_err(|source| {
        LoadProjectError::InvalidConfig {
            path: config_path.clone(),
            source,
        }
    })?;

    let workspace_root = project_directory
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            LoadProjectError::TuningLibrary(Box::new(TuningLibraryError::Invalid(format!(
                "project directory {} is not inside a workspace projects directory",
                project_directory.display()
            ))))
        })?;
    let tuning_systems = tuning_system::list_tuning_systems(workspace_root)
        .map_err(|error| LoadProjectError::TuningLibrary(Box::new(error)))?;

    let project = project_config
        .into_project(&tuning_systems)
        .map_err(|message| LoadProjectError::InvalidSequence {
            path: config_path,
            message,
        })?;
    if let Some(convolution) = project.voice_convolution() {
        convolution::inspect_project_asset(project_directory, convolution)
            .map_err(LoadProjectError::InvalidImpulseResponse)?;
    }
    for project_part in &project.parts {
        part::validate_part_file(project_directory, project_part, &project.voices)
            .map_err(LoadProjectError::InvalidPart)?;
    }

    Ok(ProjectEntry {
        project,
        project_directory: project_directory.to_path_buf(),
    })
}

pub fn save_project(
    project_directory: impl AsRef<Path>,
    project: &Project,
) -> Result<(), SaveProjectError> {
    let project_directory = project_directory.as_ref();
    recover_project_transaction(project_directory).map_err(SaveProjectError::Recovery)?;
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let pending_path = project_directory.join(format!(".{PROJECT_CONFIG_FILE}.pending"));

    fs::write(&pending_path, project.config_file_contents()).map_err(|source| {
        SaveProjectError::Write {
            path: pending_path.clone(),
            source,
        }
    })?;
    fs::rename(&pending_path, &config_path).map_err(|source| SaveProjectError::Replace {
        path: config_path,
        source,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VoiceConvolutionChange {
    Keep,
    Import(PathBuf),
    Remove,
}

pub fn save_project_with_voice_convolution(
    project_directory: impl AsRef<Path>,
    mut project: Project,
    change: VoiceConvolutionChange,
) -> Result<Project, SaveProjectError> {
    let project_directory = project_directory.as_ref();
    match change {
        VoiceConvolutionChange::Keep => {}
        VoiceConvolutionChange::Import(source_path) => {
            let (spec, _) = convolution::import_wav_file(project_directory, &source_path)
                .map_err(SaveProjectError::ImpulseResponse)?;
            project.set_voice_convolution(Some(spec));
        }
        VoiceConvolutionChange::Remove => project.set_voice_convolution(None),
    }

    save_project(project_directory, &project)?;
    Ok(project)
}

pub fn project_directory_name(name: &str) -> Option<String> {
    let mut directory_name = String::new();
    let mut previous_was_separator = false;

    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            directory_name.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !directory_name.is_empty() && !previous_was_separator {
            directory_name.push('-');
            previous_was_separator = true;
        }
    }

    if previous_was_separator {
        directory_name.pop();
    }

    (!directory_name.is_empty()).then_some(directory_name)
}

#[derive(Deserialize)]
struct ProjectConfig {
    name: String,
    description: String,
    #[serde(default)]
    beat_duration_millis: Option<u32>,
    #[serde(default)]
    beat_length: Option<u32>,
    timing_variance: u32,
    #[serde(default)]
    frequency_variance: FrequencyVariance,
    #[serde(default = "enabled_by_default")]
    mix_normalization: bool,
    seed: u64,
    #[serde(default)]
    tuning_system_id: Option<TuningSystemId>,
    // Compatibility with projects that embedded pitch rules before reusable
    // workspace tuning systems were introduced.
    #[serde(default, rename = "pitch_system")]
    embedded_pitch_system: Option<PitchSystem>,
    #[serde(default)]
    voice_convolution: Option<VoiceConvolutionSpec>,
    #[serde(default)]
    acoustic_scene: AcousticScene,
    #[serde(default)]
    next_voice_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_voices")]
    voices: Vec<Voice>,
    #[serde(default, deserialize_with = "deserialize_parts")]
    parts: Vec<Part>,
    #[serde(default)]
    sequence: Option<Vec<PartName>>,
}

#[derive(Deserialize)]
struct StoredVoice {
    #[serde(default)]
    id: Option<u64>,
    name: VoiceName,
    voice_type: VoiceType,
    #[serde(default)]
    volume_adjustment: Option<VoiceVolumeAdjustment>,
    #[serde(default)]
    position: Point3Meters,
}

fn deserialize_voices<'de, D>(deserializer: D) -> Result<Vec<Voice>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let stored_voices = Vec::<StoredVoice>::deserialize(deserializer)?;
    let mut used_ids = BTreeSet::new();

    for stored_voice in &stored_voices {
        let Some(id) = stored_voice.id else {
            continue;
        };
        if id == 0 {
            return Err(serde::de::Error::custom(
                "voice ids must be greater than zero",
            ));
        }
        if !used_ids.insert(id) {
            return Err(serde::de::Error::custom(format!(
                "voice id {id} is duplicated"
            )));
        }
    }

    let mut next_id = 1_u64;
    let mut voices = Vec::with_capacity(stored_voices.len());
    for stored_voice in stored_voices {
        let id = match stored_voice.id {
            Some(id) => id,
            None => {
                while used_ids.contains(&next_id) {
                    next_id = next_id
                        .checked_add(1)
                        .ok_or_else(|| serde::de::Error::custom("no voice ids are available"))?;
                }
                let id = next_id;
                used_ids.insert(id);
                next_id = next_id
                    .checked_add(1)
                    .ok_or_else(|| serde::de::Error::custom("no voice ids are available"))?;
                id
            }
        };
        voices.push(
            Voice::new(id, stored_voice.name, stored_voice.voice_type)
                .with_position(stored_voice.position)
                .with_volume_adjustment(stored_voice.volume_adjustment),
        );
    }

    for (index, voice) in voices.iter().enumerate() {
        if voice.name.as_str().trim().is_empty() {
            return Err(serde::de::Error::custom("voice names must not be empty"));
        }
        if voices[..index]
            .iter()
            .any(|other| other.name.eq_ignore_ascii_case(&voice.name))
        {
            return Err(serde::de::Error::custom(format!(
                "voice name {:?} is duplicated",
                voice.name.as_str()
            )));
        }
    }

    Ok(voices)
}

fn deserialize_parts<'de, D>(deserializer: D) -> Result<Vec<Part>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let parts = Vec::<Part>::deserialize(deserializer)?;

    for (index, part) in parts.iter().enumerate() {
        if part.name.as_str().trim().is_empty() {
            return Err(serde::de::Error::custom("part names must not be empty"));
        }
        if part.length == 0 {
            return Err(serde::de::Error::custom(format!(
                "part {:?} must be at least one beat long",
                part.name.as_str()
            )));
        }
        if parts[..index]
            .iter()
            .any(|other| other.name.eq_ignore_ascii_case(&part.name))
        {
            return Err(serde::de::Error::custom(format!(
                "part name {:?} is duplicated",
                part.name.as_str()
            )));
        }
        let part_file = part::csv_file_name(&part.name).map_err(|_| {
            serde::de::Error::custom(format!(
                "part {:?} cannot be used as a filename",
                part.name.as_str()
            ))
        })?;
        if parts[..index].iter().any(|other| {
            part::csv_file_name(&other.name)
                .is_ok_and(|other_file| other_file.eq_ignore_ascii_case(&part_file))
        }) {
            return Err(serde::de::Error::custom(format!(
                "part filename {part_file:?} is duplicated"
            )));
        }
    }

    Ok(parts)
}

impl ProjectConfig {
    fn into_project(self, tuning_systems: &[TuningSystem]) -> Result<Project, String> {
        let beat_duration_millis = match (self.beat_duration_millis, self.beat_length) {
            (Some(milliseconds), None) => BeatDurationMillis::new(milliseconds),
            (None, Some(samples)) => BeatDurationMillis::from_legacy_samples(samples),
            (Some(_), Some(_)) => {
                return Err(
                    "project must contain beat_duration_millis or legacy beat_length, not both"
                        .to_string(),
                );
            }
            (None, None) => return Err("project is missing beat_duration_millis".to_string()),
        }
        .map_err(|error| error.to_string())?;
        let sequence = match self.sequence {
            Some(sequence) => sequence
                .into_iter()
                .map(|part_name| {
                    self.parts
                        .iter()
                        .find(|part| part.name.eq_ignore_ascii_case(&part_name))
                        .map(|part| part.name.clone())
                        .ok_or_else(|| {
                            format!("sequence references missing part {:?}", part_name.as_str())
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => self.parts.iter().map(|part| part.name.clone()).collect(),
        };
        let selected_tuning = match (self.tuning_system_id, self.embedded_pitch_system) {
            (Some(id), None) => tuning_system::find_tuning_system(tuning_systems, &id)
                .map(|system| (Some(id.clone()), system.pitch_system().clone()))
                .ok_or_else(|| format!("references missing tuning system {:?}", id.as_str()))?,
            (None, Some(pitch_system)) => (None, pitch_system),
            (None, None) => {
                let id = TuningSystemId::default_western();
                let system = tuning_system::find_tuning_system(tuning_systems, &id)
                    .expect("the tuning library always contains the built-in western system");
                (Some(id), system.pitch_system().clone())
            }
            (Some(_), Some(_)) => {
                return Err(
                    "project must contain a tuning_system_id or an embedded pitch_system, not both"
                        .to_string(),
                );
            }
        };
        let mut project = Project::new(
            self.name,
            beat_duration_millis,
            self.timing_variance,
            Seed::new(self.seed),
        )
        .with_description(self.description)
        .with_frequency_variance(self.frequency_variance);
        project.mix_normalization_enabled = self.mix_normalization;
        project.tuning_system_id = selected_tuning.0;
        project.pitch_system = selected_tuning.1;
        project.voice_convolution = self.voice_convolution;
        self.acoustic_scene
            .validate()
            .map_err(|error| error.to_string())?;
        for voice in &self.voices {
            self.acoustic_scene
                .validate_source(voice.position())
                .map_err(|error| error.to_string())?;
        }
        project.acoustic_scene = self.acoustic_scene;
        project.voices = self.voices;
        let minimum_next_voice_id = project
            .voices
            .iter()
            .map(|voice| voice.id().value())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        project.next_voice_id = self
            .next_voice_id
            .unwrap_or(minimum_next_voice_id)
            .max(minimum_next_voice_id);
        project.parts = self.parts;
        project.sequence = sequence;
        Ok(project)
    }
}

const fn enabled_by_default() -> bool {
    true
}
