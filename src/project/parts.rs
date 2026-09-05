//! Part and arrangement operations, including coordinated metadata and score-file changes.

#[cfg(test)]
mod tests;

use super::transaction::{
    commit_project_files, recover_project_transaction, ProjectTransactionError,
};
use super::{Project, PROJECT_CONFIG_FILE};
use crate::part::{self, MajorSubdivision, Part, PartName, PartScore, SubdivisionPattern};
use crate::project;
use std::{error::Error, fmt, path::Path};

#[derive(Debug)]
pub enum EditPartRowsError {
    MissingPart(String),
    RowEdit(part::PartRowEditError),
    TooManyRows,
    Score(part::ScoreError),
    Recovery(ProjectTransactionError),
    Commit {
        source: ProjectTransactionError,
        rollback_error: Option<ProjectTransactionError>,
        recovery_error: Option<part::ScoreError>,
    },
}

impl fmt::Display for EditPartRowsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPart(name) => write!(formatter, "part {name:?} no longer exists"),
            Self::RowEdit(error) => write!(formatter, "{error}"),
            Self::TooManyRows => formatter.write_str("the part has too many beats"),
            Self::Score(error) => write!(formatter, "{error}"),
            Self::Recovery(error) => {
                write!(formatter, "failed to recover a project update: {error}")
            }
            Self::Commit {
                source,
                rollback_error: None,
                recovery_error: None,
            } => write!(formatter, "{source}"),
            Self::Commit {
                source,
                rollback_error,
                recovery_error,
            } => {
                write!(formatter, "{source}")?;
                if let Some(error) = rollback_error {
                    write!(
                        formatter,
                        "; also failed to restore the original project files: {error}"
                    )?;
                }
                if let Some(error) = recovery_error {
                    write!(
                        formatter,
                        "; also failed to preserve the unsaved score: {error}"
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl Error for EditPartRowsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RowEdit(error) => Some(error),
            Self::Score(error) => Some(error),
            Self::Recovery(error) | Self::Commit { source: error, .. } => Some(error),
            Self::MissingPart(_) | Self::TooManyRows => None,
        }
    }
}

pub fn edit_part_rows(
    project_directory: impl AsRef<Path>,
    project: &Project,
    part_name: &PartName,
    score: &part::PartScore,
    edit: part::PartRowEdit,
) -> Result<(Project, Part, part::PartScore), EditPartRowsError> {
    let project_directory = project_directory.as_ref();
    recover_project_transaction(project_directory).map_err(EditPartRowsError::Recovery)?;

    let part_index = project
        .parts
        .iter()
        .position(|part| part.name.eq_ignore_ascii_case(part_name))
        .ok_or_else(|| EditPartRowsError::MissingPart(part_name.as_str().to_string()))?;
    let original_part = project.parts[part_index].clone();
    let updated_score = score
        .edited_rows(edit, project.voices().len())
        .map_err(EditPartRowsError::RowEdit)?;
    let updated_length =
        u32::try_from(updated_score.rows().len()).map_err(|_| EditPartRowsError::TooManyRows)?;
    let mut updated_project = project.clone();
    updated_project.parts[part_index].length = updated_length;
    let updated_part = updated_project.parts[part_index].clone();
    let score_contents = updated_score
        .validated_contents(project_directory, &updated_part, &updated_project)
        .map_err(EditPartRowsError::Score)?;

    part::PartScore::clear_recovery(project_directory, &original_part)
        .map_err(EditPartRowsError::Score)?;
    let score_file_name = part::csv_file_name(&updated_part.name)
        .expect("validated project part names always produce CSV filenames");
    let files = [
        (score_file_name, score_contents),
        (
            PROJECT_CONFIG_FILE.to_string(),
            updated_project.config_file_contents().into_bytes(),
        ),
    ];

    if let Err(source) = commit_project_files(project_directory, &files) {
        let rollback_error = recover_project_transaction(project_directory).err();
        let recovery_error = score
            .save_recovery(project_directory, &original_part, project.voices())
            .err();
        return Err(EditPartRowsError::Commit {
            source,
            rollback_error,
            recovery_error,
        });
    }

    Ok((updated_project, updated_part, updated_score))
}

