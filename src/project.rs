use std::{
    error::Error,
    fmt::{self, Write as _},
    fs, io,
    path::{Path, PathBuf},
};

use crate::seed::Seed;

pub const PROJECTS_DIRECTORY: &str = "projects";
pub const PROJECT_CONFIG_FILE: &str = "project.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub beat_length: u32,
    pub timing_variance: u32,
    pub seed: Seed,
    pub description: String,
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
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn config_file_contents(&self) -> String {
        format!(
            "name = {}\ndescription = {}\nbeat_length = {}\ntiming_variance = {}\nseed = {}\n",
            toml_string(&self.name),
            toml_string(&self.description),
            self.beat_length,
            self.timing_variance,
            self.seed.value()
        )
    }
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        create_project, project_directory_name, CreateProjectError, Project, PROJECT_CONFIG_FILE,
    };
    use crate::seed::Seed;

    #[test]
    fn project_stores_the_initial_music_settings() {
        let project = Project::new("test", 4000, 100, Seed::new(19)).with_description("sketch");

        assert_eq!(project.name, "test");
        assert_eq!(project.beat_length, 4000);
        assert_eq!(project.timing_variance, 100);
        assert_eq!(project.seed, Seed::new(19));
        assert_eq!(project.description, "sketch");
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
