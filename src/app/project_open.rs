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
    part::{self, PartName, PartScore, SubdivisionPattern},
    playback::{BeatRange, Playback, PlaybackLoop},
    project::{self, Project},
    style as s,
    view::{
        button::{self, Button},
        dialog::destructive_dialog,
        dropdown::{self, Dropdown},
        status_bar,
    },
    voice_name::VoiceName,
};

use self::{
    loop_range::{LoopWorkspace, Msg as LoopWorkspaceMsg},
    parts::PartsWorkspace,
    project_settings::{ProjectSettingsMsg, ProjectSettingsWorkspace},
    score::{
        DocumentEvent, ExportRowsConfirmed, ExportRowsDialog, ExportRowsDialogMsg,
        ExportRowsRequested, PartLoopRequested, PartSelected, RowEditConfirmation,
        RowEditConfirmationMsg, RowEditRequested, SaveState, ScoreDocument, ScoreEditor,
    },
    voices::VoicesWorkspace,
};

const PLAYHEAD_REFRESH_INTERVAL: Duration = Duration::from_millis(16);

pub enum Msg {
    CloseRequested,
}

pub struct Model {
    project: Project,
    project_directory: PathBuf,
    workspace_root: PathBuf,
    workspace: Workspace,
    score_button: Entity<Button>,
    settings_button: Entity<Button>,
    parts_button: Entity<Button>,
    voices_button: Entity<Button>,
    close_button: Entity<Button>,
    pane_count_dropdown: Entity<Dropdown>,
    loop_button: Entity<Button>,
    play_button: Entity<Button>,
    stop_button: Entity<Button>,
    project_overlay: Option<ProjectOverlay>,
    score_documents: Vec<ScoreDocumentEntry>,
    score_views: Vec<ScoreViewEntry>,
    active_score_view: usize,
    loop_range: Option<BeatRange>,
    playback: Option<ActivePlayback>,
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

struct ActivePlayback {
    output: Playback,
    target: PlaybackTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PlaybackTarget {
    Arrangement,
    Part(PartName),
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

struct Workspace {
    section: WorkspaceSection,
    parts: Entity<PartsWorkspace>,
    voices: Entity<VoicesWorkspace>,
    loop_editor: Entity<LoopWorkspace>,
    project_settings: Entity<ProjectSettingsWorkspace>,
}

impl Workspace {
    fn has_draft(&self, cx: &App) -> bool {
        self.parts.read(cx).has_draft()
            || self.voices.read(cx).has_draft()
            || self.loop_editor.read(cx).is_dirty(cx)
            || self.project_settings.read(cx).is_dirty(cx)
    }
}

enum WorkspaceSection {
    Score {
        overlay: Option<score::Overlay>,
    },
    Parts {
        overlay: Option<parts::Overlay>,
    },
    Voices {
        overlay: Option<voices::Overlay>,
    },
    Loop,
    Project {
        overlay: Option<project_settings::Overlay>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceSectionKind {
    Score,
    Parts,
    Voices,
    Loop,
    Project,
}

impl WorkspaceSection {
    fn kind(&self) -> WorkspaceSectionKind {
        match self {
            Self::Score { .. } => WorkspaceSectionKind::Score,
            Self::Parts { .. } => WorkspaceSectionKind::Parts,
            Self::Voices { .. } => WorkspaceSectionKind::Voices,
            Self::Loop => WorkspaceSectionKind::Loop,
            Self::Project { .. } => WorkspaceSectionKind::Project,
        }
    }

    fn has_overlay(&self) -> bool {
        match self {
            Self::Score { overlay } => overlay.is_some(),
            Self::Parts { overlay } => overlay.is_some(),
            Self::Voices { overlay } => overlay.is_some(),
            Self::Loop => false,
            Self::Project { overlay } => overlay.is_some(),
        }
    }

    fn overlay_element(&self) -> Option<AnyElement> {
        match self {
            Self::Score {
                overlay: Some(overlay),
            } => Some(overlay.element()),
            Self::Parts {
                overlay: Some(overlay),
            } => Some(overlay.element()),
            Self::Voices {
                overlay: Some(overlay),
            } => Some(overlay.element()),
            Self::Project {
                overlay: Some(overlay),
            } => Some(overlay.element()),
            Self::Score { overlay: None }
            | Self::Parts { overlay: None }
            | Self::Voices { overlay: None }
            | Self::Loop
            | Self::Project { overlay: None } => None,
        }
    }
}

enum ProjectOverlay {
    ConfirmClose(Entity<CloseProjectDialog>),
}

enum CloseProjectMsg {
    Cancelled,
    Confirmed,
}

struct CloseProjectDialog {
    cancel_button: Entity<Button>,
    confirm_button: Entity<Button>,
}

impl EventEmitter<CloseProjectMsg> for CloseProjectDialog {}

impl CloseProjectDialog {
    fn new(cx: &mut Context<Self>) -> Self {
        let cancel_button = cx.new(|_| Button::new("keep-project-open", "keep project open"));
        let confirm_button =
            cx.new(|_| Button::new("confirm-close-project", "discard drafts and close"));
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&confirm_button, Self::on_confirm_clicked)
            .detach();
        Self {
            cancel_button,
            confirm_button,
        }
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(CloseProjectMsg::Cancelled);
    }

    fn on_confirm_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(CloseProjectMsg::Confirmed);
    }
}

impl Render for CloseProjectDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        destructive_dialog(
            "close project",
            None,
            "discard unfinished workspace changes and close this project?",
            button::action_group([self.cancel_button.clone(), self.confirm_button.clone()])
                .justify_end(),
        )
    }
}

impl EventEmitter<Msg> for Model {}

impl Model {
    pub fn new(
        project: Project,
        project_directory: PathBuf,
        workspace_root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let score_button = cx.new(|_| Button::new("score-workspace", "score").depressed(true));
        let settings_button = cx.new(|_| Button::new("project-settings", "project"));
        let parts_button = cx.new(|_| Button::new("parts", "parts"));
        let voices_button = cx.new(|_| Button::new("voices", "voices"));
        let close_button = cx.new(|_| Button::new("close-project", "close project"));
        let pane_count_dropdown =
            cx.new(|cx| Dropdown::new("score-pane-count", ["1 pane", "2 panes", "3 panes"], 0, cx));
        let arrangement_beat_count = project.arrangement_beat_count();
        let loop_range = BeatRange::new(1, arrangement_beat_count, arrangement_beat_count).ok();
        let loop_button = cx.new(|_| Button::new("loop-workspace", "loop"));
        let play_button = cx.new(|_| Button::new("play-score", "play"));
        let stop_button = cx.new(|_| Button::new("stop-score", "stop"));

        cx.subscribe(&score_button, Self::on_score_clicked).detach();
        cx.subscribe(&settings_button, Self::on_settings_clicked)
            .detach();
        cx.subscribe(&parts_button, Self::on_parts_clicked).detach();
        cx.subscribe(&voices_button, Self::on_voices_clicked)
            .detach();
        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&pane_count_dropdown, Self::on_pane_count_selected)
            .detach();
        cx.subscribe(&loop_button, Self::on_loop_clicked).detach();
        cx.subscribe(&play_button, Self::on_play_clicked).detach();
        cx.subscribe(&stop_button, Self::on_stop_clicked).detach();

