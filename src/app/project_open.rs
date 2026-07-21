mod loop_range;
mod parts;
mod project_settings;
mod score;
mod voices;

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{
    div, prelude::*, AnyElement, App, AppContext, AsyncApp, Context, CursorStyle, Entity,
    EventEmitter, MouseButton, MouseDownEvent, Task, WeakEntity, Window,
};

use crate::{
    part::{self, PartName, PartScore},
    playback::{BeatRange, Playback, PlaybackLoop},
    project::{self, Project},
    style as s,
    view::{
        button::{self, Button},
        dropdown::{self, Dropdown},
        status_bar,
    },
};

use self::{
    loop_range::{LoopRangeDialog, Msg as LoopRangeMsg},
    parts::PartsDialog,
    project_settings::{ProjectSettingsDialog, ProjectSettingsMsg},
    score::{DocumentEvent, PartSelected, SaveState, ScoreDocument, ScoreEditor},
    voices::VoicesDialog,
};

const PLAYHEAD_REFRESH_INTERVAL: Duration = Duration::from_millis(16);

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
    loop_range_button: Entity<Button>,
    play_button: Entity<Button>,
    stop_button: Entity<Button>,
    dialog: Option<Dialog>,
    score_documents: Vec<ScoreDocumentEntry>,
    score_views: Vec<ScoreViewEntry>,
    active_score_view: usize,
    loop_range: Option<BeatRange>,
    playback: Option<Playback>,
    playhead_task: Option<Task<()>>,
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
enum StatusAction {
    RevealIssue {
        part_name: PartName,
        row: usize,
        column: usize,
    },
    RetryScoreSave,
}

type ProjectStatus = status_bar::Status<StatusAction>;

