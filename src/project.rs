use std::{
    collections::BTreeSet,
    error::Error,
    fmt::{self, Write as _},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;

pub use crate::voice::{Voice, VoiceId, VoiceType};

use crate::{
    part::{self, Part, PartName},
    pitch_system::PitchSystem,
    seed::Seed,
    tuning_system::{self, TuningLibraryError, TuningSystem, TuningSystemId},
    voice_name::VoiceName,
};

pub const PROJECTS_DIRECTORY: &str = "projects";
pub const PROJECT_CONFIG_FILE: &str = "project.toml";
const PROJECT_TRANSACTION_DIRECTORY: &str = ".project-transaction";
const TRANSACTION_NEW_DIRECTORY: &str = "new";
const TRANSACTION_OLD_DIRECTORY: &str = "old";
const TRANSACTION_COMMITTING_FILE: &str = "committing";
const TRANSACTION_COMMITTED_FILE: &str = "committed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub beat_length: u32,
    pub timing_variance: u32,
    pub seed: Seed,
    pub description: String,
    tuning_system_id: Option<TuningSystemId>,
    pitch_system: PitchSystem,
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
        beat_length: u32,
        timing_variance: u32,
        seed: Seed,
    ) -> Self {
        Self {
            name: name.into(),
            beat_length,
            timing_variance,
            seed,
            description: String::new(),
            tuning_system_id: Some(TuningSystemId::default_western()),
            pitch_system: PitchSystem::default(),
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
            "name = {}\ndescription = {}\nbeat_length = {}\ntiming_variance = {}\nseed = {}\nnext_voice_id = {}\nsequence = [",
            toml_string(&self.name),
            toml_string(&self.description),
            self.beat_length,
            self.timing_variance,
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
        }

        for part in &self.parts {
            contents.push_str("\n[[parts]]\n");
            contents.push_str("name = ");
            contents.push_str(&toml_string(part.name.as_str()));
            contents.push_str("\nlength = ");
            contents.push_str(&part.length.to_string());
            contents.push('\n');
        }

        contents
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectEntry {
    pub project: Project,
    pub project_directory: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectOpened {
    pub project_name: String,
    pub project_directory: PathBuf,
}

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
        }
    }
}

impl Error for DuplicateProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Create(error) => Some(error),
            Self::InvalidPartName { source, .. } => Some(source),
            Self::CopyPart { source, .. } => Some(source),
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
    TuningLibrary(Box<TuningLibraryError>),
    Recovery(ProjectTransactionError),
}

#[derive(Debug)]
pub enum ProjectTransactionError {
    Io { path: PathBuf, source: io::Error },
    Invalid { path: PathBuf, message: String },
}

impl fmt::Display for ProjectTransactionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {source}", path.display())
            }
            Self::Invalid { path, message } => {
                write!(
                    f,
                    "invalid project transaction at {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ProjectTransactionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Invalid { .. } => None,
        }
    }
}

#[derive(Debug)]
pub enum VoiceChangeError {
    InvalidField(String),
    MissingVoice(String),
    Part(part::PartFileError),
    Transaction(ProjectTransactionError),
    Commit {
        source: ProjectTransactionError,
        rollback_error: Option<ProjectTransactionError>,
    },
}

impl fmt::Display for VoiceChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidField(message) => write!(f, "{message}"),
            Self::MissingVoice(name) => write!(f, "voice {name:?} no longer exists"),
            Self::Part(error) => write!(f, "{error}"),
            Self::Transaction(error) => write!(f, "{error}"),
            Self::Commit {
                source,
                rollback_error: None,
            } => write!(f, "{source}"),
            Self::Commit {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                f,
                "{source}; also failed to restore the original project files: {rollback_error}"
            ),
        }
    }
}

impl Error for VoiceChangeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Part(error) => Some(error),
            Self::Transaction(error) | Self::Commit { source: error, .. } => Some(error),
            Self::InvalidField(_) | Self::MissingVoice(_) => None,
        }
    }
}

#[derive(Debug)]
pub enum SaveProjectError {
    Recovery(ProjectTransactionError),
    Write { path: PathBuf, source: io::Error },
    Replace { path: PathBuf, source: io::Error },
}

impl fmt::Display for SaveProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
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
    for project_part in &project.parts {
        part::validate_part_file(project_directory, project_part, &project.voices)
            .map_err(LoadProjectError::InvalidPart)?;
    }

    Ok(ProjectEntry {
        project,
        project_directory: project_directory.to_path_buf(),
    })
}

pub fn add_voice(
    project_directory: impl AsRef<Path>,
    project: &Project,
    name: &str,
    voice_type: VoiceType,
) -> Result<Project, VoiceChangeError> {
    let name = validated_voice_name(project, None, name)?;
    let next_id = project.next_voice_id;
    let following_id = next_id
        .checked_add(1)
        .ok_or_else(|| VoiceChangeError::InvalidField("no voice ids are available".to_string()))?;
    let mut updated_project = project.clone();
    updated_project.next_voice_id = following_id;
    updated_project
        .voices
        .push(Voice::new(next_id, name, voice_type));
    persist_voice_change(project_directory.as_ref(), project, &updated_project)?;
    Ok(updated_project)
}