        let parts = project.parts.clone();
        let sequence = project.sequence().to_vec();
        let parts_workspace = cx.new(move |cx| PartsWorkspace::new(parts, sequence, cx));
        cx.subscribe(&parts_workspace, Self::on_parts_msg).detach();

        let voices = project.voices().to_vec();
        let acoustic_scene = project.acoustic_scene().clone();
        let voices_workspace = cx.new(move |cx| VoicesWorkspace::new(voices, acoustic_scene, cx));
        cx.subscribe(&voices_workspace, Self::on_voices_msg)
            .detach();

        let occurrences = project.arrangement_occurrences();
        let loop_workspace = cx.new(move |cx| LoopWorkspace::new(occurrences, loop_range, cx));
        cx.subscribe(&loop_workspace, Self::on_loop_range_msg)
            .detach();

        let settings_project = project.clone();
        let settings_project_directory = project_directory.clone();
        let settings_workspace_root = workspace_root.clone();
        let project_settings = cx.new(move |cx| {
            ProjectSettingsWorkspace::new(
                settings_project,
                settings_project_directory,
                settings_workspace_root,
                cx,
            )
        });
        cx.subscribe(&project_settings, Self::on_settings_msg)
            .detach();

        let initial_part = project.parts.first().map(|part| part.name.clone());
        let mut model = Self {
            project,
            project_directory,
            workspace_root,
            workspace: Workspace {
                section: WorkspaceSection::Score { overlay: None },
                parts: parts_workspace,
                voices: voices_workspace,
                loop_editor: loop_workspace,
                project_settings,
            },
            score_button,
            settings_button,
            parts_button,
            voices_button,
            close_button,
            pane_count_dropdown,
            loop_button,
            play_button,
            stop_button,
            project_overlay: None,
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
            crate::view::workspace::selector([
                self.score_button.clone(),
                self.parts_button.clone(),
                self.voices_button.clone(),
                self.loop_button.clone(),
                self.settings_button.clone(),
            ])
            .into_any_element(),
            div()
                .flex()
                .gap(s::S3)
                .child(self.pane_count_dropdown.clone())
                .into_any_element(),
            div()
                .flex()
                .items_center()
                .gap(s::S3)
                .child(
                    div()
                        .max_w(s::S8)
                        .truncate()
                        .text_color(s::TEXT_DEFAULT)
                        .child(loop_range_summary(&self.project, self.loop_range)),
                )
                .child(self.play_button.clone())
                .child(self.stop_button.clone())
                .into_any_element(),
            self.close_button.clone().into_any_element(),
        ]
    }

    pub fn active_overlay(&self) -> Option<AnyElement> {
        if let Some(ProjectOverlay::ConfirmClose(overlay)) = &self.project_overlay {
            return Some(overlay.clone().into_any_element());
        }
        self.workspace.section.overlay_element()
    }

    fn has_active_overlay(&self) -> bool {
        self.project_overlay.is_some() || self.workspace.section.has_overlay()
    }

    fn set_score_overlay(&mut self, overlay: Option<score::Overlay>, cx: &mut Context<Self>) {
        let WorkspaceSection::Score {
            overlay: active_overlay,
        } = &mut self.workspace.section
        else {
            return;
        };
        *active_overlay = overlay;
        cx.notify();
    }

    fn set_parts_overlay(&mut self, overlay: Option<parts::Overlay>, cx: &mut Context<Self>) {
        let WorkspaceSection::Parts {
            overlay: active_overlay,
        } = &mut self.workspace.section
        else {
            return;
        };
        *active_overlay = overlay;
        cx.notify();
    }

    fn set_voices_overlay(&mut self, overlay: Option<voices::Overlay>, cx: &mut Context<Self>) {
        let WorkspaceSection::Voices {
            overlay: active_overlay,
        } = &mut self.workspace.section
        else {
            return;
        };
        *active_overlay = overlay;
        cx.notify();
    }

    fn set_project_settings_overlay(
        &mut self,
        overlay: Option<project_settings::Overlay>,
        cx: &mut Context<Self>,
    ) {
        let WorkspaceSection::Project {
            overlay: active_overlay,
        } = &mut self.workspace.section
        else {
            return;
        };
        *active_overlay = overlay;
        cx.notify();
    }

    fn set_workspace_section(&mut self, section: WorkspaceSection, cx: &mut Context<Self>) {
        if self.has_active_overlay() || self.workspace.section.kind() == section.kind() {
            return;
        }
        self.workspace.section = section;
        self.sync_workspace_buttons(cx);
        cx.notify();
    }

    fn sync_workspace_buttons(&self, cx: &mut Context<Self>) {
        let selected = self.workspace.section.kind();
        let sections = [
            (&self.score_button, selected == WorkspaceSectionKind::Score),
            (&self.parts_button, selected == WorkspaceSectionKind::Parts),
            (
                &self.voices_button,
                selected == WorkspaceSectionKind::Voices,
            ),
            (&self.loop_button, selected == WorkspaceSectionKind::Loop),
            (
                &self.settings_button,
                selected == WorkspaceSectionKind::Project,
            ),
        ];
        for (button, depressed) in sections {
            button.update(cx, |button, cx| {
                button.set_depressed(depressed, cx);
            });
        }
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
        self.flush_part_score_changes_for(std::slice::from_ref(part_name), cx)
    }

