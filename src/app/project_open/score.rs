use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use gpui::{div, prelude::*, Context, Entity, EventEmitter, ScrollHandle, SharedString, Window};

use crate::{
    note::Note,
    part::{Part, PartScore, ScoreError},
    project::Project,
    style as s,
    view::{
        button::{self, Button},
        data_grid,
        dropdown::{self, Dropdown},
        text_input::{Changed, TextInput},
    },
};

static NEXT_EDITOR_ID: AtomicU64 = AtomicU64::new(1);

pub struct ScoreDocument {
    project: Project,
    project_directory: PathBuf,
    part: Part,
    score: PartScore,
    dirty: bool,
    last_save_error: Option<String>,
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
                        Note::parse_cell(value).err().map(|source| ParseIssue {
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
        cx.emit(DocumentEvent::CellChanged {
            source_editor,
            row,
            column,
            value,
        });
        cx.notify();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> Result<(), ScoreError> {
        match self
            .score
            .save(&self.project_directory, &self.part, self.project.voices())
        {
            Ok(()) => {
                self.dirty = false;
                self.last_save_error = None;
                cx.emit(DocumentEvent::Saved);
                cx.notify();
                Ok(())
            }
            Err(error) => {
                self.last_save_error = Some(error.to_string());
                cx.emit(DocumentEvent::SaveFailed);
                cx.notify();
                Err(error)
            }
        }
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
    scroll_handle: ScrollHandle,
    save_button: Entity<Button>,
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
        let save_button = cx.new(|_| Button::new(("save-score", editor_id), "save"));

        cx.subscribe(&document, Self::on_document_event).detach();
        cx.subscribe(&part_dropdown, Self::on_part_selected)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();

        Self {
            editor_id,
            document,
            part_names,
            part_dropdown,
            cells,
            scroll_handle: ScrollHandle::new(),
            save_button,
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
            DocumentEvent::Saved | DocumentEvent::SaveFailed | DocumentEvent::ProjectChanged => {}
        }
        cx.notify();
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let _ = self.document.update(cx, |document, cx| document.save(cx));
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
            .justify_between()
            .gap_4()
            .child(self.part_dropdown.clone())
            .child(self.save_button.clone());
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
    use std::path::PathBuf;

    use super::ScoreDocument;
    use crate::{
        part::{Part, PartScore},
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
}
