use std::{
    error::Error,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    num::NonZeroU32,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

use crate::{
    pitch_system::{FrequencyHz, ResolvePitchError},
    project::{self, Project, Voice},
};

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
    #[serde(default)]
    subdivision_pattern: Option<SubdivisionPattern>,
}

impl Part {
    pub fn new(name: impl Into<PartName>, length: u32) -> Self {
        Self {
            name: name.into(),
            length,
            subdivision_pattern: None,
        }
    }

    pub fn with_subdivision_pattern(
        mut self,
        subdivision_pattern: Option<SubdivisionPattern>,
    ) -> Self {
        self.subdivision_pattern = subdivision_pattern;
        self
    }

    pub fn subdivision_pattern(&self) -> Option<&SubdivisionPattern> {
        self.subdivision_pattern.as_ref()
    }

    pub fn beat_label(&self, beat_index: usize) -> String {
        self.subdivision_pattern.as_ref().map_or_else(
            || (beat_index + 1).to_string(),
            |pattern| pattern.beat_position(beat_index).label(),
        )
    }

    pub fn beat_is_highlighted(&self, beat_index: usize) -> bool {
        self.subdivision_pattern
            .as_ref()
            .is_some_and(|pattern| pattern.beat_position(beat_index).group_index % 2 == 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<u32>")]
pub struct SubdivisionPattern(Vec<NonZeroU32>);

impl SubdivisionPattern {
    pub fn new(
        subdivisions: impl IntoIterator<Item = u32>,
    ) -> Result<Self, InvalidSubdivisionPattern> {
        let subdivisions = subdivisions
            .into_iter()
            .map(|subdivision| {
                NonZeroU32::new(subdivision).ok_or(InvalidSubdivisionPattern::ZeroSubdivision)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if subdivisions.is_empty() {
            return Err(InvalidSubdivisionPattern::Empty);
        }

        Ok(Self(subdivisions))
    }

    pub fn subdivisions(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.iter().map(|subdivision| subdivision.get())
    }

    fn beat_position(&self, beat_index: usize) -> SubdivisionPosition {
        let cycle_length = self
            .0
            .iter()
            .map(|subdivision| u64::from(subdivision.get()))
            .sum::<u64>();
        let beat_index = beat_index as u64;
        let cycle = beat_index / cycle_length;
        let mut offset_in_cycle = beat_index % cycle_length;

        for (pattern_index, subdivision_count) in self.0.iter().enumerate() {
            let subdivision_count = u64::from(subdivision_count.get());
            if offset_in_cycle < subdivision_count {
                return SubdivisionPosition {
                    group_index: cycle * self.0.len() as u64 + pattern_index as u64,
                    subdivision_index: offset_in_cycle,
                };
            }
            offset_in_cycle -= subdivision_count;
        }

        unreachable!("a beat within a non-empty subdivision cycle always has a position")
    }
}

impl fmt::Display for SubdivisionPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, subdivision) in self.subdivisions().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "{subdivision}")?;
        }
        Ok(())
    }
}