pub fn edit_voice(
    project_directory: impl AsRef<Path>,
    project: &Project,
    original_name: &VoiceName,
    name: &str,
    voice_type: VoiceType,
) -> Result<Project, VoiceChangeError> {
    let index = project
        .voices
        .iter()
        .position(|voice| voice.name.eq_ignore_ascii_case(original_name))
        .ok_or_else(|| VoiceChangeError::MissingVoice(original_name.as_str().to_string()))?;
    let id = project.voices[index].id();
    let name = validated_voice_name(project, Some(id), name)?;
    let mut updated_project = project.clone();
    updated_project.voices[index] = Voice::new(id, name, voice_type);
    persist_voice_change(project_directory.as_ref(), project, &updated_project)?;
    Ok(updated_project)
}

pub fn delete_voice(
    project_directory: impl AsRef<Path>,
    project: &Project,
    name: &VoiceName,
) -> Result<Project, VoiceChangeError> {
    let index = project
        .voices
        .iter()
        .position(|voice| voice.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| VoiceChangeError::MissingVoice(name.as_str().to_string()))?;
    let mut updated_project = project.clone();
    updated_project.voices.remove(index);
    persist_voice_change(project_directory.as_ref(), project, &updated_project)?;
    Ok(updated_project)
}

fn validated_voice_name(
    project: &Project,
    edited_id: Option<VoiceId>,
    name: &str,
) -> Result<VoiceName, VoiceChangeError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(VoiceChangeError::InvalidField(
            "voice name must not be empty".to_string(),
        ));
    }

    let name = VoiceName::new(name);
    if project
        .voices
        .iter()
        .any(|voice| Some(voice.id()) != edited_id && voice.name.eq_ignore_ascii_case(&name))
    {
        return Err(VoiceChangeError::InvalidField(format!(
            "a voice named {:?} already exists",
            name.as_str()
        )));
    }
    Ok(name)
}

fn persist_voice_change(
    project_directory: &Path,
    old_project: &Project,
    new_project: &Project,
) -> Result<(), VoiceChangeError> {
    recover_project_transaction(project_directory).map_err(VoiceChangeError::Transaction)?;

    let mut files = Vec::with_capacity(old_project.parts.len() + 1);
    for project_part in &old_project.parts {
        let file_name = part::csv_file_name(&project_part.name)
            .expect("validated project part names always produce CSV filenames");
        let contents = part::rewritten_part_file(
            project_directory,
            project_part,
            &old_project.voices,
            &new_project.voices,
        )
        .map_err(VoiceChangeError::Part)?;
        files.push((file_name, contents));
    }
    files.push((
        PROJECT_CONFIG_FILE.to_string(),
        new_project.config_file_contents().into_bytes(),
    ));

    if let Err(source) = commit_project_files(project_directory, &files) {
        let rollback_error = recover_project_transaction(project_directory).err();
        return Err(VoiceChangeError::Commit {
            source,
            rollback_error,
        });
    }
    Ok(())
}

fn commit_project_files(
    project_directory: &Path,
    files: &[(String, Vec<u8>)],
) -> Result<(), ProjectTransactionError> {
    let transaction_directory = project_directory.join(PROJECT_TRANSACTION_DIRECTORY);
    let new_directory = transaction_directory.join(TRANSACTION_NEW_DIRECTORY);
    let old_directory = transaction_directory.join(TRANSACTION_OLD_DIRECTORY);
    fs::create_dir(&transaction_directory).map_err(|source| ProjectTransactionError::Io {
        path: transaction_directory.clone(),
        source,
    })?;
    fs::create_dir(&new_directory).map_err(|source| ProjectTransactionError::Io {
        path: new_directory.clone(),
        source,
    })?;
    fs::create_dir(&old_directory).map_err(|source| ProjectTransactionError::Io {
        path: old_directory.clone(),
        source,
    })?;

    for (file_name, contents) in files {
        write_synced(&new_directory.join(file_name), contents, true)?;
        let source_path = project_directory.join(file_name);
        let backup_path = old_directory.join(file_name);
        let original = fs::read(&source_path).map_err(|source| ProjectTransactionError::Io {
            path: source_path,
            source,
        })?;
        write_synced(&backup_path, &original, true)?;
    }
    sync_directory(&new_directory)?;
    sync_directory(&old_directory)?;

    write_synced(
        &transaction_directory.join(TRANSACTION_COMMITTING_FILE),
        b"",
        true,
    )?;
    sync_directory(&transaction_directory)?;

    for (file_name, _) in files {
        let staged_path = new_directory.join(file_name);
        let target_path = project_directory.join(file_name);
        fs::rename(&staged_path, &target_path).map_err(|source| ProjectTransactionError::Io {
            path: target_path,
            source,
        })?;
    }
    sync_directory(project_directory)?;

    write_synced(
        &transaction_directory.join(TRANSACTION_COMMITTED_FILE),
        b"",
        true,
    )?;
    sync_directory(&transaction_directory)?;
    // The committed marker makes cleanup recoverable. A failure to remove the
    // staging directory must not report the already-committed change as failed.
    fs::remove_dir_all(&transaction_directory).ok();
    Ok(())
}