#[derive(Debug)]
pub(crate) enum PartChangeError {
    Recovery(project::ProjectTransactionError),
    CreateFile(part::CreatePartError),
    AppendVariantsNeedsPart,
    EmptyVariantSuffix,
    CreateVariants {
        source: part::CreatePartError,
        rollback_errors: Vec<part::PartFileRollbackError>,
    },
    SaveVariants {
        source: project::SaveProjectError,
        rollback_errors: Vec<part::PartFileRollbackError>,
    },
    CombineNeedsTwoParts,
    CombinedPartTooLong,
    LoadCombinationScore {
        name: String,
        source: part::PartFileError,
    },
    SaveCombinedScore {
        source: part::ScoreError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
    ExportSelectionOutOfBounds,
    ExportScore {
        source: part::ScoreError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
    RenameFile(part::RenamePartError),
    DeleteFile(part::DeletePartError),
    MissingPart(String),
    PartInSequence {
        name: String,
        occurrence_count: usize,
    },
    SaveCreated {
        source: project::SaveProjectError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
    SaveDeleted {
        source: project::SaveProjectError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
    SaveRenamed {
        source: project::SaveProjectError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
}

#[derive(Debug)]
pub(crate) struct AppendedVariants {
    pub(crate) first: PartName,
    remaining: Vec<PartName>,
}

impl AppendedVariants {
    pub(crate) fn len(&self) -> usize {
        1 + self.remaining.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &PartName> {
        std::iter::once(&self.first).chain(&self.remaining)
    }
}

impl fmt::Display for PartChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
            Self::CreateFile(error) => write!(f, "{error}"),
            Self::AppendVariantsNeedsPart => {
                f.write_str("select at least one arranged part to append as a variant")
            }
            Self::EmptyVariantSuffix => f.write_str("variant suffix cannot be empty"),
            Self::CreateVariants {
                source,
                rollback_errors,
            } => write_variant_error(f, source, rollback_errors),
            Self::SaveVariants {
                source,
                rollback_errors,
            } => write_variant_error(f, source, rollback_errors),
            Self::CombineNeedsTwoParts => f.write_str("select at least two parts to combine"),
            Self::CombinedPartTooLong => f.write_str("the combined part has too many beats"),
            Self::LoadCombinationScore { name, source } => {
                write!(f, "couldn't read part {name:?}: {source}")
            }
            Self::SaveCombinedScore {
                source,
                rollback_error: None,
            } => write!(f, "{source}"),
            Self::SaveCombinedScore {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                f,
                "{source}; also failed to remove the incomplete combined part: {rollback_error}"
            ),
            Self::ExportSelectionOutOfBounds => f.write_str("the selected beats no longer exist"),
            Self::ExportScore {
                source,
                rollback_error: None,
            } => write!(f, "{source}"),
            Self::ExportScore {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                f,
                "{source}; also failed to remove the incomplete part file: {rollback_error}"
            ),
            Self::RenameFile(error) => write!(f, "{error}"),
            Self::DeleteFile(error) => write!(f, "{error}"),
            Self::MissingPart(name) => write!(f, "part {name:?} no longer exists"),
            Self::PartInSequence {
                name,
                occurrence_count,
            } => {
                let occurrence_label = if *occurrence_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                };
                write!(
                    f,
                    "remove {occurrence_count} {occurrence_label} of part {name:?} from the arrangement before deleting it"
                )
            }
            Self::SaveCreated {
                source,
                rollback_error: None,
            }
            | Self::SaveDeleted {
                source,
                rollback_error: None,
            }
            | Self::SaveRenamed {
                source,
                rollback_error: None,
            } => write!(f, "{source}"),
            Self::SaveCreated {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                f,
                "{source}; also failed to remove the new part file: {rollback_error}"
            ),
            Self::SaveDeleted {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                f,
                "{source}; also failed to restore the deleted part file: {rollback_error}"
            ),
            Self::SaveRenamed {
                source,
                rollback_error: Some(rollback_error),
            } => write!(
                f,
                "{source}; also failed to restore the renamed part file: {rollback_error}"
            ),
        }
    }
}

fn write_variant_error(
    f: &mut fmt::Formatter<'_>,
    source: &impl fmt::Display,
    rollback_errors: &[part::PartFileRollbackError],
) -> fmt::Result {
    write!(f, "{source}")?;
    if !rollback_errors.is_empty() {
        let rollback_errors = rollback_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        write!(
            f,
            "; also failed to remove incomplete variant files: {rollback_errors}"
        )?;
    }
    Ok(())
}

impl std::error::Error for PartChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::CreateFile(error) => Some(error),
            Self::CreateVariants { source, .. } => Some(source),
            Self::SaveVariants { source, .. } => Some(source),
            Self::LoadCombinationScore { source, .. } => Some(source),
            Self::SaveCombinedScore { source, .. } => Some(source),
            Self::ExportScore { source, .. } => Some(source),
            Self::RenameFile(error) => Some(error),
            Self::DeleteFile(error) => Some(error),
            Self::SaveCreated { source, .. }
            | Self::SaveDeleted { source, .. }
            | Self::SaveRenamed { source, .. } => Some(source),
            Self::AppendVariantsNeedsPart
            | Self::EmptyVariantSuffix
            | Self::CombineNeedsTwoParts
            | Self::CombinedPartTooLong
            | Self::ExportSelectionOutOfBounds
            | Self::MissingPart(_)
            | Self::PartInSequence { .. } => None,
        }
    }
}

