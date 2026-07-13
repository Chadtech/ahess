mod project_settings;
mod voices;

use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, AnyElement, AppContext, Context, Entity, EventEmitter};

use crate::{
    project::Project,
    style as s,
    view::button::{self, Button},
};

use self::{
    project_settings::{ProjectSettingsDialog, ProjectSettingsEvent},
    voices::VoicesDialog,
};

pub enum Event {
    CloseRequested,
}

pub struct Model {
    project: Project,
    project_directory: PathBuf,
    workspace_root: PathBuf,
    settings_button: Entity<Button>,
    voices_button: Entity<Button>,
    close_button: Entity<Button>,
    dialog: Option<Dialog>,
}

enum Dialog {
    ProjectSettings(Entity<ProjectSettingsDialog>),
    Voices(Entity<VoicesDialog>),
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
        let voices_button = cx.new(|_| Button::new("voices", "voices"));
        let close_button = cx.new(|_| Button::new("close-project", "close project"));

        cx.subscribe(&settings_button, Self::on_settings_clicked)
            .detach();
        cx.subscribe(&voices_button, Self::on_voices_clicked)
            .detach();
        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        Self {
            project,
            project_directory,
            workspace_root,
            settings_button,
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
            self.voices_button.clone().into_any_element(),
            self.settings_button.clone().into_any_element(),
            self.close_button.clone().into_any_element(),
        ]
    }

    pub fn active_dialog(&self) -> Option<AnyElement> {
        self.dialog.as_ref().map(|dialog| match dialog {
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

        cx.subscribe(&dialog, Self::on_settings_event).detach();
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
            Some(Dialog::ProjectSettings(_)) => return,
            None => {}
        }

        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let dialog = cx.new(move |cx| VoicesDialog::new(project, project_directory, cx));

        cx.subscribe(&dialog, Self::on_voices_event).detach();
        self.dialog = Some(Dialog::Voices(dialog));
        self.set_voices_button_depressed(true, cx);
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

        self.dialog = None;
        cx.notify();
    }

    fn on_voices_event(
        &mut self,
        _: Entity<VoicesDialog>,
        event: &voices::Event,
        cx: &mut Context<Self>,
    ) {
        match event {
            voices::Event::Updated(updated_project) => {
                self.project = updated_project.clone();
            }
            voices::Event::Closed => {
                self.dialog = None;
                self.set_voices_button_depressed(false, cx);
            }
        }

        cx.notify();
    }

    fn set_voices_button_depressed(&self, depressed: bool, cx: &mut Context<Self>) {
        self.voices_button.update(cx, |button, cx| {
            button.set_depressed(depressed, cx);
        });
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }

        cx.emit(Event::CloseRequested);
    }
}

fn workspace() -> gpui::Div {
    div().flex_1().min_h(px(0.0)).bg(s::GREEN2)
}
