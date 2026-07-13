mod parts;
mod project_settings;
mod voices;

use std::{
    fmt,
    path::{Path, PathBuf},
};

use gpui::{div, prelude::*, px, AnyElement, AppContext, Context, Entity, EventEmitter};

use crate::{
    part,
    project::{self, Project},
    style as s,
    view::button::{self, Button},
};

use self::{
    parts::PartsDialog,
    project_settings::{ProjectSettingsDialog, ProjectSettingsMsg},
    voices::VoicesDialog,
};

pub enum Msg {
    CloseRequested,
}

pub struct Model {
    project: Project,
    project_directory: PathBuf,
    workspace_root: PathBuf,
    settings_button: Entity<Button>,
    parts_button: Entity<Button>,
    voices_button: Entity<Button>,
    close_button: Entity<Button>,
    dialog: Option<Dialog>,
}

enum Dialog {
    Parts(Entity<PartsDialog>),
    ProjectSettings(Entity<ProjectSettingsDialog>),
    Voices(Entity<VoicesDialog>),
}

impl EventEmitter<Msg> for Model {}

impl Model {
    pub fn new(
        project: Project,
        project_directory: PathBuf,
        workspace_root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_button = cx.new(|_| Button::new("project-settings", "project settings"));
        let parts_button = cx.new(|_| Button::new("parts", "parts"));
        let voices_button = cx.new(|_| Button::new("voices", "voices"));
        let close_button = cx.new(|_| Button::new("close-project", "close project"));

        cx.subscribe(&settings_button, Self::on_settings_clicked)
            .detach();
        cx.subscribe(&parts_button, Self::on_parts_clicked).detach();
        cx.subscribe(&voices_button, Self::on_voices_clicked)
            .detach();
        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        Self {
            project,
            project_directory,
            workspace_root,
            settings_button,
            parts_button,
            voices_button,
            close_button,
            dialog: None,
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn project_directory(&self) -> &Path {
        &self.project_directory
    }

    pub fn bar_actions(&self) -> Vec<AnyElement> {
        vec![
            self.parts_button.clone().into_any_element(),
            self.voices_button.clone().into_any_element(),
            self.settings_button.clone().into_any_element(),
            self.close_button.clone().into_any_element(),
        ]
    }

    pub fn active_dialog(&self) -> Option<AnyElement> {
        self.dialog.as_ref().map(|dialog| match dialog {
            Dialog::Parts(dialog) => dialog.clone().into_any_element(),
            Dialog::ProjectSettings(dialog) => dialog.clone().into_any_element(),
            Dialog::Voices(dialog) => dialog.clone().into_any_element(),
        })
    }

    pub fn view(&self) -> gpui::Div {
        workspace()
    }

    fn on_settings_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() {
            return;
        }

        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let workspace_root = self.workspace_root.clone();
        let dialog = cx.new(move |cx| {
            ProjectSettingsDialog::new(project, project_directory, workspace_root, cx)
        });

        cx.subscribe(&dialog, Self::on_settings_msg).detach();
        self.dialog = Some(Dialog::ProjectSettings(dialog));
        cx.notify();
    }

    fn on_voices_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        match self.dialog {
            Some(Dialog::Voices(_)) => {
                self.dialog = None;
                self.set_voices_button_depressed(false, cx);
                cx.notify();
                return;
            }
            Some(Dialog::Parts(_)) | Some(Dialog::ProjectSettings(_)) => return,
            None => {}
        }

        let voices = self.project.voices().to_vec();
        let dialog = cx.new(move |cx| VoicesDialog::new(voices, cx));

        cx.subscribe(&dialog, Self::on_voices_msg).detach();
        self.dialog = Some(Dialog::Voices(dialog));
        self.set_voices_button_depressed(true, cx);
        cx.notify();
    }

    fn on_parts_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        match self.dialog {
            Some(Dialog::Parts(_)) => {
                self.dialog = None;
                self.set_parts_button_depressed(false, cx);
                cx.notify();
                return;
            }
            Some(Dialog::ProjectSettings(_)) | Some(Dialog::Voices(_)) => return,
            None => {}
        }

        let parts = self.project.parts.clone();
        let dialog = cx.new(move |cx| PartsDialog::new(parts, cx));

