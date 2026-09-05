//! Atomic multi-file commits, interrupted-update recovery, and history restoration.

use super::{Project, PROJECT_CONFIG_FILE};
use crate::part::{self, PartName};
use std::{
    collections::BTreeSet,
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(super) const PROJECT_TRANSACTION_DIRECTORY: &str = ".project-transaction";
pub(super) const TRANSACTION_NEW_DIRECTORY: &str = "new";
pub(super) const TRANSACTION_OLD_DIRECTORY: &str = "old";
pub(super) const TRANSACTION_CREATED_DIRECTORY: &str = "created";
pub(super) const TRANSACTION_COMMITTING_FILE: &str = "committing";
pub(super) const TRANSACTION_COMMITTED_FILE: &str = "committed";
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

pub(super) fn commit_project_files(
    project_directory: &Path,
    files: &[(String, Vec<u8>)],
) -> Result<(), ProjectTransactionError> {
    commit_project_file_changes(project_directory, files, &[])
}

fn commit_project_file_changes(
    project_directory: &Path,
    files: &[(String, Vec<u8>)],
    deleted_file_names: &[String],
) -> Result<(), ProjectTransactionError> {
    let transaction_directory = project_directory.join(PROJECT_TRANSACTION_DIRECTORY);
    let new_directory = transaction_directory.join(TRANSACTION_NEW_DIRECTORY);
    let old_directory = transaction_directory.join(TRANSACTION_OLD_DIRECTORY);
    let created_directory = transaction_directory.join(TRANSACTION_CREATED_DIRECTORY);
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
    fs::create_dir(&created_directory).map_err(|source| ProjectTransactionError::Io {
        path: created_directory.clone(),
        source,
    })?;

    for (file_name, contents) in files {
        write_synced(&new_directory.join(file_name), contents, true)?;
        let source_path = project_directory.join(file_name);
        match fs::read(&source_path) {
            Ok(original) => write_synced(&old_directory.join(file_name), &original, true)?,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                write_synced(&created_directory.join(file_name), b"", true)?;
            }
            Err(source) => {
                return Err(ProjectTransactionError::Io {
                    path: source_path,
                    source,
                });
            }
        }
    }
    for file_name in deleted_file_names {
        if files.iter().any(|(written, _)| written == file_name) {
            return Err(ProjectTransactionError::Invalid {
                path: project_directory.join(file_name),
                message: "one transaction cannot write and delete the same file".to_string(),
            });
        }
        let source_path = project_directory.join(file_name);
        match fs::read(&source_path) {
            Ok(original) => write_synced(&old_directory.join(file_name), &original, true)?,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ProjectTransactionError::Io {
                    path: source_path,
                    source,
                });
            }
        }
    }
    sync_directory(&new_directory)?;
    sync_directory(&old_directory)?;
    sync_directory(&created_directory)?;

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
    for file_name in deleted_file_names {
        let target_path = project_directory.join(file_name);
        match fs::remove_file(&target_path) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ProjectTransactionError::Io {
                    path: target_path,
                    source,
                });
            }
        }
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

pub(super) fn recover_project_transaction(
    project_directory: &Path,
) -> Result<(), ProjectTransactionError> {
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

    let created_directory = transaction_directory.join(TRANSACTION_CREATED_DIRECTORY);
    if created_directory.is_dir() {
        let entries =
            fs::read_dir(&created_directory).map_err(|source| ProjectTransactionError::Io {
                path: created_directory.clone(),
                source,
            })?;
        for entry in entries {
            let entry = entry.map_err(|source| ProjectTransactionError::Io {
                path: created_directory.clone(),
                source,
            })?;
            if !entry.path().is_file() {
                return Err(ProjectTransactionError::Invalid {
                    path: entry.path(),
                    message: "created-file marker is not a file".to_string(),
                });
            }
            let target_path = project_directory.join(entry.file_name());
            match fs::remove_file(&target_path) {
                Ok(()) => {}
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(ProjectTransactionError::Io {
                        path: target_path,
                        source,
                    });
                }
            }
        }
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

#[derive(Debug)]
pub enum RestoreProjectStateError {
    MissingScore(String),
    Score {
        part_name: String,
        source: part::ScoreError,
    },
    Recovery(ProjectTransactionError),
    Commit {
        source: ProjectTransactionError,
        rollback_error: Option<ProjectTransactionError>,
    },
}

impl fmt::Display for RestoreProjectStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingScore(part_name) => {
                write!(formatter, "history has no score for part {part_name:?}")
            }
            Self::Score { part_name, source } => {
                write!(formatter, "couldn't restore score {part_name:?}: {source}")
            }
            Self::Recovery(source) => {
                write!(formatter, "failed to recover a project update: {source}")
            }
            Self::Commit {
                source,
                rollback_error: None,
            } => write!(formatter, "{source}"),
            Self::Commit {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                formatter,
                "{source}; also failed to restore the current project files: {rollback_error}"
            ),
        }
    }
}

