use std::{
    error::Error,
    fmt::{self, Write as _},
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::pitch_system::PitchSystem;

pub const TUNING_SYSTEMS_DIRECTORY: &str = "tuning-systems";
pub const DEFAULT_TUNING_SYSTEM_ID: &str = "western-twelve-tone";
const PROJECTS_DIRECTORY: &str = "projects";
const PROJECT_CONFIG_FILE: &str = "project.toml";

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TuningSystemId(String);

impl TuningSystemId {
    pub fn new(value: impl Into<String>) -> Result<Self, TuningLibraryError> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('-')
            || value.ends_with('-')
            || value.contains("--")
            || value
                .bytes()
                .any(|byte| !byte.is_ascii_lowercase() && !byte.is_ascii_digit() && byte != b'-')
        {
            return Err(TuningLibraryError::Invalid(format!(
                "tuning system id {value:?} must contain lowercase letters, numbers, and single hyphen separators"
            )));
        }
        Ok(Self(value))
    }

    pub fn from_name(name: &str) -> Result<Self, TuningLibraryError> {
        let mut id = String::new();
        let mut previous_was_separator = false;
        for ch in name.trim().chars() {
            if ch.is_ascii_alphanumeric() {
                id.push(ch.to_ascii_lowercase());
                previous_was_separator = false;
            } else if !id.is_empty() && !previous_was_separator {
                id.push('-');
                previous_was_separator = true;
            }
        }
        if previous_was_separator {
            id.pop();
        }
        if id.is_empty() {
            return Err(TuningLibraryError::Invalid(
                "tuning system name must contain a letter or number".to_string(),
            ));
        }
        Self::new(id)
    }

    pub fn default_western() -> Self {
        Self(DEFAULT_TUNING_SYSTEM_ID.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for TuningSystemId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TuningSystemSource {
    BuiltIn,
    User,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuningSystem {
    id: TuningSystemId,
    pitch_system: PitchSystem,
    source: TuningSystemSource,
}

impl TuningSystem {
    pub fn new_user(id: TuningSystemId, pitch_system: PitchSystem) -> Self {
        Self {
            id,
            pitch_system,
            source: TuningSystemSource::User,
        }
    }

    pub fn built_in_western() -> Self {
        Self {
            id: TuningSystemId::default_western(),
            pitch_system: PitchSystem::western_twelve_tone(),
            source: TuningSystemSource::BuiltIn,
        }
    }

    pub fn id(&self) -> &TuningSystemId {
        &self.id
    }

    pub fn name(&self) -> &str {
        self.pitch_system.name()
    }

    pub fn pitch_system(&self) -> &PitchSystem {
        &self.pitch_system
    }

    pub fn source(&self) -> TuningSystemSource {
        self.source
    }
}

pub fn list_tuning_systems(
    workspace_root: impl AsRef<Path>,
) -> Result<Vec<TuningSystem>, TuningLibraryError> {
    let directory = workspace_root.as_ref().join(TUNING_SYSTEMS_DIRECTORY);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return Ok(vec![TuningSystem::built_in_western()]);
        }
        Err(source) => return Err(TuningLibraryError::io(directory, source)),
    };

    let mut systems = vec![TuningSystem::built_in_western()];
    for entry in entries {
        let entry = entry.map_err(|source| TuningLibraryError::io(directory.clone(), source))?;
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|extension| extension.to_str()) != Some("toml")
        {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .map_err(|source| TuningLibraryError::io(path.clone(), source))?;
        let stored = toml::from_str::<StoredTuningSystem>(&contents).map_err(|source| {
            TuningLibraryError::InvalidConfig {
                path: path.clone(),
                source,
            }
        })?;
        let expected_file_name = tuning_file_name(&stored.id);
        if path.file_name().and_then(|name| name.to_str()) != Some(expected_file_name.as_str()) {
            return Err(TuningLibraryError::Invalid(format!(
                "tuning system file {} must be named {expected_file_name:?} to match its id",
                path.display()
            )));
        }
        if systems.iter().any(|system| system.id == stored.id) {
            return Err(TuningLibraryError::Invalid(format!(
                "tuning system id {:?} is duplicated",
                stored.id.as_str()
            )));
        }
        systems.push(TuningSystem::new_user(stored.id, stored.pitch_system));
    }
    systems[1..].sort_by_key(|system| system.name().to_ascii_lowercase());
    Ok(systems)
}

pub fn create_tuning_system(
    workspace_root: impl AsRef<Path>,
    pitch_system: PitchSystem,
) -> Result<TuningSystem, TuningLibraryError> {
    let systems = list_tuning_systems(&workspace_root)?;
    let base_id = TuningSystemId::from_name(pitch_system.name())?;
    let id = available_id(&base_id, &systems)?;
    let system = TuningSystem::new_user(id, pitch_system);
    write_tuning_system(workspace_root.as_ref(), &system, false)?;
    Ok(system)
}

pub fn update_tuning_system(
    workspace_root: impl AsRef<Path>,
    id: &TuningSystemId,
    pitch_system: PitchSystem,
) -> Result<TuningSystem, TuningLibraryError> {
    if id.as_str() == DEFAULT_TUNING_SYSTEM_ID {
        return Err(TuningLibraryError::Invalid(
            "the built-in western tuning cannot be edited; duplicate it first".to_string(),
        ));
    }
    let existing = list_tuning_systems(&workspace_root)?;
    if !existing.iter().any(|system| system.id() == id) {
        return Err(TuningLibraryError::Invalid(format!(
            "tuning system {:?} no longer exists",
            id.as_str()
        )));
    }
    let system = TuningSystem::new_user(id.clone(), pitch_system);
    write_tuning_system(workspace_root.as_ref(), &system, true)?;
    Ok(system)
}

pub fn delete_tuning_system(
    workspace_root: impl AsRef<Path>,
    id: &TuningSystemId,
) -> Result<(), TuningLibraryError> {
    if id.as_str() == DEFAULT_TUNING_SYSTEM_ID {
        return Err(TuningLibraryError::Invalid(
            "the built-in western tuning cannot be deleted".to_string(),
        ));
    }
    let references = projects_referencing(workspace_root.as_ref(), id)?;
    if !references.is_empty() {
        return Err(TuningLibraryError::InUse {
            id: id.clone(),
            projects: references,
        });
    }
    let path = tuning_file_path(workspace_root.as_ref(), id);
    fs::remove_file(&path).map_err(|source| TuningLibraryError::io(path, source))?;
    Ok(())
}

pub fn find_tuning_system(systems: &[TuningSystem], id: &TuningSystemId) -> Option<TuningSystem> {
    systems.iter().find(|system| system.id() == id).cloned()
}

fn available_id(
    base: &TuningSystemId,
    systems: &[TuningSystem],
) -> Result<TuningSystemId, TuningLibraryError> {
    if systems.iter().all(|system| system.id() != base) {
        return Ok(base.clone());
    }
    for suffix in 2_u32.. {
        let candidate = TuningSystemId::new(format!("{}-{suffix}", base.as_str()))?;
        if systems.iter().all(|system| system.id() != &candidate) {
            return Ok(candidate);
        }
    }
    unreachable!("u32 tuning id suffixes cannot be exhausted in memory")
}

fn write_tuning_system(
    workspace_root: &Path,
    system: &TuningSystem,
    replace: bool,
) -> Result<(), TuningLibraryError> {
    let directory = workspace_root.join(TUNING_SYSTEMS_DIRECTORY);
    fs::create_dir_all(&directory)
        .map_err(|source| TuningLibraryError::io(directory.clone(), source))?;
    let path = tuning_file_path(workspace_root, system.id());
    if !replace && path.exists() {
        return Err(TuningLibraryError::Invalid(format!(
            "tuning system {:?} already exists",
            system.id().as_str()
        )));
    }
    let pending = directory.join(format!(".{}.pending", tuning_file_name(system.id())));
    let encoded_id = toml_string(system.id().as_str()).map_err(TuningLibraryError::Format)?;
    let mut contents = format!("id = {encoded_id}\n");
    system.pitch_system.append_config(&mut contents);
    fs::write(&pending, contents)
        .map_err(|source| TuningLibraryError::io(pending.clone(), source))?;
    fs::rename(&pending, &path).map_err(|source| TuningLibraryError::io(path, source))
}

fn projects_referencing(
    workspace_root: &Path,
    id: &TuningSystemId,
) -> Result<Vec<String>, TuningLibraryError> {
    let directory = workspace_root.join(PROJECTS_DIRECTORY);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(TuningLibraryError::io(directory, source)),
    };
    let mut projects = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| TuningLibraryError::io(directory.clone(), source))?;
        let path = entry.path().join(PROJECT_CONFIG_FILE);
        if !path.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .map_err(|source| TuningLibraryError::io(path.clone(), source))?;
        let stored = toml::from_str::<StoredProjectReference>(&contents).map_err(|source| {
            TuningLibraryError::InvalidConfig {
                path: path.clone(),
                source,
            }
        })?;
        if stored.tuning_system_id.as_ref() == Some(id) {
            projects.push(
                stored
                    .name
                    .unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned()),
            );
        }
    }
    projects.sort_by_key(|name| name.to_ascii_lowercase());
    Ok(projects)
}