fn recover_project_transaction(project_directory: &Path) -> Result<(), ProjectTransactionError> {
    let transaction_directory = project_directory.join(PROJECT_TRANSACTION_DIRECTORY);
    if !transaction_directory.exists() {
        return Ok(());
    }

    if transaction_directory
        .join(TRANSACTION_COMMITTED_FILE)
        .is_file()
        || !transaction_directory
            .join(TRANSACTION_COMMITTING_FILE)
            .is_file()
    {
        return fs::remove_dir_all(&transaction_directory).map_err(|source| {
            ProjectTransactionError::Io {
                path: transaction_directory,
                source,
            }
        });
    }

    let old_directory = transaction_directory.join(TRANSACTION_OLD_DIRECTORY);
    let entries = fs::read_dir(&old_directory).map_err(|source| ProjectTransactionError::Io {
        path: old_directory.clone(),
        source,
    })?;
    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ProjectTransactionError::Io {
            path: old_directory.clone(),
            source,
        })?;
        if !entry.path().is_file() {
            return Err(ProjectTransactionError::Invalid {
                path: entry.path(),
                message: "backup entry is not a file".to_string(),
            });
        }
        backups.push((entry.file_name(), entry.path()));
    }
    backups.sort_by(|left, right| left.0.cmp(&right.0));

    for (file_name, backup_path) in backups {
        let contents = fs::read(&backup_path).map_err(|source| ProjectTransactionError::Io {
            path: backup_path,
            source,
        })?;
        let recovery_path = transaction_directory.join(&file_name);
        write_synced(&recovery_path, &contents, true)?;
        let target_path = project_directory.join(file_name);
        fs::rename(&recovery_path, &target_path).map_err(|source| ProjectTransactionError::Io {
            path: target_path,
            source,
        })?;
    }
    sync_directory(project_directory)?;

    fs::remove_dir_all(&transaction_directory).map_err(|source| ProjectTransactionError::Io {
        path: transaction_directory,
        source,
    })
}

pub(crate) fn recover_pending_project_update(
    project_directory: &Path,
) -> Result<(), ProjectTransactionError> {
    recover_project_transaction(project_directory)
}

