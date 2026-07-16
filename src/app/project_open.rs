mod parts;
mod project_settings;
mod score;
mod voices;

use std::{
    fmt,
    path::{Path, PathBuf},
};

use gpui::{
    div, prelude::*, AnyElement, App, AppContext, Context, CursorStyle, Entity, EventEmitter,
    MouseButton, MouseDownEvent, Window,
};

use crate::{
    part::{self, PartName, PartScore},
    playback::{Playback, PlaybackLoop},
    project::{self, Project},
    style as s,
    view::{
        button::{self, Button},
        dropdown::{self, Dropdown},
        status_bar,
    },
};

use self::{
    parts::PartsDialog,
    project_settings::{ProjectSettingsDialog, ProjectSettingsMsg},
    score::{DocumentEvent, ScoreDocument, ScoreEditor},
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
    pane_count_dropdown: Entity<Dropdown>,
    play_button: Entity<Button>,
    stop_button: Entity<Button>,
    dialog: Option<Dialog>,
    score_documents: Vec<ScoreDocumentEntry>,
    score_views: Vec<ScoreViewEntry>,
    active_score_view: usize,
    playback: Option<Playback>,
    playing_document: Option<Entity<ScoreDocument>>,
    transport_error: Option<String>,
    workspace_error: Option<String>,
}

struct ScoreDocumentEntry {
    part_name: PartName,
    document: Entity<ScoreDocument>,
}

struct ScoreViewEntry {
    part_name: Option<PartName>,
    editor: Option<Entity<ScoreEditor>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StatusTarget {
    part_name: PartName,
    row: usize,
    column: usize,
}

type ProjectStatus = status_bar::Status<StatusTarget>;

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
        let pane_count_dropdown =
            cx.new(|cx| Dropdown::new("score-pane-count", ["1 pane", "2 panes", "3 panes"], 0, cx));
        let play_button = cx.new(|_| Button::new("play-score", "play"));
        let stop_button = cx.new(|_| Button::new("stop-score", "stop"));