impl FromStr for SubdivisionPattern {
    type Err = InvalidSubdivisionPattern;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.trim().is_empty() {
            return Err(InvalidSubdivisionPattern::Empty);
        }
        let subdivisions = value
            .split(',')
            .map(|item| {
                let item = item.trim();
                if item.is_empty() {
                    return Err(InvalidSubdivisionPattern::MissingSubdivision);
                }
                item.parse::<u32>()
                    .map_err(|_| InvalidSubdivisionPattern::NotWholeNumber(item.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(subdivisions)
    }
}

impl TryFrom<Vec<u32>> for SubdivisionPattern {
    type Error = InvalidSubdivisionPattern;

    fn try_from(subdivisions: Vec<u32>) -> Result<Self, Self::Error> {
        Self::new(subdivisions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidSubdivisionPattern {
    Empty,
    MissingSubdivision,
    NotWholeNumber(String),
    ZeroSubdivision,
}

impl fmt::Display for InvalidSubdivisionPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("subdivision pattern must not be empty"),
            Self::MissingSubdivision => {
                formatter.write_str("each comma must be followed by a subdivision")
            }
            Self::NotWholeNumber(value) => {
                write!(formatter, "subdivision {value:?} must be a whole number")
            }
            Self::ZeroSubdivision => formatter.write_str("subdivisions must be at least one beat"),
        }
    }
}

impl Error for InvalidSubdivisionPattern {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SubdivisionPosition {
    group_index: u64,
    subdivision_index: u64,
}

impl SubdivisionPosition {
    fn label(self) -> String {
        format!("{}.{}", self.group_index + 1, self.subdivision_index + 1)
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

pub(crate) fn recovery_file_name(name: &PartName) -> Result<String, InvalidPartName> {
    csv_file_name(name).map(|file_name| format!(".{file_name}.recovery"))
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

/// A successful score-file rename that has not yet been committed to project
/// metadata. The caller can keep the renamed part with `commit`, or use the
/// remembered paths to restore the original file if saving the project fails.
pub struct RenamedPartFile {
    part: Part,
    original_path: PathBuf,
    renamed_path: PathBuf,
}

impl RenamedPartFile {
    pub fn part(&self) -> &Part {
        &self.part
    }

    pub fn commit(self) -> Part {
        self.part
    }

    pub fn rollback(self) -> Result<(), PartFileRollbackError> {
        if self.original_path == self.renamed_path {
            return Ok(());
        }

        fs::rename(&self.renamed_path, &self.original_path).map_err(|source| {
            PartFileRollbackError::RestoreRenamed {
                renamed_path: self.renamed_path,
                original_path: self.original_path,
                source,
            }
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
    RestoreRenamed {
        renamed_path: PathBuf,
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
            Self::RestoreRenamed {
                renamed_path,
                original_path,
                source,
            } => write!(
                f,
                "failed to restore {} to {}: {source}",
                renamed_path.display(),
                original_path.display()
            ),
        }
    }
}

impl Error for PartFileRollbackError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RemoveCreated { source, .. }
            | Self::RestoreDeleted { source, .. }
            | Self::RestoreRenamed { source, .. } => Some(source),
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

#[derive(Debug)]
pub enum RenamePartError {
    EmptyName,
    DuplicateName(String),
    CsvAlreadyExists(PathBuf),
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for RenamePartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => write!(f, "part name must contain a letter or number"),
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

impl Error for RenamePartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
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

#[derive(Debug)]
pub enum PartFileError {
    Io { path: PathBuf, source: io::Error },
    Csv { path: PathBuf, source: csv::Error },
    Invalid { path: PathBuf, message: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartScore {
    rows: Vec<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoreRowIndex(usize);

impl ScoreRowIndex {
    pub fn new(index: usize, row_count: usize) -> Option<Self> {
        (index < row_count).then_some(Self(index))
    }

    pub fn value(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScoreRowRange {
    first: ScoreRowIndex,
    last: ScoreRowIndex,
}

impl ScoreRowRange {
    pub fn new(first: usize, last: usize, row_count: usize) -> Option<Self> {
        (first <= last).then_some(Self {
            first: ScoreRowIndex::new(first, row_count)?,
            last: ScoreRowIndex::new(last, row_count)?,
        })
    }

    pub fn first(self) -> usize {
        self.first.value()
    }

    pub fn last(self) -> usize {
        self.last.value()
    }

    pub fn len(self) -> usize {
        self.last() - self.first() + 1
    }

    pub fn contains(self, row: usize) -> bool {
        (self.first()..=self.last()).contains(&row)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartRowEdit {
    InsertBefore(ScoreRowIndex),
    InsertAfter(ScoreRowIndex),
    Clear(ScoreRowRange),
    Delete(ScoreRowRange),
}

impl PartRowEdit {
    pub fn selected_rows(self, row_count: usize) -> Option<ScoreRowRange> {
        match self {
            Self::InsertBefore(row) | Self::InsertAfter(row) => {
                ScoreRowRange::new(row.value(), row.value(), row_count)
            }
            Self::Clear(rows) | Self::Delete(rows) => {
                ScoreRowRange::new(rows.first(), rows.last(), row_count)
            }
        }
    }

    pub fn selection_after(self, original_row_count: usize) -> Option<ScoreRowRange> {
        match self {
            Self::InsertBefore(row) => {
                ScoreRowRange::new(row.value(), row.value(), original_row_count.checked_add(1)?)
            }
            Self::InsertAfter(row) => {
                let inserted = row.value().checked_add(1)?;
                ScoreRowRange::new(inserted, inserted, original_row_count.checked_add(1)?)
            }
            Self::Clear(rows) => ScoreRowRange::new(rows.first(), rows.last(), original_row_count),
            Self::Delete(rows) => {
                let remaining = original_row_count.checked_sub(rows.len())?;
                if remaining == 0 {
                    return None;
                }
                let selected = rows.first().min(remaining - 1);
                ScoreRowRange::new(selected, selected, remaining)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartRowEditError {
    SelectionOutOfBounds,
    WouldDeleteEveryRow,
}

impl fmt::Display for PartRowEditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectionOutOfBounds => formatter.write_str("the selected beats no longer exist"),
            Self::WouldDeleteEveryRow => {
                formatter.write_str("a part must keep at least one beat; clear the rows instead")
            }
        }
    }
}

impl Error for PartRowEditError {}

impl PartScore {
    pub fn from_rows(rows: Vec<Vec<String>>) -> Self {
        Self { rows }
    }

    pub fn load(
        project_directory: impl AsRef<Path>,
        part: &Part,
        voices: &[Voice],
    ) -> Result<Self, PartFileError> {
        read_part_table(project_directory.as_ref(), part, voices)
            .map(|table| Self { rows: table.rows })
    }

    pub fn load_with_recovery(
        project_directory: impl AsRef<Path>,
        part: &Part,
        voices: &[Voice],
    ) -> Result<(Self, bool), PartFileError> {
        let project_directory = project_directory.as_ref();
        let saved_score = Self::load(project_directory, part, voices)?;
        let recovery_path = part_recovery_path(project_directory, part);
        let recovery_exists = recovery_path
            .try_exists()
            .map_err(|source| PartFileError::Io {
                path: recovery_path.clone(),
                source,
            })?;
        if !recovery_exists {
            return Ok((saved_score, false));
        }

        let recovery_score = read_part_table_at_path(recovery_path.clone(), part, voices)
            .map(|table| Self { rows: table.rows })?;
        if recovery_score == saved_score {
            fs::remove_file(&recovery_path).ok();
            return Ok((saved_score, false));
        }

        Ok((recovery_score, true))
    }

    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    pub fn edited_rows(
        &self,
        edit: PartRowEdit,
        voice_count: usize,
    ) -> Result<Self, PartRowEditError> {
        let row_count = self.rows.len();
        let selected = edit
            .selected_rows(row_count)
            .ok_or(PartRowEditError::SelectionOutOfBounds)?;
        let mut rows = self.rows.clone();

        match edit {
            PartRowEdit::InsertBefore(_) => {
                rows.insert(selected.first(), vec![String::new(); voice_count]);
            }
            PartRowEdit::InsertAfter(_) => {
                rows.insert(selected.last() + 1, vec![String::new(); voice_count]);
            }
            PartRowEdit::Clear(_) => {
                for row in &mut rows[selected.first()..=selected.last()] {
                    row.fill(String::new());
                }
            }
            PartRowEdit::Delete(_) => {
                if selected.len() == row_count {
                    return Err(PartRowEditError::WouldDeleteEveryRow);
                }
                rows.drain(selected.first()..=selected.last());
            }
        }

        Ok(Self::from_rows(rows))
    }

    pub fn resolved_rows(
        &self,
        part: &Part,
        project: &Project,
    ) -> Result<Vec<Vec<Option<FrequencyHz>>>, ScoreError> {
        self.validate_shape(part, project.voices())?;

        self.rows
            .iter()
            .enumerate()
            .map(|(beat_index, row)| {
                row.iter()
                    .enumerate()
                    .map(|(voice_index, value)| {
                        project
                            .pitch_system()
                            .resolve_cell(value)
                            .map_err(|source| ScoreError::InvalidPitch {
                                beat: beat_index + 1,
                                voice: project.voices()[voice_index].name.as_str().to_string(),
                                source,
                            })
                    })
                    .collect()
            })
            .collect()
    }

    pub fn save(
        &self,
        project_directory: impl AsRef<Path>,
        part: &Part,
        project: &Project,
    ) -> Result<(), ScoreError> {
        let contents = self.validated_contents(project_directory.as_ref(), part, project)?;
        atomic_write_part_score(project_directory.as_ref(), part, &contents)
    }

    pub(crate) fn validated_contents(
        &self,
        project_directory: &Path,
        part: &Part,
        project: &Project,
    ) -> Result<Vec<u8>, ScoreError> {
        self.resolved_rows(part, project)?;
        self.score_file_contents(project_directory, part, project.voices())
    }

    pub(crate) fn score_file_contents(
        &self,
        project_directory: &Path,
        part: &Part,
        voices: &[Voice],
    ) -> Result<Vec<u8>, ScoreError> {
        self.validate_shape(part, voices)?;
        serialize_part_table(voices, &self.rows).map_err(|source| ScoreError::Csv {
            path: part_file_path(project_directory, part),
            source,
        })
    }

    pub fn save_recovery(
        &self,
        project_directory: impl AsRef<Path>,
        part: &Part,
        voices: &[Voice],
    ) -> Result<(), ScoreError> {
        let project_directory = project_directory.as_ref();
        let path = part_recovery_path(project_directory, part);
        let contents = self.recovery_contents(project_directory, part, voices)?;
        atomic_write_score(project_directory, &path, &contents)
    }

    pub(crate) fn recovery_contents(
        &self,
        project_directory: &Path,
        part: &Part,
        voices: &[Voice],
    ) -> Result<Vec<u8>, ScoreError> {
        self.validate_shape(part, voices)?;
        let path = part_recovery_path(project_directory, part);
        serialize_part_table(voices, &self.rows).map_err(|source| ScoreError::Csv { path, source })
    }

    pub fn clear_recovery(
        project_directory: impl AsRef<Path>,
        part: &Part,
    ) -> Result<(), ScoreError> {
        let project_directory = project_directory.as_ref();
        let path = part_recovery_path(project_directory, part);
        match fs::remove_file(&path) {
            Ok(()) => sync_directory(project_directory),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(ScoreError::Io { path, source }),
        }
    }

    fn validate_shape(&self, part: &Part, voices: &[Voice]) -> Result<(), ScoreError> {
        if self.rows.len() != part.length as usize {
            return Err(ScoreError::InvalidShape {
                message: format!(
                    "score has {} beat rows; expected {}",
                    self.rows.len(),
                    part.length
                ),
            });
        }

        if let Some((row_index, row)) = self
            .rows
            .iter()
            .enumerate()
            .find(|(_, row)| row.len() != voices.len())
        {
            return Err(ScoreError::InvalidShape {
                message: format!(
                    "beat row {} has {} voice cells; expected {}",
                    row_index + 1,
                    row.len(),
                    voices.len()
                ),
            });
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum ScoreError {
    InvalidShape {
        message: String,
    },
    InvalidPitch {
        beat: usize,
        voice: String,
        source: ResolvePitchError,
    },
    Csv {
        path: PathBuf,
        source: csv::Error,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for ScoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShape { message } => formatter.write_str(message),
            Self::InvalidPitch {
                beat,
                voice,
                source,
            } => write!(formatter, "beat {beat}, voice {voice:?}: {source}"),
            Self::Csv { path, source } => {
                write!(
                    formatter,
                    "failed to encode score at {}: {source}",
                    path.display()
                )
            }
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "filesystem error at {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ScoreError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPitch { source, .. } => Some(source),
            Self::Csv { source, .. } => Some(source),
            Self::Io { source, .. } => Some(source),
            Self::InvalidShape { .. } => None,
        }
    }
}

impl fmt::Display for PartFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "filesystem error at {}: {source}", path.display())
            }
            Self::Csv { path, source } => {
                write!(f, "invalid CSV at {}: {source}", path.display())
            }
            Self::Invalid { path, message } => {
                write!(f, "invalid part file {}: {message}", path.display())
            }
        }
    }
}

impl Error for PartFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Csv { source, .. } => Some(source),
            Self::Invalid { .. } => None,
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

pub fn duplicate_part_file(
    project_directory: impl AsRef<Path>,
    parts: &[Part],
    source_part: &Part,
    name: &str,
) -> Result<CreatedPartFile, CreatePartError> {
    let name = name.trim();
    let part_name = PartName::new(name);
    let file_name = csv_file_name(&part_name).map_err(|_| CreatePartError::EmptyName)?;
    if parts
        .iter()
        .any(|part| part.name.eq_ignore_ascii_case(&part_name))
    {
        return Err(CreatePartError::DuplicateName(name.to_string()));
    }

    let project_directory = project_directory.as_ref();
    let source_path = part_file_path(project_directory, source_part);
    let contents = fs::read(&source_path).map_err(|source| CreatePartError::Io {
        path: source_path,
        source,
    })?;
    let path = project_directory.join(file_name);
    let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Err(CreatePartError::CsvAlreadyExists(path));
        }
        Err(source) => return Err(CreatePartError::Io { path, source }),
    };

    if let Err(source) = file.write_all(&contents) {
        drop(file);
        fs::remove_file(&path).ok();
        return Err(CreatePartError::Io { path, source });
    }
    drop(file);

    Ok(CreatedPartFile {
        part: Part::new(part_name, source_part.length)
            .with_subdivision_pattern(source_part.subdivision_pattern().cloned()),
        path,
    })
}

pub fn rename_part_file(
    project_directory: impl AsRef<Path>,
    parts: &[Part],
    source_part: &Part,
    name: &str,
) -> Result<RenamedPartFile, RenamePartError> {
    let name = name.trim();
    let part_name = PartName::new(name);
    let file_name = csv_file_name(&part_name).map_err(|_| RenamePartError::EmptyName)?;
    if parts.iter().any(|part| {
        !part.name.eq_ignore_ascii_case(&source_part.name)
            && part.name.eq_ignore_ascii_case(&part_name)
    }) {
        return Err(RenamePartError::DuplicateName(name.to_string()));
    }

    let project_directory = project_directory.as_ref();
    let original_path = part_file_path(project_directory, source_part);
    let renamed_path = project_directory.join(file_name);
    original_path
        .metadata()
        .map_err(|source| RenamePartError::Io {
            path: original_path.clone(),
            source,
        })?;
    if original_path != renamed_path {
        match renamed_path.try_exists() {
            Ok(true) => return Err(RenamePartError::CsvAlreadyExists(renamed_path)),
            Ok(false) => {}
            Err(source) => {
                return Err(RenamePartError::Io {
                    path: renamed_path,
                    source,
                });
            }
        }
        fs::rename(&original_path, &renamed_path).map_err(|source| RenamePartError::Io {
            path: original_path.clone(),
            source,
        })?;
    }

    Ok(RenamedPartFile {
        part: Part::new(part_name, source_part.length)
            .with_subdivision_pattern(source_part.subdivision_pattern().cloned()),
        original_path,
        renamed_path,
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

struct PartTable {
    contents: Vec<u8>,
    rows: Vec<Vec<String>>,
}

pub(crate) fn validate_part_file(
    project_directory: &Path,
    part: &Part,
    voices: &[Voice],
) -> Result<(), PartFileError> {
    read_part_table(project_directory, part, voices).map(|_| ())
}

pub(crate) fn rewritten_part_file(
    project_directory: &Path,
    part: &Part,
    old_voices: &[Voice],
    new_voices: &[Voice],
) -> Result<Vec<u8>, PartFileError> {
    let table = read_part_table(project_directory, part, old_voices)?;
    let schema_is_unchanged = old_voices.len() == new_voices.len()
        && old_voices
            .iter()
            .zip(new_voices)
            .all(|(old, new)| old.id() == new.id() && old.name.as_str() == new.name.as_str());
    if schema_is_unchanged {
        return Ok(table.contents);
    }

    let rows = table
        .rows
        .iter()
        .map(|old_row| {
            new_voices
                .iter()
                .map(|new_voice| {
                    old_voices
                        .iter()
                        .position(|old_voice| old_voice.id() == new_voice.id())
                        .map(|old_index| old_row[old_index].clone())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    serialize_part_table(new_voices, &rows).map_err(|source| PartFileError::Csv {
        path: part_file_path(project_directory, part),
        source,
    })
}

fn read_part_table(
    project_directory: &Path,
    part: &Part,
    voices: &[Voice],
) -> Result<PartTable, PartFileError> {
    let path = part_file_path(project_directory, part);
    read_part_table_at_path(path, part, voices)
}

fn read_part_table_at_path(
    path: PathBuf,
    part: &Part,
    voices: &[Voice],
) -> Result<PartTable, PartFileError> {
    let contents = fs::read(&path).map_err(|source| PartFileError::Io {
        path: path.clone(),
        source,
    })?;

    if voices.is_empty() {
        let unix_contents = "\n".repeat(part.length as usize + 1).into_bytes();
        let windows_contents = "\r\n".repeat(part.length as usize + 1).into_bytes();
        if contents != unix_contents && contents != windows_contents {
            return Err(PartFileError::Invalid {
                path,
                message: format!("expected zero columns and {} beat rows", part.length),
            });
        }
        return Ok(PartTable {
            contents,
            rows: vec![Vec::new(); part.length as usize],
        });
    }

    let normalized_contents;
    let csv_contents = if voices.len() == 1 {
        normalized_contents = quote_blank_single_column_records(&contents);
        normalized_contents.as_slice()
    } else {
        contents.as_slice()
    };
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_contents);
    let mut records = reader.records();
    let headers = records
        .next()
        .transpose()
        .map_err(|source| PartFileError::Csv {
            path: path.clone(),
            source,
        })?
        .ok_or_else(|| PartFileError::Invalid {
            path: path.clone(),
            message: "missing voice header row".to_string(),
        })?;

    if headers.len() != voices.len()
        || headers
            .iter()
            .zip(voices)
            .any(|(header, voice)| header != voice.name.as_str())
    {
        let expected = voices
            .iter()
            .map(|voice| voice.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let actual = headers.iter().collect::<Vec<_>>().join(", ");
        return Err(PartFileError::Invalid {
            path,
            message: format!(
                "voice headers do not match the project (expected [{expected}], found [{actual}])"
            ),
        });
    }

    let mut rows = Vec::new();
    for (row_index, result) in records.enumerate() {
        let record = result.map_err(|source| PartFileError::Csv {
            path: path.clone(),
            source,
        })?;
        let row = if record.is_empty() && voices.len() == 1 {
            vec![String::new()]
        } else {
            record.iter().map(str::to_string).collect::<Vec<_>>()
        };
        if row.len() != voices.len() {
            return Err(PartFileError::Invalid {
                path,
                message: format!(
                    "beat row {} has {} columns; expected {}",
                    row_index + 1,
                    row.len(),
                    voices.len()
                ),
            });
        }
        rows.push(row);
    }

    if rows.len() != part.length as usize {
        return Err(PartFileError::Invalid {
            path,
            message: format!(
                "contains {} beat rows; expected {}",
                rows.len(),
                part.length
            ),
        });
    }

    Ok(PartTable { contents, rows })
}

fn quote_blank_single_column_records(contents: &[u8]) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(contents.len());
    let mut in_quotes = false;
    let mut at_record_start = true;
    let mut index = 0;

    while index < contents.len() {
        let byte = contents[index];
        if in_quotes {
            normalized.push(byte);
            if byte == b'"' {
                if contents.get(index + 1) == Some(&b'"') {
                    normalized.push(b'"');
                    index += 1;
                } else {
                    in_quotes = false;
                }
            }
            index += 1;
            continue;
        }

        if at_record_start && (byte == b'\n' || byte == b'\r') {
            normalized.extend_from_slice(b"\"\"");
        }
        normalized.push(byte);

        if byte == b'"' && at_record_start {
            in_quotes = true;
            at_record_start = false;
        } else if byte == b'\n' || byte == b'\r' {
            at_record_start = true;
            if byte == b'\r' && contents.get(index + 1) == Some(&b'\n') {
                normalized.push(b'\n');
                index += 1;
            }
        } else {
            at_record_start = false;
        }
        index += 1;
    }

    normalized
}

fn serialize_part_table(voices: &[Voice], rows: &[Vec<String>]) -> Result<Vec<u8>, csv::Error> {
    if voices.is_empty() {
        return Ok("\n".repeat(rows.len() + 1).into_bytes());
    }

    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    writer.write_record(voices.iter().map(|voice| voice.name.as_str()))?;
    for row in rows {
        writer.write_record(row)?;
    }
    writer.flush().map_err(csv::Error::from)?;
    writer
        .into_inner()
        .map_err(|error| csv::Error::from(error.into_error()))
}

fn part_file_path(project_directory: &Path, part: &Part) -> PathBuf {
    let file_name = csv_file_name(&part.name)
        .expect("validated project part names always produce CSV filenames");
    project_directory.join(file_name)
}

fn part_recovery_path(project_directory: &Path, part: &Part) -> PathBuf {
    let file_name = recovery_file_name(&part.name)
        .expect("validated project part names always produce CSV filenames");
    project_directory.join(file_name)
}

fn atomic_write_part_score(
    project_directory: &Path,
    part: &Part,
    contents: &[u8],
) -> Result<(), ScoreError> {
    let path = part_file_path(project_directory, part);
    atomic_write_score(project_directory, &path, contents)
}

fn atomic_write_score(
    project_directory: &Path,
    path: &Path,
    contents: &[u8],
) -> Result<(), ScoreError> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("validated part paths always have UTF-8 filenames");
    let pending_file_name = if file_name.starts_with('.') {
        format!("{file_name}.pending")
    } else {
        format!(".{file_name}.pending")
    };
    let pending_path = project_directory.join(pending_file_name);
    let mut pending = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&pending_path)
        .map_err(|source| ScoreError::Io {
            path: pending_path.clone(),
            source,
        })?;

    let write_result = pending.write_all(contents).and_then(|_| pending.sync_all());
    drop(pending);
    if let Err(source) = write_result {
        fs::remove_file(&pending_path).ok();
        return Err(ScoreError::Io {
            path: pending_path,
            source,
        });
    }

    if let Err(source) = fs::rename(&pending_path, path) {
        fs::remove_file(&pending_path).ok();
        return Err(ScoreError::Io {
            path: path.to_path_buf(),
            source,
        });
    }

    sync_directory(project_directory)
}

fn sync_directory(path: &Path) -> Result<(), ScoreError> {
    let directory = fs::File::open(path).map_err(|source| ScoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    directory.sync_all().map_err(|source| ScoreError::Io {
        path: path.to_path_buf(),
        source,
    })
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
        available_deleted_path, create_part_file, csv_file_name, duplicate_part_file,
        rename_part_file, soft_delete_part_file, DeletedPartPathError, Part, PartName, PartRowEdit,
        PartRowEditError, PartScore, ScoreRowIndex, ScoreRowRange, SubdivisionPattern,
        DELETED_PARTS_DIRECTORY,
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
    fn subdivision_patterns_label_and_highlight_repeating_groups() {
        let part = Part::new("mixed meter", 16)
            .with_subdivision_pattern(Some(SubdivisionPattern::new([4, 3, 3]).unwrap()));

        assert_eq!(
            (0..14)
                .map(|beat| part.beat_label(beat))
                .collect::<Vec<_>>(),
            [
                "1.1", "1.2", "1.3", "1.4", "2.1", "2.2", "2.3", "3.1", "3.2", "3.3", "4.1", "4.2",
                "4.3", "4.4",
            ]
        );
        assert_eq!(
            (0..14)
                .map(|beat| part.beat_is_highlighted(beat))
                .collect::<Vec<_>>(),
            [
                false, false, false, false, true, true, true, false, false, false, true, true,
                true, true,
            ]
        );
    }

    #[test]
    fn subdivision_patterns_reject_invalid_values() {
        assert!(SubdivisionPattern::new([]).is_err());
        assert!(SubdivisionPattern::new([4, 0, 3]).is_err());
        assert!("4,,3".parse::<SubdivisionPattern>().is_err());
        assert!("4, 1.5".parse::<SubdivisionPattern>().is_err());
    }

    #[test]
    fn row_edits_insert_one_clear_ranges_and_preserve_one_row() {
        let score = PartScore::from_rows(vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
            vec!["e".to_string(), "f".to_string()],
        ]);

        let second = ScoreRowIndex::new(1, 3).unwrap();
        let inserted_before = score
            .edited_rows(PartRowEdit::InsertBefore(second), 2)
            .unwrap();
        assert_eq!(
            inserted_before.rows(),
            &[
                vec!["a".to_string(), "b".to_string()],
                vec![String::new(), String::new()],
                vec!["c".to_string(), "d".to_string()],
                vec!["e".to_string(), "f".to_string()],
            ]
        );

        let inserted_after = score
            .edited_rows(PartRowEdit::InsertAfter(second), 2)
            .unwrap();
        assert_eq!(inserted_after.rows()[2], [String::new(), String::new()]);

        let last_two = ScoreRowRange::new(1, 2, 3).unwrap();
        let cleared = score.edited_rows(PartRowEdit::Clear(last_two), 2).unwrap();
        assert_eq!(cleared.rows()[0], ["a".to_string(), "b".to_string()]);
        assert_eq!(
            &cleared.rows()[1..],
            &[
                vec![String::new(), String::new()],
                vec![String::new(), String::new()],
            ]
        );

        let deleted = score.edited_rows(PartRowEdit::Delete(last_two), 2).unwrap();
        assert_eq!(deleted.rows(), &[vec!["a".to_string(), "b".to_string()]]);
        assert_eq!(
            score
                .edited_rows(PartRowEdit::Delete(ScoreRowRange::new(0, 2, 3).unwrap()), 2,)
                .unwrap_err(),
            PartRowEditError::WouldDeleteEveryRow
        );
    }

    #[test]
    fn insertion_selects_the_new_row_and_deletion_selects_the_nearest_survivor() {
        let selected = ScoreRowRange::new(1, 2, 4).unwrap();
        assert_eq!(
            PartRowEdit::InsertBefore(ScoreRowIndex::new(selected.first(), 4).unwrap())
                .selection_after(4),
            ScoreRowRange::new(1, 1, 5)
        );
        assert_eq!(
            PartRowEdit::InsertAfter(ScoreRowIndex::new(selected.last(), 4).unwrap())
                .selection_after(4),
            ScoreRowRange::new(3, 3, 5)
        );
        assert_eq!(
            PartRowEdit::Delete(selected).selection_after(4),
            ScoreRowRange::new(1, 1, 2)
        );
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
            project.voices(),
            "discarded",
            4,
        )
        .unwrap();
        discarded.rollback().unwrap();
        assert!(!project_directory.join("discarded.csv").exists());

        let created = create_part_file(
            &project_directory,
            &project.parts,
            project.voices(),
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
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass, low", VoiceType::Sin),
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
    fn duplicates_a_part_file_with_its_score_and_length() {
        let root = temp_root("duplicate-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )]);
        let project_directory = create_project(&root, &project).unwrap();
        let source = add_part(&project_directory, &mut project, "intro", 2);
        PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]])
            .save(&project_directory, &source, &project)
            .unwrap();

        let duplicated = duplicate_part_file(
            &project_directory,
            &project.parts,
            &source,
            "intro variation",
        )
        .unwrap();

        assert_eq!(duplicated.part().name.as_str(), "intro variation");
        assert_eq!(duplicated.part().length, source.length);
        assert_eq!(
            fs::read(project_directory.join("intro-variation.csv")).unwrap(),
            fs::read(project_directory.join("intro.csv")).unwrap()
        );
        assert_eq!(project.parts, vec![source]);

        duplicated.rollback().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renames_a_part_file_without_changing_its_score() {
        let root = temp_root("rename-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )]);
        let project_directory = create_project(&root, &project).unwrap();
        let source = add_part(&project_directory, &mut project, "intro", 2);
        let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
        score.save(&project_directory, &source, &project).unwrap();

        let renamed =
            rename_part_file(&project_directory, &project.parts, &source, "opening theme").unwrap();

        assert_eq!(renamed.part().name.as_str(), "opening theme");
        assert!(!project_directory.join("intro.csv").exists());
        assert_eq!(
            PartScore::load(&project_directory, renamed.part(), project.voices()).unwrap(),
            score
        );

        renamed.rollback().unwrap();
        assert!(project_directory.join("intro.csv").is_file());
        assert!(!project_directory.join("opening-theme.csv").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn part_scores_load_validate_and_save_pitch_cells() {
        let root = temp_root("score-round-trip");
        let mut project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let project_directory = create_project(&root, &project).unwrap();
        let part = add_part(&project_directory, &mut project, "intro", 2);
        let score = PartScore::from_rows(vec![
            vec!["C4".to_string(), "36".to_string()],
            vec![String::new(), "rest".to_string()],
        ]);

        score.save(&project_directory, &part, &project).unwrap();

        assert_eq!(
            PartScore::load(&project_directory, &part, project.voices()).unwrap(),
            score
        );
        let rows = score.resolved_rows(&part, &project).unwrap();
        assert_eq!(
            rows[0],
            vec![
                project.pitch_system().resolve_cell("C4").unwrap(),
                project.pitch_system().resolve_cell("36").unwrap(),
            ]
        );
        assert_eq!(rows[1], vec![None, None]);
        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "lead,bass\nC4,36\n,rest\n"
        );
        assert!(!project_directory.join(".intro.csv.pending").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn raw_score_recovery_preserves_invalid_cells_without_replacing_the_saved_score() {
        let root = temp_root("raw-score-recovery");
        let mut project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let project_directory = create_project(&root, &project).unwrap();
        let part = add_part(&project_directory, &mut project, "intro", 1);
        let saved_score = PartScore::load(&project_directory, &part, project.voices()).unwrap();
        let recovery_score =
            PartScore::from_rows(vec![vec!["half-typed".to_string(), "C4".to_string()]]);

        recovery_score
            .save_recovery(&project_directory, &part, project.voices())
            .unwrap();

        assert_eq!(
            PartScore::load(&project_directory, &part, project.voices()).unwrap(),
            saved_score
        );
        assert_eq!(
            PartScore::load_with_recovery(&project_directory, &part, project.voices()).unwrap(),
            (recovery_score, true)
        );
        assert!(project_directory.join(".intro.csv.recovery").is_file());
        assert!(!project_directory
            .join(".intro.csv.recovery.pending")
            .exists());

        PartScore::clear_recovery(&project_directory, &part).unwrap();
        assert!(!project_directory.join(".intro.csv.recovery").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn part_scores_report_the_beat_and_voice_for_invalid_pitches() {
        let part = Part::new("intro", 1);
        let project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )]);
        let score = PartScore::from_rows(vec![vec!["not a note".to_string()]]);

        let error = score.resolved_rows(&part, &project).unwrap_err();

        assert!(error.to_string().contains("beat 1, voice \"lead\""));
    }

    #[test]
    fn single_voice_parts_with_blank_cells_load_successfully() {
        let root = temp_root("single-voice-part");
        let mut project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![Voice::new(
            1,
            "lead",
            VoiceType::Saw,
        )]);
        let project_directory = create_project(&root, &project).unwrap();

        add_part(&project_directory, &mut project, "intro", 2);

        assert_eq!(
            fs::read_to_string(project_directory.join("intro.csv")).unwrap(),
            "lead\n\n\n"
        );
        assert_eq!(load_project(&project_directory).unwrap().project, project);

        fs::write(project_directory.join("intro.csv"), "lead\nC4\n\n").unwrap();
        let table = super::read_part_table(&project_directory, &project.parts[0], project.voices())
            .unwrap();
        assert_eq!(
            table.rows,
            vec![vec!["C4".to_string()], vec![String::new()]]
        );

        fs::write(
            project_directory.join("intro.csv"),
            "lead\n\"C4\n\nheld\"\nD4\n",
        )
        .unwrap();
        let table = super::read_part_table(&project_directory, &project.parts[0], project.voices())
            .unwrap();
        assert_eq!(
            table.rows,
            vec![vec!["C4\n\nheld".to_string()], vec!["D4".to_string()]]
        );

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
            project.voices(),
            "intro",
            0,
        )
        .is_err());
        add_part(&project_directory, &mut project, "intro", 4);
        assert!(create_part_file(
            &project_directory,
            &project.parts,
            project.voices(),
            "INTRO",
            4,
        )
        .is_err());
        assert!(create_part_file(
            &project_directory,
            &project.parts,
            project.voices(),
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
            project.voices(),
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