enum Dialog {
    LoopRange(Entity<LoopRangeDialog>),
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
        let arrangement_beat_count = project.arrangement_beat_count();
        let loop_range = BeatRange::new(1, arrangement_beat_count, arrangement_beat_count).ok();
        let loop_range_button =
            cx.new(|_| Button::new("loop-range", loop_range_button_label(&project, loop_range)));
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
        cx.subscribe(&loop_range_button, Self::on_loop_range_clicked)
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
            loop_range_button,
            play_button,
            stop_button,
            dialog: None,
            score_documents: Vec::new(),
            score_views: vec![ScoreViewEntry {
                part_name: None,
                editor: None,
            }],
            active_score_view: 0,
            loop_range,
            playback: None,
            playhead_task: None,
            transport_error: None,
            workspace_error: None,
        };
        if let Some(part_name) = initial_part {
            model.assign_part_to_view(0, part_name, cx);
        }
        let part_names = model
            .project
            .parts()
            .iter()
            .map(|part| part.name.clone())
            .collect::<Vec<_>>();
        for part_name in part_names {
            if let Err(error) = model.score_document(&part_name, cx) {
                model.workspace_error = Some(error);
                break;
            }
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
            self.loop_range_button.clone().into_any_element(),
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
            Dialog::LoopRange(dialog) => dialog.clone().into_any_element(),
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

    fn flush_all_score_changes(&self, cx: &mut Context<Self>) -> Result<(), String> {
        let documents = self
            .score_documents
            .iter()
            .filter(|entry| entry.document.read(cx).is_dirty())
            .map(|entry| (entry.part_name.clone(), entry.document.clone()))
            .collect();
        Self::flush_score_documents(documents, cx)
    }

    fn flush_part_score_changes(
        &self,
        part_name: &PartName,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let documents = self
            .score_documents
            .iter()
            .filter(|entry| {
                entry.part_name.eq_ignore_ascii_case(part_name)
                    && entry.document.read(cx).is_dirty()
            })
            .map(|entry| (entry.part_name.clone(), entry.document.clone()))
            .collect();
        Self::flush_score_documents(documents, cx)
    }

    fn flush_score_documents(
        documents: Vec<(PartName, Entity<ScoreDocument>)>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let errors = documents
            .into_iter()
            .filter_map(|(part_name, document)| {
                document
                    .update(cx, |document, cx| document.save(cx))
                    .err()
                    .map(|error| format!("{}: {error}", part_name.as_str()))
            })
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn retry_failed_score_saves(&self, cx: &mut Context<Self>) {
        let documents = self
            .score_documents
            .iter()
            .filter(|entry| entry.document.read(cx).last_save_error().is_some())
            .map(|entry| (entry.part_name.clone(), entry.document.clone()))
            .collect();
        let _ = Self::flush_score_documents(documents, cx);
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
                "{count_label} · {} · beat {} · {}: {} · click to reveal",
                part_name.as_str(),
                issue.row + 1,
                issue.voice,
                issue.message
            );
            return ProjectStatus::Error {
                message: message.into(),
                target: Some(StatusAction::RevealIssue {
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
                message: format!("{part_name}: {error} · click to retry").into(),
                target: Some(StatusAction::RetryScoreSave),
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
            let save_states = self
                .score_documents
                .iter()
                .filter_map(|entry| {
                    let document = entry.document.read(cx);
                    document.is_dirty().then(|| document.save_state())
                })
                .collect::<Vec<_>>();
            let message = if save_states.contains(&SaveState::SavingRecovered) {
                "saving recovered score changes…".to_string()
            } else if save_states.contains(&SaveState::Saving) {
                if dirty_document_count == 1 {
                    "saving score changes…".to_string()
                } else {
                    format!("saving score changes in {dirty_document_count} parts…")
                }
            } else if save_states.contains(&SaveState::RecoverySaved) {
                if dirty_document_count == 1 {
                    "score recovery saved".to_string()
                } else {
                    format!("score recovery saved for {dirty_document_count} parts")
                }
            } else if dirty_document_count == 1 {
                "unsaved score changes".to_string()
            } else {
                format!("unsaved score changes in {dirty_document_count} parts")
            };
            return ProjectStatus::Warning(message.into());
        }

        if self
            .score_documents
            .iter()
            .any(|entry| entry.document.read(cx).save_state() == SaveState::Saved)
        {
            return ProjectStatus::Message("score changes saved".into());
        }

        ProjectStatus::default()
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
        let (score, recovered) =
            PartScore::load_with_recovery(&self.project_directory, &part, self.project.voices())
                .map_err(|error| error.to_string())?;
        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let document = cx.new(move |_| ScoreDocument::new(project, project_directory, part, score));
        cx.subscribe(&document, Self::on_score_document_event)
            .detach();
        if recovered {
            document.update(cx, |document, cx| {
                document.autosave_recovered_score(cx);
            });
        }
        self.score_documents.push(ScoreDocumentEntry {
            part_name: part_name.clone(),
            document: document.clone(),
        });
        Ok(document)
    }

    fn on_score_document_event(
        &mut self,
        _: Entity<ScoreDocument>,
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
        if self.playback.is_some()
            && matches!(
                event,
                DocumentEvent::CellChanged { .. }
                    | DocumentEvent::Reset
                    | DocumentEvent::ProjectChanged
            )
        {
            self.update_live_playback(cx);
        }
        cx.notify();
    }

    fn playback_loop(&mut self, cx: &mut Context<Self>) -> Result<PlaybackLoop, String> {
        let range = self.loop_range.ok_or_else(|| {
            "add at least one part to the arrangement before starting playback".to_string()
        })?;
        let sequence = self.project.sequence().to_vec();
        let mut arrangement_scores = Vec::with_capacity(sequence.len());
        for part_name in sequence {
            let document = self.score_document(&part_name, cx)?;
            let document = document.read(cx);
            arrangement_scores.push((document.part().clone(), document.score().clone()));
        }

        PlaybackLoop::from_project_arrangement(&self.project, &arrangement_scores, range)
            .map_err(|error| error.to_string())
    }

    fn update_live_playback(&mut self, cx: &mut Context<Self>) {
        if self.playback.is_none() {
            return;
        }
        match self.playback_loop(cx) {
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
        let playback_loop = match self.playback_loop(cx) {
            Ok(playback_loop) => playback_loop,
            Err(error) => {
                self.transport_error = Some(error);
                cx.notify();
                return;
            }
        };

        self.playhead_task.take();
        self.clear_playhead_highlights(cx);
        self.playback = None;
        match Playback::start(playback_loop) {
            Ok(playback) => {
                self.playback = Some(playback);
                self.transport_error = None;
                self.set_transport_playing(true, cx);
                self.start_playhead_tracking(cx);
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
        self.playhead_task.take();
        self.playback = None;
        self.clear_playhead_highlights(cx);
        self.transport_error = None;
        self.set_transport_playing(false, cx);
        cx.notify();
    }

    fn set_transport_playing(&self, playing: bool, cx: &mut Context<Self>) {
        self.play_button.update(cx, |button, cx| {
            button.set_depressed(playing, cx);
        });
    }

    fn start_playhead_tracking(&mut self, cx: &mut Context<Self>) {
        self.playhead_task.take();
        self.sync_playhead_highlights(cx);
        self.playhead_task = Some(cx.spawn(
            async move |model: WeakEntity<Model>, cx: &mut AsyncApp| loop {
                cx.background_executor()
                    .timer(PLAYHEAD_REFRESH_INTERVAL)
                    .await;
                let keep_tracking = model
                    .update(cx, |model, cx| {
                        if model.playback.is_none() {
                            return false;
                        }
                        model.sync_playhead_highlights(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_tracking {
                    break;
                }
            },
        ));
    }

    fn sync_playhead_highlights(&self, cx: &mut Context<Self>) {
        let playing_position = self.playback.as_ref().and_then(|playback| {
            playing_score_row(&self.project, playback.current_arrangement_beat())
        });
        for view in &self.score_views {
            let playing_row = view.part_name.as_ref().and_then(|view_part| {
                playing_position.as_ref().and_then(|(playing_part, row)| {
                    view_part.eq_ignore_ascii_case(playing_part).then_some(*row)
                })
            });
            if let Some(editor) = &view.editor {
                editor.update(cx, |editor, cx| {
                    editor.set_playing_row(playing_row, cx);
                });
            }
        }
    }

    fn clear_playhead_highlights(&self, cx: &mut Context<Self>) {
        for editor in self
            .score_views
            .iter()
            .filter_map(|view| view.editor.as_ref())
        {
            editor.update(cx, |editor, cx| editor.set_playing_row(None, cx));
        }
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
                self.sync_score_editor_parts(cx);
                cx.notify();
                return;
            }
        };
        let part_names = self
            .project
            .parts()
            .iter()
            .map(|part| part.name.clone())
            .collect::<Vec<_>>();
        let editor = cx.new(move |cx| ScoreEditor::new(view_index, document, part_names, cx));
        cx.subscribe(&editor, Self::on_score_editor_part_selected)
            .detach();
        if let Some(view) = self.score_views.get_mut(view_index) {
            view.part_name = Some(part_name);
            view.editor = Some(editor);
            self.workspace_error = None;
        }
        if self.playback.is_some() {
            self.sync_playhead_highlights(cx);
        }
        cx.notify();
    }

    fn on_score_editor_part_selected(
        &mut self,
        editor: Entity<ScoreEditor>,
        selected: &PartSelected,
        cx: &mut Context<Self>,
    ) {
        let Some(view_index) = self
            .score_views
            .iter()
            .position(|view| view.editor.as_ref() == Some(&editor))
        else {
            return;
        };
        self.activate_score_view(view_index, cx);
        self.assign_part_to_view(view_index, selected.part_name.clone(), cx);
    }

    fn sync_score_editor_parts(&self, cx: &mut Context<Self>) {
        let part_names = self
            .project
            .parts()
            .iter()
            .map(|part| part.name.clone())
            .collect::<Vec<_>>();
        for editor in self
            .score_views
            .iter()
            .filter_map(|view| view.editor.as_ref())
        {
            let part_names = part_names.clone();
            editor.update(cx, |editor, cx| {
                editor.set_available_parts(part_names, cx);
            });
        }
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
        self.score_documents
            .retain(|entry| !entry.part_name.eq_ignore_ascii_case(name));
        if self.playback.is_some() {
            self.update_live_playback(cx);
        }
    }

    fn on_loop_range_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        match self.dialog {
            Some(Dialog::LoopRange(_)) => {
                self.dialog = None;
                self.set_loop_range_button_depressed(false, cx);
                cx.notify();
                return;
            }
            Some(_) => return,
            None => {}
        }

        let occurrences = self.project.arrangement_occurrences();
        let range = self.loop_range;
        let dialog = cx.new(move |cx| LoopRangeDialog::new(occurrences, range, cx));
        cx.subscribe(&dialog, Self::on_loop_range_msg).detach();
        self.dialog = Some(Dialog::LoopRange(dialog));
        self.set_loop_range_button_depressed(true, cx);
        cx.notify();
    }

    fn on_loop_range_msg(
        &mut self,
        _: Entity<LoopRangeDialog>,
        msg: &LoopRangeMsg,
        cx: &mut Context<Self>,
    ) {
        if let LoopRangeMsg::Applied(range) = msg {
            self.loop_range = Some(*range);
            self.sync_loop_range_button(cx);
            if self.playback.is_some() {
                self.update_live_playback(cx);
            } else {
                self.transport_error = None;
            }
        }

        self.dialog = None;
        self.set_loop_range_button_depressed(false, cx);
        cx.notify();
    }

    fn reconcile_loop_range(
        &mut self,
        previous_arrangement_beat_count: u64,
        cx: &mut Context<Self>,
    ) {
        let arrangement_beat_count = self.project.arrangement_beat_count();
        let followed_entire_arrangement = self.loop_range.is_none_or(|range| {
            previous_arrangement_beat_count == 0
                || (range.first() == 1 && range.last() == previous_arrangement_beat_count)
        });
        self.loop_range = if arrangement_beat_count == 0 {
            None
        } else if followed_entire_arrangement {
            BeatRange::new(1, arrangement_beat_count, arrangement_beat_count).ok()
        } else {
            self.loop_range.and_then(|range| {
                let first = range.first().min(arrangement_beat_count);
                let last = range.last().max(first).min(arrangement_beat_count);
                BeatRange::new(first, last, arrangement_beat_count).ok()
            })
        };
        self.sync_loop_range_button(cx);
        if self.playback.is_some() {
            self.update_live_playback(cx);
        }
    }

    fn sync_loop_range_button(&self, cx: &mut Context<Self>) {
        self.loop_range_button.update(cx, |button, cx| {
            button.set_label(loop_range_button_label(&self.project, self.loop_range), cx);
        });
    }

    fn set_loop_range_button_depressed(&self, depressed: bool, cx: &mut Context<Self>) {
        self.loop_range_button.update(cx, |button, cx| {
            button.set_depressed(depressed, cx);
        });
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
            Some(Dialog::LoopRange(_))
            | Some(Dialog::Parts(_))
            | Some(Dialog::ProjectSettings(_)) => return,
            None => {}
        }

        if self.flush_all_score_changes(cx).is_err() {
            return;
        }

        let voices = self.project.voices().to_vec();
        let acoustic_scene = self.project.acoustic_scene().clone();
        let dialog = cx.new(move |cx| VoicesDialog::new(voices, acoustic_scene, cx));

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
            Some(Dialog::LoopRange(_))
            | Some(Dialog::ProjectSettings(_))
            | Some(Dialog::Voices(_)) => return,
            None => {}
        }

        let parts = self.project.parts.clone();
        let sequence = self.project.sequence().to_vec();
        let dialog = cx.new(move |cx| PartsDialog::new(parts, sequence, cx));

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
            self.project = updated_project.as_ref().clone();
            self.update_score_documents_for_project_settings(cx);
            if self.playback.is_some() {
                self.update_live_playback(cx);
            }
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
        if !matches!(msg, voices::Msg::Closed) {
            if let Err(error) = self.flush_all_score_changes(cx) {
                let message = format!("couldn't save score changes: {error}");
                dialog.update(cx, |dialog, cx| match msg {
                    voices::Msg::AddRequested { .. } => dialog.add_failed(message, cx),
                    voices::Msg::EditRequested { .. } => dialog.edit_failed(message, cx),
                    voices::Msg::DeleteRequested { .. } => dialog.delete_failed(message, cx),
                    voices::Msg::Closed => {}
                });
                cx.notify();
                return;
            }
        }

        match msg {
            voices::Msg::AddRequested {
                name,
                voice_type,
                position,
            } => {
                match project::add_voice_at(
                    &self.project_directory,
                    &self.project,
                    name,
                    *voice_type,
                    *position,
                ) {
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
                position,
            } => {
                let edited_id = self.project.voice(original_name).map(|voice| voice.id());
                match project::edit_voice_at(
                    &self.project_directory,
                    &self.project,
                    original_name,
                    name,
                    *voice_type,
                    *position,
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
                        self.sync_score_editor_parts(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::DuplicateRequested { source, name } => {
                if let Err(error) = self.flush_part_score_changes(source, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog
                            .duplicate_failed(format!("couldn't save score changes: {error}"), cx);
                    });
                    return;
                }
                match duplicate_project_part(
                    &self.project_directory,
                    &mut self.project,
                    source,
                    name,
                ) {
                    Ok(part) => {
                        let duplicated_name = part.name.clone();
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_added(parts, part.name, cx);
                        });
                        self.select_part(duplicated_name, cx);
                        self.sync_score_editor_parts(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.duplicate_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::RenameRequested { source, name } => {
                if let Err(error) = self.flush_part_score_changes(source, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog.rename_failed(format!("couldn't save score changes: {error}"), cx);
                    });
                    return;
                }
                match rename_project_part(&self.project_directory, &mut self.project, source, name)
                {
                    Ok(part) => {
                        let renamed_name = part.name.clone();
                        let project = self.project.clone();
                        for entry in &mut self.score_documents {
                            if entry.part_name.eq_ignore_ascii_case(source) {
                                entry.part_name = renamed_name.clone();
                                let score = entry.document.read(cx).score().clone();
                                let project = project.clone();
                                let part = part.clone();
                                entry.document.update(cx, |document, cx| {
                                    document.replace_project_and_score(project, part, score, cx);
                                });
                            } else {
                                let project = project.clone();
                                entry.document.update(cx, |document, cx| {
                                    document.project_settings_changed(project, cx);
                                });
                            }
                        }
                        for view in &mut self.score_views {
                            if view
                                .part_name
                                .as_ref()
                                .is_some_and(|part_name| part_name.eq_ignore_ascii_case(source))
                            {
                                view.part_name = Some(renamed_name.clone());
                            }
                        }
                        let parts = self.project.parts.clone();
                        let sequence = self.project.sequence().to_vec();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_renamed(parts, sequence, renamed_name, cx);
                        });
                        self.sync_score_editor_parts(cx);
                        self.workspace_error = None;
                        if self.playback.is_some() {
                            self.update_live_playback(cx);
                        }
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.rename_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::DeleteRequested { name } => {
                if let Err(error) = self.flush_part_score_changes(name, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog.delete_failed(format!("couldn't save score changes: {error}"), cx);
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
                        self.sync_score_editor_parts(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.delete_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::SequenceChangeRequested {
                sequence,
                selected_occurrence,
            } => {
                let previous_arrangement_beat_count = self.project.arrangement_beat_count();
                match update_project_sequence(
                    &self.project_directory,
                    &mut self.project,
                    sequence.clone(),
                ) {
                    Ok(sequence) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.sequence_changed(sequence, *selected_occurrence, cx);
                        });
                        self.reconcile_loop_range(previous_arrangement_beat_count, cx);
                        self.update_score_documents_for_project_settings(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.sequence_change_failed(error.to_string(), cx);
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
            target: Some(action),
            ..
        } = self.project_status(cx)
        else {
            return;
        };

        let (part_name, row, column) = match action {
            StatusAction::RevealIssue {
                part_name,
                row,
                column,
            } => (part_name, row, column),
            StatusAction::RetryScoreSave => {
                self.retry_failed_score_saves(cx);
                return;
            }
        };

        let active_view_has_target = self
            .score_views
            .get(self.active_score_view)
            .and_then(|view| view.part_name.as_ref())
            .is_some_and(|name| name.eq_ignore_ascii_case(&part_name));
        let view_index = if active_view_has_target {
            self.active_score_view
        } else {
            self.score_views
                .iter()
                .position(|view| {
                    view.part_name
                        .as_ref()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&part_name))
                })
                .unwrap_or(self.active_score_view)
        };
        let target_is_open = self
            .score_views
            .get(view_index)
            .and_then(|view| view.part_name.as_ref())
            .is_some_and(|name| name.eq_ignore_ascii_case(&part_name));
        if target_is_open {
            self.activate_score_view(view_index, cx);
        } else {
            self.assign_part_to_view(view_index, part_name, cx);
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
                editor.reveal_issue(row, column, window, cx);
            });
        });
        cx.notify();
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if self.dialog.is_some() {
            return;
        }

        if self.flush_all_score_changes(cx).is_err() {
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

fn loop_range_button_label(project: &Project, range: Option<BeatRange>) -> String {
    let Some(range) = range else {
        return "set loop".to_string();
    };
    let occurrences = project.arrangement_occurrences();
    if range.first() == 1 && range.last() == project.arrangement_beat_count() {
        return "loop all".to_string();
    }
    let first = occurrences
        .iter()
        .position(|occurrence| occurrence.first_beat() == range.first());
    let last = occurrences
        .iter()
        .position(|occurrence| occurrence.last_beat() == range.last());
    match (first, last) {
        (Some(first), Some(last)) if first == last => format!(
            "loop {}. {}",
            occurrences[first].index() + 1,
            occurrences[first].part_name().as_str()
        ),
        (Some(first), Some(last)) if first < last => format!(
            "loop parts {}–{}",
            occurrences[first].index() + 1,
            occurrences[last].index() + 1
        ),
        _ => format!("loop beats {}–{}", range.first(), range.last()),
    }
}

fn playing_score_row(project: &Project, arrangement_beat: u64) -> Option<(PartName, usize)> {
    for occurrence in project.arrangement_occurrences() {
        if (occurrence.first_beat()..=occurrence.last_beat()).contains(&arrangement_beat) {
            return Some((
                occurrence.part_name().clone(),
                (arrangement_beat - occurrence.first_beat()) as usize,
            ));
        }
    }
    None
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
        ProjectStatus::Empty | ProjectStatus::Message(_) | ProjectStatus::Warning(_) => false,
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

impl fmt::Display for PartChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
            Self::CreateFile(error) => write!(f, "{error}"),
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

impl std::error::Error for PartChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::CreateFile(error) => Some(error),
            Self::RenameFile(error) => Some(error),
            Self::DeleteFile(error) => Some(error),
            Self::SaveCreated { source, .. }
            | Self::SaveDeleted { source, .. }
            | Self::SaveRenamed { source, .. } => Some(source),
            Self::MissingPart(_) | Self::PartInSequence { .. } => None,
        }
    }
}

#[derive(Debug)]
enum ArrangementChangeError {
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

fn update_project_sequence(
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

fn duplicate_project_part(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
) -> Result<part::Part, PartChangeError> {
    project::recover_pending_project_update(project_directory)
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

fn rename_project_part(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
) -> Result<part::Part, PartChangeError> {
    project::recover_pending_project_update(project_directory)
        .map_err(PartChangeError::Recovery)?;
    let index = project
        .parts
        .iter()
        .position(|part| part.name.eq_ignore_ascii_case(source_name))
        .ok_or_else(|| PartChangeError::MissingPart(source_name.as_str().to_string()))?;
    let source_part = project.parts[index].clone();
    let renamed = part::rename_part_file(project_directory, &project.parts, &source_part, name)
        .map_err(PartChangeError::RenameFile)?;
    let renamed_part = renamed.part().clone();
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

    Ok(renamed.commit())
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use gpui::{px, size, AppContext, TestAppContext};

    use super::{
        create_project_part, delete_project_part, duplicate_project_part, loop_range_button_label,
        parts, playing_score_row, rename_project_part, update_project_sequence, Model,
        PartChangeError, PartsDialog, StatusAction,
    };
    use crate::{
        part::{Part, PartName, PartScore},
        playback::BeatRange,
        project::{self, Project, Voice, VoiceType},
        seed::Seed,
        view::{button, status_bar},
    };

    #[test]
    fn maps_arrangement_beats_to_part_score_rows() {
        let first = Part::new("first", 2);
        let second = Part::new("second", 3);
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_parts(vec![first.clone(), second.clone()])
            .with_sequence(vec![
                first.name.clone(),
                second.name.clone(),
                first.name.clone(),
            ]);

        let position = |beat| {
            playing_score_row(&project, beat).map(|(part, row)| (part.as_str().to_string(), row))
        };
        assert_eq!(position(1), Some(("first".to_string(), 0)));
        assert_eq!(position(2), Some(("first".to_string(), 1)));
        assert_eq!(position(3), Some(("second".to_string(), 0)));
        assert_eq!(position(5), Some(("second".to_string(), 2)));
        assert_eq!(position(6), Some(("first".to_string(), 0)));
        assert_eq!(position(7), Some(("first".to_string(), 1)));
        assert_eq!(position(0), None);
        assert_eq!(position(8), None);
    }

    #[test]
    fn loop_button_labels_part_aligned_and_exact_ranges_semantically() {
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_parts(vec![Part::new("intro", 8), Part::new("verse", 16)])
            .with_sequence(vec!["intro".into(), "verse".into(), "verse".into()]);

        assert_eq!(
            loop_range_button_label(&project, BeatRange::new(1, 40, 40).ok()),
            "loop all"
        );
        assert_eq!(
            loop_range_button_label(&project, BeatRange::new(9, 24, 40).ok()),
            "loop 2. verse"
        );
        assert_eq!(
            loop_range_button_label(&project, BeatRange::new(9, 40, 40).ok()),
            "loop parts 2–3"
        );
        assert_eq!(
            loop_range_button_label(&project, BeatRange::new(10, 23, 40).ok()),
            "loop beats 10–23"
        );
        assert_eq!(loop_range_button_label(&project, None), "set loop");
    }

    #[test]
    fn arrangement_changes_persist_and_prevent_deleting_referenced_parts() {
        let root = temp_root("part-arrangement");
        let mut project = Project::new("test project", 800, 0, Seed::new(12));
        let project_directory = project::create_project(&root, &project).unwrap();
        let part = create_project_part(&project_directory, &mut project, "part-a", 4).unwrap();

        update_project_sequence(
            &project_directory,
            &mut project,
            vec![part.name.clone(), part.name.clone()],
        )
        .unwrap();

        assert_eq!(project.sequence().len(), 2);
        assert_eq!(
            project::load_project(&project_directory)
                .unwrap()
                .project
                .sequence(),
            project.sequence()
        );
        let error = delete_project_part(&project_directory, &mut project, &part.name).unwrap_err();
        assert!(matches!(
            error,
            PartChangeError::PartInSequence {
                occurrence_count: 2,
                ..
            }
        ));
        assert!(project_directory.join("part-a.csv").is_file());

        update_project_sequence(&project_directory, &mut project, Vec::new()).unwrap();
        delete_project_part(&project_directory, &mut project, &part.name).unwrap();

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn duplicated_parts_copy_the_score_and_persist_project_metadata() {
        let root = temp_root("duplicate-project-part");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let source = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
        let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
        score.save(&project_directory, &source, &project).unwrap();

        let duplicated = duplicate_project_part(
            &project_directory,
            &mut project,
            &source.name,
            "intro variation",
        )
        .unwrap();

        assert_eq!(duplicated.length, source.length);
        assert_eq!(
            PartScore::load(&project_directory, &duplicated, project.voices()).unwrap(),
            score
        );
        assert_eq!(
            project::load_project(&project_directory).unwrap().project,
            project
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renamed_parts_keep_their_score_and_update_every_arrangement_occurrence() {
        let root = temp_root("rename-project-part");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let intro = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
        let verse = create_project_part(&project_directory, &mut project, "verse", 2).unwrap();
        let score = PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]]);
        score.save(&project_directory, &intro, &project).unwrap();
        update_project_sequence(
            &project_directory,
            &mut project,
            vec![intro.name.clone(), verse.name.clone(), intro.name.clone()],
        )
        .unwrap();

        let renamed = rename_project_part(
            &project_directory,
            &mut project,
            &intro.name,
            "opening theme",
        )
        .unwrap();

        assert_eq!(renamed.name.as_str(), "opening theme");
        assert_eq!(
            project
                .sequence()
                .iter()
                .map(PartName::as_str)
                .collect::<Vec<_>>(),
            ["opening theme", "verse", "opening theme"]
        );
        assert!(!project_directory.join("intro.csv").exists());
        assert_eq!(
            PartScore::load(&project_directory, &renamed, project.voices()).unwrap(),
            score
        );
        assert_eq!(
            project::load_project(&project_directory).unwrap().project,
            project
        );

        let error = rename_project_part(&project_directory, &mut project, &renamed.name, "verse")
            .unwrap_err();
        assert!(matches!(error, PartChangeError::RenameFile(_)));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn renaming_an_open_part_updates_its_score_document_and_view(cx: &mut TestAppContext) {
        let root = temp_root("rename-open-project-part");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let intro = create_project_part(&project_directory, &mut project, "intro", 1).unwrap();
        let score = PartScore::from_rows(vec![vec!["C4".to_string()]]);
        score.save(&project_directory, &intro, &project).unwrap();
        let dialog_parts = project.parts.clone();
        let dialog_sequence = project.sequence().to_vec();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let dialog = cx.new(|cx| PartsDialog::new(dialog_parts, dialog_sequence, cx));

        model.update(cx, |model, cx| {
            model.on_parts_msg(
                dialog,
                &parts::Msg::RenameRequested {
                    source: intro.name,
                    name: "opening theme".to_string(),
                },
                cx,
            );
        });

        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.project.parts()[0].name.as_str(), "opening theme");
            assert_eq!(model.score_documents[0].part_name.as_str(), "opening theme");
            assert_eq!(
                model.score_documents[0]
                    .document
                    .read(cx)
                    .part()
                    .name
                    .as_str(),
                "opening theme"
            );
            assert_eq!(
                model.score_views[0].part_name.as_ref().unwrap().as_str(),
                "opening theme"
            );
        });
        assert!(!project_directory.join("intro.csv").exists());
        assert!(project_directory.join("opening-theme.csv").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_arrangement_saves_restore_the_in_memory_sequence() {
        let part = Part::new("part-a", 4);
        let mut project =
            Project::new("test project", 800, 0, Seed::new(12)).with_parts(vec![part.clone()]);
        let original_sequence = project.sequence().to_vec();

        let error = update_project_sequence(
            Path::new("/a/project/directory/that/does/not/exist"),
            &mut project,
            vec![part.name.clone(), part.name],
        )
        .unwrap_err();

        assert!(matches!(error, super::ArrangementChangeError::Save(_)));
        assert_eq!(project.sequence(), original_sequence);
    }

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
            .save(&project_directory, &part, &project)
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
    fn score_editor_outlines_only_the_current_playback_row(cx: &mut TestAppContext) {
        let root = temp_root("score-playback-row");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 3);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]; 3])
            .save(&project_directory, &part, &project)
            .unwrap();

        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        let editor = cx.update(|_, cx| model.read(cx).score_views[0].editor.clone().unwrap());
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();
        assert!(cx.debug_bounds("score-playback-row-0").is_none());
        assert!(cx.debug_bounds("score-playback-row-1").is_none());
        assert!(cx.debug_bounds("score-playback-row-2").is_none());

        editor.update(cx, |editor, cx| editor.set_playing_row(Some(1), cx));
        cx.run_until_parked();

        assert!(cx.debug_bounds("score-playback-row-0").is_none());
        assert!(cx.debug_bounds("score-playback-row-1").is_some());
        assert!(cx.debug_bounds("score-playback-row-2").is_none());

        editor.update(cx, |editor, cx| editor.set_playing_row(None, cx));
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).playing_row()), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn each_score_view_can_select_its_own_part(cx: &mut TestAppContext) {
        let root = temp_root("score-view-part-selectors");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let first_part = Part::new("part-a", 4);
        let second_part = Part::new("part-b", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![first_part.clone(), second_part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]; 4])
            .save(&project_directory, &first_part, &project)
            .unwrap();
        PartScore::from_rows(vec![vec![String::new()]; 2])
            .save(&project_directory, &second_part, &project)
            .unwrap();

        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        model.update(cx, |model, cx| model.set_view_count(2, cx));
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let trigger = cx.debug_bounds("score-part-1-trigger").unwrap();
        cx.simulate_click(trigger.center(), Default::default());
        let second_option = cx.debug_bounds("score-part-1-option-1").unwrap();
        cx.simulate_click(second_option.center(), Default::default());
        cx.run_until_parked();

        let (first_selection, second_selection, active_view) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.score_views[0]
                    .part_name
                    .as_ref()
                    .unwrap()
                    .as_str()
                    .to_string(),
                model.score_views[1]
                    .part_name
                    .as_ref()
                    .unwrap()
                    .as_str()
                    .to_string(),
                model.active_score_view,
            )
        });
        assert_eq!(first_selection, "part-a");
        assert_eq!(second_selection, "part-b");
        assert_eq!(active_view, 1);

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
            .save(&project_directory, &part, &project)
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
                target: Some(StatusAction::RevealIssue {
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

        cx.executor().advance_clock(Duration::from_millis(750));
        cx.run_until_parked();

        let clean_status = cx.update(|_, cx| model.read(cx).project_status(cx));
        assert_eq!(
            clean_status,
            status_bar::Status::Message("score changes saved".into())
        );
        assert_eq!(cx.debug_bounds("score-view-0").unwrap(), pane_before);
        assert_eq!(
            cx.debug_bounds("project-status-bar").unwrap(),
            status_bar_before
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn guarded_actions_flush_valid_score_changes_immediately(cx: &mut TestAppContext) {
        let root = temp_root("guarded-score-flush");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let (document, voices_button) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.score_documents[0].document.clone(),
                model.voices_button.clone(),
            )
        });
        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );

        model.update(cx, |model, cx| {
            model.on_voices_clicked(voices_button, &button::Clicked, cx);
        });

        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(cx.update(|_, cx| model.read(cx).active_dialog().is_some()));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn guarded_actions_stay_blocked_by_invalid_score_changes(cx: &mut TestAppContext) {
        let root = temp_root("guarded-invalid-score");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let (document, voices_button) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.score_documents[0].document.clone(),
                model.voices_button.clone(),
            )
        });
        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "not-a-note".to_string(), cx);
        });

        model.update(cx, |model, cx| {
            model.on_voices_clicked(voices_button, &button::Clicked, cx);
        });

        assert!(cx.update(|_, cx| model.read(cx).active_dialog().is_none()));
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );
        assert!(project_directory.join(".part-a.csv.recovery").is_file());
        assert!(matches!(
            cx.update(|_, cx| model.read(cx).project_status(cx)),
            status_bar::Status::Error {
                target: Some(StatusAction::RevealIssue { .. }),
                ..
            }
        ));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn failed_score_saves_retry_from_the_status_bar(cx: &mut TestAppContext) {
        let root = temp_root("score-save-retry");
        let project_directory = root.join("project");
        let moved_project_directory = root.join("project-unavailable");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());
        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });
        fs::rename(&project_directory, &moved_project_directory).unwrap();

        let error = model.update(cx, |model, cx| model.flush_all_score_changes(cx));
        assert!(error.is_err());
        let status = cx.update(|_, cx| model.read(cx).project_status(cx));
        match status {
            status_bar::Status::Error {
                message,
                target: Some(StatusAction::RetryScoreSave),
            } => assert!(message.as_ref().contains("click to retry")),
            status => panic!("expected a retryable save error, got {status:?}"),
        }

        fs::rename(&moved_project_directory, &project_directory).unwrap();
        let status_bar = cx.debug_bounds("project-status-bar").unwrap();
        cx.simulate_click(status_bar.center(), Default::default());
        cx.run_until_parked();

        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project_status(cx)),
            status_bar::Status::Message("score changes saved".into())
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_changes_autosave_and_invalid_edits_restore_from_recovery(cx: &mut TestAppContext) {
        let root = temp_root("score-autosave");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();

        let project_for_restore = project.clone();
        let directory_for_restore = project_directory.clone();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );

        cx.executor().advance_clock(Duration::from_millis(749));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();

        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));
        assert!(!project_directory.join(".part-a.csv.recovery").exists());
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project_status(cx)),
            status_bar::Status::Message("score changes saved".into())
        );

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "half-typed".to_string(), cx);
        });
        cx.executor().advance_clock(Duration::from_millis(750));
        cx.run_until_parked();

        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(project_directory.join(".part-a.csv.recovery").is_file());
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));

        let (restored_model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project_for_restore, directory_for_restore, root.clone(), cx)
        });
        let restored_document =
            cx.update(|_, cx| restored_model.read(cx).score_documents[0].document.clone());
        assert_eq!(
            cx.update(|_, cx| restored_document.read(cx).score().rows()[0][0].clone()),
            "half-typed"
        );
        assert!(cx.update(|_, cx| restored_document.read(cx).is_dirty()));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn continuous_score_edits_checkpoint_within_the_maximum_delay(cx: &mut TestAppContext) {
        let root = temp_root("continuous-score-autosave");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());

        for midi_note in 60..70 {
            document.update(cx, |document, cx| {
                document.update_cell(u64::MAX, 0, 0, midi_note.to_string(), cx);
            });
            if midi_note < 69 {
                cx.executor().advance_clock(Duration::from_millis(500));
            }
        }

        cx.executor().advance_clock(Duration::from_millis(499));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );

        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n69\n"
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
