//! Voice schema changes and the coordinated score-file updates they require.

use super::transaction::{
    commit_project_files, recover_project_transaction, ProjectTransactionError,
};
use super::{Project, Voice, VoiceId, VoiceType, VoiceVolumeAdjustment, PROJECT_CONFIG_FILE};
use crate::{acoustics::Point3Meters, part, voice_name::VoiceName};
use std::{error::Error, fmt, path::Path};

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

pub fn add_voice(
    project_directory: impl AsRef<Path>,
    project: &Project,
    name: &str,
    voice_type: VoiceType,
) -> Result<Project, VoiceChangeError> {
    add_voice_at(
        project_directory,
        project,
        name,
        voice_type,
        project.acoustic_scene.listener(),
    )
}

pub fn add_voice_at(
    project_directory: impl AsRef<Path>,
    project: &Project,
    name: &str,
    voice_type: VoiceType,
    position: Point3Meters,
) -> Result<Project, VoiceChangeError> {
    add_voice_with_adjustment_at(project_directory, project, name, voice_type, position, None)
}

pub fn add_voice_with_adjustment_at(
    project_directory: impl AsRef<Path>,
    project: &Project,
    name: &str,
    voice_type: VoiceType,
    position: Point3Meters,
    volume_adjustment: Option<VoiceVolumeAdjustment>,
) -> Result<Project, VoiceChangeError> {
    let name = validated_voice_name(project, None, name)?;
    project
        .acoustic_scene
        .validate_source(position)
        .map_err(|error| VoiceChangeError::InvalidField(error.to_string()))?;
    let next_id = project.next_voice_id;
    let following_id = next_id
        .checked_add(1)
        .ok_or_else(|| VoiceChangeError::InvalidField("no voice ids are available".to_string()))?;
    let mut updated_project = project.clone();
    updated_project.next_voice_id = following_id;
    updated_project.voices.push(
        Voice::new(next_id, name, voice_type)
            .with_position(position)
            .with_volume_adjustment(volume_adjustment),
    );
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
    let position = project
        .voices
        .iter()
        .find(|voice| voice.name.eq_ignore_ascii_case(original_name))
        .map(Voice::position)
        .ok_or_else(|| VoiceChangeError::MissingVoice(original_name.as_str().to_string()))?;
    edit_voice_at(
        project_directory,
        project,
        original_name,
        name,
        voice_type,
        position,
    )
}

pub fn edit_voice_at(
    project_directory: impl AsRef<Path>,
    project: &Project,
    original_name: &VoiceName,
    name: &str,
    voice_type: VoiceType,
    position: Point3Meters,
) -> Result<Project, VoiceChangeError> {
    let volume_adjustment = project
        .voices
        .iter()
        .find(|voice| voice.name.eq_ignore_ascii_case(original_name))
        .map(Voice::volume_adjustment)
        .ok_or_else(|| VoiceChangeError::MissingVoice(original_name.as_str().to_string()))?;
    edit_voice_with_adjustment_at(
        project_directory,
        project,
        original_name,
        name,
        voice_type,
        position,
        volume_adjustment,
    )
}

pub fn edit_voice_with_adjustment_at(
    project_directory: impl AsRef<Path>,
    project: &Project,
    original_name: &VoiceName,
    name: &str,
    voice_type: VoiceType,
    position: Point3Meters,
    volume_adjustment: Option<VoiceVolumeAdjustment>,
) -> Result<Project, VoiceChangeError> {
    let index = project
        .voices
        .iter()
        .position(|voice| voice.name.eq_ignore_ascii_case(original_name))
        .ok_or_else(|| VoiceChangeError::MissingVoice(original_name.as_str().to_string()))?;
    let id = project.voices[index].id();
    let name = validated_voice_name(project, Some(id), name)?;
    project
        .acoustic_scene
        .validate_source(position)
        .map_err(|error| VoiceChangeError::InvalidField(error.to_string()))?;
    let mut updated_project = project.clone();
    updated_project.voices[index] = Voice::new(id, name, voice_type)
        .with_position(position)
        .with_volume_adjustment(volume_adjustment);
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