#[derive(Debug)]
pub(crate) enum ArrangementChangeError {
    MissingPart(String),
    Save(project::SaveProjectError),
}

impl fmt::Display for ArrangementChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingPart(name) => write!(f, "part {name:?} no longer exists"),
            Self::Save(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ArrangementChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingPart(_) => None,
            Self::Save(error) => Some(error),
        }
    }
}

pub(crate) fn update_project_sequence(
    project_directory: &Path,
    project: &mut Project,
    sequence: Vec<PartName>,
) -> Result<Vec<PartName>, ArrangementChangeError> {
    let sequence = sequence
        .into_iter()
        .map(|name| {
            project
                .part(&name)
                .map(|part| part.name.clone())
                .ok_or_else(|| ArrangementChangeError::MissingPart(name.as_str().to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let original_sequence = project.sequence().to_vec();
    project.set_sequence(sequence.clone());

    if let Err(error) = project::save_project(project_directory, project) {
        project.set_sequence(original_sequence);
        return Err(ArrangementChangeError::Save(error));
    }

    Ok(sequence)
}

#[cfg(test)]
pub(crate) fn create_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &str,
    length: u32,
) -> Result<part::Part, PartChangeError> {
    create_configured_project_part(project_directory, project, name, length, None)
}

#[cfg(test)]
pub(crate) fn create_configured_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &str,
    length: u32,
    subdivision_pattern: Option<SubdivisionPattern>,
) -> Result<part::Part, PartChangeError> {
    create_configured_project_part_with_major(
        project_directory,
        project,
        name,
        length,
        subdivision_pattern,
        None,
    )
}

pub(crate) fn create_configured_project_part_with_major(
    project_directory: &Path,
    project: &mut Project,
    name: &str,
    length: u32,
    subdivision_pattern: Option<SubdivisionPattern>,
    major_subdivision: Option<MajorSubdivision>,
) -> Result<part::Part, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let created = part::create_part_file(
        project_directory,
        &project.parts,
        project.voices(),
        name,
        length,
    )
    .map_err(PartChangeError::CreateFile)?;
    let part = created
        .part()
        .clone()
        .with_subdivision_pattern(subdivision_pattern)
        .with_major_subdivision(major_subdivision);
    project.add_part(part.clone());

    if let Err(source) = project::save_project(project_directory, project) {
        project.remove_part(&part.name);
        return Err(PartChangeError::SaveCreated {
            source,
            rollback_error: created.rollback().err(),
        });
    }

    created.commit();
    Ok(part)
}

pub(crate) fn duplicate_project_part(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
) -> Result<part::Part, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let source_part = project
        .part(source_name)
        .cloned()
        .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))?;
    let created = part::duplicate_part_file(project_directory, &project.parts, &source_part, name)
        .map_err(PartChangeError::CreateFile)?;
    let part = created.part().clone();
    project.add_part(part.clone());

    if let Err(source) = project::save_project(project_directory, project) {
        project.remove_part(&part.name);
        return Err(PartChangeError::SaveCreated {
            source,
            rollback_error: created.rollback().err(),
        });
    }

    Ok(created.commit())
}