fn tuning_file_name(id: &TuningSystemId) -> String {
    format!("{}.toml", id.as_str())
}

fn tuning_file_path(workspace_root: &Path, id: &TuningSystemId) -> PathBuf {
    workspace_root
        .join(TUNING_SYSTEMS_DIRECTORY)
        .join(tuning_file_name(id))
}

#[derive(Deserialize)]
struct StoredTuningSystem {
    id: TuningSystemId,
    pitch_system: PitchSystem,
}

#[derive(Deserialize)]
struct StoredProjectReference {
    name: Option<String>,
    tuning_system_id: Option<TuningSystemId>,
}

#[derive(Debug)]
pub enum TuningLibraryError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidConfig {
        path: PathBuf,
        source: toml::de::Error,
    },
    Format(fmt::Error),
    Invalid(String),
    InUse {
        id: TuningSystemId,
        projects: Vec<String>,
    },
}

impl TuningLibraryError {
    fn io(path: PathBuf, source: io::Error) -> Self {
        Self::Io { path, source }
    }
}

impl fmt::Display for TuningLibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "filesystem error at {}: {source}",
                    path.display()
                )
            }
            Self::InvalidConfig { path, source } => {
                write!(
                    formatter,
                    "invalid tuning configuration at {}: {source}",
                    path.display()
                )
            }
            Self::Format(source) => write!(formatter, "failed to encode TOML: {source}"),
            Self::Invalid(message) => formatter.write_str(message),
            Self::InUse { id, projects } => write!(
                formatter,
                "tuning system {:?} is used by {}",
                id.as_str(),
                projects.join(", ")
            ),
        }
    }
}