fn write_synced(
    path: &Path,
    contents: &[u8],
    create_new: bool,
) -> Result<(), ProjectTransactionError> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(create_new)
        .create(!create_new)
        .truncate(!create_new);
    let mut file = options
        .open(path)
        .map_err(|source| ProjectTransactionError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|source| ProjectTransactionError::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn sync_directory(path: &Path) -> Result<(), ProjectTransactionError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ProjectTransactionError::Io {
            path: path.to_path_buf(),
            source,
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

#[derive(Deserialize)]
struct ProjectConfig {
    name: String,
    description: String,
    beat_length: u32,
    timing_variance: u32,
    seed: u64,
    #[serde(default)]
    tuning_system_id: Option<TuningSystemId>,
    // Compatibility with projects that embedded pitch rules before reusable
    // workspace tuning systems were introduced.
    #[serde(default, rename = "pitch_system")]
    embedded_pitch_system: Option<PitchSystem>,
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
        voices.push(Voice::new(id, stored_voice.name, stored_voice.voice_type));
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
            self.beat_length,
            self.timing_variance,
            Seed::new(self.seed),
        )
        .with_description(self.description);
        project.tuning_system_id = selected_tuning.0;
        project.pitch_system = selected_tuning.1;
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

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        add_voice, create_project, delete_voice, duplicate_project, edit_voice, list_projects,
        load_project, project_directory_name, save_project, CreateProjectError,
        DuplicateProjectError, LoadProjectError, Project, ProjectEntry, Voice, VoiceType,
        PROJECT_CONFIG_FILE, PROJECT_TRANSACTION_DIRECTORY, TRANSACTION_COMMITTING_FILE,
        TRANSACTION_NEW_DIRECTORY, TRANSACTION_OLD_DIRECTORY,
    };
    use crate::{
        part::{self, Part, PartName, PartScore},
        pitch_system::{
            ExplicitPitchSystem, FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem,
            PitchSystem,
        },
        seed::Seed,
        tuning_system,
        voice_name::VoiceName,
    };

    const DEFAULT_TUNING_REFERENCE: &str = "tuning_system_id = \"western-twelve-tone\"\n";

    #[test]
    fn project_stores_the_initial_music_settings() {
        let project = Project::new("test", 4000, 100, Seed::new(19)).with_description("sketch");

        assert_eq!(project.name, "test");
        assert_eq!(project.beat_length, 4000);
        assert_eq!(project.timing_variance, 100);
        assert_eq!(project.seed, Seed::new(19));
        assert_eq!(project.description, "sketch");
        assert!(project.voices.is_empty());
        assert!(project.sequence().is_empty());
    }

    #[test]
    fn project_directory_name_is_filesystem_safe() {
        assert_eq!(
            project_directory_name("Arc Light Sketch!"),
            Some("arc-light-sketch".to_string())
        );
        assert_eq!(
            project_directory_name("../Score"),
            Some("score".to_string())
        );
        assert_eq!(project_directory_name("!!!"), None);
    }

    #[test]
    fn config_file_contents_are_toml_compatible() {
        let project = Project::new("test \"score\"", 4000, 100, Seed::new(1234))
            .with_description("line one\nline two");

        assert_eq!(
            project.config_file_contents(),
            format!(
                "name = \"test \\\"score\\\"\"\ndescription = \"line one\\nline two\"\nbeat_length = 4000\ntiming_variance = 100\nseed = 1234\nnext_voice_id = 1\nsequence = []\n{DEFAULT_TUNING_REFERENCE}"
            )
        );
    }

    #[test]
    fn config_file_contents_store_voices_in_column_order() {
        let project = Project::new("test", 4000, 100, Seed::new(1234)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);

        assert_eq!(
            project.config_file_contents(),
            format!(
                "name = \"test\"\ndescription = \"\"\nbeat_length = 4000\ntiming_variance = 100\nseed = 1234\nnext_voice_id = 3\nsequence = []\n{DEFAULT_TUNING_REFERENCE}\n[[voices]]\nid = 1\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nid = 2\nname = \"bass\"\nvoice_type = \"sin\"\n"
            )
        );
    }

    #[test]
    fn config_file_contents_store_part_metadata_without_redundant_filenames() {
        let project = Project::new("test", 4000, 100, Seed::new(1234))
            .with_parts(vec![Part::new("intro", 8), Part::new("verse", 16)]);

        assert_eq!(
            project.config_file_contents(),
            format!(
                "name = \"test\"\ndescription = \"\"\nbeat_length = 4000\ntiming_variance = 100\nseed = 1234\nnext_voice_id = 1\nsequence = [\"intro\", \"verse\"]\n{DEFAULT_TUNING_REFERENCE}\n[[parts]]\nname = \"intro\"\nlength = 8\n\n[[parts]]\nname = \"verse\"\nlength = 16\n"
            )
        );
    }

    #[test]
    fn config_file_contents_preserve_repeated_part_occurrences() {
        let project = Project::new("test", 4000, 100, Seed::new(1234))
            .with_parts(vec![Part::new("part-a", 8), Part::new("part-b", 16)])
            .with_sequence(vec!["part-a".into(), "part-b".into(), "part-b".into()]);

        assert!(project
            .config_file_contents()
            .contains("sequence = [\"part-a\", \"part-b\", \"part-b\"]\n"));
    }

    #[test]
    fn arrangement_occurrences_include_repeated_parts_and_global_beat_spans() {
        let project = Project::new("test", 4000, 100, Seed::new(1234))
            .with_parts(vec![Part::new("intro", 8), Part::new("verse", 16)])
            .with_sequence(vec!["intro".into(), "verse".into(), "verse".into()]);

        let occurrences = project.arrangement_occurrences();

        assert_eq!(occurrences.len(), 3);
        assert_eq!(occurrences[0].index(), 0);
        assert_eq!(occurrences[0].part_name().as_str(), "intro");
        assert_eq!(
            (occurrences[0].first_beat(), occurrences[0].last_beat()),
            (1, 8)
        );
        assert_eq!(occurrences[1].index(), 1);
        assert_eq!(
            (occurrences[1].first_beat(), occurrences[1].last_beat()),
            (9, 24)
        );
        assert_eq!(occurrences[2].index(), 2);
        assert_eq!(occurrences[2].part_name().as_str(), "verse");
        assert_eq!(
            (occurrences[2].first_beat(), occurrences[2].last_beat()),
            (25, 40)
        );
    }

    #[test]
    fn voices_are_found_by_name() {
        let project = Project::new("test", 4000, 100, Seed::new(1234)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
            Voice::new(3, "harmony", VoiceType::Saw),
        ]);

        assert_eq!(
            project.voice(&VoiceName::new("BASS")),
            Some(&Voice::new(2, "bass", VoiceType::Sin))
        );
        assert_eq!(project.voice(&VoiceName::new("missing")), None);
    }

    #[test]
    fn create_project_writes_config_under_projects_directory() {
        let root = temp_root("writes-config");
        let project = Project::new("Arc Light Sketch", 4000, 100, Seed::new(1234))
            .with_description("first generated sketch");

        let project_directory = create_project(&root, &project).unwrap();

        assert_eq!(
            project_directory,
            root.join("projects").join("arc-light-sketch")
        );
        assert_eq!(
            fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
            project.config_file_contents()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn create_project_rejects_existing_project_directory() {
        let root = temp_root("existing-project");
        let project = Project::new("Arc Light Sketch", 4000, 100, Seed::new(1234));

        create_project(&root, &project).unwrap();
        let error = create_project(&root, &project).unwrap_err();

        assert!(matches!(error, CreateProjectError::ProjectAlreadyExists(_)));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_project_copies_project_metadata_and_scores_under_a_new_name() {
        let root = temp_root("duplicate-project");
        let mut project = Project::new("Original", 4000, 100, Seed::new(1234))
            .with_description("first version")
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let source_directory = create_project(&root, &project).unwrap();
        add_test_part(&source_directory, &mut project, "intro", 2);
        let score = PartScore::from_rows(vec![vec!["A4".to_string()], vec![String::new()]]);
        score
            .save(&source_directory, &project.parts[0], &project)
            .unwrap();
        let source = load_project(&source_directory).unwrap();

        let duplicated = duplicate_project(&root, &source, "  Original variation  ").unwrap();

        let mut expected_project = project.clone();
        expected_project.name = "Original variation".to_string();
        assert_eq!(duplicated.project, expected_project);
        assert_eq!(
            duplicated.project_directory,
            root.join("projects").join("original-variation")
        );
        assert_eq!(
            PartScore::load(
                &duplicated.project_directory,
                &duplicated.project.parts[0],
                duplicated.project.voices()
            )
            .unwrap(),
            score
        );
        assert_eq!(
            load_project(&duplicated.project_directory).unwrap(),
            duplicated
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_project_does_not_overwrite_an_existing_project() {
        let root = temp_root("duplicate-project-existing");
        let source_directory =
            create_project(&root, &Project::new("Original", 800, 0, Seed::new(1))).unwrap();
        let source = load_project(source_directory).unwrap();
        let existing = Project::new("Existing", 4000, 10, Seed::new(2));
        let existing_directory = create_project(&root, &existing).unwrap();

        let error = duplicate_project(&root, &source, "Existing").unwrap_err();

        assert!(matches!(
            error,
            DuplicateProjectError::Create(CreateProjectError::ProjectAlreadyExists(_))
        ));
        assert_eq!(load_project(existing_directory).unwrap().project, existing);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_project_reports_an_invalid_part_filename_without_creating_a_copy() {
        let root = temp_root("duplicate-project-invalid-part");
        let source = ProjectEntry {
            project: Project::new("Original", 800, 0, Seed::new(1))
                .with_parts(vec![Part::new("!!!", 2)]),
            project_directory: root.join("source"),
        };

        let error = duplicate_project(&root, &source, "Copy").unwrap_err();

        assert!(matches!(
            error,
            DuplicateProjectError::InvalidPartName { name, .. } if name == "!!!"
        ));
        assert!(!root.join("projects").join("copy").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicate_project_removes_an_incomplete_copy_when_a_score_cannot_be_copied() {
        let root = temp_root("duplicate-project-rollback");
        let mut project = Project::new("Original", 800, 0, Seed::new(1));
        let source_directory = create_project(&root, &project).unwrap();
        add_test_part(&source_directory, &mut project, "intro", 2);
        let source = load_project(&source_directory).unwrap();
        fs::remove_file(source_directory.join("intro.csv")).unwrap();

        let error = duplicate_project(&root, &source, "Copy").unwrap_err();

        assert!(matches!(
            error,
            DuplicateProjectError::CopyPart {
                cleanup_error: None,
                ..
            }
        ));
        assert!(!root.join("projects").join("copy").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_reads_config_from_project_directory() {
        let root = temp_root("load-project");
        let project = Project::new("test \"score\"", 4000, 100, Seed::new(1234))
            .with_description("line one\nline two");
        let project_directory = create_project(&root, &project).unwrap();

        let loaded_project = load_project(&project_directory).unwrap();

        assert_eq!(loaded_project.project, project);
        assert!(loaded_project.project.voices.is_empty());
        assert_eq!(loaded_project.project_directory, project_directory);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_pitch_systems_round_trip_through_config() {
        let root = temp_root("pitch-system-round-trip");
        let periodic = PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "slendro sketch",
                FrequencyHz::new(25.0).unwrap(),
                Interval::ratio(2, 1).unwrap(),
                vec![
                    Interval::ratio(1, 1).unwrap(),
                    Interval::ratio(8, 7).unwrap(),
                    Interval::ratio(21, 16).unwrap(),
                    Interval::ratio(32, 21).unwrap(),
                    Interval::ratio(7, 4).unwrap(),
                ],
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        );
        let periodic_project =
            Project::new("periodic", 800, 0, Seed::new(1)).with_pitch_system(periodic);
        let periodic_directory = create_project(&root, &periodic_project).unwrap();

        assert_eq!(
            load_project(&periodic_directory).unwrap().project,
            periodic_project
        );
        let periodic_config =
            fs::read_to_string(periodic_directory.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(periodic_config.contains("fundamental_hz = 25"));
        assert!(periodic_config.contains("degrees = [\"1/1\", \"8/7\""));
        assert!(periodic_config.contains("kind = \"radler_digits\""));

        let explicit = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([
                    ("ember".to_string(), FrequencyHz::new(197.3).unwrap()),
                    ("⟟".to_string(), FrequencyHz::new(316.4).unwrap()),
                ]),
            )
            .unwrap(),
        );
        let explicit_project =
            Project::new("explicit", 800, 0, Seed::new(2)).with_pitch_system(explicit);
        let explicit_directory = create_project(&root, &explicit_project).unwrap();

        assert_eq!(
            load_project(&explicit_directory).unwrap().project,
            explicit_project
        );
        let explicit_config =
            fs::read_to_string(explicit_directory.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(explicit_config.contains("[pitch_system.pitches]"));
        assert!(explicit_config.contains("\"⟟\" = 316.4"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn projects_store_and_resolve_reusable_tuning_system_references() {
        let root = temp_root("reusable-tuning-reference");
        let tuning = tuning_system::create_tuning_system(
            &root,
            PitchSystem::explicit(
                ExplicitPitchSystem::new(
                    "embers",
                    BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
                )
                .unwrap(),
            ),
        )
        .unwrap();
        let project = Project::new("piece", 800, 0, Seed::new(1)).with_tuning_system(&tuning);
        let directory = create_project(&root, &project).unwrap();

        let config = fs::read_to_string(directory.join(PROJECT_CONFIG_FILE)).unwrap();
        assert!(config.contains("tuning_system_id = \"embers\""));
        assert!(!config.contains("[pitch_system]"));
        assert_eq!(load_project(&directory).unwrap().project, project);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rejects_a_missing_tuning_system_reference() {
        let root = temp_root("missing-tuning-reference");
        let project_directory = root.join("projects").join("piece");
        fs::create_dir_all(&project_directory).unwrap();
        fs::write(
            project_directory.join(PROJECT_CONFIG_FILE),
            "name = \"piece\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\ntuning_system_id = \"missing-system\"\n",
        )
        .unwrap();

        let error = load_project(&project_directory).unwrap_err();

        assert!(error
            .to_string()
            .contains("references missing tuning system \"missing-system\""));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_projects_load_with_western_tuning_and_save_its_library_reference() {
        let root = temp_root("legacy-pitch-system");
        let project_directory = root.join("projects").join("legacy");
        fs::create_dir_all(&project_directory).unwrap();
        fs::write(
            project_directory.join(PROJECT_CONFIG_FILE),
            "name = \"legacy\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n",
        )
        .unwrap();

        let project = load_project(&project_directory).unwrap().project;

        assert!(
            (project
                .pitch_system()
                .resolve_cell("A4")
                .unwrap()
                .unwrap()
                .as_hz()
                - 440.0)
                .abs()
                < 1e-10
        );
        save_project(&project_directory, &project).unwrap();
        assert!(
            fs::read_to_string(project_directory.join(PROJECT_CONFIG_FILE))
                .unwrap()
                .contains("tuning_system_id = \"western-twelve-tone\"")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rejects_invalid_pitch_system_data() {
        let root = temp_root("invalid-pitch-system");
        let project_directory = root.join("projects").join("test");
        fs::create_dir_all(&project_directory).unwrap();
        fs::write(
            project_directory.join(PROJECT_CONFIG_FILE),
            "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[pitch_system]\nkind = \"periodic\"\nname = \"broken\"\nfundamental_hz = 0.0\nperiod = \"2/1\"\ndegrees = [\"1/1\"]\n\n[pitch_system.notation]\nkind = \"radler_digits\"\nplace_value = 10\n",
        )
        .unwrap();

        let error = load_project(&project_directory).unwrap_err();

        assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
        assert!(error.to_string().contains("frequency must be a positive"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_projects_default_the_sequence_to_each_part_once() {
        let root = temp_root("legacy-part-sequence");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);
        add_test_part(&project_directory, &mut project, "verse", 2);
        let config_path = project_directory.join(PROJECT_CONFIG_FILE);
        let legacy_config = fs::read_to_string(&config_path)
            .unwrap()
            .replace("sequence = []\n", "");
        fs::write(&config_path, legacy_config).unwrap();

        let loaded = load_project(&project_directory).unwrap().project;

        assert_eq!(
            loaded
                .sequence()
                .iter()
                .map(PartName::as_str)
                .collect::<Vec<_>>(),
            vec!["intro", "verse"]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_empty_sequence_remains_empty() {
        let root = temp_root("empty-part-sequence");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);

        let loaded = load_project(&project_directory).unwrap().project;

        assert!(loaded.sequence().is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rejects_sequence_references_to_missing_parts() {
        let root = temp_root("missing-sequence-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);
        let config_path = project_directory.join(PROJECT_CONFIG_FILE);
        let invalid_config = fs::read_to_string(&config_path)
            .unwrap()
            .replace("sequence = []", "sequence = [\"missing\"]");
        fs::write(&config_path, invalid_config).unwrap();

        let error = load_project(&project_directory).unwrap_err();

        assert!(matches!(error, LoadProjectError::InvalidSequence { .. }));
        assert!(error
            .to_string()
            .contains("sequence references missing part \"missing\""));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sequence_references_use_the_part_name_casing() {
        let root = temp_root("sequence-name-casing");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_test_part(&project_directory, &mut project, "Intro", 2);
        let config_path = project_directory.join(PROJECT_CONFIG_FILE);
        let config = fs::read_to_string(&config_path)
            .unwrap()
            .replace("sequence = []", "sequence = [\"INTRO\", \"intro\"]");
        fs::write(&config_path, config).unwrap();

        let loaded = load_project(&project_directory).unwrap().project;

        assert_eq!(
            loaded
                .sequence()
                .iter()
                .map(PartName::as_str)
                .collect::<Vec<_>>(),
            vec!["Intro", "Intro"]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rejects_voice_names_that_cannot_be_identifiers() {
        let root = temp_root("invalid-voice-identifiers");
        let project_directory = root.join("projects").join("test");
        fs::create_dir_all(&project_directory).unwrap();
        fs::write(
            project_directory.join(PROJECT_CONFIG_FILE),
            "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[[voices]]\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nname = \"LEAD\"\nvoice_type = \"sin\"\n",
        )
        .unwrap();

        let error = load_project(&project_directory).unwrap_err();

        assert!(matches!(error, LoadProjectError::InvalidConfig { .. }));
        assert!(error
            .to_string()
            .contains("voice name \"LEAD\" is duplicated"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_voice_configs_receive_stable_ids_when_loaded() {
        let root = temp_root("legacy-voice-ids");
        let project_directory = root.join("projects").join("test");
        fs::create_dir_all(&project_directory).unwrap();
        fs::write(
            project_directory.join(PROJECT_CONFIG_FILE),
            "name = \"test\"\ndescription = \"\"\nbeat_length = 800\ntiming_variance = 0\nseed = 1\n\n[[voices]]\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nname = \"bass\"\nvoice_type = \"sin\"\n",
        )
        .unwrap();

        let project = load_project(&project_directory).unwrap().project;

        assert_eq!(project.voices[0].id().value(), 1);
        assert_eq!(project.voices[1].id().value(), 2);
        assert_eq!(project.next_voice_id, 3);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn voice_changes_update_every_part_and_preserve_cells_by_voice_id() {
        let root = temp_root("voice-columns");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);
        add_test_part(&project_directory, &mut project, "verse", 2);

        project = add_voice(&project_directory, &project, " lead ", VoiceType::Saw).unwrap();
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "lead\n\"\"\n\"\"\n"
        );
        assert_eq!(project.voices[0].id().value(), 1);

        fs::write(project_directory.join("intro.csv"), "lead\nC4\nD4\n").unwrap();
        fs::write(project_directory.join("verse.csv"), "lead\n\"E,4\"\nF4\n").unwrap();

        project = add_voice(&project_directory, &project, "bass", VoiceType::Sin).unwrap();
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "lead,bass\nC4,\nD4,\n"
        );
        assert_eq!(
            fs::read_to_string(project_directory.join("verse.csv")).unwrap(),
            "lead,bass\n\"E,4\",\nF4,\n"
        );

        project = edit_voice(
            &project_directory,
            &project,
            &VoiceName::new("LEAD"),
            "melody",
            VoiceType::Sin,
        )
        .unwrap();
        assert_eq!(project.voices[0].id().value(), 1);
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "melody,bass\nC4,\nD4,\n"
        );

        project = delete_voice(&project_directory, &project, &VoiceName::new("bass")).unwrap();
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "melody\nC4\nD4\n"
        );
        assert_eq!(
            fs::read_to_string(project_directory.join("verse.csv")).unwrap(),
            "melody\n\"E,4\"\nF4\n"
        );

        project = add_voice(&project_directory, &project, "harmony", VoiceType::Saw).unwrap();
        assert_eq!(project.voices[1].id().value(), 3);
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "melody,harmony\nC4,\nD4,\n"
        );
        assert_eq!(load_project(&project_directory).unwrap().project, project);
        assert!(!project_directory
            .join(PROJECT_TRANSACTION_DIRECTORY)
            .exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_voice_changes_leave_project_files_untouched() {
        let root = temp_root("invalid-voice-change");
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();

        assert!(add_voice(&project_directory, &project, " ", VoiceType::Saw).is_err());
        let project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
        let saved_config = fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();

        assert!(add_voice(&project_directory, &project, "LEAD", VoiceType::Sin).is_err());
        assert!(edit_voice(
            &project_directory,
            &project,
            &VoiceName::new("lead"),
            " ",
            VoiceType::Sin,
        )
        .is_err());
        assert!(delete_voice(&project_directory, &project, &VoiceName::new("missing"),).is_err());
        assert_eq!(
            fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
            saved_config
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rejects_part_files_with_the_wrong_voice_schema() {
        let root = temp_root("invalid-part-schema");
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        let mut project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);
        fs::write(project_directory.join("intro.csv"), "wrong\nC4\nD4\n").unwrap();

        let error = load_project(&project_directory).unwrap_err();

        assert!(matches!(error, LoadProjectError::InvalidPart(_)));
        assert!(error.to_string().contains("voice headers do not match"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rejects_beat_rows_with_the_wrong_column_count() {
        let root = temp_root("invalid-beat-columns");
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        let project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
        let mut project = add_voice(&project_directory, &project, "bass", VoiceType::Sin).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);
        fs::write(
            project_directory.join("intro.csv"),
            "lead,bass\nC4\nD4,D2\n",
        )
        .unwrap();

        let error = load_project(&project_directory).unwrap_err();

        assert!(matches!(error, LoadProjectError::InvalidPart(_)));
        assert!(error
            .to_string()
            .contains("beat row 1 has 1 columns; expected 2"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_project_rolls_back_an_interrupted_multi_file_commit() {
        let root = temp_root("recover-voice-transaction");
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        let mut project = add_voice(&project_directory, &project, "lead", VoiceType::Saw).unwrap();
        add_test_part(&project_directory, &mut project, "intro", 2);
        fs::write(project_directory.join("intro.csv"), "lead\nC4\nD4\n").unwrap();
        let original_config = fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap();
        let original_part = fs::read(project_directory.join("intro.csv")).unwrap();

        let transaction_directory = project_directory.join(PROJECT_TRANSACTION_DIRECTORY);
        let old_directory = transaction_directory.join(TRANSACTION_OLD_DIRECTORY);
        fs::create_dir_all(&old_directory).unwrap();
        fs::create_dir(transaction_directory.join(TRANSACTION_NEW_DIRECTORY)).unwrap();
        fs::write(old_directory.join(PROJECT_CONFIG_FILE), &original_config).unwrap();
        fs::write(old_directory.join("intro.csv"), &original_part).unwrap();
        fs::write(transaction_directory.join(TRANSACTION_COMMITTING_FILE), "").unwrap();
        fs::write(project_directory.join(PROJECT_CONFIG_FILE), "invalid").unwrap();
        fs::write(project_directory.join("intro.csv"), "invalid").unwrap();

        let recovered = load_project(&project_directory).unwrap().project;

        assert_eq!(recovered, project);
        assert_eq!(
            fs::read(project_directory.join(PROJECT_CONFIG_FILE)).unwrap(),
            original_config
        );
        assert_eq!(
            fs::read(project_directory.join("intro.csv")).unwrap(),
            original_part
        );
        assert!(!transaction_directory.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn every_voice_type_round_trips_through_project_config() {
        let root = temp_root("voice-types-round-trip");
        let voices = VoiceType::ALL
            .into_iter()
            .enumerate()
            .map(|(index, voice_type)| Voice::new(index as u64 + 1, voice_type.label(), voice_type))
            .collect();
        let project = Project::new("voice types", 800, 0, Seed::new(1)).with_voices(voices);
        let project_directory = create_project(&root, &project).unwrap();

        assert_eq!(load_project(project_directory).unwrap().project, project);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_project_replaces_the_config_without_renaming_the_directory() {
        let root = temp_root("save-project");
        let original = Project::new("Original Name", 800, 10, Seed::new(1));
        let project_directory = create_project(&root, &original).unwrap();
        let updated = Project::new("Updated Name", 4000, 100, Seed::new(99))
            .with_description("updated description")
            .with_voices(vec![
                Voice::new(1, "lead", VoiceType::Saw),
                Voice::new(2, "bass", VoiceType::Sin),
            ]);

        save_project(&project_directory, &updated).unwrap();

        assert_eq!(
            project_directory,
            root.join("projects").join("original-name")
        );
        assert_eq!(load_project(&project_directory).unwrap().project, updated);
        assert!(!project_directory.join(".project.toml.pending").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_projects_returns_projects_sorted_by_name() {
        let root = temp_root("list-projects");
        create_project(
            &root,
            &Project::new("Zinc", 4000, 100, Seed::new(1)).with_description("last"),
        )
        .unwrap();
        create_project(
            &root,
            &Project::new("Arc", 4000, 100, Seed::new(2)).with_description("first"),
        )
        .unwrap();

        let projects = list_projects(&root).unwrap();

        assert_eq!(
            projects
                .iter()
                .map(|entry| entry.project.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Arc", "Zinc"]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_projects_allows_a_missing_projects_directory() {
        let root = temp_root("missing-projects-directory");
        fs::remove_dir_all(root.join("projects")).unwrap_or(());

        assert!(list_projects(&root).unwrap().is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    fn add_test_part(
        project_directory: &std::path::Path,
        project: &mut Project,
        name: &str,
        length: u32,
    ) {
        let created = part::create_part_file(
            project_directory,
            &project.parts,
            project.voices(),
            name,
            length,
        )
        .unwrap();
        project.add_part(created.commit());
        save_project(project_directory, project).unwrap();
    }

    fn temp_root(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ahess-{test_name}-{}-{unique}", std::process::id()));

        fs::create_dir_all(&root).unwrap();
        root
    }
}