pub(crate) fn append_project_variants(
    project_directory: &Path,
    project: &mut Project,
    sources: &[PartName],
    suffix: &str,
) -> Result<AppendedVariants, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    if sources.is_empty() {
        return Err(PartChangeError::AppendVariantsNeedsPart);
    }
    let suffix = suffix.trim();
    if suffix.is_empty() {
        return Err(PartChangeError::EmptyVariantSuffix);
    }

    let mut mappings = Vec::<(part::Part, PartName)>::new();
    for source_name in sources {
        if mappings
            .iter()
            .any(|(source, _)| source.name.eq_ignore_ascii_case(source_name))
        {
            continue;
        }
        let source = project
            .part(source_name)
            .cloned()
            .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))?;
        let variant_name = variant_part_name(&source.name, suffix);
        mappings.push((source, variant_name));
    }
    let appended = sources
        .iter()
        .map(|source_name| {
            mappings
                .iter()
                .find(|(source, _)| source.name.eq_ignore_ascii_case(source_name))
                .map(|(_, variant_name)| variant_name.clone())
                .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut appended = appended.into_iter();
    let Some(first_appended) = appended.next() else {
        return Err(PartChangeError::AppendVariantsNeedsPart);
    };
    let appended = AppendedVariants {
        first: first_appended,
        remaining: appended.collect(),
    };

    let mut reserved_parts = project.parts().to_vec();
    let mut created_files = Vec::new();
    for (source, variant_name) in &mappings {
        match part::duplicate_part_file(
            project_directory,
            &reserved_parts,
            source,
            variant_name.as_str(),
        ) {
            Ok(created) => {
                reserved_parts.push(created.part().clone());
                created_files.push(created);
            }
            Err(source) => {
                return Err(PartChangeError::CreateVariants {
                    source,
                    rollback_errors: rollback_created_part_files(created_files),
                });
            }
        }
    }

    let original_sequence = project.sequence().to_vec();
    for created in &created_files {
        project.add_part(created.part().clone());
    }
    let mut updated_sequence = original_sequence.clone();
    updated_sequence.extend(appended.iter().cloned());
    project.set_sequence(updated_sequence);

    if let Err(source) = project::save_project(project_directory, project) {
        project.set_sequence(original_sequence);
        for (_, variant_name) in mappings.iter().rev() {
            project.remove_part(variant_name);
        }
        return Err(PartChangeError::SaveVariants {
            source,
            rollback_errors: rollback_created_part_files(created_files),
        });
    }

    for created in created_files {
        created.commit();
    }
    Ok(appended)
}

fn rollback_created_part_files(
    created_files: Vec<part::CreatedPartFile>,
) -> Vec<part::PartFileRollbackError> {
    created_files
        .into_iter()
        .rev()
        .filter_map(|created| created.rollback().err())
        .collect()
}

pub(crate) fn combine_project_parts(
    project_directory: &Path,
    project: &mut Project,
    sources: &[PartName],
    name: &str,
) -> Result<part::Part, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    if sources.len() < 2 {
        return Err(PartChangeError::CombineNeedsTwoParts);
    }
    let source_parts = sources
        .iter()
        .map(|source_name| {
            project
                .part(source_name)
                .cloned()
                .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let combined_length = source_parts
        .iter()
        .try_fold(0_u32, |length, part| length.checked_add(part.length));
    let combined_length = combined_length.ok_or(PartChangeError::CombinedPartTooLong)?;
    let scores = source_parts
        .iter()
        .map(|source_part| {
            PartScore::load(project_directory, source_part, project.voices()).map_err(|source| {
                PartChangeError::LoadCombinationScore {
                    name: source_part.name.as_str().to_string(),
                    source,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let combined_score = PartScore::from_rows(
        scores
            .into_iter()
            .flat_map(|score| score.rows().to_vec())
            .collect(),
    );
    let created = part::create_part_file(
        project_directory,
        &project.parts,
        project.voices(),
        name,
        combined_length,
    )
    .map_err(PartChangeError::CreateFile)?;
    let combined_part = created
        .part()
        .clone()
        .with_subdivision_pattern(combined_subdivision_pattern(&source_parts))
        .with_major_subdivision(combined_major_subdivision(&source_parts));
    if let Err(source) = combined_score.save(project_directory, &combined_part, project) {
        return Err(PartChangeError::SaveCombinedScore {
            source,
            rollback_error: created.rollback().err(),
        });
    }

    project.add_part(combined_part.clone());
    if let Err(source) = project::save_project(project_directory, project) {
        project.remove_part(&combined_part.name);
        return Err(PartChangeError::SaveCreated {
            source,
            rollback_error: created.rollback().err(),
        });
    }

    created.commit();
    Ok(combined_part)
}

pub(crate) fn export_project_part_rows(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    source_score: &PartScore,
    rows: part::ScoreRowRange,
    name: &str,
) -> Result<part::Part, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let source_part = project
        .part(source_name)
        .cloned()
        .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))?;
    let rows = part::ScoreRowRange::new(rows.first(), rows.last(), source_score.rows().len())
        .ok_or(PartChangeError::ExportSelectionOutOfBounds)?;
    let exported_score =
        PartScore::from_rows(source_score.rows()[rows.first()..=rows.last()].to_vec());
    let length =
        u32::try_from(rows.len()).expect("a selection from a u32-length part always fits in u32");
    let created = part::create_part_file(
        project_directory,
        &project.parts,
        project.voices(),
        name,
        length,
    )
    .map_err(PartChangeError::CreateFile)?;
    let exported_part = created
        .part()
        .clone()
        .with_subdivision_pattern(source_part.subdivision_pattern().cloned())
        .with_major_subdivision(source_part.major_subdivision());

    if let Err(source) = exported_score.save(project_directory, &exported_part, project) {
        return Err(PartChangeError::ExportScore {
            source,
            rollback_error: created.rollback().err(),
        });
    }

    project.add_part(exported_part.clone());
    if let Err(source) = project::save_project(project_directory, project) {
        project.remove_part(&exported_part.name);
        return Err(PartChangeError::SaveCreated {
            source,
            rollback_error: created.rollback().err(),
        });
    }

    created.commit();
    Ok(exported_part)
}

#[cfg(test)]
pub(crate) fn rename_project_part(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
) -> Result<part::Part, PartChangeError> {
    let subdivision_pattern = project
        .part(source_name)
        .and_then(|part| part.subdivision_pattern().cloned());
    update_project_part(
        project_directory,
        project,
        source_name,
        name,
        subdivision_pattern,
    )
}

#[cfg(test)]
pub(crate) fn update_project_part(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
    subdivision_pattern: Option<SubdivisionPattern>,
) -> Result<part::Part, PartChangeError> {
    let major_subdivision = project
        .part(source_name)
        .and_then(part::Part::major_subdivision);
    update_project_part_settings(
        project_directory,
        project,
        source_name,
        name,
        subdivision_pattern,
        major_subdivision,
    )
}

pub(crate) fn update_project_part_settings(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
    subdivision_pattern: Option<SubdivisionPattern>,
    major_subdivision: Option<MajorSubdivision>,
) -> Result<part::Part, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let index = project
        .parts
        .iter()
        .position(|part| part.name.eq_ignore_ascii_case(source_name))
        .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))?;
    let source_part = project.parts[index].clone();
    let renamed = part::rename_part_file(project_directory, &project.parts, &source_part, name)
        .map_err(PartChangeError::RenameFile)?;
    let renamed_part = renamed
        .part()
        .clone()
        .with_subdivision_pattern(subdivision_pattern)
        .with_major_subdivision(major_subdivision);
    let original_sequence = project.sequence().to_vec();
    let updated_sequence = original_sequence
        .iter()
        .map(|part_name| {
            if part_name.eq_ignore_ascii_case(&source_part.name) {
                renamed_part.name.clone()
            } else {
                part_name.clone()
            }
        })
        .collect();
    project.parts[index] = renamed_part.clone();
    project.set_sequence(updated_sequence);

    if let Err(source) = project::save_project(project_directory, project) {
        project.parts[index] = source_part;
        project.set_sequence(original_sequence);
        return Err(PartChangeError::SaveRenamed {
            source,
            rollback_error: renamed.rollback().err(),
        });
    }

    renamed.commit();
    Ok(renamed_part)
}

pub(crate) fn delete_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &part::PartName,
) -> Result<part::Part, PartChangeError> {
    super::transaction::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let index = project
        .parts
        .iter()
        .position(|part| part.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| PartChangeError::MissingPart(name.as_str().to_string()))?;
    let occurrence_count = project
        .sequence()
        .iter()
        .filter(|part_name| part_name.eq_ignore_ascii_case(name))
        .count();
    if occurrence_count > 0 {
        return Err(PartChangeError::PartInSequence {
            name: project.parts[index].name.as_str().to_string(),
            occurrence_count,
        });
    }
    let deleted = part::soft_delete_part_file(project_directory, &project.parts[index])
        .map_err(PartChangeError::DeleteFile)?;
    let removed_part = project.parts.remove(index);

    if let Err(source) = project::save_project(project_directory, project) {
        project.parts.insert(index, removed_part);
        return Err(PartChangeError::SaveDeleted {
            source,
            rollback_error: deleted.rollback().err(),
        });
    }

    Ok(deleted.commit())
}

pub(crate) fn variant_part_name(source: &PartName, suffix: &str) -> PartName {
    PartName::new(format!("{} {}", source.as_str(), suffix.trim()))
}

pub(crate) fn combined_subdivision_pattern(parts: &[Part]) -> Option<SubdivisionPattern> {
    let first = parts.first()?.subdivision_pattern()?;
    parts
        .iter()
        .all(|part| part.subdivision_pattern() == Some(first))
        .then(|| first.clone())
}

pub(crate) fn combined_major_subdivision(parts: &[Part]) -> Option<MajorSubdivision> {
    let first = parts.first()?.major_subdivision()?;
    parts
        .iter()
        .all(|part| part.major_subdivision() == Some(first))
        .then_some(first)
}
