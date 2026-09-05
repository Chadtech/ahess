#[cfg(test)]
use crate::project::parts::create_project_part;
use crate::project::parts::{
    append_project_variants, combine_project_parts, create_configured_project_part_with_major,
    delete_project_part, duplicate_project_part, export_project_part_rows,
    update_project_part_settings, update_project_sequence,
};

mod build_workspace;
mod history;
mod loop_range;
mod parts;
mod project_settings;
mod score;
mod voices;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use gpui::{
    div, prelude::*, AnyElement, App, AppContext, AsyncApp, Context, CursorStyle, Entity,
    EventEmitter, MouseButton, MouseDownEvent, Task, WeakEntity, Window,
};
use serde::{Deserialize, Serialize};

use crate::{
    audio_build,
    part::{self, MajorSubdivision, Part, PartName, PartScore, SubdivisionPattern},
    playback::{reset_mts_esp_master, BeatRange, Playback, PlaybackLoop},
    project::{self, Project},
    style as s,
    view::{
        button::{self, Button, ButtonVariant},
        dialog::destructive_dialog,
        dropdown::{self, Dropdown},
        range_selection_list::{ContextActionSelected, RangeSelectionList, SelectedRange},
        status_bar,
    },
    voice_name::VoiceName,
};

use self::{
    build_workspace::{BuildRequest, BuildWorkspace},
    history::{ProjectHistory, ProjectState as HistoryState},
    loop_range::{LoopWorkspace, Request as LoopRangeRequest},
    parts::PartsWorkspace,
    project_settings::{ProjectSettingsMsg, ProjectSettingsWorkspace},
    score::{
        DocumentEvent, EditPartRequested, EditSubdivisionRequested, ExportRowsConfirmed,
        ExportRowsDialog, ExportRowsDialogMsg, ExportRowsRequested, PartLoopRequested,
        PartSelected, RowEditConfirmation, RowEditConfirmationMsg, RowEditRequested, SaveState,
        ScoreCellEdit, ScoreDocument, ScoreEditor, SubdivisionDialog, SubdivisionDialogMsg,
    },
    voices::VoicesWorkspace,
};

const PLAYHEAD_REFRESH_INTERVAL: Duration = Duration::from_millis(16);

pub enum Msg {
    CloseRequested,
    UiStateChanged,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(super) struct UiState {
    #[serde(default)]
    pub(super) workspace: WorkspaceSectionKind,
    #[serde(default = "default_score_pane_count")]
    pub(super) score_pane_count: usize,
    #[serde(default)]
    pub(super) open_score_parts: Vec<String>,
    #[serde(default)]
    pub(super) active_score_pane: usize,
    #[serde(default = "default_score_arrangement_visible")]
    pub(super) score_arrangement_visible: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            workspace: WorkspaceSectionKind::Score,
            score_pane_count: default_score_pane_count(),
            open_score_parts: Vec::new(),
            active_score_pane: 0,
            score_arrangement_visible: default_score_arrangement_visible(),
        }
    }
}

const fn default_score_pane_count() -> usize {
    1
}

const fn default_score_arrangement_visible() -> bool {
    true
}

pub struct Model {
    project: Project,
    project_directory: PathBuf,
    workspace: Workspace,
    score_button: Entity<Button>,
    settings_button: Entity<Button>,
    parts_button: Entity<Button>,
    voices_button: Entity<Button>,
    build_button: Entity<Button>,
    close_button: Entity<Button>,
    pane_count_dropdown: Entity<Dropdown>,
    score_arrangement_button: Entity<Button>,
    loop_button: Entity<Button>,
    transport_button: Entity<Button>,
    undo_button: Entity<Button>,
    redo_button: Entity<Button>,
    project_overlay: Option<ProjectOverlay>,
    score_documents: Vec<ScoreDocumentEntry>,
    score_views: Vec<ScorePane>,
    active_score_view: usize,
    score_arrangement_visible: bool,
    loop_range: Option<BeatRange>,
    playback: Option<ActivePlayback>,
    playhead_task: Option<Task<()>>,
    build_task: Option<Task<()>>,
    transport_error: Option<TransportError>,
    workspace_error: Option<String>,
    history: ProjectHistory,
    history_activity: HistoryActivity,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum HistoryActivity {
    #[default]
    Recording,
    Restoring,
}

struct ScoreDocumentEntry {
    part_name: PartName,
    document: Entity<ScoreDocument>,
}

enum ScorePane {
    Empty,
    Open {
        part_name: PartName,
        editor: Entity<ScoreEditor>,
    },
}

impl ScorePane {
    fn part_name(&self) -> Option<&PartName> {
        match self {
            Self::Empty => None,
            Self::Open { part_name, .. } => Some(part_name),
        }
    }