        cx.subscribe(&dialog, Self::on_parts_msg).detach();
        self.dialog = Some(Dialog::Parts(dialog));
        self.set_parts_button_depressed(true, cx);
        cx.notify();
    }

    fn on_settings_msg(
        &mut self,
        _: Entity<ProjectSettingsDialog>,
        msg: &ProjectSettingsMsg,
        cx: &mut Context<Self>,
    ) {
        if let ProjectSettingsMsg::Saved(updated_project) = msg {
            self.project = updated_project.clone();
        }

        self.dialog = None;
        cx.notify();
    }

    fn on_voices_msg(
        &mut self,
        dialog: Entity<VoicesDialog>,
        msg: &voices::Msg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            voices::Msg::AddRequested { name, voice_type } => {
                match project::add_voice(&self.project_directory, &self.project, name, *voice_type)
                {
                    Ok(updated_project) => {
                        let added = updated_project
                            .voices()
                            .last()
                            .expect("adding a voice must append it to the project")
                            .name
                            .clone();
                        self.project = updated_project;
                        let voices = self.project.voices().to_vec();
                        dialog.update(cx, |dialog, cx| {
                            dialog.voice_added(voices, added, cx);
                        });
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            voices::Msg::EditRequested {
                original_name,
                name,
                voice_type,
            } => {
                let edited_id = self.project.voice(original_name).map(|voice| voice.id());
                match project::edit_voice(
                    &self.project_directory,
                    &self.project,
                    original_name,
                    name,
                    *voice_type,
                ) {
                    Ok(updated_project) => {
                        let edited = edited_id
                            .and_then(|id| {
                                updated_project
                                    .voices()
                                    .iter()
                                    .find(|voice| voice.id() == id)
                            })
                            .expect("editing a voice must preserve its id")
                            .name
                            .clone();
                        self.project = updated_project;
                        let voices = self.project.voices().to_vec();
                        dialog.update(cx, |dialog, cx| {
                            dialog.voice_edited(voices, edited, cx);
                        });
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.edit_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            voices::Msg::DeleteRequested { name } => {
                match project::delete_voice(&self.project_directory, &self.project, name) {
                    Ok(updated_project) => {
                        self.project = updated_project;
                        let voices = self.project.voices().to_vec();
                        dialog.update(cx, |dialog, cx| {
                            dialog.voice_deleted(voices, name, cx);
                        });
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.delete_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            voices::Msg::Closed => {
                self.dialog = None;
                self.set_voices_button_depressed(false, cx);
            }
        }

        cx.notify();
    }

    fn on_parts_msg(
        &mut self,
        dialog: Entity<PartsDialog>,
        msg: &parts::Msg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            parts::Msg::AddRequested { name, length } => {
                match create_project_part(&self.project_directory, &mut self.project, name, *length)
                {
                    Ok(part) => {
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_added(parts, part.name, cx);
                        });
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::DeleteRequested { name } => {
                match delete_project_part(&self.project_directory, &mut self.project, name) {
                    Ok(part) => {
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_deleted(parts, &part.name, cx);
                        });
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.delete_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::Closed => {
                self.dialog = None;
                self.set_parts_button_depressed(false, cx);
            }
        }

        cx.notify();
    }

    fn set_voices_button_depressed(&self, depressed: bool, cx: &mut Context<Self>) {
        self.voices_button.update(cx, |button, cx| {
            button.set_depressed(depressed, cx);
        });
    }

    fn set_parts_button_depressed(&self, depressed: bool, cx: &mut Context<Self>) {
        self.parts_button.update(cx, |button, cx| {
            button.set_depressed(depressed, cx);
        });
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }

        cx.emit(Msg::CloseRequested);
    }
}

fn workspace() -> gpui::Div {
    div().flex_1().min_h(px(0.0)).bg(s::GREEN2)
}

#[derive(Debug)]
enum PartChangeError {
    Recovery(project::ProjectTransactionError),
    CreateFile(part::CreatePartError),
    DeleteFile(part::DeletePartError),
    MissingPart(String),
    SaveCreated {
        source: project::SaveProjectError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
    SaveDeleted {
        source: project::SaveProjectError,
        rollback_error: Option<part::PartFileRollbackError>,
    },
}

impl fmt::Display for PartChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
            Self::CreateFile(error) => write!(f, "{error}"),
            Self::DeleteFile(error) => write!(f, "{error}"),
            Self::MissingPart(name) => write!(f, "part {name:?} no longer exists"),
            Self::SaveCreated {
                source,
                rollback_error: None,
            }
            | Self::SaveDeleted {
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
        }
    }
}

impl std::error::Error for PartChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::CreateFile(error) => Some(error),
            Self::DeleteFile(error) => Some(error),
            Self::SaveCreated { source, .. } | Self::SaveDeleted { source, .. } => Some(source),
            Self::MissingPart(_) => None,
        }
    }
}

fn create_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &str,
    length: u32,
) -> Result<part::Part, PartChangeError> {
    project::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let created = part::create_part_file(
        project_directory,
        &project.parts,
        project.voices(),
        name,
        length,
    )
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

fn delete_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &part::PartName,
) -> Result<part::Part, PartChangeError> {
    project::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let index = project
        .parts
        .iter()
        .position(|part| part.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| PartChangeError::MissingPart(name.as_str().to_string()))?;
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