    fn flush_part_score_changes_for(
        &self,
        part_names: &[PartName],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let documents = self
            .score_documents
            .iter()
            .filter(|entry| {
                part_names
                    .iter()
                    .any(|name| entry.part_name.eq_ignore_ascii_case(name))
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

        let mut parse_issues = Vec::new();
        for entry in &self.score_documents {
            let document = entry.document.read(cx);
            parse_issues.extend(
                document
                    .parse_issues()
                    .iter()
                    .cloned()
                    .map(|issue| (entry.part_name.clone(), issue)),
            );
        }
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
        let clears_workspace_error = match event {
            DocumentEvent::CellChanged { .. }
            | DocumentEvent::RowsCleared
            | DocumentEvent::StructureChanged { .. }
            | DocumentEvent::SaveFailed => true,
            DocumentEvent::Saved
            | DocumentEvent::RecoverySaved
            | DocumentEvent::Reset
            | DocumentEvent::ProjectChanged => false,
        };
        if clears_workspace_error {
            self.workspace_error = None;
        }
        if let DocumentEvent::Saved = event {
            if !self.has_unsaved_score(cx) {
                self.workspace_error = None;
            }
        }
        let changes_playback = match event {
            DocumentEvent::CellChanged { .. }
            | DocumentEvent::RowsCleared
            | DocumentEvent::StructureChanged { .. }
            | DocumentEvent::Reset
            | DocumentEvent::ProjectChanged => true,
            DocumentEvent::Saved | DocumentEvent::RecoverySaved | DocumentEvent::SaveFailed => {
                false
            }
        };
        if self.playback.is_some() && changes_playback {
            self.update_live_playback(cx);
        }
        cx.notify();
    }

    fn arrangement_playback_loop(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<PlaybackLoop, String> {
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

    fn part_playback_loop(
        &mut self,
        part_name: &PartName,
        cx: &mut Context<Self>,
    ) -> Result<PlaybackLoop, String> {
        let document = self.score_document(part_name, cx)?;
        let document = document.read(cx);
        PlaybackLoop::from_part(&self.project, document.part(), document.score())
            .map_err(|error| error.to_string())
    }

    fn playback_loop_for_target(
        &mut self,
        target: &PlaybackTarget,
        cx: &mut Context<Self>,
    ) -> Result<PlaybackLoop, String> {
        match target {
            PlaybackTarget::Arrangement => self.arrangement_playback_loop(cx),
            PlaybackTarget::Part(part_name) => self.part_playback_loop(part_name, cx),
        }
    }

    fn update_live_playback(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self
            .playback
            .as_ref()
            .map(|playback| playback.target.clone())
        else {
            return;
        };
        match self.playback_loop_for_target(&target, cx) {
            Ok(playback_loop) => {
                if let Some(playback) = &self.playback {
                    playback.output.update(playback_loop);
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
        let target = PlaybackTarget::Arrangement;
        let playback_loop = match self.playback_loop_for_target(&target, cx) {
            Ok(playback_loop) => playback_loop,
            Err(error) => {
                self.transport_error = Some(error);
                cx.notify();
                return;
            }
        };

        self.start_playback(target, playback_loop, cx);
    }

    fn start_playback(
        &mut self,
        target: PlaybackTarget,
        playback_loop: PlaybackLoop,
        cx: &mut Context<Self>,
    ) {
        self.playhead_task.take();
        self.clear_playhead_highlights(cx);
        self.playback = None;
        match Playback::start(playback_loop) {
            Ok(playback) => {
                self.playback = Some(ActivePlayback {
                    output: playback,
                    target,
                });
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
            let beat = playback.output.current_arrangement_beat();
            match &playback.target {
                PlaybackTarget::Arrangement => playing_score_row(&self.project, beat),
                PlaybackTarget::Part(part_name) => usize::try_from(beat.checked_sub(1)?)
                    .ok()
                    .map(|row| (part_name.clone(), row)),
            }
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
        cx.subscribe(&editor, Self::on_score_editor_row_edit_requested)
            .detach();
        cx.subscribe(&editor, Self::on_score_editor_part_loop_requested)
            .detach();
        cx.subscribe(&editor, Self::on_score_editor_export_rows_requested)
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

    fn on_score_editor_part_loop_requested(
        &mut self,
        editor: Entity<ScoreEditor>,
        request: &PartLoopRequested,
        cx: &mut Context<Self>,
    ) {
        if let Some(view_index) = self
            .score_views
            .iter()
            .position(|view| view.editor.as_ref() == Some(&editor))
        {
            self.activate_score_view(view_index, cx);
        }

        let target = PlaybackTarget::Part(request.part_name.clone());
        match self.playback_loop_for_target(&target, cx) {
            Ok(playback_loop) => self.start_playback(target, playback_loop, cx),
            Err(error) => {
                self.transport_error = Some(error);
                cx.notify();
            }
        }
    }

    fn on_score_editor_export_rows_requested(
        &mut self,
        editor: Entity<ScoreEditor>,
        request: &ExportRowsRequested,
        cx: &mut Context<Self>,
    ) {
        if self.has_active_overlay() {
            return;
        }
        if let Some(view_index) = self
            .score_views
            .iter()
            .position(|view| view.editor.as_ref() == Some(&editor))
        {
            self.activate_score_view(view_index, cx);
        }

        let request = request.clone();
        let dialog = cx.new(move |cx| ExportRowsDialog::new(request, cx));
        cx.subscribe(&dialog, Self::on_export_rows_dialog_msg)
            .detach();
        self.set_score_overlay(Some(score::Overlay::ExportRows(dialog)), cx);
    }

    fn on_export_rows_dialog_msg(
        &mut self,
        dialog: Entity<ExportRowsDialog>,
        msg: &ExportRowsDialogMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            ExportRowsDialogMsg::Cancelled => {
                self.set_score_overlay(None, cx);
            }
            ExportRowsDialogMsg::Confirmed(request) => {
                self.export_selected_rows(dialog, request, cx);
            }
        }
        cx.notify();
    }

    fn export_selected_rows(
        &mut self,
        dialog: Entity<ExportRowsDialog>,
        request: &ExportRowsConfirmed,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self
            .score_documents
            .iter()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(&request.part_name))
            .map(|entry| entry.document.clone())
        else {
            dialog.update(cx, |dialog, cx| {
                dialog.export_failed(
                    format!("part {:?} no longer exists", request.part_name.as_str()),
                    cx,
                );
            });
            return;
        };
        let score = document.read(cx).score().clone();

        match export_project_part_rows(
            &self.project_directory,
            &mut self.project,
            &request.part_name,
            &score,
            request.rows,
            &request.new_part_name,
        ) {
            Ok(part) => {
                let part_name = part.name.clone();
                self.set_score_overlay(None, cx);
                self.update_score_documents_for_project_settings(cx);
                self.select_part(part_name, cx);
                self.sync_score_editor_parts(cx);
                self.sync_workspace_project(cx);
                self.workspace_error = None;
            }
            Err(error) => {
                dialog.update(cx, |dialog, cx| {
                    dialog.export_failed(error.to_string(), cx);
                });
            }
        }
    }

    fn on_score_editor_row_edit_requested(
        &mut self,
        _: Entity<ScoreEditor>,
        request: &RowEditRequested,
        cx: &mut Context<Self>,
    ) {
        if self.has_active_overlay() {
            return;
        }
        let destructive_edit = match &request.edit {
            part::PartRowEdit::Clear(_) | part::PartRowEdit::Delete(_) => true,
            part::PartRowEdit::InsertBefore(_) | part::PartRowEdit::InsertAfter(_) => false,
        };
        let needs_confirmation = request.populated_cell_count > 0 && destructive_edit;
        if needs_confirmation {
            let request = request.clone();
            let dialog = cx.new(move |cx| RowEditConfirmation::new(request, cx));
            cx.subscribe(&dialog, Self::on_row_edit_confirmation_msg)
                .detach();
            self.set_score_overlay(Some(score::Overlay::RowEdit(dialog)), cx);
        } else {
            self.apply_row_edit(request.clone(), cx);
        }
    }

    fn on_row_edit_confirmation_msg(
        &mut self,
        _: Entity<RowEditConfirmation>,
        msg: &RowEditConfirmationMsg,
        cx: &mut Context<Self>,
    ) {
        self.set_score_overlay(None, cx);
        if let RowEditConfirmationMsg::Confirmed(request) = msg {
            self.apply_row_edit(request.clone(), cx);
        }
        cx.notify();
    }

    fn apply_row_edit(&mut self, request: RowEditRequested, cx: &mut Context<Self>) {
        let Some(document) = self
            .score_documents
            .iter()
            .find(|entry| entry.part_name.eq_ignore_ascii_case(&request.part_name))
            .map(|entry| entry.document.clone())
        else {
            self.workspace_error = Some(format!(
                "part {:?} no longer exists",
                request.part_name.as_str()
            ));
            cx.notify();
            return;
        };

        if let part::PartRowEdit::Clear(rows) = request.edit {
            match document.update(cx, |document, cx| document.clear_rows(rows, cx)) {
                Ok(()) => self.workspace_error = None,
                Err(error) => self.workspace_error = Some(error.to_string()),
            }
            cx.notify();
            return;
        }

        let score = document.read(cx).score().clone();
        let Some(selected_rows) = request.edit.selection_after(score.rows().len()) else {
            self.workspace_error =
                Some("a part must keep at least one beat; clear the rows instead".to_string());
            cx.notify();
            return;
        };
        let previous_arrangement_beat_count = self.project.arrangement_beat_count();
        match project::edit_part_rows(
            &self.project_directory,
            &self.project,
            &request.part_name,
            &score,
            request.edit,
        ) {
            Ok((project, part, score)) => {
                self.stop_playback(cx);
                self.project = project;
                let updated_project = self.project.clone();
                document.update(cx, |document, cx| {
                    document.apply_saved_structure_change(
                        updated_project.clone(),
                        part,
                        score,
                        request.source_editor,
                        selected_rows,
                        cx,
                    );
                });
                for entry in &self.score_documents {
                    if !entry.part_name.eq_ignore_ascii_case(&request.part_name) {
                        let project = updated_project.clone();
                        entry.document.update(cx, |document, cx| {
                            document.project_settings_changed(project, cx);
                        });
                    }
                }
                self.reconcile_loop_range(previous_arrangement_beat_count, cx);
                self.sync_workspace_project(cx);
                self.workspace_error = None;
            }
            Err(error) => self.workspace_error = Some(error.to_string()),
        }
        cx.notify();
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

    fn sync_workspace_project(&self, cx: &mut Context<Self>) {
        let project = self.project.clone();
        let parts = project.parts().to_vec();
        let sequence = project.sequence().to_vec();
        self.workspace.parts.update(cx, |workspace, cx| {
            workspace.sync_project(parts, sequence, cx);
        });

        let voices = project.voices().to_vec();
        let acoustic_scene = project.acoustic_scene().clone();
        self.workspace.voices.update(cx, |workspace, cx| {
            workspace.sync_project(voices, acoustic_scene, cx);
        });

        self.workspace.project_settings.update(cx, |workspace, cx| {
            workspace.sync_project(project, cx);
        });
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

    fn on_loop_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.set_workspace_section(WorkspaceSection::Loop, cx);
    }

    fn on_loop_range_msg(
        &mut self,
        workspace: Entity<LoopWorkspace>,
        msg: &LoopWorkspaceMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            LoopWorkspaceMsg::Applied(range) => {
                self.loop_range = Some(*range);
                workspace.update(cx, |workspace, cx| {
                    workspace.applied(*range, cx);
                });
                if self.playback.is_some() {
                    self.update_live_playback(cx);
                } else {
                    self.transport_error = None;
                }
            }
            LoopWorkspaceMsg::ResetRequested => self.reset_loop_workspace(cx),
        }
        cx.notify();
    }

    fn reset_loop_workspace(&mut self, cx: &mut Context<Self>) {
        let occurrences = self.project.arrangement_occurrences();
        let range = self.loop_range;
        let workspace = cx.new(move |cx| LoopWorkspace::new(occurrences, range, cx));
        cx.subscribe(&workspace, Self::on_loop_range_msg).detach();
        self.workspace.loop_editor = workspace;
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
        self.reset_loop_workspace(cx);
        if self.playback.is_some() {
            self.update_live_playback(cx);
        }
    }

    fn on_score_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.set_workspace_section(WorkspaceSection::Score { overlay: None }, cx);
    }

    fn on_settings_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.set_workspace_section(WorkspaceSection::Project { overlay: None }, cx);
    }

    fn on_voices_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.set_workspace_section(WorkspaceSection::Voices { overlay: None }, cx);
    }

    fn on_parts_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.set_workspace_section(WorkspaceSection::Parts { overlay: None }, cx);
    }

    fn on_settings_msg(
        &mut self,
        _: Entity<ProjectSettingsWorkspace>,
        msg: &ProjectSettingsMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            ProjectSettingsMsg::Saved(updated_project) => {
                self.project = updated_project.as_ref().clone();
                self.update_score_documents_for_project_settings(cx);
                self.sync_workspace_project(cx);
                if self.playback.is_some() {
                    self.update_live_playback(cx);
                }
            }
            ProjectSettingsMsg::ResetRequested => self.reset_project_settings_workspace(cx),
            ProjectSettingsMsg::ResetConfirmationRequested => {
                let overlay = cx.new(project_settings::ResetDialog::new);
                cx.subscribe(&overlay, Self::on_project_settings_reset_dialog_msg)
                    .detach();
                self.set_project_settings_overlay(
                    Some(project_settings::Overlay::ConfirmReset(overlay)),
                    cx,
                );
            }
        }
        cx.notify();
    }

    fn on_project_settings_reset_dialog_msg(
        &mut self,
        _: Entity<project_settings::ResetDialog>,
        msg: &project_settings::ResetDialogMsg,
        cx: &mut Context<Self>,
    ) {
        self.set_project_settings_overlay(None, cx);
        match msg {
            project_settings::ResetDialogMsg::Cancelled => {}
            project_settings::ResetDialogMsg::Confirmed => {
                self.reset_project_settings_workspace(cx);
            }
        }
    }

    fn reset_project_settings_workspace(&mut self, cx: &mut Context<Self>) {
        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        let workspace_root = self.workspace_root.clone();
        let workspace = cx.new(move |cx| {
            ProjectSettingsWorkspace::new(project, project_directory, workspace_root, cx)
        });
        cx.subscribe(&workspace, Self::on_settings_msg).detach();
        self.workspace.project_settings = workspace;
    }

    fn on_voices_msg(
        &mut self,
        workspace: Entity<VoicesWorkspace>,
        msg: &voices::Msg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            voices::Msg::Change(change) => self.apply_voice_change(workspace, change, cx),
            voices::Msg::DeleteRequested { name } => {
                self.open_voice_delete_dialog(name.clone(), cx);
            }
        }
    }

    fn apply_voice_change(
        &mut self,
        workspace: Entity<VoicesWorkspace>,
        change: &voices::Change,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.flush_all_score_changes(cx) {
            let message = format!("couldn't save score changes: {error}");
            workspace.update(cx, |workspace, cx| match change {
                voices::Change::Add { .. } => workspace.add_failed(message, cx),
                voices::Change::Edit { .. } => workspace.edit_failed(message, cx),
            });
            cx.notify();
            return;
        }

        match change {
            voices::Change::Add {
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
                        workspace.update(cx, |workspace, cx| {
                            workspace.voice_added(voices, added, cx);
                        });
                        self.refresh_score_documents_after_voice_change(cx);
                        self.sync_workspace_project(cx);
                    }
                    Err(error) => {
                        workspace.update(cx, |workspace, cx| {
                            workspace.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            voices::Change::Edit {
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
                        workspace.update(cx, |workspace, cx| {
                            workspace.voice_edited(voices, edited, cx);
                        });
                        self.refresh_score_documents_after_voice_change(cx);
                        self.sync_workspace_project(cx);
                    }
                    Err(error) => {
                        workspace.update(cx, |workspace, cx| {
                            workspace.edit_failed(error.to_string(), cx);
                        });
                    }
                }
            }
        }

        cx.notify();
    }

    fn open_voice_delete_dialog(&mut self, name: VoiceName, cx: &mut Context<Self>) {
        if self.has_active_overlay() {
            return;
        }
        let dialog = cx.new(|cx| voices::DeleteDialog::new(name, cx));
        cx.subscribe(&dialog, Self::on_voice_delete_dialog_msg)
            .detach();
        self.set_voices_overlay(Some(voices::Overlay::ConfirmDelete(dialog)), cx);
    }

    fn on_voice_delete_dialog_msg(
        &mut self,
        dialog: Entity<voices::DeleteDialog>,
        msg: &voices::DeleteDialogMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            voices::DeleteDialogMsg::Cancelled => self.set_voices_overlay(None, cx),
            voices::DeleteDialogMsg::Confirmed { name } => {
                self.delete_voice_from_dialog(dialog, name, cx)
            }
        }
    }

    fn delete_voice_from_dialog(
        &mut self,
        confirmation: Entity<voices::DeleteDialog>,
        name: &VoiceName,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.flush_all_score_changes(cx) {
            confirmation.update(cx, |dialog, cx| {
                dialog.failed(format!("couldn't save score changes: {error}"), cx);
            });
            return;
        }

        match project::delete_voice(&self.project_directory, &self.project, name) {
            Ok(updated_project) => {
                self.project = updated_project;
                let voices = self.project.voices().to_vec();
                self.workspace.voices.update(cx, |workspace, cx| {
                    workspace.voice_deleted(voices, name, cx);
                });
                self.refresh_score_documents_after_voice_change(cx);
                self.sync_workspace_project(cx);
                self.set_voices_overlay(None, cx);
            }
            Err(error) => {
                confirmation.update(cx, |dialog, cx| {
                    dialog.failed(error.to_string(), cx);
                });
            }
        }
        cx.notify();
    }

    fn on_parts_msg(
        &mut self,
        dialog: Entity<PartsWorkspace>,
        msg: &parts::Msg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            parts::Msg::Add {
                name,
                length,
                subdivision_pattern,
            } => {
                match create_configured_project_part(
                    &self.project_directory,
                    &mut self.project,
                    name,
                    *length,
                    subdivision_pattern.clone(),
                ) {
                    Ok(part) => {
                        let added_name = part.name.clone();
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_added(parts, part.name, cx);
                        });
                        self.select_part(added_name, cx);
                        self.sync_score_editor_parts(cx);
                        self.sync_workspace_project(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::Duplicate { source, name } => {
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
                        self.sync_workspace_project(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.duplicate_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::Update {
                source,
                name,
                subdivision_pattern,
            } => {
                if let Err(error) = self.flush_part_score_changes(source, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog.update_failed(format!("couldn't save score changes: {error}"), cx);
                    });
                    return;
                }
                match update_project_part(
                    &self.project_directory,
                    &mut self.project,
                    source,
                    name,
                    subdivision_pattern.clone(),
                ) {
                    Ok(part) => {
                        let updated_name = part.name.clone();
                        let project = self.project.clone();
                        for entry in &mut self.score_documents {
                            if entry.part_name.eq_ignore_ascii_case(source) {
                                entry.part_name = updated_name.clone();
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
                                view.part_name = Some(updated_name.clone());
                            }
                        }
                        let parts = self.project.parts.clone();
                        let sequence = self.project.sequence().to_vec();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_updated(parts, sequence, updated_name, cx);
                        });
                        self.sync_score_editor_parts(cx);
                        self.sync_workspace_project(cx);
                        self.workspace_error = None;
                        if self.playback.is_some() {
                            self.update_live_playback(cx);
                        }
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.update_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::DeleteRequested { name } => {
                self.open_part_delete_dialog(name.clone(), cx);
            }
            parts::Msg::Combine { sources, name } => {
                if let Err(error) = self.flush_part_score_changes_for(sources, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog.combine_failed(
                            format!("couldn't save source score changes: {error}"),
                            cx,
                        );
                    });
                    return;
                }
                match combine_project_parts(
                    &self.project_directory,
                    &mut self.project,
                    sources,
                    name,
                ) {
                    Ok(part) => {
                        let combined_name = part.name.clone();
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.part_added(parts, part.name, cx);
                        });
                        self.update_score_documents_for_project_settings(cx);
                        self.select_part(combined_name, cx);
                        self.sync_score_editor_parts(cx);
                        self.sync_workspace_project(cx);
                        self.workspace_error = None;
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.combine_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Msg::SequenceChange {
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
                        self.sync_workspace_project(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.sequence_change_failed(error.to_string(), cx);
                        });
                    }
                }
            }
        }

        cx.notify();
    }