        cx.subscribe(&settings_button, Self::on_settings_clicked)
            .detach();
        cx.subscribe(&parts_button, Self::on_parts_clicked).detach();
        cx.subscribe(&voices_button, Self::on_voices_clicked)
            .detach();
        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&pane_count_dropdown, Self::on_pane_count_selected)
            .detach();
        cx.subscribe(&play_button, Self::on_play_clicked).detach();
        cx.subscribe(&stop_button, Self::on_stop_clicked).detach();

        let initial_part = project.parts.first().map(|part| part.name.clone());
        let mut model = Self {
            project,
            project_directory,
            workspace_root,
            settings_button,
            parts_button,
            voices_button,
            close_button,
            pane_count_dropdown,
            play_button,
            stop_button,
            dialog: None,
            score_documents: Vec::new(),
            score_views: vec![ScoreViewEntry {
                part_name: None,
                editor: None,
            }],
            active_score_view: 0,
            playback: None,
            playing_document: None,
            transport_error: None,
            workspace_error: None,
        };
        if let Some(part_name) = initial_part {
            model.assign_part_to_view(0, part_name, cx);
        }
        model
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn project_directory(&self) -> &Path {
        &self.project_directory
    }

    pub fn bar_actions(&self) -> Vec<AnyElement> {
        vec![
            self.pane_count_dropdown.clone().into_any_element(),
            self.play_button.clone().into_any_element(),
            self.stop_button.clone().into_any_element(),
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

    fn has_unsaved_score(&self, cx: &Context<Self>) -> bool {
        self.score_documents
            .iter()
            .any(|entry| entry.document.read(cx).is_dirty())
    }

    fn project_status(&self, cx: &App) -> ProjectStatus {
        if let Some(message) = &self.workspace_error {
            return ProjectStatus::Error {
                message: message.clone().into(),
                target: None,
            };
        }

        let mut parse_issues = self
            .score_documents
            .iter()
            .flat_map(|entry| {
                entry
                    .document
                    .read(cx)
                    .parse_issues()
                    .into_iter()
                    .map(|issue| (entry.part_name.clone(), issue))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        if !parse_issues.is_empty() {
            if let Some(active_part) = self.active_part() {
                if let Some(active_issue) = parse_issues
                    .iter()
                    .position(|(part_name, _)| part_name.eq_ignore_ascii_case(active_part))
                {
                    parse_issues.swap(0, active_issue);
                }
            }

            let issue_count = parse_issues.len();
            let (part_name, issue) = parse_issues.remove(0);
            let count_label = if issue_count == 1 {
                "parse issue".to_string()
            } else {
                format!("{issue_count} parse issues")
            };
            let message = format!(
                "{count_label} · {} · beat {} · {}: {}",
                part_name.as_str(),
                issue.row + 1,
                issue.voice,
                issue.message
            );
            return ProjectStatus::Error {
                message: message.into(),
                target: Some(StatusTarget {
                    part_name,
                    row: issue.row,
                    column: issue.column,
                }),
            };
        }

        if let Some((part_name, error)) = self.score_documents.iter().find_map(|entry| {
            entry
                .document
                .read(cx)
                .last_save_error()
                .map(|error| (entry.part_name.as_str(), error))
        }) {
            return ProjectStatus::Error {
                message: format!("{part_name}: {error}").into(),
                target: None,
            };
        }

        if let Some(message) = &self.transport_error {
            return ProjectStatus::Error {
                message: message.clone().into(),
                target: None,
            };
        }

        let dirty_document_count = self
            .score_documents
            .iter()
            .filter(|entry| entry.document.read(cx).is_dirty())
            .count();
        if dirty_document_count > 0 {
            let message = if dirty_document_count == 1 {
                "unsaved score changes".to_string()
            } else {
                format!("unsaved score changes in {dirty_document_count} parts")
            };
            return ProjectStatus::Warning(message.into());
        }

        ProjectStatus::default()
    }

    fn part_has_unsaved_score(&self, name: &PartName, cx: &Context<Self>) -> bool {
        self.score_documents.iter().any(|entry| {
            entry.part_name.eq_ignore_ascii_case(name) && entry.document.read(cx).is_dirty()
        })
    }

    fn reject_if_score_unsaved(&mut self, message: &'static str, cx: &mut Context<Self>) -> bool {
        if !self.has_unsaved_score(cx) {
            return false;
        }
        self.workspace_error = Some(message.to_string());
        cx.notify();
        true
    }

    fn select_part(&mut self, name: PartName, cx: &mut Context<Self>) {
        if self
            .score_views
            .get(self.active_score_view)
            .is_some_and(|view| view.part_name.as_ref() == Some(&name) && view.editor.is_some())
        {
            return;
        }

        self.assign_part_to_view(self.active_score_view, name, cx);
    }

    fn active_part(&self) -> Option<&PartName> {
        self.score_views
            .get(self.active_score_view)
            .and_then(|view| view.part_name.as_ref())
    }

    fn score_document(
        &mut self,
        part_name: &PartName,
        cx: &mut Context<Self>,
    ) -> Result<Entity<ScoreDocument>, String> {
        if let Some(entry) = self
            .score_documents
            .iter()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(part_name))
        {
            return Ok(entry.document.clone());
        }

        let part = self
            .project
            .part(part_name)
            .cloned()
            .ok_or_else(|| format!("part {:?} no longer exists", part_name.as_str()))?;
        let score = PartScore::load(&self.project_directory, &part, self.project.voices())
            .map_err(|error| error.to_string())?;
        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let document = cx.new(move |_| ScoreDocument::new(project, project_directory, part, score));
        cx.subscribe(&document, Self::on_score_document_event)
            .detach();
        self.score_documents.push(ScoreDocumentEntry {
            part_name: part_name.clone(),
            document: document.clone(),
        });
        Ok(document)
    }

    fn active_document(&self) -> Option<Entity<ScoreDocument>> {
        let part_name = self.active_part()?;
        self.score_documents
            .iter()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(part_name))
            .map(|entry| entry.document.clone())
    }

    fn on_score_document_event(
        &mut self,
        document: Entity<ScoreDocument>,
        event: &DocumentEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(
            event,
            DocumentEvent::CellChanged { .. } | DocumentEvent::SaveFailed
        ) {
            self.workspace_error = None;
        }
        if matches!(event, DocumentEvent::Saved) && !self.has_unsaved_score(cx) {
            self.workspace_error = None;
        }
        if self.playing_document.as_ref() == Some(&document)
            && matches!(
                event,
                DocumentEvent::CellChanged { .. }
                    | DocumentEvent::Reset
                    | DocumentEvent::ProjectChanged
            )
        {
            self.update_live_playback(&document, cx);
        }
        cx.notify();
    }

    fn playback_loop(
        document: &Entity<ScoreDocument>,
        cx: &Context<Self>,
    ) -> Result<PlaybackLoop, String> {
        let document = document.read(cx);
        PlaybackLoop::from_project_score(document.project(), document.part(), document.score())
            .map_err(|error| error.to_string())
    }

    fn update_live_playback(&mut self, document: &Entity<ScoreDocument>, cx: &mut Context<Self>) {
        if self.playback.is_none() {
            return;
        }
        match Self::playback_loop(document, cx) {
            Ok(playback_loop) => {
                if let Some(playback) = &self.playback {
                    playback.update(playback_loop);
                }
                self.transport_error = None;
            }
            Err(error) => {
                self.transport_error =
                    Some(format!("playback is keeping the last valid loop: {error}"));
            }
        }
        cx.notify();
    }

    fn on_play_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(document) = self.active_document() else {
            self.transport_error =
                Some("open a part in the active view before playing".to_string());
            cx.notify();
            return;
        };
        let playback_loop = match Self::playback_loop(&document, cx) {
            Ok(playback_loop) => playback_loop,
            Err(error) => {
                self.transport_error = Some(error);
                cx.notify();
                return;
            }
        };

        self.playback = None;
        self.playing_document = None;
        match Playback::start(playback_loop) {
            Ok(playback) => {
                self.playback = Some(playback);
                self.playing_document = Some(document);
                self.transport_error = None;
                self.set_transport_playing(true, cx);
            }
            Err(error) => {
                self.transport_error = Some(error.to_string());
                self.set_transport_playing(false, cx);
            }
        }
        cx.notify();
    }

    fn on_stop_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.stop_playback(cx);
    }

    fn stop_playback(&mut self, cx: &mut Context<Self>) {
        self.playback = None;
        self.playing_document = None;
        self.transport_error = None;
        self.set_transport_playing(false, cx);
        cx.notify();
    }

    fn set_transport_playing(&self, playing: bool, cx: &mut Context<Self>) {
        self.play_button.update(cx, |button, cx| {
            button.set_depressed(playing, cx);
        });
    }

    fn assign_part_to_view(
        &mut self,
        view_index: usize,
        part_name: PartName,
        cx: &mut Context<Self>,
    ) {
        let document = match self.score_document(&part_name, cx) {
            Ok(document) => document,
            Err(error) => {
                self.workspace_error = Some(error);
                cx.notify();
                return;
            }
        };
        let editor = cx.new(move |cx| ScoreEditor::new(document, cx));
        if let Some(view) = self.score_views.get_mut(view_index) {
            view.part_name = Some(part_name);
            view.editor = Some(editor);
            self.workspace_error = None;
        }
        cx.notify();
    }

    fn activate_score_view(&mut self, view_index: usize, cx: &mut Context<Self>) {
        if view_index >= self.score_views.len() || view_index == self.active_score_view {
            return;
        }
        self.active_score_view = view_index;
        self.workspace_error = None;
        cx.notify();
    }

    fn on_pane_count_selected(
        &mut self,
        _: Entity<Dropdown>,
        selected: &dropdown::Selected,
        cx: &mut Context<Self>,
    ) {
        self.set_view_count(selected.index + 1, cx);
    }

    fn set_view_count(&mut self, count: usize, cx: &mut Context<Self>) {
        let count = count.clamp(1, 3);
        let template_part = self
            .active_part()
            .cloned()
            .or_else(|| self.project.parts.first().map(|part| part.name.clone()));

        while self.score_views.len() < count {
            let view_index = self.score_views.len();
            self.score_views.push(ScoreViewEntry {
                part_name: None,
                editor: None,
            });
            if let Some(part_name) = template_part.clone() {
                self.assign_part_to_view(view_index, part_name, cx);
            }
        }
        self.score_views.truncate(count);
        self.active_score_view = self.active_score_view.min(count - 1);
        self.pane_count_dropdown.update(cx, |dropdown, cx| {
            if dropdown.selected_index() != count - 1 {
                dropdown.set_selected_index(count - 1, cx);
            }
        });
        cx.notify();
    }

    fn update_score_documents_for_project_settings(&self, cx: &mut Context<Self>) {
        for entry in &self.score_documents {
            let project = self.project.clone();
            entry.document.update(cx, |document, cx| {
                document.project_settings_changed(project, cx);
            });
        }
    }

    fn refresh_score_documents_after_voice_change(&mut self, cx: &mut Context<Self>) {
        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let mut errors = Vec::new();
        for entry in &self.score_documents {
            let Some(part) = project.part(&entry.part_name).cloned() else {
                continue;
            };
            match PartScore::load(&project_directory, &part, project.voices()) {
                Ok(score) => {
                    let project = project.clone();
                    entry.document.update(cx, |document, cx| {
                        document.replace_project_and_score(project, part, score, cx);
                    });
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        self.workspace_error = (!errors.is_empty()).then(|| errors.join("; "));
    }

    fn remove_score_document(&mut self, name: &PartName, cx: &mut Context<Self>) {
        let removed = self
            .score_documents
            .iter()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(name))
            .map(|entry| entry.document.clone());
        if removed.as_ref() == self.playing_document.as_ref() {
            self.stop_playback(cx);
        }
        self.score_documents
            .retain(|entry| !entry.part_name.eq_ignore_ascii_case(name));
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

        if self.reject_if_score_unsaved("save score changes before changing voices", cx) {
            return;
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
            self.update_score_documents_for_project_settings(cx);
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
        if !matches!(msg, voices::Msg::Closed) && self.has_unsaved_score(cx) {
            let message = "save score changes before changing voices".to_string();
            dialog.update(cx, |dialog, cx| match msg {
                voices::Msg::AddRequested { .. } => dialog.add_failed(message, cx),
                voices::Msg::EditRequested { .. } => dialog.edit_failed(message, cx),
                voices::Msg::DeleteRequested { .. } => dialog.delete_failed(message, cx),
                voices::Msg::Closed => {}
            });
            self.workspace_error = Some("save score changes before changing voices".to_string());
            cx.notify();
            return;
        }

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
                        self.refresh_score_documents_after_voice_change(cx);
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
                        self.refresh_score_documents_after_voice_change(cx);
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
                        self.refresh_score_documents_after_voice_change(cx);
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
                        let added_name = part.name.clone();
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_added(parts, part.name, cx);
                        });
                        self.select_part(added_name, cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::DeleteRequested { name } => {
                if self.part_has_unsaved_score(name, cx) {
                    self.workspace_error =
                        Some("save score changes before deleting this part".to_string());
                    dialog.update(cx, |dialog, cx| {
                        dialog.delete_failed(
                            "save score changes before deleting this part".to_string(),
                            cx,
                        );
                    });
                    return;
                }
                match delete_project_part(&self.project_directory, &mut self.project, name) {
                    Ok(part) => {
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_deleted(parts, &part.name, cx);
                        });
                        let affected_views = self
                            .score_views
                            .iter()
                            .enumerate()
                            .filter_map(|(index, view)| {
                                view.part_name
                                    .as_ref()
                                    .is_some_and(|name| name.eq_ignore_ascii_case(&part.name))
                                    .then_some(index)
                            })
                            .collect::<Vec<_>>();
                        let fallback = self.project.parts.first().map(|part| part.name.clone());
                        self.remove_score_document(&part.name, cx);
                        for view_index in affected_views {
                            if let Some(view) = self.score_views.get_mut(view_index) {
                                view.part_name = None;
                                view.editor = None;
                            }
                            if let Some(part_name) = fallback.clone() {
                                self.assign_part_to_view(view_index, part_name, cx);
                            }
                        }
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

    fn on_status_bar_clicked(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ProjectStatus::Error {
            target: Some(target),
            ..
        } = self.project_status(cx)
        else {
            return;
        };

        let active_view_has_target = self
            .score_views
            .get(self.active_score_view)
            .and_then(|view| view.part_name.as_ref())
            .is_some_and(|name| name.eq_ignore_ascii_case(&target.part_name));
        let view_index = if active_view_has_target {
            self.active_score_view
        } else {
            self.score_views
                .iter()
                .position(|view| {
                    view.part_name
                        .as_ref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&target.part_name))
                })
                .unwrap_or(self.active_score_view)
        };
        let target_is_open = self
            .score_views
            .get(view_index)
            .and_then(|view| view.part_name.as_ref())
            .is_some_and(|name| name.eq_ignore_ascii_case(&target.part_name));
        if target_is_open {
            self.activate_score_view(view_index, cx);
        } else {
            self.assign_part_to_view(view_index, target.part_name, cx);
        }

        let Some(editor) = self
            .score_views
            .get(view_index)
            .and_then(|view| view.editor.clone())
        else {
            return;
        };
        window.on_next_frame(move |window, cx| {
            editor.update(cx, |editor, cx| {
                editor.reveal_issue(target.row, target.column, window, cx);
            });
        });
        cx.notify();
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }

        if self.reject_if_score_unsaved("save score changes before closing the project", cx) {
            return;
        }

        cx.emit(Msg::CloseRequested);
    }
}