impl Error for TuningLibraryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidConfig { source, .. } => Some(source),
            Self::Format(source) => Some(source),
            Self::Invalid(_) | Self::InUse { .. } => None,
        }
    }
}

/// Encodes a Rust string as a quoted TOML basic string, escaping characters
/// that would otherwise change the value or make the generated TOML invalid.
fn toml_string(value: &str) -> Result<String, fmt::Error> {
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
                write!(output, "\\u{:04x}", u32::from(ch))?;
            }
            ch if ch.is_control() => {
                write!(output, "\\U{:08x}", u32::from(ch))?;
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        create_tuning_system, delete_tuning_system, list_tuning_systems, update_tuning_system,
        TuningSystemSource,
    };
    use crate::{
        pitch_system::{ExplicitPitchSystem, FrequencyHz, PitchSystem},
        project::{self, Project},
        seed::Seed,
    };

    #[test]
    fn library_round_trips_user_tunings_and_preserves_ids_when_renamed() {
        let root = temp_root("round-trip");
        let created = create_tuning_system(&root, explicit("embers", 197.3)).unwrap();

        assert_eq!(created.id().as_str(), "embers");
        assert_eq!(list_tuning_systems(&root).unwrap().len(), 2);

        let renamed = update_tuning_system(&root, created.id(), explicit("coals", 241.8)).unwrap();
        assert_eq!(renamed.id(), created.id());
        assert_eq!(renamed.name(), "coals");
        assert_eq!(list_tuning_systems(&root).unwrap()[1], renamed);

        delete_tuning_system(&root, created.id()).unwrap();
        assert_eq!(list_tuning_systems(&root).unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn library_always_contains_an_immutable_builtin() {
        let root = temp_root("builtin");
        let systems = list_tuning_systems(&root).unwrap();

        assert_eq!(systems.len(), 1);
        assert_eq!(systems[0].source(), TuningSystemSource::BuiltIn);
        assert!(delete_tuning_system(&root, systems[0].id()).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn referenced_tunings_cannot_be_deleted() {
        let root = temp_root("referenced");
        let tuning = create_tuning_system(&root, explicit("embers", 197.3)).unwrap();
        let project = Project::new("ember piece", 800, 0, Seed::new(1)).with_tuning_system(&tuning);
        project::create_project(&root, &project).unwrap();

        let error = delete_tuning_system(&root, tuning.id()).unwrap_err();

        assert!(error.to_string().contains("ember piece"));
        assert_eq!(list_tuning_systems(&root).unwrap().len(), 2);
        fs::remove_dir_all(root).unwrap();
    }

    fn explicit(name: &str, frequency: f64) -> PitchSystem {
        PitchSystem::explicit(
            ExplicitPitchSystem::new(
                name,
                BTreeMap::from([("tone".to_string(), FrequencyHz::new(frequency).unwrap())]),
            )
            .unwrap(),
        )
    }

    fn temp_root(test_name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ahess-tuning-{test_name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