    fn open_part_delete_dialog(&mut self, name: PartName, cx: &mut Context<Self>) {
        if self.has_active_overlay() {
            return;
        }
        let dialog = cx.new(|cx| parts::DeleteDialog::new(name, cx));
        cx.subscribe(&dialog, Self::on_part_delete_dialog_msg)
            .detach();
        self.set_parts_overlay(Some(parts::Overlay::ConfirmDelete(dialog)), cx);
    }

    fn on_part_delete_dialog_msg(
        &mut self,
        dialog: Entity<parts::DeleteDialog>,
        msg: &parts::DeleteDialogMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            parts::DeleteDialogMsg::Cancelled => self.set_parts_overlay(None, cx),
            parts::DeleteDialogMsg::Confirmed { name } => {
                self.delete_part_from_dialog(dialog, name, cx)
            }
        }
    }

    fn delete_part_from_dialog(
        &mut self,
        confirmation: Entity<parts::DeleteDialog>,
        name: &PartName,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.flush_part_score_changes(name, cx) {
            confirmation.update(cx, |dialog, cx| {
                dialog.failed(format!("couldn't save score changes: {error}"), cx);
            });
            return;
        }

        match delete_project_part(&self.project_directory, &mut self.project, name) {
            Ok(part) => {
                let parts = self.project.parts.clone();
                self.workspace.parts.update(cx, |workspace, cx| {
                    workspace.part_deleted(parts, &part.name, cx);
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
                self.sync_workspace_project(cx);
                self.set_parts_overlay(None, cx);
            }
            Err(error) => {
                confirmation.update(cx, |dialog, cx| {
                    dialog.failed(error.to_string(), cx);
                });
            }
        }
        cx.notify();
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
        if self.has_active_overlay() {
            return;
        }

        if self.workspace.has_draft(cx) {
            let overlay = cx.new(CloseProjectDialog::new);
            cx.subscribe(&overlay, Self::on_close_project_msg).detach();
            self.project_overlay = Some(ProjectOverlay::ConfirmClose(overlay));
            cx.notify();
            return;
        }

        self.finish_close(cx);
    }

    fn on_close_project_msg(
        &mut self,
        _: Entity<CloseProjectDialog>,
        msg: &CloseProjectMsg,
        cx: &mut Context<Self>,
    ) {
        self.project_overlay = None;
        match msg {
            CloseProjectMsg::Cancelled => cx.notify(),
            CloseProjectMsg::Confirmed => self.finish_close(cx),
        }
    }

    fn finish_close(&mut self, cx: &mut Context<Self>) {
        if self.flush_all_score_changes(cx).is_err() {
            cx.notify();
            return;
        }

        cx.emit(Msg::CloseRequested);
    }
}

impl Render for Model {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.workspace.section {
            WorkspaceSection::Score { .. } => {
                let project_status = self.project_status(cx);
                score_workspace(&self.score_views, project_status, cx).into_any_element()
            }
            WorkspaceSection::Parts { .. } => self.workspace.parts.clone().into_any_element(),
            WorkspaceSection::Voices { .. } => self.workspace.voices.clone().into_any_element(),
            WorkspaceSection::Loop => self.workspace.loop_editor.clone().into_any_element(),
            WorkspaceSection::Project { .. } => {
                self.workspace.project_settings.clone().into_any_element()
            }
        }
    }
}

