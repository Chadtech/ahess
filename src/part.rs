use std::{
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::project::{self, Voice};

pub const DELETED_PARTS_DIRECTORY: &str = "deleted";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct PartName(String);

impl PartName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn eq_ignore_ascii_case(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl From<&str> for PartName {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for PartName {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Part {
    pub name: PartName,
    pub length: u32,
}

impl Part {
    pub fn new(name: impl Into<PartName>, length: u32) -> Self {
        Self {
            name: name.into(),
            length,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPartName;

impl fmt::Display for InvalidPartName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "part name must contain a letter or number")
    }
}

impl Error for InvalidPartName {}

pub fn csv_file_name(name: &PartName) -> Result<String, InvalidPartName> {
    let file_stem = project::project_directory_name(name.as_str()).ok_or(InvalidPartName)?;
    Ok(format!("{file_stem}.csv"))
}

pub struct CreatedPartFile {
    part: Part,
    path: PathBuf,
}

impl CreatedPartFile {
    pub fn part(&self) -> &Part {
        &self.part
    }

    pub fn commit(self) -> Part {
        self.part
    }

    pub fn rollback(self) -> Result<(), PartFileRollbackError> {
        fs::remove_file(&self.path).map_err(|source| PartFileRollbackError::RemoveCreated {
            path: self.path,
            source,
        })
    }
}

pub struct DeletedPartFile {
    part: Part,
    original_path: PathBuf,
    deleted_path: PathBuf,
}

impl DeletedPartFile {
    pub fn part(&self) -> &Part {
        &self.part
    }

    pub fn commit(self) -> Part {
        self.part
    }

    pub fn rollback(self) -> Result<(), PartFileRollbackError> {
        fs::rename(&self.deleted_path, &self.original_path).map_err(|source| {
            PartFileRollbackError::RestoreDeleted {
                deleted_path: self.deleted_path,
                original_path: self.original_path,
                source,
            }
        })
    }
}

#[derive(Debug)]
pub enum PartFileRollbackError {
    RemoveCreated {
        path: PathBuf,
        source: io::Error,
    },
    RestoreDeleted {
        deleted_path: PathBuf,
        original_path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for PartFileRollbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RemoveCreated { path, source } => {
                write!(f, "failed to remove {}: {source}", path.display())
            }
            Self::RestoreDeleted {
                deleted_path,
                original_path,
                source,
            } => write!(
                f,
                "failed to restore {} to {}: {source}",
                deleted_path.display(),
                original_path.display()
            ),
        }
    }
}

impl Error for PartFileRollbackError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RemoveCreated { source, .. } | Self::RestoreDeleted { source, .. } => {
                Some(source)
            }
        }
    }
}

#[derive(Debug)]
pub enum DeletedPartPathError {
    Inspect { path: PathBuf, source: io::Error },
    InvalidCsvFileName(String),
    NamesExhausted(String),
}

impl fmt::Display for DeletedPartPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspect { path, source } => {
                write!(f, "failed to inspect {}: {source}", path.display())
            }
            Self::InvalidCsvFileName(file_name) => {
                write!(f, "part filename {file_name:?} does not end in .csv")
            }
            Self::NamesExhausted(file_name) => {
                write!(f, "no archive filename is available for {file_name:?}")
            }
        }
    }
}

impl Error for DeletedPartPathError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Inspect { source, .. } => Some(source),
            Self::InvalidCsvFileName(_) | Self::NamesExhausted(_) => None,
        }
    }
}

#[derive(Debug)]
pub enum CreatePartError {
    EmptyName,
    ZeroLength,
    DuplicateName(String),
    CsvAlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for CreatePartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "part name must contain a letter or number"),
            Self::ZeroLength => write!(f, "part length must be at least one beat"),
            Self::DuplicateName(name) => write!(f, "a part named {name:?} already exists"),
            Self::CsvAlreadyExists(path) => {
                write!(f, "a file already exists at {}", path.display())
            }
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {source}", path.display())
            }
        }
    }
}

impl Error for CreatePartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum DeletePartError {
    InvalidName {
        name: String,
        source: InvalidPartName,
    },
    DeletedPath(DeletedPartPathError),
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for DeletePartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName { name, .. } => write!(f, "part {name:?} has an invalid name"),
            Self::DeletedPath(error) => write!(f, "{error}"),
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {source}", path.display())
            }
        }
    }
}

impl Error for DeletePartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidName { source, .. } => Some(source),
            Self::DeletedPath(error) => Some(error),
            Self::Io { source, .. } => Some(source),
        }
    }
}