    fn editor(&self) -> Option<&Entity<ScoreEditor>> {
        match self {
            Self::Empty => None,
            Self::Open { editor, .. } => Some(editor),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScoreArrangementContextAction {
    OpenInPanel(usize),
    Remove,
}

impl ScoreArrangementContextAction {
    fn available(panel_count: usize) -> Vec<Self> {
        (0..panel_count)
            .map(Self::OpenInPanel)
            .chain([Self::Remove])
            .collect()
    }

    fn label(self) -> String {
        match self {
            Self::OpenInPanel(panel_index) => format!("open in panel {}", panel_index + 1),
            Self::Remove => "remove from arrangement".to_string(),
        }
    }
}

struct ActivePlayback {
    output: Playback,
    target: PlaybackTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TransportError {
    Message(String),
    MtsMasterAlreadyActive {
        message: String,
        retry_target: PlaybackTarget,
    },
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
    ResetMtsEspAndRetry(PlaybackTarget),
}

type ProjectStatus = status_bar::Status<StatusAction>;

struct Workspace {
    section: WorkspaceSection,
    parts: Entity<PartsWorkspace>,
    voices: Entity<VoicesWorkspace>,
    loop_editor: Entity<LoopWorkspace>,
    project_settings: Entity<ProjectSettingsWorkspace>,
    audio_build: Entity<BuildWorkspace>,
}

impl Workspace {
    fn has_draft(&self, cx: &App) -> bool {
        self.parts.read(cx).has_draft()
            || self.voices.read(cx).has_draft()
            || self.project_settings.read(cx).is_dirty(cx)
    }
}

enum WorkspaceSection {
    Score { overlay: Option<score::Overlay> },
    Parts { overlay: Option<parts::Overlay> },
    Voices { overlay: Option<voices::Overlay> },
    Loop,
    Project,
    Build,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum WorkspaceSectionKind {
    #[default]
    Score,
    Parts,
    Voices,
    Loop,
    Project,
    Build,
}

impl WorkspaceSection {
    fn new(kind: WorkspaceSectionKind) -> Self {
        match kind {
            WorkspaceSectionKind::Score => Self::Score { overlay: None },
            WorkspaceSectionKind::Parts => Self::Parts { overlay: None },
            WorkspaceSectionKind::Voices => Self::Voices { overlay: None },
            WorkspaceSectionKind::Loop => Self::Loop,
            WorkspaceSectionKind::Project => Self::Project,
            WorkspaceSectionKind::Build => Self::Build,
        }
    }

    fn kind(&self) -> WorkspaceSectionKind {
        match self {
            Self::Score { .. } => WorkspaceSectionKind::Score,
            Self::Parts { .. } => WorkspaceSectionKind::Parts,
            Self::Voices { .. } => WorkspaceSectionKind::Voices,
            Self::Loop => WorkspaceSectionKind::Loop,
            Self::Project => WorkspaceSectionKind::Project,
            Self::Build => WorkspaceSectionKind::Build,
        }
    }

    fn has_overlay(&self) -> bool {
        match self {
            Self::Score { overlay } => overlay.is_some(),
            Self::Parts { overlay } => overlay.is_some(),
            Self::Voices { overlay } => overlay.is_some(),
            Self::Loop | Self::Project | Self::Build => false,
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
            Self::Score { overlay: None }
            | Self::Parts { overlay: None }
            | Self::Voices { overlay: None }
            | Self::Loop
            | Self::Project
            | Self::Build => None,
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
        Self::new_with_ui_state(
            project,
            project_directory,
            workspace_root,
            UiState::default(),
            cx,
        )
    }

    pub(super) fn new_with_ui_state(
        project: Project,
        project_directory: PathBuf,
        workspace_root: PathBuf,
        ui_state: UiState,
        cx: &mut Context<Self>,
    ) -> Self {
        let score_button = cx.new(|_| Button::new("score-workspace", "score").depressed(true));
        let settings_button = cx.new(|_| Button::new("project-settings", "project"));
        let parts_button = cx.new(|_| Button::new("parts", "parts"));
        let voices_button = cx.new(|_| Button::new("voices", "voices"));
        let build_button = cx.new(|_| Button::new("build-workspace", "build"));
        let close_button = cx.new(|_| Button::new("close-project", "close project"));
        let pane_count_dropdown =
            cx.new(|cx| Dropdown::new("score-pane-count", ["1 pane", "2 panes", "3 panes"], 0, cx));
        let score_arrangement_button =
            cx.new(|_| Button::new("toggle-score-arrangement", "arrangement").depressed(true));
        let arrangement_beat_count = project.arrangement_beat_count();
        let loop_range = BeatRange::new(1, arrangement_beat_count, arrangement_beat_count).ok();
        let loop_button = cx.new(|_| Button::new("loop-workspace", "loop"));
        let transport_button =
            cx.new(|_| Button::new("toggle-playback", "play").variant(ButtonVariant::Primary));
        let undo_button = cx.new(|_| Button::new("undo-project-change", "undo").disabled(true));
        let redo_button = cx.new(|_| Button::new("redo-project-change", "redo").disabled(true));

        cx.subscribe(&score_button, Self::on_score_clicked).detach();
        cx.subscribe(&settings_button, Self::on_settings_clicked)
            .detach();
        cx.subscribe(&parts_button, Self::on_parts_clicked).detach();
        cx.subscribe(&voices_button, Self::on_voices_clicked)
            .detach();
        cx.subscribe(&build_button, Self::on_build_clicked).detach();
        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&pane_count_dropdown, Self::on_pane_count_selected)
            .detach();
        cx.subscribe(
            &score_arrangement_button,
            Self::on_score_arrangement_clicked,
        )
        .detach();
        cx.subscribe(&loop_button, Self::on_loop_clicked).detach();
        cx.subscribe(&transport_button, Self::on_transport_clicked)
            .detach();
        cx.subscribe(&undo_button, Self::on_undo_clicked).detach();
        cx.subscribe(&redo_button, Self::on_redo_clicked).detach();

        let parts = project.parts.clone();
        let sequence = project.sequence().to_vec();
        let parts_workspace = cx.new(move |cx| PartsWorkspace::new(parts, sequence, cx));
        cx.subscribe(&parts_workspace, Self::on_parts_request)
            .detach();

        let voices = project.voices().to_vec();
        let acoustic_scene = project.acoustic_scene().clone();
        let voices_workspace = cx.new(move |cx| VoicesWorkspace::new(voices, acoustic_scene, cx));
        cx.subscribe(&voices_workspace, Self::on_voices_request)
            .detach();

        let occurrences = project.arrangement_occurrences();
        let loop_workspace = cx.new(move |cx| LoopWorkspace::new(occurrences, loop_range, cx));
        cx.subscribe(&loop_workspace, Self::on_loop_range_request)
            .detach();
        let loop_arrangement_range = loop_workspace.read(cx).arrangement_range();
        cx.subscribe(
            &loop_arrangement_range,
            Self::on_score_arrangement_context_action_selected,
        )
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

        let build_project = project.clone();
        let audio_build = cx.new(move |cx| BuildWorkspace::new(build_project, cx));
        cx.subscribe(&audio_build, Self::on_build_request).detach();

        let initial_history = ProjectHistory::new(HistoryState::new(Arc::new(project.clone()), []));
        let mut model = Self {
            project,
            project_directory,
            workspace: Workspace {
                section: WorkspaceSection::Score { overlay: None },
                parts: parts_workspace,
                voices: voices_workspace,
                loop_editor: loop_workspace,
                project_settings,
                audio_build,
            },
            score_button,
            settings_button,
            parts_button,
            voices_button,
            build_button,
            close_button,
            pane_count_dropdown,
            score_arrangement_button,
            loop_button,
            transport_button,
            undo_button,
            redo_button,
            project_overlay: None,
            score_documents: Vec::new(),
            score_views: vec![ScorePane::Empty],
            active_score_view: 0,
            score_arrangement_visible: default_score_arrangement_visible(),
            loop_range,
            playback: None,
            playhead_task: None,
            build_task: None,
            transport_error: None,
            workspace_error: None,
            history: initial_history,
            history_activity: HistoryActivity::Recording,
        };
        model.restore_ui_state(ui_state, cx);
        model.sync_score_arrangement_active_part(cx);
        model.sync_score_arrangement_context_actions(cx);
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
        match model.collect_project_state_for_history_diff(cx) {
            Ok(initial_state) => model.history.reset(initial_state),
            Err(error) => model.workspace_error = Some(error),
        }
        model.sync_history_buttons(cx);
        model
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn project_directory(&self) -> &Path {
        &self.project_directory
    }

    pub(super) fn ui_state(&self) -> UiState {
        UiState {
            workspace: self.workspace.section.kind(),
            score_pane_count: self.score_views.len(),
            open_score_parts: self
                .score_views
                .iter()
                .filter_map(|view| view.part_name())
                .map(|part_name| part_name.as_str().to_string())
                .collect(),
            active_score_pane: self.active_score_view,
            score_arrangement_visible: self.score_arrangement_visible,
        }
    }

    fn restore_ui_state(&mut self, ui_state: UiState, cx: &mut Context<Self>) {
        let pane_count = ui_state.score_pane_count.clamp(1, 3);
        self.score_views = (0..pane_count).map(|_| ScorePane::Empty).collect();

        let fallback_part = self.project.parts.first().map(|part| part.name.clone());
        for view_index in 0..pane_count {
            let part_name = ui_state
                .open_score_parts
                .get(view_index)
                .and_then(|name| self.project.part(&PartName::new(name.clone())))
                .map(|part| part.name.clone())
                .or_else(|| fallback_part.clone());
            if let Some(part_name) = part_name {
                self.assign_part_to_view(view_index, part_name, cx);
            }
        }

        self.active_score_view = ui_state.active_score_pane.min(pane_count - 1);
        self.score_arrangement_visible = ui_state.score_arrangement_visible;
        self.workspace.section = WorkspaceSection::new(ui_state.workspace);
        self.pane_count_dropdown.update(cx, |dropdown, cx| {
            dropdown.set_selected_index(pane_count - 1, cx);
        });
        self.score_arrangement_button.update(cx, |button, cx| {
            button.set_depressed(self.score_arrangement_visible, cx);
        });
        self.sync_workspace_buttons(cx);
    }

    pub fn bar_actions(&self) -> Vec<AnyElement> {
        let mut actions = vec![
            self.transport_button.clone().into_any_element(),
            button::action_group([self.undo_button.clone(), self.redo_button.clone()])
                .into_any_element(),
        ];
        if self.workspace.section.kind() == WorkspaceSectionKind::Score {
            actions.push(
                div()
                    .flex()
                    .gap(s::S3)
                    .child(self.pane_count_dropdown.clone())
                    .child(self.score_arrangement_button.clone())
                    .debug_selector(|| "score-view-controls".to_string())
                    .into_any_element(),
            );
        }
        actions.extend([
            div()
                .flex()
                .gap(s::S3)
                .children([
                    self.score_button.clone(),
                    self.parts_button.clone(),
                    self.voices_button.clone(),
                    self.loop_button.clone(),
                    self.settings_button.clone(),
                    self.build_button.clone(),
                ])
                .into_any_element(),
            self.close_button.clone().into_any_element(),
        ]);
        actions
    }

    pub fn active_overlay(&self) -> Option<AnyElement> {
        if let Some(ProjectOverlay::ConfirmClose(overlay)) = &self.project_overlay {
            return Some(overlay.clone().into_any_element());
        }
        self.workspace.section.overlay_element()
    }

    pub fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.has_active_overlay() {
            return;
        }
        if self.playback.is_some() {
            self.stop_playback(cx);
            return;
        }

        let target = PlaybackTarget::Arrangement;
        let playback_loop = match self.playback_loop_for_target(&target, cx) {
            Ok(playback_loop) => playback_loop,
            Err(error) => {
                self.transport_error = Some(TransportError::Message(error));
                cx.notify();
                return;
            }
        };

        self.start_playback(target, playback_loop, cx);
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.has_active_overlay() || self.history_activity == HistoryActivity::Restoring {
            return;
        }
        let target = match self.history.undo_target() {
            Ok(Some(target)) => target,
            Ok(None) => return,
            Err(error) => {
                self.workspace_error = Some(format!("couldn't undo: {error}"));
                cx.notify();
                return;
            }
        };
        match self.restore_history_state(
            &target.state,
            target.project_changed,
            &target.affected_parts,
            cx,
        ) {
            Ok(()) => {
                self.history.commit_undo(target.state);
                self.workspace_error = None;
            }
            Err(error) => self.workspace_error = Some(format!("couldn't undo: {error}")),
        }
        self.sync_history_buttons(cx);
        cx.notify();
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.has_active_overlay() || self.history_activity == HistoryActivity::Restoring {
            return;
        }
        let target = match self.history.redo_target() {
            Ok(Some(target)) => target,
            Ok(None) => return,
            Err(error) => {
                self.workspace_error = Some(format!("couldn't redo: {error}"));
                cx.notify();
                return;
            }
        };
        match self.restore_history_state(
            &target.state,
            target.project_changed,
            &target.affected_parts,
            cx,
        ) {
            Ok(()) => {
                self.history.commit_redo(target.state);
                self.workspace_error = None;
            }
            Err(error) => self.workspace_error = Some(format!("couldn't redo: {error}")),
        }
        self.sync_history_buttons(cx);
        cx.notify();
    }

    fn on_undo_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.undo(cx);
    }

    fn on_redo_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.redo(cx);
    }

    fn sync_history_buttons(&self, cx: &mut Context<Self>) {
        let blocked =
            self.has_active_overlay() || self.history_activity == HistoryActivity::Restoring;
        self.undo_button.update(cx, |button, cx| {
            button.set_disabled(blocked || !self.history.can_undo(), cx);
        });
        self.redo_button.update(cx, |button, cx| {
            button.set_disabled(blocked || !self.history.can_redo(), cx);
        });
    }

    // Broad project operations can affect an unknown set of score files. Collect
    // their resulting state once so ProjectHistory can retain only the delta.
    fn collect_project_state_for_history_diff(&self, cx: &App) -> Result<HistoryState, String> {
        let current = self.history.current();
        let project = if current.project.as_ref() == &self.project {
            current.project.clone()
        } else {
            Arc::new(self.project.clone())
        };
        let mut scores = Vec::with_capacity(self.project.parts().len());
        for project_part in self.project.parts() {
            let score = if let Some(entry) = self
                .score_documents
                .iter()
                .find(|entry| entry.part_name.eq_ignore_ascii_case(&project_part.name))
            {
                let document = entry.document.read(cx);
                if let Some(current_score) = current.score(&project_part.name) {
                    if current_score.as_ref() == document.score() {
                        current_score.clone()
                    } else {
                        Arc::new(document.score().clone())
                    }
                } else {
                    Arc::new(document.score().clone())
                }
            } else {
                let loaded =
                    PartScore::load(&self.project_directory, project_part, self.project.voices())
                        .map_err(|error| {
                        format!(
                            "couldn't capture score {:?} for undo: {error}",
                            project_part.name.as_str()
                        )
                    })?;
                if let Some(current_score) = current.score(&project_part.name) {
                    if current_score.as_ref() == &loaded {
                        current_score.clone()
                    } else {
                        Arc::new(loaded)
                    }
                } else {
                    Arc::new(loaded)
                }
            };
            let saved_score = match score.resolved_strikes(project_part, &self.project) {
                Ok(_) => score.clone(),
                Err(crate::part::ScoreError::InvalidPitch { .. }) => {
                    let current_saved = (current.project.as_ref() == &self.project)
                        .then(|| current.saved_score(&project_part.name).cloned())
                        .flatten();
                    match current_saved {
                        Some(saved_score) => saved_score,
                        None => PartScore::load(
                            &self.project_directory,
                            project_part,
                            self.project.voices(),
                        )
                        .map(Arc::new)
                        .map_err(|error| {
                            format!(
                                "couldn't capture the saved score {:?} for undo: {error}",
                                project_part.name.as_str()
                            )
                        })?,
                    }
                }
                Err(error) => {
                    return Err(format!(
                        "couldn't capture score {:?} for undo: {error}",
                        project_part.name.as_str()
                    ));
                }
            };
            scores.push((project_part.name.clone(), score, saved_score));
        }
        Ok(HistoryState::new(project, scores))
    }

    fn record_project_history_change(&mut self, cx: &mut Context<Self>) {
        if self.history_activity == HistoryActivity::Restoring {
            return;
        }
        match self.collect_project_state_for_history_diff(cx) {
            Ok(state) => {
                self.history.record_project(state);
                self.sync_history_buttons(cx);
            }
            Err(error) => self.workspace_error = Some(error),
        }
    }

    fn record_score_cell_history(
        &mut self,
        part_name: PartName,
        edit: ScoreCellEdit,
        cx: &mut Context<Self>,
    ) {
        match self.history.record_score_cell(part_name, edit) {
            Ok(_) => self.sync_history_buttons(cx),
            Err(error) => self.workspace_error = Some(error),
        }
    }

    fn record_score_rows_history(
        &mut self,
        part_name: PartName,
        score: PartScore,
        cx: &mut Context<Self>,
    ) {
        match self.history.record_score_rows(part_name, score) {
            Ok(_) => self.sync_history_buttons(cx),
            Err(error) => self.workspace_error = Some(error),
        }
    }

    fn restore_history_state(
        &mut self,
        target: &HistoryState,
        project_changed: bool,
        affected_parts: &[PartName],
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let scores = target
            .scores()
            .map(|(part_name, score, saved_score)| {
                (part_name, score.as_ref(), saved_score.as_ref())
            })
            .collect::<Vec<_>>();
        project::restore_project_state(
            &self.project_directory,
            &self.project,
            target.project.as_ref(),
            &scores,
            project_changed,
            affected_parts,
        )
        .map_err(|error| error.to_string())?;

        self.history_activity = HistoryActivity::Restoring;
        self.apply_history_state(target, cx);
        self.history_activity = HistoryActivity::Recording;
        Ok(())
    }

    fn apply_history_state(&mut self, target: &HistoryState, cx: &mut Context<Self>) {
        let current_project = self.project.clone();
        let previous_arrangement_beat_count = current_project.arrangement_beat_count();
        let previous_ui_state = self.ui_state();
        let target_project = target.project.as_ref().clone();
        let mut remaining_documents = std::mem::take(&mut self.score_documents);
        let mut document_name_changes = Vec::new();
        let mut restored_documents = Vec::with_capacity(target_project.parts().len());

        self.project = target_project.clone();
        for (target_index, target_part) in target_project.parts().iter().enumerate() {
            let exact = remaining_documents
                .iter()
                .position(|entry| entry.part_name.eq_ignore_ascii_case(&target_part.name));
            let renamed = exact.or_else(|| {
                remaining_documents.iter().position(|entry| {
                    target_project.part(&entry.part_name).is_none()
                        && current_project
                            .parts()
                            .iter()
                            .position(|part| part.name.eq_ignore_ascii_case(&entry.part_name))
                            == Some(target_index)
                })
            });
            let score = target
                .score(&target_part.name)
                .expect("every history project part must have a score")
                .as_ref()
                .clone();
            let has_recovery = target
                .saved_score(&target_part.name)
                .expect("every history project part must have a saved score")
                .as_ref()
                != &score;

            let (old_name, document) = if let Some(index) = renamed {
                let entry = remaining_documents.remove(index);
                (entry.part_name, entry.document)
            } else {
                let project = target_project.clone();
                let project_directory = self.project_directory.clone();
                let part = target_part.clone();
                let initial_score = score.clone();
                let document = cx.new(move |_| {
                    ScoreDocument::new(project, project_directory, part, initial_score)
                });
                cx.subscribe(&document, Self::on_score_document_event)
                    .detach();
                (target_part.name.clone(), document)
            };
            document.update(cx, |document, cx| {
                document.restore_history_content(
                    target_project.clone(),
                    target_part.clone(),
                    score,
                    has_recovery,
                    cx,
                );
            });
            document_name_changes.push((
                old_name.clone(),
                target_part.name.clone(),
                document.clone(),
            ));
            restored_documents.push(ScoreDocumentEntry {
                part_name: target_part.name.clone(),
                document,
            });
        }
        self.score_documents = restored_documents;

        for view_index in 0..self.score_views.len() {
            let current_name = self.score_views[view_index].part_name().cloned();
            let desired_name = current_name.as_ref().and_then(|name| {
                target_project
                    .part(name)
                    .map(|part| part.name.clone())
                    .or_else(|| {
                        document_name_changes
                            .iter()
                            .find(|(old_name, _, _)| old_name.eq_ignore_ascii_case(name))
                            .map(|(_, new_name, _)| new_name.clone())
                    })
                    .or_else(|| {
                        current_project
                            .parts()
                            .iter()
                            .position(|part| part.name.eq_ignore_ascii_case(name))
                            .and_then(|index| target_project.parts().get(index))
                            .map(|part| part.name.clone())
                    })
            });
            let desired_name = desired_name
                .or_else(|| target_project.parts().first().map(|part| part.name.clone()));
            let can_keep_editor = match (current_name.as_ref(), desired_name.as_ref()) {
                (Some(current_name), Some(desired_name)) => {
                    document_name_changes.iter().any(|(old_name, new_name, _)| {
                        old_name.eq_ignore_ascii_case(current_name)
                            && new_name.eq_ignore_ascii_case(desired_name)
                    })
                }
                _ => false,
            };
            match (&mut self.score_views[view_index], desired_name) {
                (ScorePane::Open { part_name, .. }, Some(desired_name)) if can_keep_editor => {
                    *part_name = desired_name;
                }
                (view, desired_name) => {
                    *view = ScorePane::Empty;
                    if let Some(part_name) = desired_name {
                        self.assign_part_to_view(view_index, part_name, cx);
                    }
                }
            }
        }

        self.reconcile_history_loop_range(previous_arrangement_beat_count);
        self.sync_score_editor_parts(cx);
        self.sync_workspace_project(cx);
        self.sync_score_arrangement_active_part(cx);
        self.sync_score_arrangement_context_actions(cx);
        if self.playback.is_some() {
            self.update_live_playback(cx);
        }
        if self.ui_state() != previous_ui_state {
            cx.emit(Msg::UiStateChanged);
        }
    }

    fn reconcile_history_loop_range(&mut self, previous_arrangement_beat_count: u64) {
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
        self.sync_history_buttons(cx);
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
        self.sync_history_buttons(cx);
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
        self.sync_history_buttons(cx);
        cx.notify();
    }

    fn set_workspace_section(&mut self, section: WorkspaceSection, cx: &mut Context<Self>) {
        if self.has_active_overlay() || self.workspace.section.kind() == section.kind() {
            return;
        }
        self.workspace.section = section;
        self.sync_workspace_buttons(cx);
        self.sync_score_arrangement_context_actions(cx);
        cx.emit(Msg::UiStateChanged);
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
            (&self.build_button, selected == WorkspaceSectionKind::Build),
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

        if let Some(error) = &self.transport_error {
            return match error {
                TransportError::Message(message) => ProjectStatus::Error {
                    message: message.clone().into(),
                    target: None,
                },
                TransportError::MtsMasterAlreadyActive {
                    message,
                    retry_target,
                } => ProjectStatus::Error {
                    message: format!(
                        "{message} · reset only if no other tuning master should be active"
                    )
                    .into(),
                    target: Some(StatusAction::ResetMtsEspAndRetry(retry_target.clone())),
                },
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
            .is_some_and(|view| view.part_name() == Some(&name))
        {
            return;
        }

        if self.assign_part_to_view(self.active_score_view, name, cx) {
            self.sync_score_arrangement_active_part(cx);
            cx.emit(Msg::UiStateChanged);
        }
    }

    fn active_part(&self) -> Option<&PartName> {
        self.score_views
            .get(self.active_score_view)
            .and_then(|view| view.part_name())
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
        document: Entity<ScoreDocument>,
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
            | DocumentEvent::HistoryRestored { .. }
            | DocumentEvent::ProjectChanged
            | DocumentEvent::PartSettingsChanged => false,
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
            | DocumentEvent::HistoryRestored { .. }
            | DocumentEvent::ProjectChanged => true,
            DocumentEvent::Saved
            | DocumentEvent::RecoverySaved
            | DocumentEvent::SaveFailed
            | DocumentEvent::PartSettingsChanged => false,
        };
        if changes_playback {
            self.workspace.audio_build.update(cx, |workspace, cx| {
                workspace.mark_project_changed(cx);
            });
        }
        if self.playback.is_some() && changes_playback {
            self.update_live_playback(cx);
        }
        if self.history_activity == HistoryActivity::Recording {
            let part_name = self
                .score_documents
                .iter()
                .find(|entry| entry.document == document)
                .map(|entry| entry.part_name.clone())
                .unwrap_or_else(|| document.read(cx).part().name.clone());
            match event {
                DocumentEvent::CellChanged { edit, .. } => {
                    self.record_score_cell_history(part_name, edit.clone(), cx);
                }
                DocumentEvent::RowsCleared => {
                    let score = document.read(cx).score().clone();
                    self.record_score_rows_history(part_name, score, cx);
                }
                _ => {}
            }
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
        self.arrangement_playback_loop_for_range(range, cx)
    }

    fn full_arrangement_build_data(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<(PlaybackLoop, Vec<(Part, PartScore)>), String> {
        let beat_count = self.project.arrangement_beat_count();
        let range = BeatRange::new(1, beat_count, beat_count).map_err(|error| error.to_string())?;
        let arrangement_scores = self.arrangement_scores(cx)?;
        let playback_loop =
            PlaybackLoop::from_project_arrangement(&self.project, &arrangement_scores, range)
                .map_err(|error| error.to_string())?;
        Ok((playback_loop, arrangement_scores))
    }

    fn arrangement_playback_loop_for_range(
        &mut self,
        range: BeatRange,
        cx: &mut Context<Self>,
    ) -> Result<PlaybackLoop, String> {
        let arrangement_scores = self.arrangement_scores(cx)?;
        PlaybackLoop::from_project_arrangement(&self.project, &arrangement_scores, range)
            .map_err(|error| error.to_string())
    }

    fn arrangement_scores(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<Vec<(Part, PartScore)>, String> {
        let sequence = self.project.sequence().to_vec();
        let mut arrangement_scores = Vec::with_capacity(sequence.len());
        for part_name in sequence {
            let document = self.score_document(&part_name, cx)?;
            let document = document.read(cx);
            arrangement_scores.push((document.part().clone(), document.score().clone()));
        }
        Ok(arrangement_scores)
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
                self.transport_error = Some(TransportError::Message(format!(
                    "playback is keeping the last valid loop: {error}"
                )));
            }
        }
        cx.notify();
    }

    fn on_transport_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.toggle_playback(cx);
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
                self.sync_transport_button(cx);
                self.start_playhead_tracking(cx);
            }
            Err(error) => {
                let message = error.to_string();
                let audio_build_is_active = self.workspace.audio_build.read(cx).is_building();
                self.transport_error =
                    Some(if error.can_reset_mts_esp() && audio_build_is_active {
                        TransportError::Message(
                        "the audio build is using MTS-ESP; wait for it to finish before playing"
                            .to_string(),
                    )
                    } else if error.can_reset_mts_esp() {
                        TransportError::MtsMasterAlreadyActive {
                            message,
                            retry_target: target,
                        }
                    } else {
                        TransportError::Message(message)
                    });
                self.sync_transport_button(cx);
            }
        }
        cx.notify();
    }

    fn stop_playback(&mut self, cx: &mut Context<Self>) {
        self.playhead_task.take();
        self.playback = None;
        self.clear_playhead_highlights(cx);
        self.transport_error = None;
        self.sync_transport_button(cx);
        cx.notify();
    }

    fn sync_transport_button(&self, cx: &mut Context<Self>) {
        let playing = self.playback.is_some();
        self.transport_button.update(cx, |button, cx| {
            button.set_label(if playing { "stop" } else { "play" }, cx);
            button.set_variant(
                if playing {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Primary
                },
                cx,
            );
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
        let (playing_position, playing_occurrence) =
            self.playback.as_ref().map_or((None, None), |playback| {
                let beat = playback.output.current_arrangement_beat();
                match &playback.target {
                    PlaybackTarget::Arrangement => {
                        let position = playing_arrangement_position(&self.project, beat);
                        (
                            position
                                .as_ref()
                                .map(|position| (position.part_name.clone(), position.row)),
                            position.map(|position| position.occurrence),
                        )
                    }
                    PlaybackTarget::Part(part_name) => (
                        beat.checked_sub(1)
                            .and_then(|beat| usize::try_from(beat).ok())
                            .map(|row| (part_name.clone(), row)),
                        None,
                    ),
                }
            });
        for view in &self.score_views {
            let playing_row = view.part_name().and_then(|view_part| {
                playing_position.as_ref().and_then(|(playing_part, row)| {
                    view_part.eq_ignore_ascii_case(playing_part).then_some(*row)
                })
            });
            if let Some(editor) = view.editor() {
                editor.update(cx, |editor, cx| {
                    editor.set_playing_row(playing_row, cx);
                });
            }
        }
        let arrangement_range = self.workspace.loop_editor.read(cx).arrangement_range();
        arrangement_range.update(cx, |range, cx| {
            range.sync_playing_row(playing_occurrence, cx);
        });
    }

    fn clear_playhead_highlights(&self, cx: &mut Context<Self>) {
        for editor in self.score_views.iter().filter_map(|view| view.editor()) {
            editor.update(cx, |editor, cx| editor.set_playing_row(None, cx));
        }
        let arrangement_range = self.workspace.loop_editor.read(cx).arrangement_range();
        arrangement_range.update(cx, |range, cx| {
            range.sync_playing_row(None, cx);
        });
    }

    fn assign_part_to_view(
        &mut self,
        view_index: usize,
        part_name: PartName,
        cx: &mut Context<Self>,
    ) -> bool {
        let document = match self.score_document(&part_name, cx) {
            Ok(document) => document,
            Err(error) => {
                self.workspace_error = Some(error);
                self.sync_score_editor_parts(cx);
                cx.notify();
                return false;
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
        cx.subscribe(&editor, Self::on_score_editor_edit_part_requested)
            .detach();
        cx.subscribe(&editor, Self::on_score_editor_edit_subdivision_requested)
            .detach();
        cx.subscribe(&editor, Self::on_score_editor_row_edit_requested)
            .detach();
        cx.subscribe(&editor, Self::on_score_editor_part_loop_requested)
            .detach();
        cx.subscribe(&editor, Self::on_score_editor_export_rows_requested)
            .detach();
        let Some(view) = self.score_views.get_mut(view_index) else {
            return false;
        };
        let changed = view.part_name() != Some(&part_name);
        *view = ScorePane::Open { part_name, editor };
        if view_index == self.active_score_view {
            self.sync_score_arrangement_active_part(cx);
        }
        self.workspace_error = None;
        if self.playback.is_some() {
            self.sync_playhead_highlights(cx);
        }
        cx.notify();
        changed
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
            .position(|view| view.editor() == Some(&editor))
        else {
            return;
        };
        self.activate_score_view(view_index, cx);
        if self.assign_part_to_view(view_index, selected.part_name.clone(), cx) {
            cx.emit(Msg::UiStateChanged);
        }
    }

    fn on_score_editor_edit_part_requested(
        &mut self,
        editor: Entity<ScoreEditor>,
        request: &EditPartRequested,
        cx: &mut Context<Self>,
    ) {
        if self.has_active_overlay() {
            return;
        }
        if let Some(view_index) = self
            .score_views
            .iter()
            .position(|view| view.editor() == Some(&editor))
        {
            self.activate_score_view(view_index, cx);
        }

        let began_editing = self.workspace.parts.update(cx, |workspace, cx| {
            workspace.begin_editing_part(&request.part_name, cx)
        });
        if !began_editing {
            self.workspace_error = Some(format!(
                "finish or cancel the current parts change before editing {:?}",
                request.part_name.as_str()
            ));
            cx.notify();
            return;
        }

        self.workspace_error = None;
        self.set_workspace_section(WorkspaceSection::Parts { overlay: None }, cx);
    }

    fn on_score_editor_edit_subdivision_requested(
        &mut self,
        editor: Entity<ScoreEditor>,
        request: &EditSubdivisionRequested,
        cx: &mut Context<Self>,
    ) {
        if self.has_active_overlay() {
            return;
        }
        if let Some(view_index) = self
            .score_views
            .iter()
            .position(|view| view.editor() == Some(&editor))
        {
            self.activate_score_view(view_index, cx);
        }

        let Some(part) = self.project.part(&request.part_name).cloned() else {
            self.workspace_error = Some(format!(
                "part {:?} no longer exists",
                request.part_name.as_str()
            ));
            cx.notify();
            return;
        };
        let dialog = cx.new(move |cx| SubdivisionDialog::new(&part, cx));
        cx.subscribe(&dialog, Self::on_subdivision_dialog_msg)
            .detach();
        self.workspace_error = None;
        self.set_score_overlay(Some(score::Overlay::Subdivision(dialog)), cx);
    }

    fn on_subdivision_dialog_msg(
        &mut self,
        dialog: Entity<SubdivisionDialog>,
        msg: &SubdivisionDialogMsg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            SubdivisionDialogMsg::Cancelled => self.set_score_overlay(None, cx),
            SubdivisionDialogMsg::Confirmed {
                part_name,
                subdivision_pattern,
                major_subdivision,
            } => self.save_score_subdivision(
                dialog,
                part_name,
                subdivision_pattern.clone(),
                *major_subdivision,
                cx,
            ),
        }
    }

    fn save_score_subdivision(
        &mut self,
        dialog: Entity<SubdivisionDialog>,
        part_name: &PartName,
        subdivision_pattern: Option<SubdivisionPattern>,
        major_subdivision: Option<MajorSubdivision>,
        cx: &mut Context<Self>,
    ) {
        let Some(current_part) = self.project.part(part_name) else {
            dialog.update(cx, |dialog, cx| {
                dialog.save_failed(
                    format!("part {:?} no longer exists", part_name.as_str()),
                    cx,
                );
            });
            return;
        };
        if current_part.subdivision_pattern() == subdivision_pattern.as_ref()
            && current_part.major_subdivision() == major_subdivision
        {
            self.set_score_overlay(None, cx);
            return;
        }

        let unchanged_name = current_part.name.as_str().to_string();
        match update_project_part_settings(
            &self.project_directory,
            &mut self.project,
            part_name,
            &unchanged_name,
            subdivision_pattern,
            major_subdivision,
        ) {
            Ok(part) => {
                let project = self.project.clone();
                for entry in &self.score_documents {
                    if entry.part_name.eq_ignore_ascii_case(part_name) {
                        let project = project.clone();
                        let part = part.clone();
                        entry.document.update(cx, |document, cx| {
                            document.part_settings_changed(project, part, cx);
                        });
                    } else {
                        let project = project.clone();
                        entry.document.update(cx, |document, cx| {
                            document.project_settings_changed(project, cx);
                        });
                    }
                }
                self.sync_workspace_project(cx);
                self.workspace_error = None;
                self.set_score_overlay(None, cx);
                self.record_project_history_change(cx);
            }
            Err(error) => {
                dialog.update(cx, |dialog, cx| {
                    dialog.save_failed(error.to_string(), cx);
                });
            }
        }
        cx.notify();
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
            .position(|view| view.editor() == Some(&editor))
        {
            self.activate_score_view(view_index, cx);
        }

        let target = PlaybackTarget::Part(request.part_name.clone());
        match self.playback_loop_for_target(&target, cx) {
            Ok(playback_loop) => self.start_playback(target, playback_loop, cx),
            Err(error) => {
                self.transport_error = Some(TransportError::Message(error));
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
            .position(|view| view.editor() == Some(&editor))
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
                self.record_project_history_change(cx);
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
                self.record_project_history_change(cx);
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
        for editor in self.score_views.iter().filter_map(|view| view.editor()) {
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
        self.sync_score_arrangement_active_part(cx);
        self.workspace_error = None;
        cx.emit(Msg::UiStateChanged);
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

    fn on_score_arrangement_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.score_arrangement_visible = !self.score_arrangement_visible;
        self.score_arrangement_button.update(cx, |button, cx| {
            button.set_depressed(self.score_arrangement_visible, cx);
        });
        cx.emit(Msg::UiStateChanged);
        cx.notify();
    }

    fn on_score_arrangement_context_action_selected(
        &mut self,
        _: Entity<RangeSelectionList>,
        selected: &ContextActionSelected,
        cx: &mut Context<Self>,
    ) {
        if self.workspace.section.kind() != WorkspaceSectionKind::Score {
            return;
        }
        let actions = ScoreArrangementContextAction::available(self.score_views.len());
        let Some(action) = actions.get(selected.action).copied() else {
            return;
        };
        match action {
            ScoreArrangementContextAction::OpenInPanel(panel_index) => {
                self.open_arrangement_part_in_score_panel(selected.row, panel_index, cx);
            }
            ScoreArrangementContextAction::Remove => {
                self.remove_score_arrangement_occurrence(selected.row, cx);
            }
        }
    }

    fn open_arrangement_part_in_score_panel(
        &mut self,
        occurrence_index: usize,
        panel_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(part_name) = self.project.sequence().get(occurrence_index).cloned() else {
            return;
        };
        let already_open = self.score_views.get(panel_index).is_some_and(|view| {
            view.part_name()
                .is_some_and(|open_part| open_part.eq_ignore_ascii_case(&part_name))
        });
        if panel_index >= self.score_views.len() {
            return;
        }

        self.activate_score_view(panel_index, cx);
        if !already_open && self.assign_part_to_view(panel_index, part_name, cx) {
            cx.emit(Msg::UiStateChanged);
        }
    }

    fn remove_score_arrangement_occurrence(
        &mut self,
        occurrence_index: usize,
        cx: &mut Context<Self>,
    ) {
        let mut sequence = self.project.sequence().to_vec();
        if occurrence_index >= sequence.len() {
            cx.notify();
            return;
        }
        sequence.remove(occurrence_index);

        let previous_arrangement_beat_count = self.project.arrangement_beat_count();
        match update_project_sequence(&self.project_directory, &mut self.project, sequence) {
            Ok(_) => {
                self.reconcile_loop_range(previous_arrangement_beat_count, cx);
                self.update_score_documents_for_project_settings(cx);
                self.sync_workspace_project(cx);
                self.workspace_error = None;
                self.record_project_history_change(cx);
            }
            Err(error) => {
                self.workspace_error =
                    Some(format!("couldn't remove part from arrangement: {error}"));
            }
        }
        cx.notify();
    }

    fn set_view_count(&mut self, count: usize, cx: &mut Context<Self>) {
        let previous_state = self.ui_state();
        let count = count.clamp(1, 3);
        let template_part = self
            .active_part()
            .cloned()
            .or_else(|| self.project.parts.first().map(|part| part.name.clone()));

        while self.score_views.len() < count {
            let view_index = self.score_views.len();
            self.score_views.push(ScorePane::Empty);
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
        self.sync_score_arrangement_active_part(cx);
        self.sync_score_arrangement_context_actions(cx);
        if self.ui_state() != previous_state {
            cx.emit(Msg::UiStateChanged);
        }
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

    fn sync_score_arrangement_active_part(&self, cx: &mut Context<Self>) {
        let active_part = self.active_part().cloned();
        self.workspace.loop_editor.update(cx, |workspace, cx| {
            workspace.sync_active_part(active_part, cx);
        });
    }

    fn sync_score_arrangement_context_actions(&self, cx: &mut Context<Self>) {
        let enabled = self.workspace.section.kind() == WorkspaceSectionKind::Score;
        let panel_count = self.score_views.len();
        let arrangement_range = self.workspace.loop_editor.read(cx).arrangement_range();
        arrangement_range.update(cx, |range, cx| {
            if enabled {
                let labels = ScoreArrangementContextAction::available(panel_count)
                    .into_iter()
                    .map(ScoreArrangementContextAction::label);
                range.set_context_actions(labels, cx);
            } else {
                range.set_context_actions(std::iter::empty::<&str>(), cx);
            }
        });
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

        let occurrences = project.arrangement_occurrences();
        let active_part = self.active_part().cloned();
        self.workspace.loop_editor.update(cx, |workspace, cx| {
            workspace.sync_occurrences(occurrences, cx);
            workspace.sync_active_part(active_part, cx);
        });

        self.workspace.project_settings.update(cx, |workspace, cx| {
            workspace.sync_project(project.clone(), cx);
        });

        self.workspace.audio_build.update(cx, |workspace, cx| {
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

    fn on_build_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.set_workspace_section(WorkspaceSection::Build, cx);
    }

    fn on_build_request(
        &mut self,
        workspace: Entity<BuildWorkspace>,
        request: &BuildRequest,
        cx: &mut Context<Self>,
    ) {
        let BuildRequest {
            request_id,
            sample_rate,
        } = *request;
        let (playback_loop, arrangement_scores) = match self.full_arrangement_build_data(cx) {
            Ok(build_data) => build_data,
            Err(error) => {
                workspace.update(cx, |workspace, cx| {
                    workspace.build_finished(request_id, Err(error), cx);
                });
                return;
            }
        };
        let project = self.project.clone();
        let project_directory = self.project_directory.clone();
        self.build_task.take();
        self.build_task = Some(cx.spawn(
            async move |model: WeakEntity<Model>, cx: &mut AsyncApp| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        audio_build::build_project_audio(
                            project_directory,
                            &project,
                            &arrangement_scores,
                            playback_loop,
                            sample_rate,
                        )
                        .map_err(|error| error.to_string())
                    })
                    .await;
                model
                    .update(cx, |model, cx| {
                        model.workspace.audio_build.update(cx, |workspace, cx| {
                            workspace.build_finished(request_id, result, cx);
                        });
                    })
                    .ok();
            },
        ));
    }

    fn on_loop_range_request(
        &mut self,
        _: Entity<LoopWorkspace>,
        request: &LoopRangeRequest,
        cx: &mut Context<Self>,
    ) {
        match request {
            LoopRangeRequest::SetRange(range) => {
                if self.loop_range == Some(*range) {
                    return;
                }
                self.loop_range = Some(*range);
                if self.playback.is_some() {
                    self.update_live_playback(cx);
                } else {
                    self.transport_error = None;
                }
            }
        }
        cx.notify();
    }

    fn reset_loop_workspace(&mut self, cx: &mut Context<Self>) {
        let occurrences = self.project.arrangement_occurrences();
        let range = self.loop_range;
        let workspace = cx.new(move |cx| LoopWorkspace::new(occurrences, range, cx));
        cx.subscribe(&workspace, Self::on_loop_range_request)
            .detach();
        let arrangement_range = workspace.read(cx).arrangement_range();
        cx.subscribe(
            &arrangement_range,
            Self::on_score_arrangement_context_action_selected,
        )
        .detach();
        self.workspace.loop_editor = workspace;
        self.sync_score_arrangement_active_part(cx);
        self.sync_score_arrangement_context_actions(cx);
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
        self.set_workspace_section(WorkspaceSection::Project, cx);
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
                self.record_project_history_change(cx);
            }
        }
        cx.notify();
    }

    fn on_voices_request(
        &mut self,
        workspace: Entity<VoicesWorkspace>,
        request: &voices::Request,
        cx: &mut Context<Self>,
    ) {
        match request {
            voices::Request::Change(change) => self.apply_voice_change(workspace, change, cx),
            voices::Request::ConfirmDelete { name } => {
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
                volume_adjustment,
            } => {
                match project::add_voice_with_adjustment_at(
                    &self.project_directory,
                    &self.project,
                    name,
                    *voice_type,
                    *position,
                    *volume_adjustment,
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
                        self.record_project_history_change(cx);
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
                volume_adjustment,
            } => {
                let edited_id = self.project.voice(original_name).map(|voice| voice.id());
                match project::edit_voice_with_adjustment_at(
                    &self.project_directory,
                    &self.project,
                    original_name,
                    name,
                    *voice_type,
                    *position,
                    *volume_adjustment,
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
                        self.record_project_history_change(cx);
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
                self.record_project_history_change(cx);
            }
            Err(error) => {
                confirmation.update(cx, |dialog, cx| {
                    dialog.failed(error.to_string(), cx);
                });
            }
        }
        cx.notify();
    }

    fn on_parts_request(
        &mut self,
        dialog: Entity<PartsWorkspace>,
        request: &parts::Request,
        cx: &mut Context<Self>,
    ) {
        match request {
            parts::Request::Add {
                name,
                length,
                subdivision_pattern,
                major_subdivision,
            } => {
                match create_configured_project_part_with_major(
                    &self.project_directory,
                    &mut self.project,
                    name,
                    *length,
                    subdivision_pattern.clone(),
                    *major_subdivision,
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
                        self.record_project_history_change(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.add_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Request::Duplicate { source, name } => {
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
                        self.record_project_history_change(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.duplicate_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Request::Update {
                source,
                name,
                subdivision_pattern,
                major_subdivision,
            } => {
                if let Err(error) = self.flush_part_score_changes(source, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog.update_failed(format!("couldn't save score changes: {error}"), cx);
                    });
                    return;
                }
                match update_project_part_settings(
                    &self.project_directory,
                    &mut self.project,
                    source,
                    name,
                    subdivision_pattern.clone(),
                    *major_subdivision,
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
                            if let ScorePane::Open { part_name, .. } = view {
                                if part_name.eq_ignore_ascii_case(source) {
                                    *part_name = updated_name.clone();
                                }
                            }
                        }
                        cx.emit(Msg::UiStateChanged);
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
                        self.record_project_history_change(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.update_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Request::ConfirmDelete { name } => {
                self.open_part_delete_dialog(name.clone(), cx);
            }
            parts::Request::Combine { sources, name } => {
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
                        self.record_project_history_change(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.combine_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Request::AppendVariants { sources, suffix } => {
                if let Err(error) = self.flush_part_score_changes_for(sources, cx) {
                    dialog.update(cx, |dialog, cx| {
                        dialog.append_variants_failed(
                            format!("couldn't save source score changes: {error}"),
                            cx,
                        );
                    });
                    return;
                }
                let previous_arrangement_beat_count = self.project.arrangement_beat_count();
                match append_project_variants(
                    &self.project_directory,
                    &mut self.project,
                    sources,
                    suffix,
                ) {
                    Ok(appended) => {
                        let first_variant = appended.first.clone();
                        let sequence = self.project.sequence().to_vec();
                        let first = sequence.len() - appended.len();
                        let selected_range =
                            SelectedRange::new(first, sequence.len() - 1, sequence.len())
                                .expect("the appended variant range must be selectable");
                        let parts = self.project.parts.clone();
                        dialog.update(cx, |dialog, cx| {
                            dialog.variants_appended(
                                parts,
                                sequence,
                                first_variant.clone(),
                                selected_range,
                                cx,
                            );
                        });
                        self.reconcile_loop_range(previous_arrangement_beat_count, cx);
                        self.update_score_documents_for_project_settings(cx);
                        self.select_part(first_variant, cx);
                        self.sync_score_editor_parts(cx);
                        self.sync_workspace_project(cx);
                        self.workspace_error = None;
                        self.record_project_history_change(cx);
                    }
                    Err(error) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.append_variants_failed(error.to_string(), cx);
                        });
                    }
                }
            }
            parts::Request::ChangeSequence {
                sequence,
                selected_range,
            } => {
                let previous_arrangement_beat_count = self.project.arrangement_beat_count();
                match update_project_sequence(
                    &self.project_directory,
                    &mut self.project,
                    sequence.clone(),
                ) {
                    Ok(sequence) => {
                        dialog.update(cx, |dialog, cx| {
                            dialog.sequence_changed(sequence, *selected_range, cx);
                        });
                        self.reconcile_loop_range(previous_arrangement_beat_count, cx);
                        self.update_score_documents_for_project_settings(cx);
                        self.sync_workspace_project(cx);
                        self.record_project_history_change(cx);
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
                        view.part_name()
                            .is_some_and(|name| name.eq_ignore_ascii_case(&part.name))
                            .then_some(index)
                    })
                    .collect::<Vec<_>>();
                let fallback = self.project.parts.first().map(|part| part.name.clone());
                self.remove_score_document(&part.name, cx);
                for view_index in affected_views {
                    if let Some(view) = self.score_views.get_mut(view_index) {
                        *view = ScorePane::Empty;
                    }
                    if let Some(part_name) = fallback.clone() {
                        self.assign_part_to_view(view_index, part_name, cx);
                    }
                }
                cx.emit(Msg::UiStateChanged);
                self.sync_score_editor_parts(cx);
                self.sync_workspace_project(cx);
                self.set_parts_overlay(None, cx);
                self.record_project_history_change(cx);
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
            StatusAction::ResetMtsEspAndRetry(_) => return,
        };

        let active_view_has_target = self
            .score_views
            .get(self.active_score_view)
            .and_then(|view| view.part_name())
            .is_some_and(|name| name.eq_ignore_ascii_case(&part_name));
        let view_index = if active_view_has_target {
            self.active_score_view
        } else {
            self.score_views
                .iter()
                .position(|view| {
                    view.part_name()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&part_name))
                })
                .unwrap_or(self.active_score_view)
        };
        let target_is_open = self
            .score_views
            .get(view_index)
            .and_then(|view| view.part_name())
            .is_some_and(|name| name.eq_ignore_ascii_case(&part_name));
        if target_is_open {
            self.activate_score_view(view_index, cx);
        } else if self.assign_part_to_view(view_index, part_name, cx) {
            cx.emit(Msg::UiStateChanged);
        }

        let Some(editor) = self
            .score_views
            .get(view_index)
            .and_then(|view| view.editor().cloned())
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

    fn on_reset_mts_esp_clicked(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let Some(TransportError::MtsMasterAlreadyActive { retry_target, .. }) =
            self.transport_error.clone()
        else {
            return;
        };

        if self.workspace.audio_build.read(cx).is_building() {
            self.transport_error = Some(TransportError::Message(
                "the audio build is using MTS-ESP; wait for it to finish before resetting"
                    .to_string(),
            ));
            cx.notify();
            return;
        }

        if let Err(error) = reset_mts_esp_master() {
            self.transport_error = Some(TransportError::Message(format!(
                "couldn't reset the MTS-ESP master: {error}"
            )));
            cx.notify();
            return;
        }

        self.transport_error = None;
        match self.playback_loop_for_target(&retry_target, cx) {
            Ok(playback_loop) => self.start_playback(retry_target, playback_loop, cx),
            Err(error) => {
                self.transport_error = Some(TransportError::Message(error));
                cx.notify();
            }
        }
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if self.has_active_overlay() {
            return;
        }

        if self.workspace.has_draft(cx) {
            let overlay = cx.new(CloseProjectDialog::new);
            cx.subscribe(&overlay, Self::on_close_project_msg).detach();
            self.project_overlay = Some(ProjectOverlay::ConfirmClose(overlay));
            self.sync_history_buttons(cx);
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
        self.sync_history_buttons(cx);
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
                let arrangement_range = self.workspace.loop_editor.read(cx).arrangement_range();
                score_workspace(
                    &self.score_views,
                    &self.project,
                    self.loop_range,
                    arrangement_range,
                    self.score_arrangement_visible,
                    project_status,
                    cx,
                )
                .into_any_element()
            }
            WorkspaceSection::Parts { .. } => self.workspace.parts.clone().into_any_element(),
            WorkspaceSection::Voices { .. } => self.workspace.voices.clone().into_any_element(),
            WorkspaceSection::Loop => self.workspace.loop_editor.clone().into_any_element(),
            WorkspaceSection::Project => self.workspace.project_settings.clone().into_any_element(),
            WorkspaceSection::Build => self.workspace.audio_build.clone().into_any_element(),
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

fn arrangement_duration_summary(project: &Project) -> String {
    let duration_millis = u128::from(project.arrangement_beat_count())
        * u128::from(project.beat_duration_millis.get());
    let rounded_seconds = (duration_millis + 500) / 1_000;
    let minutes = rounded_seconds / 60;
    let seconds = rounded_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

struct PlayingArrangementPosition {
    occurrence: usize,
    part_name: PartName,
    row: usize,
}

fn playing_arrangement_position(
    project: &Project,
    arrangement_beat: u64,
) -> Option<PlayingArrangementPosition> {
    for (occurrence_index, occurrence) in project.arrangement_occurrences().into_iter().enumerate()
    {
        if (occurrence.first_beat()..=occurrence.last_beat()).contains(&arrangement_beat) {
            return Some(PlayingArrangementPosition {
                occurrence: occurrence_index,
                part_name: occurrence.part_name().clone(),
                row: (arrangement_beat - occurrence.first_beat()) as usize,
            });
        }
    }
    None
}

fn score_workspace(
    score_views: &[ScorePane],
    project: &Project,
    loop_range: Option<BeatRange>,
    arrangement_range: Entity<RangeSelectionList>,
    arrangement_visible: bool,
    project_status: ProjectStatus,
    cx: &mut Context<Model>,
) -> gpui::Div {
    let panes = score_views
        .iter()
        .enumerate()
        .map(|(index, view)| {
            let content = match view {
                ScorePane::Open { editor, .. } => editor.clone().into_any_element(),
                ScorePane::Empty => div()
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
        .children(panes)
        .when(arrangement_visible, |editors| {
            editors.child(score_arrangement_panel(
                project,
                loop_range,
                arrangement_range,
            ))
        });
    let status_is_actionable = match &project_status {
        ProjectStatus::Error {
            target: Some(StatusAction::RevealIssue { .. } | StatusAction::RetryScoreSave),
            ..
        } => true,
        ProjectStatus::Error { .. } => false,
        ProjectStatus::Empty | ProjectStatus::Message(_) | ProjectStatus::Warning(_) => false,
    };
    let can_reset_mts_esp = matches!(
        &project_status,
        ProjectStatus::Error {
            target: Some(StatusAction::ResetMtsEspAndRetry(_)),
            ..
        }
    );
    let project_status_bar = status_bar::bar(project_status)
        .id("project-status-bar")
        .debug_selector(|| "project-status-bar".to_string())
        .when(can_reset_mts_esp, |bar| {
            bar.child(
                status_bar::action_button("reset-mts-esp", "reset MTS").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Model::on_reset_mts_esp_clicked),
                ),
            )
        })
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

fn score_arrangement_panel(
    project: &Project,
    loop_range: Option<BeatRange>,
    arrangement_range: Entity<RangeSelectionList>,
) -> gpui::Div {
    let occurrences = project.arrangement_occurrences();
    let occurrence_label = if occurrences.len() == 1 {
        "part"
    } else {
        "parts"
    };
    let panel = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(s::S0)
        .overflow_hidden()
        .bg(s::GRAY2)
        .p(s::CONTENT_PADDING)
        .child(
            div()
                .flex()
                .items_center()
                .pb(s::S4)
                .gap(s::S3)
                .text_color(s::TEXT_DEFAULT)
                .child(format!("{} {occurrence_label},", occurrences.len()))
                .child(
                    div()
                        .debug_selector(|| "score-arrangement-duration-summary".to_string())
                        .child(arrangement_duration_summary(project)),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(s::S3)
                .pb(s::S4)
                .text_color(s::TEXT_DEFAULT)
                .child(
                    div()
                        .debug_selector(|| "score-arrangement-loop-summary".to_string())
                        .min_w(s::S0)
                        .truncate()
                        .child(loop_range_summary(project, loop_range)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_h(s::S0)
                .w_full()
                .debug_selector(|| "score-arrangement-list".to_string())
                .child(arrangement_range),
        );

    s::raised(panel)
        .flex()
        .flex_none()
        .w(s::S9)
        .min_w(s::S9)
        .min_h(s::S0)
        .overflow_hidden()
        .debug_selector(|| "score-arrangement-panel".to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    use gpui::{px, size, AppContext, Modifiers, MouseButton, TestAppContext};

    use super::score::{self, ScoreAction};
    use super::{
        arrangement_duration_summary, create_project_part, loop_range_summary, parts,
        playing_arrangement_position, update_project_sequence, voices, BuildRequest,
        ExportRowsConfirmed, ExportRowsDialogMsg, Model, PartsWorkspace, PlaybackTarget,
        ProjectOverlay, ProjectSettingsMsg, RowEditConfirmationMsg, RowEditRequested, StatusAction,
        TransportError, UiState, WorkspaceSection, WorkspaceSectionKind,
    };
    use crate::{
        acoustics::Point3Meters,
        audio_build::{planned_audio_files, BuildSampleRate},
        part::{
            MajorSubdivision, Part, PartName, PartRowEdit, PartScore, ScoreRowRange,
            SubdivisionPattern,
        },
        pitch_system::{ExplicitPitchSystem, FrequencyHz, PitchSystem},
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
            playing_arrangement_position(&project, beat)
                .map(|position| (position.part_name.as_str().to_string(), position.row))
        };
        assert_eq!(position(1), Some(("first".to_string(), 0)));
        assert_eq!(position(2), Some(("first".to_string(), 1)));
        assert_eq!(position(3), Some(("second".to_string(), 0)));
        assert_eq!(position(5), Some(("second".to_string(), 2)));
        assert_eq!(position(6), Some(("first".to_string(), 0)));
        assert_eq!(position(7), Some(("first".to_string(), 1)));
        assert_eq!(position(0), None);
        assert_eq!(position(8), None);

        assert_eq!(
            playing_arrangement_position(&project, 1).map(|position| position.occurrence),
            Some(0)
        );
        assert_eq!(
            playing_arrangement_position(&project, 6).map(|position| position.occurrence),
            Some(2),
            "repeated parts should outline the occurrence that owns the current beat"
        );
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
    fn arrangement_duration_summary_uses_the_complete_arrangement() {
        let project = Project::new("test project", 5_125, 0, Seed::new(12))
            .with_parts(vec![Part::new("intro", 3), Part::new("verse", 5)])
            .with_sequence(vec!["intro".into(), "verse".into(), "verse".into()]);

        assert_eq!(arrangement_duration_summary(&project), "1:07");

        let one_second = Project::new("test project", 500, 0, Seed::new(12))
            .with_parts(vec![Part::new("intro", 2)]);
        assert_eq!(arrangement_duration_summary(&one_second), "0:01");

        let whole_minutes = Project::new("test project", 30_000, 0, Seed::new(12))
            .with_parts(vec![Part::new("intro", 4)]);
        assert_eq!(arrangement_duration_summary(&whole_minutes), "2:00");

        let empty = Project::new("test project", 500, 0, Seed::new(12));
        assert_eq!(arrangement_duration_summary(&empty), "0:00");
    }

    #[gpui::test]
    fn audio_build_renders_the_full_arrangement_instead_of_the_playback_loop(
        cx: &mut TestAppContext,
    ) {
        let root = temp_root("full-arrangement-audio-build");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();
        let part = Part::new("intro", 2);
        let project = Project::new("test project", 8, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec!["C4".to_string()], vec!["D4".to_string()]])
            .save(&project_directory, &part, &project)
            .unwrap();
        let output_files = planned_audio_files(&project);
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });

        model.update(cx, |model, cx| {
            model.loop_range = BeatRange::new(1, 1, 2).ok();
            let workspace = model.workspace.audio_build.clone();
            model.on_build_request(
                workspace,
                &BuildRequest {
                    request_id: 1,
                    sample_rate: BuildSampleRate::Hz48000,
                },
                cx,
            );
        });
        cx.run_until_parked();

        for output_file in output_files {
            let bytes = fs::read(project_directory.join("build").join(output_file.file_name))
                .expect("the background build should publish every planned WAV");
            assert_eq!(
                u32::from_le_bytes(bytes[46..50].try_into().unwrap()),
                768,
                "two eight-millisecond beats should be rendered even when playback loops one beat"
            );
        }

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
        update_project_sequence(
            &project_directory,
            &mut project,
            vec![intro.name.clone(), intro.name.clone()],
        )
        .unwrap();
        let dialog_parts = project.parts.clone();
        let dialog_sequence = project.sequence().to_vec();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let original_document =
            cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());
        let dialog = cx.new(|cx| PartsWorkspace::new(dialog_parts, dialog_sequence, cx));

        model.update(cx, |model, cx| {
            model.on_parts_request(
                dialog,
                &parts::Request::Update {
                    source: intro.name,
                    name: "opening theme".to_string(),
                    subdivision_pattern: Some(SubdivisionPattern::new([4, 3, 3]).unwrap()),
                    major_subdivision: None,
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
                model.score_views[0].part_name().unwrap().as_str(),
                "opening theme"
            );
            assert_eq!(
                model.workspace.loop_editor.read(cx).occurrence_names(),
                ["opening theme", "opening theme"]
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

        model.update(cx, |model, cx| model.undo(cx));
        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.project.parts()[0].name.as_str(), "intro");
            assert_eq!(model.score_documents[0].document, original_document);
            assert_eq!(model.score_views[0].part_name().unwrap().as_str(), "intro");
            assert_eq!(
                model.workspace.loop_editor.read(cx).occurrence_names(),
                ["intro", "intro"]
            );
        });
        assert!(project_directory.join("intro.csv").is_file());
        assert!(!project_directory.join("opening-theme.csv").exists());

        model.update(cx, |model, cx| model.redo(cx));
        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.project.parts()[0].name.as_str(), "opening theme");
            assert_eq!(model.score_documents[0].document, original_document);
            assert_eq!(
                model.score_views[0].part_name().unwrap().as_str(),
                "opening theme"
            );
        });
        assert!(!project_directory.join("intro.csv").exists());
        assert!(project_directory.join("opening-theme.csv").is_file());

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

        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.parts()[0].length),
            3
        );
        assert_eq!(cx.update(|_, cx| document.read(cx).score().rows().len()), 3);
        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.parts()[0].length),
            2
        );
        assert_eq!(cx.update(|_, cx| document.read(cx).score().rows().len()), 2);
        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.parts()[0].length),
            3
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
                .editor()
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
                model.score_views[0].part_name().unwrap().as_str(),
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
                .editor()
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

    #[gpui::test]
    fn restores_the_open_workspace_and_score_panes(cx: &mut TestAppContext) {
        let root = temp_root("restore-project-ui-state");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let intro = Part::new("intro", 2);
        let verse = Part::new("verse", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone(), verse.clone()]);
        for part in [&intro, &verse] {
            PartScore::from_rows(vec![vec![String::new()]; 2])
                .save(&project_directory, part, &project)
                .unwrap();
        }
        let ui_state = UiState {
            workspace: WorkspaceSectionKind::Parts,
            score_pane_count: 3,
            open_score_parts: vec![
                "verse".to_string(),
                "intro".to_string(),
                "verse".to_string(),
            ],
            active_score_pane: 1,
            score_arrangement_visible: false,
        };
        let expected_ui_state = ui_state.clone();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new_with_ui_state(project, project_directory, root.clone(), ui_state, cx)
        });

        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.ui_state(), expected_ui_state);
            assert_eq!(model.pane_count_dropdown.read(cx).selected_index(), 2);
        });

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn invalid_saved_score_panes_restore_to_valid_fallbacks(cx: &mut TestAppContext) {
        let root = temp_root("invalid-project-ui-state");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let intro = Part::new("intro", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone()]);
        PartScore::from_rows(vec![vec![String::new()]; 2])
            .save(&project_directory, &intro, &project)
            .unwrap();
        let ui_state = UiState {
            workspace: WorkspaceSectionKind::Score,
            score_pane_count: usize::MAX,
            open_score_parts: vec!["missing".to_string()],
            active_score_pane: usize::MAX,
            score_arrangement_visible: true,
        };

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new_with_ui_state(project, project_directory, root.clone(), ui_state, cx)
        });

        cx.update(|_, cx| {
            assert_eq!(
                model.read(cx).ui_state(),
                UiState {
                    workspace: WorkspaceSectionKind::Score,
                    score_pane_count: 3,
                    open_score_parts: vec!["intro".to_string(); 3],
                    active_score_pane: 2,
                    score_arrangement_visible: true,
                }
            );
        });

        fs::remove_dir_all(root).unwrap();
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
        let arrangement = cx.debug_bounds("score-arrangement-panel").unwrap();
        let arrangement_duration = cx
            .debug_bounds("score-arrangement-duration-summary")
            .unwrap();
        let workspace_right = workspace.origin.x + workspace.size.width;
        let arrangement_right = arrangement.origin.x + arrangement.size.width;

        assert!(panes.iter().all(|pane| pane.size.width > px(0.0)));
        assert!((panes[0].size.width / panes[1].size.width - 1.0).abs() < 0.01);
        assert!((panes[1].size.width / panes[2].size.width - 1.0).abs() < 0.01);
        assert_eq!(arrangement.size.width, crate::style::S9);
        assert!(panes[2].right() < arrangement.left());
        assert!(arrangement_duration.right() <= arrangement.right());
        assert!(arrangement_right <= workspace_right + px(1.0));

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_controls_only_show_in_score_and_toggle_the_arrangement_panel(cx: &mut TestAppContext) {
        let root = temp_root("score-arrangement-toggle");
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

        let pane_with_arrangement = cx.debug_bounds("score-view-0").unwrap();
        assert!(cx.debug_bounds("score-arrangement-panel").is_some());
        assert_eq!(cx.update(|_, cx| model.read(cx).bar_actions().len()), 5);

        let arrangement_button = cx.update(|_, cx| model.read(cx).score_arrangement_button.clone());
        model.update(cx, |model, cx| {
            model.on_score_arrangement_clicked(arrangement_button.clone(), &button::Clicked, cx);
        });
        cx.run_until_parked();

        assert!(!cx.update(|_, cx| model.read(cx).ui_state().score_arrangement_visible));
        let pane_without_arrangement = cx.debug_bounds("score-view-0").unwrap();
        assert!(pane_without_arrangement.size.width > pane_with_arrangement.size.width);

        let parts_button = cx.update(|_, cx| model.read(cx).parts_button.clone());
        model.update(cx, |model, cx| {
            model.on_parts_clicked(parts_button, &button::Clicked, cx);
        });
        assert_eq!(cx.update(|_, cx| model.read(cx).bar_actions().len()), 4);

        let score_button = cx.update(|_, cx| model.read(cx).score_button.clone());
        model.update(cx, |model, cx| {
            model.on_score_clicked(score_button, &button::Clicked, cx);
        });
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| model.read(cx).bar_actions().len()), 5);
        assert!(!cx.update(|_, cx| model.read(cx).ui_state().score_arrangement_visible));
        assert_eq!(
            cx.debug_bounds("score-view-0").unwrap().size.width,
            pane_without_arrangement.size.width
        );

        model.update(cx, |model, cx| {
            model.on_score_arrangement_clicked(arrangement_button, &button::Clicked, cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|_, cx| model.read(cx).ui_state().score_arrangement_visible));
        assert_eq!(
            cx.debug_bounds("score-view-0").unwrap().size.width,
            pane_with_arrangement.size.width
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_arrangement_highlights_every_occurrence_of_the_active_pane(cx: &mut TestAppContext) {
        let root = temp_root("score-arrangement-active-part");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let intro = Part::new("intro", 2);
        let verse = Part::new("verse", 4);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone(), verse.clone()])
            .with_sequence(vec![
                intro.name.clone(),
                verse.name.clone(),
                verse.name.clone(),
            ]);
        for part in [&intro, &verse] {
            PartScore::from_rows(vec![vec![String::new()]; part.length as usize])
                .save(&project_directory, part, &project)
                .unwrap();
        }
        let ui_state = UiState {
            workspace: WorkspaceSectionKind::Score,
            score_pane_count: 2,
            open_score_parts: vec!["intro".to_string(), "verse".to_string()],
            active_score_pane: 0,
            score_arrangement_visible: true,
        };
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new_with_ui_state(project, project_directory, root.clone(), ui_state, cx)
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        assert!(cx
            .debug_bounds("loop-arrangement-list-row-0-indicator")
            .is_some());

        model.update(cx, |model, cx| model.activate_score_view(1, cx));
        cx.run_until_parked();

        assert!(cx
            .debug_bounds("loop-arrangement-list-row-1-indicator")
            .is_some());
        assert!(cx
            .debug_bounds("loop-arrangement-list-row-2-indicator")
            .is_some());

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_arrangement_click_and_drag_updates_the_playback_loop(cx: &mut TestAppContext) {
        let root = temp_root("score-arrangement-loop");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let intro = Part::new("intro", 2);
        let verse = Part::new("verse", 4);
        let outro = Part::new("outro", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone(), verse.clone(), outro.clone()])
            .with_sequence(vec![
                intro.name.clone(),
                verse.name.clone(),
                outro.name.clone(),
            ]);
        for part in [&intro, &verse, &outro] {
            PartScore::from_rows(vec![vec![String::new()]; part.length as usize])
                .save(&project_directory, part, &project)
                .unwrap();
        }

        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let verse_row = cx.debug_bounds("loop-arrangement-list-row-1").unwrap();
        let outro_row = cx.debug_bounds("loop-arrangement-list-row-2").unwrap();
        cx.simulate_mouse_down(verse_row.center(), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            outro_row.center(),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(outro_row.center(), MouseButton::Left, Modifiers::default());
        cx.run_until_parked();

        assert_eq!(
            cx.update(|_, cx| model.read(cx).loop_range),
            BeatRange::new(3, 8, 8).ok()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_arrangement_context_menu_opens_the_clicked_part_in_a_specific_panel(
        cx: &mut TestAppContext,
    ) {
        let root = temp_root("score-arrangement-context-open-panel");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let intro = Part::new("intro", 2);
        let verse = Part::new("verse", 4);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone(), verse.clone()])
            .with_sequence(vec![intro.name.clone(), verse.name.clone()]);
        for part in [&intro, &verse] {
            PartScore::from_rows(vec![vec![String::new()]; part.length as usize])
                .save(&project_directory, part, &project)
                .unwrap();
        }
        let ui_state = UiState {
            workspace: WorkspaceSectionKind::Score,
            score_pane_count: 2,
            open_score_parts: vec!["intro".to_string(), "intro".to_string()],
            active_score_pane: 0,
            score_arrangement_visible: true,
        };
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new_with_ui_state(project, project_directory, root.clone(), ui_state, cx)
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let verse_occurrence = cx.debug_bounds("loop-arrangement-list-row-1").unwrap();
        cx.simulate_mouse_down(
            verse_occurrence.center(),
            MouseButton::Right,
            Modifiers::default(),
        );
        let open_in_panel_2 = cx
            .debug_bounds("loop-arrangement-list-context-action-1")
            .unwrap();
        cx.simulate_click(open_in_panel_2.center(), Modifiers::default());
        cx.run_until_parked();

        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.active_score_view, 1);
            assert_eq!(
                model.score_views[1].part_name().map(PartName::as_str),
                Some("verse")
            );
            assert_eq!(
                model.ui_state().open_score_parts,
                vec!["intro".to_string(), "verse".to_string()]
            );
        });
        assert!(cx
            .debug_bounds("loop-arrangement-list-context-menu")
            .is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_arrangement_context_menu_removes_only_the_clicked_occurrence(cx: &mut TestAppContext) {
        let root = temp_root("score-arrangement-context-remove");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let intro = Part::new("intro", 2);
        let verse = Part::new("verse", 4);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone(), verse.clone()])
            .with_sequence(vec![
                intro.name.clone(),
                verse.name.clone(),
                intro.name.clone(),
            ]);
        for part in [&intro, &verse] {
            PartScore::from_rows(vec![vec![String::new()]; part.length as usize])
                .save(&project_directory, part, &project)
                .unwrap();
        }
        project::save_project(&project_directory, &project).unwrap();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        cx.simulate_resize(size(px(1_000.0), px(700.0)));
        cx.run_until_parked();

        let first_occurrence = cx.debug_bounds("loop-arrangement-list-row-0").unwrap();
        cx.simulate_mouse_down(
            first_occurrence.center(),
            MouseButton::Right,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            first_occurrence.center(),
            MouseButton::Right,
            Modifiers::default(),
        );

        assert!(cx
            .debug_bounds("loop-arrangement-list-context-menu")
            .is_some());
        let remove = cx
            .debug_bounds("loop-arrangement-list-context-action-1")
            .unwrap();
        cx.simulate_click(remove.center(), Modifiers::default());
        cx.run_until_parked();

        let expected = vec![verse.name.clone(), intro.name.clone()];
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.sequence().to_vec()),
            expected
        );
        assert_eq!(
            project::load_project(&project_directory)
                .unwrap()
                .project
                .sequence(),
            expected
        );
        assert!(cx
            .debug_bounds("loop-arrangement-list-context-menu")
            .is_none());
        assert_eq!(
            cx.update(|_, cx| {
                model
                    .read(cx)
                    .workspace
                    .loop_editor
                    .read(cx)
                    .arrangement_range()
                    .read(cx)
                    .row_count()
            }),
            2
        );

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
        let editor = cx.update(|_, cx| model.read(cx).score_views[0].editor().cloned().unwrap());
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
                    .part_name()
                    .unwrap()
                    .as_str()
                    .to_string(),
                model.score_views[1]
                    .part_name()
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
    fn score_edit_part_action_opens_that_part_in_the_parts_workspace(cx: &mut TestAppContext) {
        let root = temp_root("score-edit-part-action");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let first_part = Part::new("part-a", 4);
        let second_part = Part::new("part-b", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_parts(vec![first_part.clone(), second_part.clone()]);
        PartScore::from_rows(vec![Vec::new(); 4])
            .save(&project_directory, &first_part, &project)
            .unwrap();
        PartScore::from_rows(vec![Vec::new(); 2])
            .save(&project_directory, &second_part, &project)
            .unwrap();

        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        model.update(cx, |model, cx| {
            model.select_part(second_part.name.clone(), cx);
        });
        let actions = cx.update(|_, cx| {
            model.read(cx).score_views[0]
                .editor()
                .unwrap()
                .read(cx)
                .actions()
        });

        actions.update(cx, |menu, cx| {
            menu.activate(ScoreAction::EditPart.index(), cx);
        });
        cx.run_until_parked();

        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.workspace.section.kind(), WorkspaceSectionKind::Parts);
            assert_eq!(
                model.workspace.parts.read(cx).editing_part(),
                Some(&second_part.name)
            );
        });

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_subdivision_action_updates_the_part_without_leaving_score_or_clearing_dirty_cells(
        cx: &mut TestAppContext,
    ) {
        let root = temp_root("score-subdivision-action");
        let project_directory = root.join("project");
        fs::create_dir_all(&project_directory).unwrap();

        let part = Part::new("part-a", 6);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        PartScore::from_rows(vec![vec![String::new()]; 6])
            .save(&project_directory, &part, &project)
            .unwrap();

        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let (document, actions) = cx.update(|_, cx| {
            let model = model.read(cx);
            let editor = model.score_views[0].editor().unwrap().read(cx);
            (model.score_documents[0].document.clone(), editor.actions())
        });
        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });

        actions.update(cx, |menu, cx| {
            menu.activate(ScoreAction::EditSubdivision.index(), cx);
        });
        cx.run_until_parked();

        let dialog = cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.workspace.section.kind(), WorkspaceSectionKind::Score);
            let WorkspaceSection::Score {
                overlay: Some(score::Overlay::Subdivision(dialog)),
            } = &model.workspace.section
            else {
                panic!("the score subdivision action should open its dialog");
            };
            dialog.clone()
        });
        model.update(cx, |model, cx| {
            model.on_subdivision_dialog_msg(
                dialog,
                &score::SubdivisionDialogMsg::Confirmed {
                    part_name: part.name.clone(),
                    subdivision_pattern: Some(SubdivisionPattern::new([2, 3]).unwrap()),
                    major_subdivision: Some(MajorSubdivision::new(12).unwrap()),
                },
                cx,
            );
        });

        cx.update(|_, cx| {
            let model = model.read(cx);
            assert_eq!(model.workspace.section.kind(), WorkspaceSectionKind::Score);
            assert!(model.active_overlay().is_none());
            assert_eq!(
                model.project.parts()[0]
                    .subdivision_pattern()
                    .unwrap()
                    .subdivisions()
                    .collect::<Vec<_>>(),
                [2, 3]
            );
            assert_eq!(
                document
                    .read(cx)
                    .part()
                    .major_subdivision()
                    .unwrap()
                    .beats(),
                12
            );
            assert!(document.read(cx).is_dirty());
            assert_eq!(document.read(cx).score().rows()[0][0], "C4");
            assert_eq!(
                document
                    .read(cx)
                    .part()
                    .subdivision_pattern()
                    .unwrap()
                    .subdivisions()
                    .collect::<Vec<_>>(),
                [2, 3]
            );
        });
        assert_eq!(
            project::load_project(&project_directory)
                .unwrap()
                .project
                .parts()[0]
                .subdivision_pattern()
                .unwrap()
                .subdivisions()
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(
            project::load_project(&project_directory)
                .unwrap()
                .project
                .parts()[0]
                .major_subdivision()
                .unwrap()
                .beats(),
            12
        );

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
    fn mts_master_error_shows_only_its_compact_reset_action(cx: &mut TestAppContext) {
        let root = temp_root("mts-master-reset-action");
        let project = Project::new("test project", 800, 0, Seed::new(12));
        let project_directory = project::create_project(&root, &project).unwrap();
        let (model, cx) =
            cx.add_window_view(|_, cx| Model::new(project, project_directory, root.clone(), cx));
        model.update(cx, |model, cx| {
            model.transport_error = Some(TransportError::MtsMasterAlreadyActive {
                message: "another MTS-ESP master is already active".to_string(),
                retry_target: PlaybackTarget::Arrangement,
            });
            cx.notify();
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("reset-mts-esp").is_some());
        assert!(cx.debug_bounds("copy-status-error").is_some());

        model.update(cx, |model, cx| {
            model.transport_error = Some(TransportError::Message("ordinary error".to_string()));
            cx.notify();
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("reset-mts-esp").is_none());
        assert!(cx.debug_bounds("copy-status-error").is_some());

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
        let (parts_workspace, voices_workspace, parts_button, voices_button, settings_button) = cx
            .update(|_, cx| {
                let model = model.read(cx);
                (
                    model.workspace.parts.clone(),
                    model.workspace.voices.clone(),
                    model.parts_button.clone(),
                    model.voices_button.clone(),
                    model.settings_button.clone(),
                )
            });

        model.update(cx, |model, cx| {
            model.on_parts_clicked(parts_button, &button::Clicked, cx);
            model.on_parts_request(
                parts_workspace,
                &parts::Request::ConfirmDelete {
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
            model.on_voices_request(
                voices_workspace,
                &voices::Request::ConfirmDelete {
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
        });
        cx.update(|_, cx| {
            assert_eq!(
                model.read(cx).workspace.section.kind(),
                WorkspaceSectionKind::Project
            );
            assert!(model.read(cx).active_overlay().is_none());
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
    fn project_history_undoes_and_redoes_score_edits_in_order(cx: &mut TestAppContext) {
        let root = temp_root("score-history");
        let part = Part::new("part-a", 2);
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let project_directory = project::create_project(&root, &project).unwrap();
        PartScore::from_rows(vec![vec![String::new()], vec![String::new()]])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
            document.update_cell(u64::MAX, 0, 0, "D4".to_string(), cx);
        });
        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 1, 0, "E4".to_string(), cx);
        });

        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows().to_vec()),
            vec![vec!["D4".to_string()], vec![String::new()]]
        );
        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows().to_vec()),
            vec![vec![String::new()], vec![String::new()]]
        );
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\n\"\"\n\"\"\n"
        );

        model.update(cx, |model, cx| model.redo(cx));
        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows().to_vec()),
            vec![vec!["D4".to_string()], vec!["E4".to_string()]]
        );
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nD4\nE4\n"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn score_panes_transition_together_through_rename_delete_and_history(cx: &mut TestAppContext) {
        let root = temp_root("score-pane-state");
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let project_directory = project::create_project(&root, &project).unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });

        model.update(cx, |model, cx| {
            model.set_view_count(3, cx);
            assert!(model
                .score_views
                .iter()
                .all(|pane| matches!(pane, super::ScorePane::Empty)));
            assert!(model.ui_state().open_score_parts.is_empty());

            model.on_parts_request(
                model.workspace.parts.clone(),
                &parts::Request::Add {
                    name: "intro".to_string(),
                    length: 2,
                    subdivision_pattern: None,
                    major_subdivision: None,
                },
                cx,
            );
            assert!(matches!(
                model.score_views[0],
                super::ScorePane::Open { .. }
            ));
            // New panes inherit the active part when one is available.
            model.set_view_count(1, cx);
            model.set_view_count(3, cx);
            let editors = model
                .score_views
                .iter()
                .map(|pane| pane.editor().unwrap().clone())
                .collect::<Vec<_>>();

            model.on_parts_request(
                model.workspace.parts.clone(),
                &parts::Request::Update {
                    source: PartName::new("intro"),
                    name: "opening".to_string(),
                    subdivision_pattern: None,
                    major_subdivision: None,
                },
                cx,
            );
            for (pane, original_editor) in model.score_views.iter().zip(&editors) {
                let super::ScorePane::Open { part_name, editor } = pane else {
                    panic!("renaming must keep every pane open");
                };
                assert_eq!(part_name.as_str(), "opening");
                assert_eq!(editor, original_editor);
            }

            let name = PartName::new("opening");
            let confirmation = cx.new(|cx| parts::DeleteDialog::new(name.clone(), cx));
            model.delete_part_from_dialog(confirmation, &name, cx);
            assert!(model.project.parts().is_empty());
            assert!(model
                .score_views
                .iter()
                .all(|pane| matches!(pane, super::ScorePane::Empty)));
            assert!(model.active_part().is_none());
            assert!(model.ui_state().open_score_parts.is_empty());

            model.undo(cx);
            for pane in &model.score_views {
                let super::ScorePane::Open { part_name, .. } = pane else {
                    panic!("undoing deletion must reopen the restored part");
                };
                assert_eq!(part_name.as_str(), "opening");
            }
            model.redo(cx);
            assert!(model
                .score_views
                .iter()
                .all(|pane| matches!(pane, super::ScorePane::Empty)));
        });

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn project_history_restores_added_part_files(cx: &mut TestAppContext) {
        let root = temp_root("part-file-history");
        let intro = Part::new("intro", 1);
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![intro.clone()]);
        let project_directory = project::create_project(&root, &project).unwrap();
        PartScore::from_rows(vec![vec![String::new()]])
            .save(&project_directory, &intro, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let parts_workspace = cx.update(|_, cx| model.read(cx).workspace.parts.clone());

        model.update(cx, |model, cx| {
            model.on_parts_request(
                parts_workspace,
                &parts::Request::Add {
                    name: "verse".to_string(),
                    length: 2,
                    subdivision_pattern: None,
                    major_subdivision: None,
                },
                cx,
            );
        });
        assert!(project_directory.join("verse.csv").is_file());
        assert!(cx.update(|_, cx| {
            model
                .read(cx)
                .project
                .part(&PartName::new("verse"))
                .is_some()
        }));

        model.update(cx, |model, cx| model.undo(cx));
        let undo_error = cx.update(|_, cx| model.read(cx).workspace_error.clone());
        assert!(
            !project_directory.join("verse.csv").exists(),
            "undo error: {undo_error:?}"
        );
        assert!(cx.update(|_, cx| {
            model
                .read(cx)
                .project
                .part(&PartName::new("verse"))
                .is_none()
        }));

        model.update(cx, |model, cx| model.redo(cx));
        assert!(project_directory.join("verse.csv").is_file());
        let (restored_part, voices) = cx.update(|_, cx| {
            let model = model.read(cx);
            (
                model.project.part(&PartName::new("verse")).unwrap().clone(),
                model.project.voices().to_vec(),
            )
        });
        assert_eq!(
            PartScore::load(&project_directory, &restored_part, &voices)
                .unwrap()
                .rows()
                .len(),
            2
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn project_history_redoes_invalid_score_edits_through_recovery(cx: &mut TestAppContext) {
        let root = temp_root("invalid-score-history");
        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let project_directory = project::create_project(&root, &project).unwrap();
        PartScore::from_rows(vec![vec!["C4".to_string()]])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "half-typed".to_string(), cx);
        });
        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "C4"
        );
        assert!(!project_directory.join(".part-a.csv.recovery").exists());

        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "half-typed"
        );
        assert!(project_directory.join(".part-a.csv.recovery").is_file());
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));

        let mut updated_project = cx.update(|_, cx| model.read(cx).project.clone());
        updated_project.beat_duration_millis = 1_200.into();
        project::save_project(&project_directory, &updated_project).unwrap();
        let settings_workspace =
            cx.update(|_, cx| model.read(cx).workspace.project_settings.clone());
        model.update(cx, |model, cx| {
            model.on_settings_msg(
                settings_workspace,
                &ProjectSettingsMsg::Saved(Box::new(updated_project)),
                cx,
            );
        });
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.beat_duration_millis.get()),
            1_200
        );

        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.beat_duration_millis.get()),
            800
        );
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "half-typed"
        );
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(project_directory.join(".part-a.csv.recovery").is_file());

        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).project.beat_duration_millis.get()),
            1_200
        );
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "half-typed"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn project_history_undoes_score_edits_after_tuning_invalidates_the_saved_score(
        cx: &mut TestAppContext,
    ) {
        let root = temp_root("tuning-invalid-score-history");
        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let project_directory = project::create_project(&root, &project).unwrap();
        PartScore::from_rows(vec![vec!["C4".to_string()]])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let document = cx.update(|_, cx| model.read(cx).score_documents[0].document.clone());

        let incompatible_tuning = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
            )
            .unwrap(),
        );
        let updated_project = cx
            .update(|_, cx| model.read(cx).project.clone())
            .with_pitch_system(incompatible_tuning);
        project::save_project(&project_directory, &updated_project).unwrap();
        let settings_workspace =
            cx.update(|_, cx| model.read(cx).workspace.project_settings.clone());
        model.update(cx, |model, cx| {
            model.on_settings_msg(
                settings_workspace,
                &ProjectSettingsMsg::Saved(Box::new(updated_project)),
                cx,
            );
        });
        assert_eq!(cx.update(|_, cx| document.read(cx).parse_issues().len()), 1);
        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "half-typed".to_string(), cx);
        });
        model.update(cx, |model, cx| model.undo(cx));

        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "C4"
        );
        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(!project_directory.join(".part-a.csv.recovery").exists());

        model.update(cx, |model, cx| model.undo(cx));
        assert!(cx.update(|_, cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));

        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(cx.update(|_, cx| document.read(cx).parse_issues().len()), 1);
        assert!(!cx.update(|_, cx| document.read(cx).is_dirty()));
        assert!(!project_directory.join(".part-a.csv.recovery").exists());

        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            cx.update(|_, cx| document.read(cx).score().rows()[0][0].clone()),
            "half-typed"
        );
        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );
        assert!(project_directory.join(".part-a.csv.recovery").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn project_history_restores_voice_schema_changes(cx: &mut TestAppContext) {
        let root = temp_root("voice-history");
        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 800, 0, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let project_directory = project::create_project(&root, &project).unwrap();
        PartScore::from_rows(vec![vec!["C4".to_string()]])
            .save(&project_directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project, project_directory.clone(), root.clone(), cx)
        });
        let voices_workspace = cx.update(|_, cx| model.read(cx).workspace.voices.clone());

        model.update(cx, |model, cx| {
            model.on_voices_request(
                voices_workspace,
                &voices::Request::Change(voices::Change::Add {
                    name: "harmony".to_string(),
                    voice_type: VoiceType::Sin,
                    position: Point3Meters::default(),
                    volume_adjustment: None,
                }),
                cx,
            );
        });
        assert_eq!(cx.update(|_, cx| model.read(cx).project.voices().len()), 2);
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead,harmony\nC4,\n"
        );

        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(cx.update(|_, cx| model.read(cx).project.voices().len()), 1);
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead\nC4\n"
        );

        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(cx.update(|_, cx| model.read(cx).project.voices().len()), 2);
        assert_eq!(
            fs::read_to_string(project_directory.join("part-a.csv")).unwrap(),
            "lead,harmony\nC4,\n"
        );

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