fn loop_range_summary(project: &Project, range: Option<BeatRange>) -> String {
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

fn score_workspace(
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

impl fmt::Display for PartChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => write!(f, "failed to recover a project update: {error}"),
            Self::CreateFile(error) => write!(f, "{error}"),
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

impl std::error::Error for PartChangeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Recovery(error) => Some(error),
            Self::CreateFile(error) => Some(error),
            Self::LoadCombinationScore { source, .. } => Some(source),
            Self::SaveCombinedScore { source, .. } => Some(source),
            Self::ExportScore { source, .. } => Some(source),
            Self::RenameFile(error) => Some(error),
            Self::DeleteFile(error) => Some(error),
            Self::SaveCreated { source, .. }
            | Self::SaveDeleted { source, .. }
            | Self::SaveRenamed { source, .. } => Some(source),
            Self::CombineNeedsTwoParts
            | Self::CombinedPartTooLong
            | Self::ExportSelectionOutOfBounds
            | Self::MissingPart(_)
            | Self::PartInSequence { .. } => None,
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

#[cfg(test)]
fn create_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &str,
    length: u32,
) -> Result<part::Part, PartChangeError> {
    create_configured_project_part(project_directory, project, name, length, None)
}

fn create_configured_project_part(
    project_directory: &Path,
    project: &mut Project,
    name: &str,
    length: u32,
    subdivision_pattern: Option<SubdivisionPattern>,
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
    let part = created
        .part()
        .clone()
        .with_subdivision_pattern(subdivision_pattern);
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

fn combine_project_parts(
    project_directory: &Path,
    project: &mut Project,
    sources: &[PartName],
    name: &str,
) -> Result<part::Part, PartChangeError> {
    project::recover_pending_project_update(project_directory)
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
        .with_subdivision_pattern(parts::combined_subdivision_pattern(&source_parts));
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

fn export_project_part_rows(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    source_score: &PartScore,
    rows: part::ScoreRowRange,
    name: &str,
) -> Result<part::Part, PartChangeError> {
    project::recover_pending_project_update(project_directory)
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
        .with_subdivision_pattern(source_part.subdivision_pattern().cloned());

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
fn rename_project_part(
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

fn update_project_part(
    project_directory: &Path,
    project: &mut Project,
    source_name: &PartName,
    name: &str,
    subdivision_pattern: Option<SubdivisionPattern>,
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
    let renamed_part = renamed
        .part()
        .clone()
        .with_subdivision_pattern(subdivision_pattern);
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

    use super::score::{self, ScoreAction};
    use super::{
        combine_project_parts, create_configured_project_part, create_project_part,
        delete_project_part, duplicate_project_part, export_project_part_rows, loop_range_summary,
        parts, playing_score_row, project_settings, rename_project_part, update_project_sequence,
        voices, ExportRowsConfirmed, ExportRowsDialogMsg, Model, PartChangeError, PartsWorkspace,
        ProjectOverlay, RowEditConfirmationMsg, RowEditRequested, StatusAction, WorkspaceSection,
        WorkspaceSectionKind,
    };
    use crate::{
        part::{Part, PartName, PartRowEdit, PartScore, ScoreRowRange, SubdivisionPattern},
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
    fn loop_summaries_describe_part_aligned_and_exact_ranges() {
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_parts(vec![Part::new("intro", 8), Part::new("verse", 16)])
            .with_sequence(vec!["intro".into(), "verse".into(), "verse".into()]);

        assert_eq!(
            loop_range_summary(&project, BeatRange::new(1, 40, 40).ok()),
            "loop all"
        );
        assert_eq!(
            loop_range_summary(&project, BeatRange::new(9, 24, 40).ok()),
            "loop 2. verse"
        );
        assert_eq!(
            loop_range_summary(&project, BeatRange::new(9, 40, 40).ok()),
            "loop parts 2–3"
        );
        assert_eq!(
            loop_range_summary(&project, BeatRange::new(10, 23, 40).ok()),
            "loop beats 10–23"
        );
        assert_eq!(loop_range_summary(&project, None), "set loop");
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
        let PartChangeError::PartInSequence {
            occurrence_count, ..
        } = error
        else {
            panic!("deleting an arranged part should report its occurrences");
        };
        assert_eq!(occurrence_count, 2);
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
        let source = create_configured_project_part(
            &project_directory,
            &mut project,
            "intro",
            2,
            Some(SubdivisionPattern::new([4, 3, 3]).unwrap()),
        )
        .unwrap();
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
            duplicated.subdivision_pattern(),
            source.subdivision_pattern()
        );
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
    fn combined_parts_concatenate_an_explicit_source_list_without_changing_the_arrangement() {
        let root = temp_root("combine-project-parts");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let pattern = SubdivisionPattern::new([2]).unwrap();
        let intro = create_configured_project_part(
            &project_directory,
            &mut project,
            "intro",
            2,
            Some(pattern.clone()),
        )
        .unwrap();
        let verse = create_configured_project_part(
            &project_directory,
            &mut project,
            "verse",
            2,
            Some(pattern.clone()),
        )
        .unwrap();
        let outro = create_project_part(&project_directory, &mut project, "outro", 1).unwrap();
        PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]])
            .save(&project_directory, &intro, &project)
            .unwrap();
        PartScore::from_rows(vec![vec!["E4".to_string()], vec!["F4".to_string()]])
            .save(&project_directory, &verse, &project)
            .unwrap();
        update_project_sequence(
            &project_directory,
            &mut project,
            vec![
                intro.name.clone(),
                verse.name.clone(),
                verse.name.clone(),
                outro.name.clone(),
            ],
        )
        .unwrap();
        let sequence_before = project.sequence().to_vec();
        let sources = vec![intro.name.clone(), verse.name.clone(), verse.name.clone()];

        let combined = combine_project_parts(
            &project_directory,
            &mut project,
            &sources,
            "intro and verses",
        )
        .unwrap();

        assert_eq!(combined.length, 6);
        assert_eq!(
            combined.subdivision_pattern(),
            Some(&pattern),
            "a common subdivision pattern should be preserved"
        );
        assert_eq!(
            PartScore::load(&project_directory, &combined, project.voices())
                .unwrap()
                .rows(),
            [
                vec!["C4".to_string()],
                vec!["D4".to_string()],
                vec!["E4".to_string()],
                vec!["F4".to_string()],
                vec!["E4".to_string()],
                vec!["F4".to_string()],
            ]
        );
        assert_eq!(project.sequence(), sequence_before);
        assert!(project.part(&intro.name).is_some());
        assert!(project.part(&verse.name).is_some());
        assert_eq!(
            project::load_project(&project_directory).unwrap().project,
            project
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combining_requires_at_least_two_sources() {
        let root = temp_root("combine-project-parts-invalid-sources");
        let mut project = Project::new("test project", 800, 0, Seed::new(12));
        let project_directory = project::create_project(&root, &project).unwrap();
        let intro = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
        update_project_sequence(&project_directory, &mut project, vec![intro.name.clone()])
            .unwrap();

        let error = combine_project_parts(
            &project_directory,
            &mut project,
            std::slice::from_ref(&intro.name),
            "not combined",
        )
        .err()
        .unwrap();

        let PartChangeError::CombineNeedsTwoParts = error else {
            panic!("combining one part should require another source");
        };
        assert!(project.part(&"not combined".into()).is_none());
        assert!(!project_directory.join("not-combined.csv").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selected_rows_export_as_a_new_part_with_the_source_meter() {
        let root = temp_root("export-project-part-rows");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let source = create_configured_project_part(
            &project_directory,
            &mut project,
            "theme",
            4,
            Some(SubdivisionPattern::new([2, 3]).unwrap()),
        )
        .unwrap();
        let score = PartScore::from_rows(vec![
            vec!["C4".to_string()],
            vec!["D4".to_string()],
            vec!["E4".to_string()],
            vec!["F4".to_string()],
        ]);
        score.save(&project_directory, &source, &project).unwrap();

        let exported = export_project_part_rows(
            &project_directory,
            &mut project,
            &source.name,
            &score,
            ScoreRowRange::new(1, 2, 4).unwrap(),
            "theme middle",
        )
        .unwrap();

        assert_eq!(exported.length, 2);
        assert_eq!(exported.subdivision_pattern(), source.subdivision_pattern());
        assert_eq!(
            PartScore::load(&project_directory, &exported, project.voices())
                .unwrap()
                .rows(),
            [vec!["D4".to_string()], vec!["E4".to_string()]]
        );
        assert_eq!(
            project::load_project(&project_directory).unwrap().project,
            project
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_selected_rows_do_not_leave_an_incomplete_export() {
        let root = temp_root("invalid-export-project-part-rows");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let source = create_project_part(&project_directory, &mut project, "theme", 2).unwrap();
        let score =
            PartScore::from_rows(vec![vec!["not-a-note".to_string()], vec!["C4".to_string()]]);

        let error = export_project_part_rows(
            &project_directory,
            &mut project,
            &source.name,
            &score,
            ScoreRowRange::new(0, 0, 2).unwrap(),
            "broken excerpt",
        )
        .unwrap_err();

        let PartChangeError::ExportScore { .. } = error else {
            panic!("a failed score export should preserve its error kind");
        };
        assert_eq!(project.parts().len(), 1);
        assert!(!project_directory.join("broken-excerpt.csv").exists());

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
        let PartChangeError::RenameFile(_) = error else {
            panic!("a conflicting part name should fail while renaming its file");
        };

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn editing_an_open_part_updates_its_score_document_and_view(cx: &mut TestAppContext) {
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
        let dialog = cx.new(|cx| PartsWorkspace::new(dialog_parts, dialog_sequence, cx));

        model.update(cx, |model, cx| {
            model.on_parts_msg(
                dialog,
                &parts::Msg::Update {
                    source: intro.name,
                    name: "opening theme".to_string(),
                    subdivision_pattern: Some(SubdivisionPattern::new([4, 3, 3]).unwrap()),
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
            assert_eq!(
                model.score_documents[0]
                    .document
                    .read(cx)
                    .part()
                    .subdivision_pattern()
                    .unwrap()
                    .subdivisions()
                    .collect::<Vec<_>>(),
                [4, 3, 3]
            );
        });
        assert!(!project_directory.join("intro.csv").exists());
        assert!(project_directory.join("opening-theme.csv").is_file());
        assert_eq!(
            project::load_project(&project_directory)
                .unwrap()
                .project
                .parts()[0]
                .subdivision_pattern()
                .unwrap()
                .subdivisions()
                .collect::<Vec<_>>(),
            [4, 3, 3]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn repeated_insert_clicks_add_one_selected_row_at_a_time(cx: &mut TestAppContext) {
        let root = temp_root("repeated-row-insert");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let part = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
        PartScore::from_rows(vec![vec![String::new()]; 2])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let first_row = cx.debug_bounds("score-row-header-0").unwrap();
        cx.simulate_click(first_row.center(), Default::default());
        let insert_after = cx.debug_bounds("insert-row-after-control").unwrap();
        cx.simulate_click(insert_after.center(), Default::default());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.parts()[0].length),
            3
        );

        cx.simulate_click(insert_after.center(), Default::default());
        cx.run_until_parked();

        let (project, document) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.project.clone(),
                model.score_documents[0].document.clone(),
            )
        });
        assert_eq!(project.parts()[0].length, 4);
        assert_eq!(cx.update(|_, cx| document.read(cx).score().rows().len()), 4);
        assert_eq!(
            project::load_project(&project_directory).unwrap().project,
            project
        );
        assert_eq!(
            PartScore::load(&project_directory, &project.parts()[0], project.voices())
                .unwrap()
                .rows()
                .len(),
            4
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn exporting_selected_rows_creates_and_selects_the_new_part(cx: &mut TestAppContext) {
        let root = temp_root("open-export-score-rows");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let part = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
        PartScore::from_rows(vec![vec![String::new()]; 2])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let first_row = cx.debug_bounds("score-row-header-0").unwrap();
        cx.simulate_click(first_row.center(), Default::default());
        let actions = cx.update(|_, cx| {
            model.read(cx).score_views[0]
                .editor
                .as_ref()
                .unwrap()
                .read(cx)
                .actions()
        });
        actions.update(cx, |menu, cx| {
            menu.activate(ScoreAction::ExportRows.index(), cx);
        });
        cx.run_until_parked();

        let dialog = cx.update(|_, cx| match &model.read(cx).workspace.section {
            WorkspaceSection::Score {
                overlay: Some(score::Overlay::ExportRows(dialog)),
            } => dialog.clone(),
            _ => panic!("expected an export rows dialog"),
        });
        model.update(cx, |model, cx| {
            model.on_export_rows_dialog_msg(
                dialog,
                &ExportRowsDialogMsg::Confirmed(ExportRowsConfirmed {
                    part_name: part.name,
                    rows: ScoreRowRange::new(0, 0, 2).unwrap(),
                    new_part_name: "intro excerpt".to_string(),
                }),
                cx,
            );
        });

        cx.update(|_, cx| {
            let model = model.read(cx);
            assert!(!model.has_active_overlay());
            assert_eq!(model.project.parts().len(), 2);
            assert_eq!(
                model.score_views[0].part_name.as_ref().unwrap().as_str(),
                "intro excerpt"
            );
        });
        assert!(project_directory.join("intro-excerpt.csv").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn clearing_populated_rows_requires_confirmation_and_keeps_the_part_length(
        cx: &mut TestAppContext,
    ) {
        let root = temp_root("confirmed-row-clear");
        let mut project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let part = create_project_part(&project_directory, &mut project, "intro", 2).unwrap();
        PartScore::from_rows(vec![vec!["C4".to_string()], vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let first_row = cx.debug_bounds("score-row-header-0").unwrap();
        cx.simulate_click(first_row.center(), Default::default());
        let actions = cx.update(|_, cx| {
            model.read(cx).score_views[0]
                .editor
                .as_ref()
                .unwrap()
                .read(cx)
                .actions()
        });
        actions.update(cx, |menu, cx| {
            menu.activate(ScoreAction::ClearRows.index(), cx);
        });
        cx.run_until_parked();

        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());
        assert!(cx.update(|_, cx| model.read(cx).active_overlay().is_some()));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "C4"
        );
        let parts_button = cx.update(|_, cx| model.read(cx).parts_button.clone());
        model.update(cx, |model, cx| {
            model.on_parts_clicked(parts_button, &button::Clicked, cx);
        });
        cx.update(|_, cx| {
            let WorkspaceSection::Score {
                overlay: Some(score::Overlay::RowEdit(_)),
            } = &model.read(cx).workspace.section
            else {
                panic!("row editing should keep the score workspace active");
            };
        });

        let dialog = cx.update(|_, cx| match &model.read(cx).workspace.section {
            WorkspaceSection::Score {
                overlay: Some(score::Overlay::RowEdit(dialog)),
            } => dialog.clone(),
            _ => panic!("expected a row edit confirmation"),
        });
        model.update(cx, |model, cx| {
            model.on_row_edit_confirmation_msg(
                dialog,
                &RowEditConfirmationMsg::Confirmed(RowEditRequested {
                    source_editor: u64::MAX,
                    part_name: part.name,
                    edit: PartRowEdit::Clear(ScoreRowRange::new(0, 0, 2).unwrap()),
                    populated_cell_count: 1,
                }),
                cx,
            );
        });

        assert!(cx.update(|_, cx| model.read(cx).active_overlay().is_none()));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.parts()[0].length),
            2
        );
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            ""
        );
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));

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

        let super::ArrangementChangeError::Save(_) = error else {
            panic!("saving to a missing directory should report a save error");
        };
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
        let status_bar::Status::Error {
            target:
                Some(StatusAction::RevealIssue {
                    row: 0, column: 0, ..
                }),
            ..
        } = error_status
        else {
            panic!("an invalid score cell should produce a targeted error");
        };
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
        let status_bar::Status::Warning(_) = warning_status else {
            panic!("a corrected but unsaved score should produce a warning");
        };
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
    fn workspace_navigation_does_not_force_valid_score_changes_to_save(cx: &mut TestAppContext) {
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

        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );
        assert_eq!(
            cx.update(|_, cx| model.read(cx).workspace.section.kind()),
            WorkspaceSectionKind::Voices
        );
        assert!(cx.update(|_, cx| model.read(cx).active_overlay().is_none()));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn workspace_navigation_reuses_persistent_section_models(cx: &mut TestAppContext) {
        let root = temp_root("persistent-workspace-sections");
        let project = Project::new("test project", 800, 0, Seed::new(12));
        let project_directory = project::create_project(&root, &project).unwrap();
        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        let (parts_workspace, parts_button, score_button) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.workspace.parts.clone(),
                model.parts_button.clone(),
                model.score_button.clone(),
            )
        });

        model.update(cx, |model, cx| {
            model.on_parts_clicked(parts_button.clone(), &button::Clicked, cx);
        });
        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.workspace.section.kind(), WorkspaceSectionKind::Parts);
            assert_eq!(model.workspace.parts, parts_workspace);
        });

        model.update(cx, |model, cx| {
            model.on_score_clicked(score_button, &button::Clicked, cx);
            model.on_parts_clicked(parts_button, &button::Clicked, cx);
        });
        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.workspace.section.kind(), WorkspaceSectionKind::Parts);
            assert_eq!(model.workspace.parts, parts_workspace);
        });

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn workspace_overlays_are_owned_by_their_active_sections(cx: &mut TestAppContext) {
        let root = temp_root("workspace-owned-overlays");
        let part = Part::new("intro", 4);
        let voice = Voice::new(1, "lead", VoiceType::Saw);
        let part_name = part.name.clone();
        let voice_name = voice.name.clone();
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_parts(vec![part])
            .with_voices(vec![voice]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        let (
            parts_workspace,
            voices_workspace,
            settings_workspace,
            parts_button,
            voices_button,
            settings_button,
        ) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.workspace.parts.clone(),
                model.workspace.voices.clone(),
                model.workspace.project_settings.clone(),
                model.parts_button.clone(),
                model.voices_button.clone(),
                model.settings_button.clone(),
            )
        });

        model.update(cx, |model, cx| {
            model.on_parts_clicked(parts_button, &button::Clicked, cx);
            model.on_parts_msg(
                parts_workspace,
                &parts::Msg::DeleteRequested {
                    name: part_name.clone(),
                },
                cx,
            );
        });
        let part_dialog = cx.update(|_, cx| {
            let WorkspaceSection::Parts {
                overlay: Some(parts::Overlay::ConfirmDelete(dialog)),
            } = &model.read(cx).workspace.section
            else {
                panic!("part deletion should open a parts overlay");
            };
            dialog.clone()
        });

        model.update(cx, |model, cx| {
            model.on_voices_clicked(voices_button.clone(), &button::Clicked, cx);
        });
        cx.update(|_, cx| {
            let WorkspaceSection::Parts {
                overlay: Some(parts::Overlay::ConfirmDelete(_)),
            } = &model.read(cx).workspace.section
            else {
                panic!("the parts overlay should block workspace navigation");
            };
        });

        model.update(cx, |model, cx| {
            model.on_part_delete_dialog_msg(part_dialog, &parts::DeleteDialogMsg::Cancelled, cx);
            model.on_voices_clicked(voices_button, &button::Clicked, cx);
            model.on_voices_msg(
                voices_workspace,
                &voices::Msg::DeleteRequested {
                    name: voice_name.clone(),
                },
                cx,
            );
        });
        let voice_dialog = cx.update(|_, cx| {
            let WorkspaceSection::Voices {
                overlay: Some(voices::Overlay::ConfirmDelete(dialog)),
            } = &model.read(cx).workspace.section
            else {
                panic!("voice deletion should open a voices overlay");
            };
            dialog.clone()
        });

        model.update(cx, |model, cx| {
            model.on_voice_delete_dialog_msg(voice_dialog, &voices::DeleteDialogMsg::Cancelled, cx);
            model.on_settings_clicked(settings_button, &button::Clicked, cx);
            model.on_settings_msg(
                settings_workspace,
                &project_settings::ProjectSettingsMsg::ResetConfirmationRequested,
                cx,
            );
        });
        cx.update(|_, cx| {
            let WorkspaceSection::Project {
                overlay: Some(project_settings::Overlay::ConfirmReset(_)),
            } = &model.read(cx).workspace.section
            else {
                panic!("project reset should open a project settings overlay");
            };
        });

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn closing_with_an_unfinished_workspace_opens_a_project_overlay(cx: &mut TestAppContext) {
        let root = temp_root("close-with-workspace-draft");
        let project = Project::new("test project", 800, 0, Seed::new(12));
        let project_directory = project::create_project(&root, &project).unwrap();
        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        let (parts_button, close_button) = cx.update(|_, cx| {
            let model = model.read(cx);
            (model.parts_button.clone(), model.close_button.clone())
        });
        model.update(cx, |model, cx| {
            model.on_parts_clicked(parts_button, &button::Clicked, cx);
        });
        let parts = cx.update(|_, cx| model.read(cx).workspace.parts.clone());
        parts.update(cx, |parts, cx| parts.start_add_for_test(cx));
        assert!(cx.update(|_, cx| model.read(cx).workspace.parts.read(cx).has_draft()));

        model.update(cx, |model, cx| {
            model.on_close_clicked(close_button, &button::Clicked, cx);
        });

        cx.update(|_, cx| {
            let Some(ProjectOverlay::ConfirmClose(_)) = &model.read(cx).project_overlay else {
                panic!("closing with a draft should open a project overlay");
            };
        });
        assert!(cx.update(|_, cx| model.read(cx).active_overlay().is_some()));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn workspace_navigation_preserves_invalid_score_changes(cx: &mut TestAppContext) {
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

        assert!(cx.update(|_, cx| model.read(cx).active_overlay().is_none()));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).workspace.section.kind()),
            WorkspaceSectionKind::Voices
        );
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n"
        );
        let status = cx.update(|_, cx| model.read(cx).project_status(cx));
        let status_bar::Status::Error {
            target: Some(StatusAction::RevealIssue { .. }),
            ..
        } = status
        else {
            panic!("invalid score changes should remain visible after navigation");
        };

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
