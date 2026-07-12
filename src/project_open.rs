use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Entity, EventEmitter};

use crate::{
    project::Project,
    project_settings::{ProjectSettingsDialog, ProjectSettingsEvent},
    view::button::{self, Button},
};

pub enum Event {
    CloseRequested,
}

pub struct Model {
    project: Project,
    project_directory: PathBuf,
    workspace_root: PathBuf,
    settings_button: Entity<Button>,
    close_button: Entity<Button>,
    settings_dialog: Option<Entity<ProjectSettingsDialog>>,
}

impl EventEmitter<Event> for Model {}

impl Model {
    pub fn new(
        project: Project,
        project_directory: PathBuf,
        workspace_root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let settings_button = cx.new(|_| Button::new("project-settings", "project settings"));
        let close_button = cx.new(|_| Button::new("close-project", "close project"));

        cx.subscribe(&settings_button, Self::on_settings_clicked)
            .detach();
        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        Self {
            project,
            project_directory,
            workspace_root,
            settings_button,
            close_button,
            settings_dialog: None,
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn project_directory(&self) -> &Path {
        &self.project_directory
    }

    pub fn settings_button(&self) -> Entity<Button> {
        self.settings_button.clone()
    }

    pub fn close_button(&self) -> Entity<Button> {
        self.close_button.clone()
    }

    pub fn settings_dialog(&self) -> Option<Entity<ProjectSettingsDialog>> {
        self.settings_dialog.clone()
    }

    fn on_settings_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if self.settings_dialog.is_some() {
            return;
        }

        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let workspace_root = self.workspace_root.clone();
        let dialog = cx.new(move |cx| {
            ProjectSettingsDialog::new(project, project_directory, workspace_root, cx)
        });

        cx.subscribe(&dialog, Self::on_settings_event).detach();
        self.settings_dialog = Some(dialog);
        cx.notify();
    }

    fn on_settings_event(
        &mut self,
        _: Entity<ProjectSettingsDialog>,
        event: &ProjectSettingsEvent,
        cx: &mut Context<Self>,
    ) {
        if let ProjectSettingsEvent::Saved(updated_project) = event {
            self.project = updated_project.clone();
        }

        self.settings_dialog = None;
        cx.notify();
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        cx.emit(Event::CloseRequested);
    }
}
