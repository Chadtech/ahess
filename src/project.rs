use std::{
    error::Error,
    fmt::{self, Write as _},
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{seed::Seed, voice_name::VoiceName};

pub const PROJECTS_DIRECTORY: &str = "projects";
pub const PROJECT_CONFIG_FILE: &str = "project.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub beat_length: u32,
    pub timing_variance: u32,
    pub seed: Seed,
    pub description: String,
    pub voices: Vec<Voice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Voice {
    pub name: VoiceName,
    pub voice_type: VoiceType,
}

impl Voice {
    pub fn new(name: impl Into<VoiceName>, voice_type: VoiceType) -> Self {
        Self {
            name: name.into(),
            voice_type,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VoiceType {
    Sin,
    Saw,
}

impl VoiceType {
    pub const ALL: [Self; 2] = [Self::Sin, Self::Saw];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
        }
    }

    const fn config_value(self) -> &'static str {
        match self {
            Self::Sin => "sin",
            Self::Saw => "saw",
        }
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
            voices: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_voices(mut self, voices: Vec<Voice>) -> Self {
        self.voices = voices;
        self
    }

    pub fn add_voice(&mut self, voice: Voice) {
        self.voices.push(voice);
    }

    pub fn voice(&self, name: &VoiceName) -> Option<&Voice> {
        self.voices
            .iter()
            .find(|voice| voice.name.eq_ignore_ascii_case(name))
    }

    pub fn remove_voice(&mut self, name: &VoiceName) -> Option<Voice> {
        let index = self
            .voices
            .iter()
            .position(|voice| voice.name.eq_ignore_ascii_case(name))?;
        Some(self.voices.remove(index))
    }

    pub fn config_file_contents(&self) -> String {
        let mut contents = format!(
            "name = {}\ndescription = {}\nbeat_length = {}\ntiming_variance = {}\nseed = {}\n",
            toml_string(&self.name),
            toml_string(&self.description),
            self.beat_length,
            self.timing_variance,
            self.seed.value()
        );

        for voice in &self.voices {
            contents.push_str("\n[[voices]]\n");
            contents.push_str("name = ");
            contents.push_str(&toml_string(voice.name.as_str()));
            contents.push_str("\nvoice_type = ");
            contents.push_str(&toml_string(voice.voice_type.config_value()));
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
pub enum LoadProjectError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    InvalidConfig {
        path: PathBuf,
        source: toml::de::Error,
    },
}

#[derive(Debug)]
pub enum SaveProjectError {
    Write { path: PathBuf, source: io::Error },
    Replace { path: PathBuf, source: io::Error },
}

impl fmt::Display for SaveProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
        }
    }
}

impl Error for LoadProjectError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidConfig { source, .. } => Some(source),
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
    let config_path = project_directory.join(PROJECT_CONFIG_FILE);
    let config = fs::read_to_string(&config_path).map_err(|source| LoadProjectError::Io {
        path: config_path.clone(),
        source,
    })?;
    let project_config = toml::from_str::<ProjectConfig>(&config).map_err(|source| {
        LoadProjectError::InvalidConfig {
            path: config_path,
            source,
        }
    })?;

    Ok(ProjectEntry {
        project: project_config.into_project(),
        project_directory: project_directory.to_path_buf(),
    })
}

pub fn save_project(
    project_directory: impl AsRef<Path>,
    project: &Project,
) -> Result<(), SaveProjectError> {
    let project_directory = project_directory.as_ref();
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
    #[serde(default, deserialize_with = "deserialize_voices")]
    voices: Vec<Voice>,
}

fn deserialize_voices<'de, D>(deserializer: D) -> Result<Vec<Voice>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let voices = Vec::<Voice>::deserialize(deserializer)?;

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

impl ProjectConfig {
    fn into_project(self) -> Project {
        Project::new(
            self.name,
            self.beat_length,
            self.timing_variance,
            Seed::new(self.seed),
        )
        .with_description(self.description)
        .with_voices(self.voices)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        create_project, list_projects, load_project, project_directory_name, save_project,
        CreateProjectError, LoadProjectError, Project, Voice, VoiceType, PROJECT_CONFIG_FILE,
    };
    use crate::{seed::Seed, voice_name::VoiceName};

    #[test]
    fn project_stores_the_initial_music_settings() {
        let project = Project::new("test", 4000, 100, Seed::new(19)).with_description("sketch");

        assert_eq!(project.name, "test");
        assert_eq!(project.beat_length, 4000);
        assert_eq!(project.timing_variance, 100);
        assert_eq!(project.seed, Seed::new(19));
        assert_eq!(project.description, "sketch");
        assert!(project.voices.is_empty());
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
            "name = \"test \\\"score\\\"\"\ndescription = \"line one\\nline two\"\nbeat_length = 4000\ntiming_variance = 100\nseed = 1234\n"
        );
    }

    #[test]
    fn config_file_contents_store_voices_in_column_order() {
        let project = Project::new("test", 4000, 100, Seed::new(1234)).with_voices(vec![
            Voice::new("lead", VoiceType::Saw),
            Voice::new("bass", VoiceType::Sin),
        ]);

        assert_eq!(
            project.config_file_contents(),
            "name = \"test\"\ndescription = \"\"\nbeat_length = 4000\ntiming_variance = 100\nseed = 1234\n\n[[voices]]\nname = \"lead\"\nvoice_type = \"saw\"\n\n[[voices]]\nname = \"bass\"\nvoice_type = \"sin\"\n"
        );
    }

    #[test]
    fn voices_are_found_and_removed_by_name_without_changing_column_order() {
        let mut project = Project::new("test", 4000, 100, Seed::new(1234)).with_voices(vec![
            Voice::new("lead", VoiceType::Saw),
            Voice::new("bass", VoiceType::Sin),
            Voice::new("harmony", VoiceType::Saw),
        ]);

        assert_eq!(
            project.voice(&VoiceName::new("BASS")),
            Some(&Voice::new("bass", VoiceType::Sin))
        );
        assert_eq!(
            project.remove_voice(&VoiceName::new("bass")),
            Some(Voice::new("bass", VoiceType::Sin))
        );
        assert_eq!(
            project.voices,
            vec![
                Voice::new("lead", VoiceType::Saw),
                Voice::new("harmony", VoiceType::Saw),
            ]
        );
        assert_eq!(project.voice(&VoiceName::new("bass")), None);
        assert_eq!(project.remove_voice(&VoiceName::new("missing")), None);
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
    fn every_voice_type_round_trips_through_project_config() {
        let root = temp_root("voice-types-round-trip");
        let voices = VoiceType::ALL
            .into_iter()
            .map(|voice_type| Voice::new(voice_type.label(), voice_type))
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
                Voice::new("lead", VoiceType::Saw),
                Voice::new("bass", VoiceType::Sin),
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