pub fn create_part_file(
    project_directory: impl AsRef<Path>,
    parts: &[Part],
    voices: &[Voice],
    name: &str,
    length: u32,
) -> Result<CreatedPartFile, CreatePartError> {
    let name = name.trim();
    if length == 0 {
        return Err(CreatePartError::ZeroLength);
    }

    let part_name = PartName::new(name);
    let file_name = csv_file_name(&part_name).map_err(|_| CreatePartError::EmptyName)?;
    if parts
        .iter()
        .any(|part| part.name.eq_ignore_ascii_case(&part_name))
    {
        return Err(CreatePartError::DuplicateName(name.to_string()));
    }

    let path = project_directory.as_ref().join(file_name);
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Err(CreatePartError::CsvAlreadyExists(path));
        }
        Err(source) => return Err(CreatePartError::Io { path, source }),
    };

    if let Err(source) = file.write_all(part_csv_contents(voices, length).as_bytes()) {
        drop(file);
        fs::remove_file(&path).ok();
        return Err(CreatePartError::Io { path, source });
    }
    drop(file);

    Ok(CreatedPartFile {
        part: Part::new(part_name, length),
        path,
    })
}

pub fn soft_delete_part_file(
    project_directory: impl AsRef<Path>,
    part: &Part,
) -> Result<DeletedPartFile, DeletePartError> {
    let project_directory = project_directory.as_ref();
    let file_name = csv_file_name(&part.name).map_err(|source| DeletePartError::InvalidName {
        name: part.name.as_str().to_string(),
        source,
    })?;
    let source_path = project_directory.join(&file_name);
    let deleted_directory = project_directory.join(DELETED_PARTS_DIRECTORY);

    fs::create_dir_all(&deleted_directory).map_err(|source| DeletePartError::Io {
        path: deleted_directory.clone(),
        source,
    })?;
    let deleted_path = available_deleted_path(&deleted_directory, &file_name)
        .map_err(DeletePartError::DeletedPath)?;
    fs::rename(&source_path, &deleted_path).map_err(|source| DeletePartError::Io {
        path: source_path.clone(),
        source,
    })?;

    Ok(DeletedPartFile {
        part: part.clone(),
        original_path: source_path,
        deleted_path,
    })
}

fn part_csv_contents(voices: &[Voice], length: u32) -> String {
    let mut contents = voices
        .iter()
        .map(|voice| escape_csv_value(voice.name.as_str()))
        .collect::<Vec<_>>()
        .join(",");
    contents.push('\n');

    let empty_row = ",".repeat(voices.len().saturating_sub(1));
    for _ in 0..length {
        contents.push_str(&empty_row);
        contents.push('\n');
    }

    contents
}

