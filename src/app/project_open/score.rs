use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use gpui::{
    div, prelude::*, AsyncApp, Context, Entity, EventEmitter, ScrollHandle, SharedString, Task,
    WeakEntity, Window,
};

use crate::{
    part::{Part, PartScore, ScoreError},
    project::Project,
    style as s,
    view::{
        data_grid,
        dropdown::{self, Dropdown},
        text_input::{Changed, TextInput},
    },
};

static NEXT_EDITOR_ID: AtomicU64 = AtomicU64::new(1);
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(750);
const AUTOSAVE_MAX_DELAY: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SaveState {
    Idle,
    Saving,
    SavingRecovered,
    RecoverySaved,
    Saved,
}

pub struct ScoreDocument {
    project: Project,
    project_directory: PathBuf,
    part: Part,
    score: PartScore,
    dirty: bool,
    last_save_error: Option<String>,
    save_state: SaveState,
    pending_autosave_since: Option<Instant>,
    autosave_task: Option<Task<()>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ParseIssue {
    pub row: usize,
    pub column: usize,
    pub voice: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum DocumentEvent {
    CellChanged {
        source_editor: u64,
        row: usize,
        column: usize,
        value: String,
    },
    Saved,
    RecoverySaved,
    SaveFailed,
    Reset,
    ProjectChanged,
}

impl EventEmitter<DocumentEvent> for ScoreDocument {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartSelected {
    pub part_name: crate::part::PartName,
}

impl ScoreDocument {
    pub fn new(project: Project, project_directory: PathBuf, part: Part, score: PartScore) -> Self {
        Self {
            project,
            project_directory,
            part,
            score,
            dirty: false,
            last_save_error: None,
            save_state: SaveState::Idle,
            pending_autosave_since: None,
            autosave_task: None,
        }
    }

    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn part(&self) -> &Part {
        &self.part
    }

    pub fn score(&self) -> &PartScore {
        &self.score
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn last_save_error(&self) -> Option<&str> {
        self.last_save_error.as_deref()
    }

    pub(super) fn save_state(&self) -> SaveState {
        self.save_state
    }

    pub(super) fn autosave_recovered_score(&mut self, cx: &mut Context<Self>) {
        self.dirty = true;
        self.save_state = SaveState::SavingRecovered;
        self.schedule_autosave(cx);
        cx.notify();
    }

    pub fn parse_issues(&self) -> Vec<ParseIssue> {
        self.score
            .rows()
            .iter()
            .enumerate()
            .flat_map(|(row, values)| {
                values
                    .iter()
                    .enumerate()
                    .filter_map(move |(column, value)| {
                        self.project
                            .pitch_system()
                            .resolve_cell(value)
                            .err()
                            .map(|source| ParseIssue {
                                row,
                                column,
                                voice: self
                                    .project
                                    .voices()
                                    .get(column)
                                    .map(|voice| voice.name.as_str().to_string())
                                    .unwrap_or_else(|| format!("column {}", column + 1)),
                                message: source.to_string(),
                            })
                    })
            })
            .collect()
    }

    pub fn update_cell(
        &mut self,
        source_editor: u64,
        row: usize,
        column: usize,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let mut rows = self.score.rows().to_vec();
        let Some(cell) = rows.get_mut(row).and_then(|row| row.get_mut(column)) else {
            return;
        };
        if *cell == value {
            return;
        }

        *cell = value.clone();
        self.score = PartScore::from_rows(rows);
        self.dirty = true;
        self.last_save_error = None;
        self.schedule_autosave(cx);
        cx.emit(DocumentEvent::CellChanged {
            source_editor,
            row,
            column,
            value,
        });
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        self.autosave_task.take();
        self.pending_autosave_since = None;
        if let Err(error) =
            self.score
                .save_recovery(&self.project_directory, &self.part, self.project.voices())
        {
            return self.save_failed(error, cx);
        }

        match self
            .score
            .save(&self.project_directory, &self.part, &self.project)
        {
            Ok(()) => self.finish_save(cx),
            Err(error) => {
                self.save_state = SaveState::RecoverySaved;
                self.save_failed(error, cx)
            }
        }
    }

    fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let pending_since = *self.pending_autosave_since.get_or_insert(now);
        let remaining =
            AUTOSAVE_MAX_DELAY.saturating_sub(now.saturating_duration_since(pending_since));
        let delay = AUTOSAVE_DEBOUNCE.min(remaining);
        if self.save_state != SaveState::SavingRecovered {
            self.save_state = SaveState::Saving;
        }

        self.autosave_task = Some(cx.spawn(
            async move |document: WeakEntity<ScoreDocument>, cx: &mut AsyncApp| {
                cx.background_executor().timer(delay).await;
                document
                    .update(cx, |document, cx| document.autosave(cx))
                    .ok();
            },
        ));
    }

    fn autosave(&mut self, cx: &mut Context<Self>) {
        self.pending_autosave_since = None;
        if let Err(error) =
            self.score
                .save_recovery(&self.project_directory, &self.part, self.project.voices())
        {
            let _ = self.save_failed(error, cx);
            return;
        }

        match self
            .score
            .save(&self.project_directory, &self.part, &self.project)
        {
            Ok(()) => {
                let _ = self.finish_save(cx);
            }
            Err(ScoreError::InvalidPitch { .. }) => {
                self.last_save_error = None;
                self.save_state = SaveState::RecoverySaved;
                cx.emit(DocumentEvent::RecoverySaved);
                cx.notify();
            }
            Err(error) => {
                let _ = self.save_failed(error, cx);
            }
        }
    }

    fn finish_save(&mut self, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        self.save_state = SaveState::Saved;
        match PartScore::clear_recovery(&self.project_directory, &self.part) {
            Ok(()) => {
                self.dirty = false;
                self.last_save_error = None;
                cx.emit(DocumentEvent::Saved);
                cx.notify();
                Ok(())
            }
            Err(error) => self.save_failed(error, cx),
        }
    }

    fn save_failed(&mut self, error: ScoreError, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        self.last_save_error = Some(error.to_string());
        cx.emit(DocumentEvent::SaveFailed);
        cx.notify();
        Err(error)
    }

    pub fn replace_project_and_score(
        &mut self,
        project: Project,
        part: Part,
        score: PartScore,
        cx: &mut Context<Self>,
    ) {
        self.project = project;
        self.part = part;
        self.score = score;
        self.dirty = false;
        self.last_save_error = None;
        self.save_state = SaveState::Idle;
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::Reset);
        cx.notify();
    }

    pub fn project_settings_changed(&mut self, project: Project, cx: &mut Context<Self>) {
        self.project = project;
        self.last_save_error = None;
        cx.emit(DocumentEvent::ProjectChanged);
        cx.notify();
    }
}

pub struct ScoreEditor {
    editor_id: u64,
    document: Entity<ScoreDocument>,
    part_names: Vec<crate::part::PartName>,
    part_dropdown: Entity<Dropdown>,
    cells: Vec<Vec<Entity<TextInput>>>,
    playing_row: Option<usize>,
    scroll_handle: ScrollHandle,
}

impl EventEmitter<PartSelected> for ScoreEditor {}

impl ScoreEditor {
    pub fn new(
        view_index: usize,
        document: Entity<ScoreDocument>,
        part_names: Vec<crate::part::PartName>,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor_id = NEXT_EDITOR_ID.fetch_add(1, Ordering::Relaxed);
        let document_state = document.read(cx);
        let score = document_state.score().clone();
        let selected_part = document_state.part().name.clone();
        let selected_index = part_names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(&selected_part))
            .expect("the editor part must be present in the project");
        let dropdown_options = part_names
            .iter()
            .map(|name| name.as_str().to_string())
            .collect::<Vec<_>>();
        let part_dropdown = cx.new(move |cx| {
            Dropdown::new(
                ("score-part", view_index),
                dropdown_options,
                selected_index,
                cx,
            )
        });
        let cells = Self::build_cells(editor_id, &score, cx);

        cx.subscribe(&document, Self::on_document_event).detach();
        cx.subscribe(&part_dropdown, Self::on_part_selected)
            .detach();

        Self {
            editor_id,
            document,
            part_names,
            part_dropdown,
            cells,
            playing_row: None,
            scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn set_available_parts(
        &mut self,
        part_names: Vec<crate::part::PartName>,
        cx: &mut Context<Self>,
    ) {
        let selected_part = &self.document.read(cx).part().name;
        let selected_index = part_names
            .iter()
            .position(|name| name.eq_ignore_ascii_case(selected_part))
            .expect("the editor part must be present in the project");
        let options = part_names
            .iter()
            .map(|name| name.as_str().to_string())
            .collect::<Vec<_>>();
        self.part_names = part_names;
        self.part_dropdown.update(cx, |dropdown, cx| {
            dropdown.set_options(options, selected_index, cx);
        });
        cx.notify();
    }

    fn on_part_selected(
        &mut self,
        _: Entity<Dropdown>,
        selected: &dropdown::Selected,
        cx: &mut Context<Self>,
    ) {
        let Some(part_name) = self.part_names.get(selected.index).cloned() else {
            return;
        };
        cx.emit(PartSelected { part_name });
    }

    fn build_cells(
        editor_id: u64,
        score: &PartScore,
        cx: &mut Context<Self>,
    ) -> Vec<Vec<Entity<TextInput>>> {
        score
            .rows()
            .iter()
            .enumerate()
            .map(|(row_index, row)| {
                row.iter()
                    .enumerate()
                    .map(|(column_index, value)| {
                        let input = cx.new(|cx| TextInput::new(value.clone(), "", cx));
                        cx.subscribe(&input, move |editor, input, _: &Changed, cx| {
                            editor.on_cell_changed(editor_id, row_index, column_index, input, cx);
                        })
                        .detach();
                        input
                    })
                    .collect()
            })
            .collect()
    }

    fn on_cell_changed(
        &mut self,
        editor_id: u64,
        row: usize,
        column: usize,
        input: Entity<TextInput>,
        cx: &mut Context<Self>,
    ) {
        let value = input.read(cx).value();
        self.document.update(cx, |document, cx| {
            document.update_cell(editor_id, row, column, value, cx);
        });
    }

    fn on_document_event(
        &mut self,
        _: Entity<ScoreDocument>,
        event: &DocumentEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            DocumentEvent::CellChanged {
                source_editor,
                row,
                column,
                value,
            } => {
                if *source_editor != self.editor_id {
                    if let Some(cell) = self.cells.get(*row).and_then(|row| row.get(*column)) {
                        let value: SharedString = value.clone().into();
                        cell.update(cx, |cell, cx| cell.sync_value(value, cx));
                    }
                }
            }
            DocumentEvent::Reset => {
                let score = self.document.read(cx).score().clone();
                self.cells = Self::build_cells(self.editor_id, &score, cx);
            }
            DocumentEvent::Saved
            | DocumentEvent::RecoverySaved
            | DocumentEvent::SaveFailed
            | DocumentEvent::ProjectChanged => {}
        }
        cx.notify();
    }

    pub(super) fn reveal_issue(
        &mut self,
        row: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        data_grid::reveal_cell(&self.scroll_handle, row, column);
        if let Some(cell) = self.cells.get(row).and_then(|row| row.get(column)) {
            cell.read(cx).focus(window);
        }
        cx.notify();
    }

    pub(super) fn set_playing_row(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        if self.playing_row != row {
            self.playing_row = row;
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(super) fn playing_row(&self) -> Option<usize> {
        self.playing_row
    }
}

impl Render for ScoreEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let document = self.document.read(cx);
        let column_labels = document
            .project()
            .voices()
            .iter()
            .map(|voice| format!("{} ({})", voice.name.as_str(), voice.voice_type.label()))
            .collect::<Vec<_>>();
        let has_voices = !column_labels.is_empty();
        let invalid_cells = document
            .parse_issues()
            .into_iter()
            .map(|issue| (issue.row, issue.column))
            .collect::<Vec<_>>();
        let header = div()
            .flex()
            .items_center()
            .child(self.part_dropdown.clone());
        let score_content = if !has_voices {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(s::TEXT_DEFAULT)
                .child("add a sin or saw voice to edit and play this part")
        } else {
            data_grid::editable(
                ("score-grid", self.editor_id),
                column_labels,
                &self.cells,
                &invalid_cells,
                self.playing_row,
                &self.scroll_handle,
            )
        };

        s::raised(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .w_full()
                .min_w(s::S0)
                .min_h(s::S0)
                .overflow_hidden()
                .gap(s::CONTENT_PADDING)
                .bg(s::GRAY2)
                .p(s::CONTENT_PADDING)
                .child(header)
                .child(score_content),
        )
        .flex()
        .flex_1()
        .w_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use super::ScoreDocument;
    use crate::{
        part::{Part, PartScore},
        pitch_system::{ExplicitPitchSystem, FrequencyHz, PitchSystem},
        project::{Project, Voice, VoiceType},
        seed::Seed,
    };

    #[test]
    fn reports_every_invalid_score_cell() {
        let project = Project::new("test project", 20_000, 32, Seed::new(12)).with_voices(vec![
            Voice::new(1, "lead", VoiceType::Saw),
            Voice::new(2, "bass", VoiceType::Sin),
        ]);
        let part = Part::new("part-a", 2);
        let score = PartScore::from_rows(vec![
            vec!["not-a-note".to_string(), "C4".to_string()],
            vec!["128".to_string(), "H4".to_string()],
        ]);
        let document = ScoreDocument::new(project, PathBuf::new(), part, score);

        let issues = document.parse_issues();

        assert_eq!(issues.len(), 3);
        assert_eq!((issues[0].row, issues[0].column), (0, 0));
        assert_eq!(issues[0].voice, "lead");
        assert_eq!((issues[1].row, issues[1].column), (1, 0));
        assert_eq!((issues[2].row, issues[2].column), (1, 1));
        assert_eq!(issues[2].voice, "bass");
    }

    #[test]
    fn score_issues_use_the_projects_explicit_notation() {
        let pitch_system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
            )
            .unwrap(),
        );
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)]);
        let part = Part::new("part-a", 2);
        let score = PartScore::from_rows(vec![vec!["ember".to_string()], vec!["C4".to_string()]]);
        let document = ScoreDocument::new(project, PathBuf::new(), part, score);

        let issues = document.parse_issues();

        assert_eq!(issues.len(), 1);
        assert_eq!((issues[0].row, issues[0].column), (1, 0));
        assert!(issues[0].message.contains("not defined in \"embers\""));
    }
}