impl Render for Model {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let project_status = self.project_status(cx);
        workspace(&self.score_views, project_status, cx)
    }
}

fn workspace(
    score_views: &[ScoreViewEntry],
    project_status: ProjectStatus,
    cx: &mut Context<Model>,
) -> gpui::Div {
    let panes = score_views
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let content = match &view.editor {
                Some(editor) => editor.clone().into_any_element(),
                None => div()
                    .flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_color(s::TEXT_DEFAULT)
                    .child("add a part to begin composing")
                    .into_any_element(),
            };
            div()
                .flex()
                .flex_1()
                .w(s::S0)
                .min_w(s::S0)
                .min_h(s::S0)
                .overflow_hidden()
                .child(content)
                .id(("score-view", index))
                .debug_selector(|| format!("score-view-{index}"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |model, _: &MouseDownEvent, _: &mut Window, cx| {
                        model.activate_score_view(index, cx);
                    }),
                )
        })
        .collect::<Vec<_>>();
    let editors = div()
        .flex()
        .flex_row()
        .flex_1()
        .w_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
        .gap(s::CONTENT_PADDING)
        .children(panes);
    let status_is_actionable = match &project_status {
        ProjectStatus::Error { target, .. } => target.is_some(),
        ProjectStatus::Empty | ProjectStatus::Warning(_) => false,
    };
    let project_status_bar = status_bar::bar(project_status)
        .id("project-status-bar")
        .debug_selector(|| "project-status-bar".to_string())
        .when(status_is_actionable, |bar| {
            bar.cursor(CursorStyle::PointingHand)
                .on_mouse_down(MouseButton::Left, cx.listener(Model::on_status_bar_clicked))
        });
    let editor_workspace = div()
        .flex()
        .flex_1()
        .w_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
        .p(s::CONTENT_PADDING)
        .child(editors);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
        .bg(s::GREEN2)
        .debug_selector(|| "score-workspace".to_string())
        .child(editor_workspace)
        .child(project_status_bar)
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use gpui::{px, size, TestAppContext};

    use super::{Model, StatusTarget};
    use crate::{
        part::{Part, PartScore},
        project::{Project, Voice, VoiceType},
        seed::Seed,
        view::status_bar,
    };

    #[gpui::test]
    fn three_score_views_share_width_and_stay_inside_the_workspace(cx: &mut TestAppContext) {
        let root = temp_root("three-score-views");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 16);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![
                Voice::new(1, "first", VoiceType::Saw),
                Voice::new(2, "second", VoiceType::Saw),
                Voice::new(3, "third", VoiceType::Sin),
            ])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new(); 3]; 16])
            .save(&project_directory, &part, project.voices())
            .unwrap();

        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        model.update(cx, |model, cx| model.set_view_count(3, cx));
        assert_eq!(
            cx.update(|_, cx| { model.read(cx).pane_count_dropdown.read(cx).selected_index() }),
            2
        );
        cx.simulate_resize(size(px(1_200.0), px(800.0)));
        cx.run_until_parked();

        let workspace = cx.debug_bounds("score-workspace").unwrap();
        let panes = [
            cx.debug_bounds("score-view-0").unwrap(),
            cx.debug_bounds("score-view-1").unwrap(),
            cx.debug_bounds("score-view-2").unwrap(),
        ];
        let workspace_right = workspace.origin.x + workspace.size.width;
        let third_right = panes[2].origin.x + panes[2].size.width;

        assert!(panes.iter().all(|pane| pane.size.width > px(0.0)));
        assert!((panes[0].size.width / panes[1].size.width - 1.0).abs() < 0.01);
        assert!((panes[1].size.width / panes[2].size.width - 1.0).abs() < 0.01);
        assert!(third_right <= workspace_right + px(1.0));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn project_status_changes_without_reflowing_the_score_workspace(cx: &mut TestAppContext) {
        let root = temp_root("stable-project-status");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 4);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]; 4])
            .save(&project_directory, &part, project.voices())
            .unwrap();

        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let pane_before = cx.debug_bounds("score-view-0").unwrap();
        let workspace = cx.debug_bounds("score-workspace").unwrap();
        let status_bar_before = cx.debug_bounds("project-status-bar").unwrap();
        assert_eq!(status_bar_before.origin.x, workspace.origin.x);
        assert_eq!(status_bar_before.size.width, workspace.size.width);
        assert_eq!(
            status_bar_before.origin.y + status_bar_before.size.height,
            workspace.origin.y + workspace.size.height
        );
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "not-a-note".to_string(), cx);
        });
        cx.run_until_parked();

        let error_status = cx.update(|_, cx| model.read(cx).project_status(cx));
        assert!(matches!(
            error_status,
            status_bar::Status::Error {
                target: Some(StatusTarget {
                    row: 0,
                    column: 0,
                    ..
                }),
                ..
            }
        ));
        assert_eq!(cx.debug_bounds("score-view-0").unwrap(), pane_before);
        assert_eq!(
            cx.debug_bounds("project-status-bar").unwrap(),
            status_bar_before
        );

        cx.simulate_click(status_bar_before.center(), Default::default());
        cx.run_until_parked();

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });
        cx.run_until_parked();

        let warning_status = cx.update(|_, cx| model.read(cx).project_status(cx));
        assert!(matches!(warning_status, status_bar::Status::Warning(_)));
        assert_eq!(cx.debug_bounds("score-view-0").unwrap(), pane_before);
        assert_eq!(
            cx.debug_bounds("project-status-bar").unwrap(),
            status_bar_before
        );

        document
            .update(cx, |document, cx| document.save(cx))
            .unwrap();
        cx.run_until_parked();

        let clean_status = cx.update(|_, cx| model.read(cx).project_status(cx));
        assert_eq!(clean_status, status_bar::Status::Empty);
        assert_eq!(cx.debug_bounds("score-view-0").unwrap(), pane_before);
        assert_eq!(
            cx.debug_bounds("project-status-bar").unwrap(),
            status_bar_before
        );

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