fn escape_csv_value(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn available_deleted_path(
    deleted_directory: &Path,
    file_name: &str,
) -> Result<PathBuf, DeletedPartPathError> {
    let stem = file_name
        .strip_suffix(".csv")
        .ok_or_else(|| DeletedPartPathError::InvalidCsvFileName(file_name.to_string()))?;
    let first_choice = deleted_directory.join(file_name);
    if !first_choice
        .try_exists()
        .map_err(|source| DeletedPartPathError::Inspect {
            path: first_choice.clone(),
            source,
        })?
    {
        return Ok(first_choice);
    }

    for suffix in 2..=u32::MAX {
        let candidate = deleted_directory.join(format!("{stem}-{suffix}.csv"));
        if !candidate
            .try_exists()
            .map_err(|source| DeletedPartPathError::Inspect {
                path: candidate.clone(),
                source,
            })?
        {
            return Ok(candidate);
        }
    }

    Err(DeletedPartPathError::NamesExhausted(file_name.to_string()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        available_deleted_path, create_part_file, csv_file_name, soft_delete_part_file,
        DeletedPartPathError, Part, PartName, DELETED_PARTS_DIRECTORY,
    };
    use crate::{
        project::{create_project, load_project, save_project, Project, Voice, VoiceType},
        seed::Seed,
    };

    #[test]
    fn csv_filenames_are_derived_from_part_names() {
        assert_eq!(
            csv_file_name(&PartName::new("Part A!")).unwrap(),
            "part-a.csv"
        );
        assert!(csv_file_name(&PartName::new("!!!")).is_err());
    }

    #[test]
    fn deleted_paths_report_invalid_filenames_with_a_domain_error() {
        let root = temp_root("invalid-deleted-filename");

        let error = available_deleted_path(&root, "intro.txt").unwrap_err();

        assert!(matches!(
            error,
            DeletedPartPathError::InvalidCsvFileName(file_name) if file_name == "intro.txt"
        ));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn part_file_operations_do_not_edit_project_metadata() {
        let root = temp_root("file-operation-boundary");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();

        let discarded = create_part_file(
            &project_directory,
            &project.parts,
            &project.voices,
            "discarded",
            4,
        )
        .unwrap();
        discarded.rollback().unwrap();
        assert!(!project_directory.join("discarded.csv").exists());

        let created = create_part_file(
            &project_directory,
            &project.parts,
            &project.voices,
            "intro",
            4,
        )
        .unwrap();
        assert!(project.parts.is_empty());

        let part = created.commit();
        project.add_part(part.clone());
        save_project(&project_directory, &project).unwrap();
        let deleted = soft_delete_part_file(&project_directory, &part).unwrap();
        assert_eq!(project.parts, vec![part]);

        deleted.rollback().unwrap();
        assert!(project_directory.join("intro.csv").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_a_csv_with_voice_headers_and_one_row_per_beat() {
        let root = temp_root("create-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![
            Voice::new("lead", VoiceType::Saw),
            Voice::new("bass, low", VoiceType::Sin),
        ]);
        let project_directory = create_project(&root, &project).unwrap();

        let part = add_part(&project_directory, &mut project, " Intro ", 3);

        assert_eq!(project.parts.len(), 1);
        assert_eq!(part.name.as_str(), "Intro");
        assert_eq!(part.length, 3);
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "lead,\"bass, low\"\n,\n,\n,\n"
        );
        assert_eq!(load_project(&project_directory).unwrap().project, project);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_zero_length_duplicate_and_colliding_parts_without_overwriting_files() {
        let root = temp_root("invalid-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();

        assert!(create_part_file(
            &project_directory,
            &project.parts,
            &project.voices,
            "intro",
            0,
        )
        .is_err());
        add_part(&project_directory, &mut project, "intro", 4);
        assert!(create_part_file(
            &project_directory,
            &project.parts,
            &project.voices,
            "INTRO",
            4,
        )
        .is_err());
        assert!(create_part_file(
            &project_directory,
            &project.parts,
            &project.voices,
            "intro!",
            4,
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "\n\n\n\n\n"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn soft_delete_moves_the_csv_and_removes_the_part_from_the_project() {
        let root = temp_root("delete-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_part(&project_directory, &mut project, "intro", 4);

        delete_part(&project_directory, &mut project, &PartName::new("INTRO"));

        assert!(project.parts.is_empty());
        assert!(!project_directory.join("intro.csv").exists());
        assert!(project_directory
            .join(DELETED_PARTS_DIRECTORY)
            .join("intro.csv")
            .is_file());
        assert!(load_project(&project_directory)
            .unwrap()
            .project
            .parts
            .is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn soft_delete_never_overwrites_an_older_deleted_csv() {
        let root = temp_root("delete-part-collision");
        let mut project = Project::new("test", 800, 0, Seed::new(1));
        let project_directory = create_project(&root, &project).unwrap();
        add_part(&project_directory, &mut project, "intro", 2);
        delete_part(&project_directory, &mut project, &PartName::new("intro"));
        add_part(&project_directory, &mut project, "intro", 3);

        delete_part(&project_directory, &mut project, &PartName::new("intro"));

        let deleted = project_directory.join(DELETED_PARTS_DIRECTORY);
        assert!(deleted.join("intro.csv").is_file());
        assert!(deleted.join("intro-2.csv").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    fn add_part(
        project_directory: &std::path::Path,
        project: &mut Project,
        name: &str,
        length: u32,
    ) -> Part {
        let created = create_part_file(
            project_directory,
            &project.parts,
            &project.voices,
            name,
            length,
        )
        .unwrap();
        let part = created.commit();
        project.add_part(part.clone());
        save_project(project_directory, project).unwrap();
        part
    }

    fn delete_part(
        project_directory: &std::path::Path,
        project: &mut Project,
        name: &PartName,
    ) -> Part {
        let index = project
            .parts
            .iter()
            .position(|part| part.name.eq_ignore_ascii_case(name))
            .unwrap();
        let deleted = soft_delete_part_file(project_directory, &project.parts[index]).unwrap();
        project.parts.remove(index);
        save_project(project_directory, project).unwrap();
        deleted.commit()
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