impl Error for RestoreProjectStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Score { source, .. } => Some(source),
            Self::Recovery(source) | Self::Commit { source, .. } => Some(source),
            Self::MissingScore(_) => None,
        }
    }
}

pub fn restore_project_state(
    project_directory: impl AsRef<Path>,
    current_project: &Project,
    target_project: &Project,
    scores: &[(&PartName, &part::PartScore, &part::PartScore)],
    project_changed: bool,
    affected_parts: &[PartName],
) -> Result<(), RestoreProjectStateError> {
    let project_directory = project_directory.as_ref();
    recover_project_transaction(project_directory).map_err(RestoreProjectStateError::Recovery)?;

    let mut files = Vec::with_capacity(affected_parts.len() + usize::from(project_changed));
    let mut target_file_names = BTreeSet::new();
    let mut recovery_file_names_to_keep = BTreeSet::new();
    for project_part in &target_project.parts {
        let file_name = part::csv_file_name(&project_part.name)
            .expect("validated project part names always produce CSV filenames");
        target_file_names.insert(file_name.clone());
        if !affected_parts
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&project_part.name))
        {
            continue;
        }
        let (score, saved_score) = scores
            .iter()
            .find(|(part_name, _, _)| part_name.eq_ignore_ascii_case(&project_part.name))
            .map(|(_, score, saved_score)| (*score, *saved_score))
            .ok_or_else(|| {
                RestoreProjectStateError::MissingScore(project_part.name.as_str().to_string())
            })?;
        let saved_contents = saved_score
            .score_file_contents(project_directory, project_part, target_project.voices())
            .map_err(|source| RestoreProjectStateError::Score {
                part_name: project_part.name.as_str().to_string(),
                source,
            })?;
        files.push((file_name, saved_contents));
        if score != saved_score {
            let recovery_file_name = part::recovery_file_name(&project_part.name)
                .expect("validated project part names always produce recovery filenames");
            let contents = score
                .recovery_contents(project_directory, project_part, target_project.voices())
                .map_err(|source| RestoreProjectStateError::Score {
                    part_name: project_part.name.as_str().to_string(),
                    source,
                })?;
            recovery_file_names_to_keep.insert(recovery_file_name.clone());
            files.push((recovery_file_name, contents));
        }
    }
    if project_changed {
        files.push((
            PROJECT_CONFIG_FILE.to_string(),
            target_project.config_file_contents().into_bytes(),
        ));
    }

    let mut deleted_file_names = current_project
        .parts
        .iter()
        .filter(|project_part| {
            affected_parts
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&project_part.name))
        })
        .map(|project_part| {
            part::csv_file_name(&project_part.name)
                .expect("validated project part names always produce CSV filenames")
        })
        .filter(|file_name| !target_file_names.contains(file_name))
        .collect::<Vec<_>>();
    deleted_file_names.extend(
        current_project
            .parts
            .iter()
            .chain(target_project.parts.iter())
            .filter(|project_part| {
                affected_parts
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&project_part.name))
            })
            .map(|project_part| {
                part::recovery_file_name(&project_part.name)
                    .expect("validated project part names always produce recovery filenames")
            })
            .filter(|file_name| !recovery_file_names_to_keep.contains(file_name)),
    );
    deleted_file_names.sort();
    deleted_file_names.dedup();

    if let Err(source) = commit_project_file_changes(project_directory, &files, &deleted_file_names)
    {
        let rollback_error = recover_project_transaction(project_directory).err();
        return Err(RestoreProjectStateError::Commit {
            source,
            rollback_error,
        });
    }
    Ok(())
}
