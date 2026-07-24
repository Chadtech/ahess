use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use gpui::{
    div, prelude::*, AsyncApp, Context, Entity, EventEmitter, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ScrollHandle, SharedString, Task, WeakEntity, Window,
};

use crate::{
    part::{
        Part, PartRowEdit, PartRowEditError, PartScore, ScoreError, ScoreRowIndex, ScoreRowRange,
    },
    project::Project,
    style as s,
    view::{
        button::{self, Button},
        data_grid,
        dialog::{destructive_confirmation, title_bar},
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
    RowsCleared,
    StructureChanged {
        source_editor: u64,
        selected_rows: ScoreRowRange,
    },
    Reset,
    ProjectChanged,
}

impl EventEmitter<DocumentEvent> for ScoreDocument {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartSelected {
    pub part_name: crate::part::PartName,
}

#[derive(Clone, Debug)]
pub struct RowEditRequested {
    pub source_editor: u64,
    pub part_name: crate::part::PartName,
    pub edit: PartRowEdit,
    pub populated_cell_count: usize,
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

    pub fn clear_rows(
        &mut self,
        rows: ScoreRowRange,
        cx: &mut Context<Self>,
    ) -> Result<(), PartRowEditError> {
        let score = self
            .score
            .edited_rows(PartRowEdit::Clear(rows), self.project.voices().len())?;
        if score == self.score {
            return Ok(());
        }

        self.score = score;
        self.dirty = true;
        self.last_save_error = None;
        self.schedule_autosave(cx);
        cx.emit(DocumentEvent::RowsCleared);
        cx.notify();
        Ok(())
    }

    pub fn apply_saved_structure_change(
        &mut self,
        project: Project,
        part: Part,
        score: PartScore,
        source_editor: u64,
        selected_rows: ScoreRowRange,
        cx: &mut Context<Self>,
    ) {
        self.project = project;
        self.part = part;
        self.score = score;
        self.dirty = false;
        self.last_save_error = None;
        self.save_state = SaveState::Saved;
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::StructureChanged {
            source_editor,
            selected_rows,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AnchoredRowSelection {
    anchor: usize,
    head: usize,
}

impl AnchoredRowSelection {
    fn new(anchor: usize, head: usize, row_count: usize) -> Option<Self> {
        (anchor < row_count && head < row_count).then_some(Self { anchor, head })
    }

    fn rows(self, row_count: usize) -> Option<ScoreRowRange> {
        ScoreRowRange::new(
            self.anchor.min(self.head),
            self.anchor.max(self.head),
            row_count,
        )
    }
}

pub enum RowEditConfirmationMsg {
    Confirmed(RowEditRequested),
    Cancelled,
}

pub struct RowEditConfirmation {
    request: RowEditRequested,
    cancel_button: Entity<Button>,
    confirm_button: Entity<Button>,
}

impl EventEmitter<RowEditConfirmationMsg> for RowEditConfirmation {}

impl RowEditConfirmation {
    pub fn new(request: RowEditRequested, cx: &mut Context<Self>) -> Self {
        let source_editor = request.source_editor;
        let cancel_button =
            cx.new(move |_| Button::new(("cancel-row-edit", source_editor), "keep rows"));
        let confirm_label = match request.edit {
            PartRowEdit::Clear(_) => "clear rows",
            PartRowEdit::Delete(_) => "delete rows",
            PartRowEdit::InsertBefore(_) | PartRowEdit::InsertAfter(_) => "continue",
        };
        let confirm_button =
            cx.new(move |_| Button::new(("confirm-row-edit", source_editor), confirm_label));
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&confirm_button, Self::on_confirm_clicked)
            .detach();

        Self {
            request,
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
        cx.emit(RowEditConfirmationMsg::Cancelled);
    }

    fn on_confirm_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(RowEditConfirmationMsg::Confirmed(self.request.clone()));
    }

    fn message(&self) -> String {
        let cell_label = if self.request.populated_cell_count == 1 {
            "score value"
        } else {
            "score values"
        };
        let (verb, rows, consequence) = match self.request.edit {
            PartRowEdit::Clear(rows) => ("clear", rows, "the part length will stay the same"),
            PartRowEdit::Delete(rows) => (
                "delete",
                rows,
                "later beats will shift earlier and the part will become shorter",
            ),
            PartRowEdit::InsertBefore(_) | PartRowEdit::InsertAfter(_) => {
                return "continue with this row change?".to_string();
            }
        };
        let beat_label = if rows.len() == 1 {
            format!("beat {}", rows.first() + 1)
        } else {
            format!("beats {}–{}", rows.first() + 1, rows.last() + 1)
        };
        format!(
            "{verb} {beat_label}? {} {cell_label} will be removed; {consequence}.",
            self.request.populated_cell_count
        )
    }
}

impl Render for RowEditConfirmation {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let actions = div()
            .flex()
            .justify_end()
            .gap(s::S3)
            .child(
                div()
                    .debug_selector(|| "cancel-row-edit-control".to_string())
                    .child(self.cancel_button.clone()),
            )
            .child(
                div()
                    .debug_selector(|| "confirm-row-edit-control".to_string())
                    .child(self.confirm_button.clone()),
            );
        s::raised(
            div()
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .child(title_bar("confirm row change", None))
                .child(
                    div()
                        .p(s::CONTENT_PADDING)
                        .child(destructive_confirmation(self.message(), actions)),
                ),
        )
    }
}

pub struct ScoreEditor {
    editor_id: u64,
    document: Entity<ScoreDocument>,
    part_names: Vec<crate::part::PartName>,
    part_dropdown: Entity<Dropdown>,
    cells: Vec<Vec<Entity<TextInput>>>,
    row_selection: Option<AnchoredRowSelection>,
    drag_anchor: Option<usize>,
    insert_before_button: Entity<Button>,
    insert_after_button: Entity<Button>,
    clear_rows_button: Entity<Button>,
    delete_rows_button: Entity<Button>,
    playing_row: Option<usize>,
    scroll_handle: ScrollHandle,
}

impl EventEmitter<PartSelected> for ScoreEditor {}
impl EventEmitter<RowEditRequested> for ScoreEditor {}

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
            Dropdown::new_with_trigger_max_width(
                ("score-part", view_index),
                dropdown_options,
                selected_index,
                s::S8,
                cx,
            )
        });
        let cells = Self::build_cells(editor_id, &score, cx);
        let insert_before_button = cx
            .new(move |_| Button::new(("insert-row-before", editor_id), "+ above").disabled(true));
        let insert_after_button =
            cx.new(move |_| Button::new(("insert-row-after", editor_id), "+ below").disabled(true));
        let clear_rows_button =
            cx.new(move |_| Button::new(("clear-score-rows", editor_id), "clear").disabled(true));
        let delete_rows_button =
            cx.new(move |_| Button::new(("delete-score-rows", editor_id), "delete").disabled(true));

        cx.subscribe(&document, Self::on_document_event).detach();
        cx.subscribe(&part_dropdown, Self::on_part_selected)
            .detach();
        cx.subscribe(&insert_before_button, Self::on_insert_before_clicked)
            .detach();
        cx.subscribe(&insert_after_button, Self::on_insert_after_clicked)
            .detach();
        cx.subscribe(&clear_rows_button, Self::on_clear_rows_clicked)
            .detach();
        cx.subscribe(&delete_rows_button, Self::on_delete_rows_clicked)
            .detach();

        Self {
            editor_id,
            document,
            part_names,
            part_dropdown,
            cells,
            row_selection: None,
            drag_anchor: None,
            insert_before_button,
            insert_after_button,
            clear_rows_button,
            delete_rows_button,
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

    fn selected_rows(&self) -> Option<ScoreRowRange> {
        self.row_selection
            .and_then(|selection| selection.rows(self.cells.len()))
    }

    fn set_row_selection(
        &mut self,
        selection: Option<AnchoredRowSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.row_selection == selection {
            return;
        }
        self.row_selection = selection;
        self.sync_row_action_buttons(cx);
        cx.notify();
    }

    fn sync_row_action_buttons(&self, cx: &mut Context<Self>) {
        let selected = self.selected_rows();
        let no_selection = selected.is_none();
        let delete_disabled = selected.is_none_or(|rows| rows.len() == self.cells.len());
        for button in [
            &self.insert_before_button,
            &self.insert_after_button,
            &self.clear_rows_button,
        ] {
            button.update(cx, |button, cx| button.set_disabled(no_selection, cx));
        }
        self.delete_rows_button.update(cx, |button, cx| {
            button.set_disabled(delete_disabled, cx);
        });
    }

    fn on_row_mouse_down(
        &mut self,
        row: usize,
        event: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let toggles_off = !event.modifiers.shift
            && self
                .selected_rows()
                .is_some_and(|rows| rows.len() == 1 && rows.contains(row));
        if toggles_off {
            self.drag_anchor = Some(row);
            self.set_row_selection(None, cx);
            return;
        }
        let anchor = if event.modifiers.shift {
            self.row_selection.map_or(row, |selection| selection.anchor)
        } else {
            row
        };
        self.drag_anchor = Some(anchor);
        self.set_row_selection(AnchoredRowSelection::new(anchor, row, self.cells.len()), cx);
    }

    fn on_row_mouse_move(
        &mut self,
        row: usize,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let Some(anchor) = self.drag_anchor else {
            return;
        };
        self.set_row_selection(AnchoredRowSelection::new(anchor, row, self.cells.len()), cx);
    }

    fn on_row_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag_anchor = None;
    }

    fn on_insert_before_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        let Some(row) = ScoreRowIndex::new(rows.first(), self.cells.len()) else {
            return;
        };
        self.request_row_edit(PartRowEdit::InsertBefore(row), 0, cx);
    }

    fn on_insert_after_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        let Some(row) = ScoreRowIndex::new(rows.last(), self.cells.len()) else {
            return;
        };
        self.request_row_edit(PartRowEdit::InsertAfter(row), 0, cx);
    }

    fn on_clear_rows_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        let populated_cell_count = self.populated_cell_count(rows, cx);
        self.request_row_edit(PartRowEdit::Clear(rows), populated_cell_count, cx);
    }

    fn on_delete_rows_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        if rows.len() == self.cells.len() {
            return;
        }
        let populated_cell_count = self.populated_cell_count(rows, cx);
        self.request_row_edit(PartRowEdit::Delete(rows), populated_cell_count, cx);
    }

    fn populated_cell_count(&self, rows: ScoreRowRange, cx: &Context<Self>) -> usize {
        self.document.read(cx).score().rows()[rows.first()..=rows.last()]
            .iter()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .count()
    }

    fn request_row_edit(
        &self,
        edit: PartRowEdit,
        populated_cell_count: usize,
        cx: &mut Context<Self>,
    ) {
        cx.emit(RowEditRequested {
            source_editor: self.editor_id,
            part_name: self.document.read(cx).part().name.clone(),
            edit,
            populated_cell_count,
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
            DocumentEvent::RowsCleared => {
                let score = self.document.read(cx).score().clone();
                self.cells = Self::build_cells(self.editor_id, &score, cx);
            }
            DocumentEvent::StructureChanged {
                source_editor,
                selected_rows,
            } => {
                let score = self.document.read(cx).score().clone();
                self.cells = Self::build_cells(self.editor_id, &score, cx);
                self.drag_anchor = None;
                self.row_selection = if *source_editor == self.editor_id {
                    AnchoredRowSelection::new(
                        selected_rows.first(),
                        selected_rows.last(),
                        self.cells.len(),
                    )
                } else {
                    None
                };
                self.sync_row_action_buttons(cx);
            }
            DocumentEvent::Reset => {
                let score = self.document.read(cx).score().clone();
                self.cells = Self::build_cells(self.editor_id, &score, cx);
                self.drag_anchor = None;
                self.row_selection = None;
                self.sync_row_action_buttons(cx);
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
        let row_actions = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_end()
            .gap(s::S4)
            .children([
                div()
                    .debug_selector(|| "insert-row-before-control".to_string())
                    .child(self.insert_before_button.clone()),
                div()
                    .debug_selector(|| "insert-row-after-control".to_string())
                    .child(self.insert_after_button.clone()),
                div()
                    .debug_selector(|| "clear-score-rows-control".to_string())
                    .child(self.clear_rows_button.clone()),
                div()
                    .debug_selector(|| "delete-score-rows-control".to_string())
                    .child(self.delete_rows_button.clone()),
            ]);
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .min_w(s::S0)
            .gap(s::S4)
            .child(
                div()
                    .flex_shrink()
                    .min_w(s::S0)
                    .overflow_hidden()
                    .child(self.part_dropdown.clone()),
            )
            .child(row_actions);
        let selected_rows = self.selected_rows();
        let score_content = if !has_voices {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(s::TEXT_DEFAULT)
                .child("add a sin or saw voice to edit and play this part")
        } else {
            data_grid::editable_with_row_selection(
                ("score-grid", self.editor_id),
                column_labels,
                &self.cells,
                &invalid_cells,
                selected_rows,
                self.playing_row,
                &self.scroll_handle,
                |row, header| {
                    header
                        .debug_selector(move || format!("score-row-header-{row}"))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |editor, event, window, cx| {
                                editor.on_row_mouse_down(row, event, window, cx);
                            }),
                        )
                        .on_mouse_move(cx.listener(move |editor, event, window, cx| {
                            editor.on_row_mouse_move(row, event, window, cx);
                        }))
                        .on_mouse_up(MouseButton::Left, cx.listener(Self::on_row_mouse_up))
                        .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_row_mouse_up))
                },
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

    use gpui::{point, px, AppContext, Modifiers, TestAppContext};

    use super::{ScoreDocument, ScoreEditor};
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

    #[gpui::test]
    fn row_actions_stay_visible_and_follow_the_row_selection(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec![String::new()], vec![String::new()]]);
        let part_name = part.name.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| {
            let document = cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part, score));
            ScoreEditor::new(0, document, vec![part_name], cx)
        });
        let (insert_before, insert_after, clear, delete) = cx.update(|_, cx| {
            let editor = editor.read(cx);
            (
                editor.insert_before_button.clone(),
                editor.insert_after_button.clone(),
                editor.clear_rows_button.clone(),
                editor.delete_rows_button.clone(),
            )
        });

        for id in [
            "insert-row-before-control",
            "insert-row-after-control",
            "clear-score-rows-control",
            "delete-score-rows-control",
        ] {
            assert!(cx.debug_bounds(id).is_some(), "missing visible button {id}");
        }
        assert!(cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| clear.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| delete.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| editor.read(cx).selected_rows().is_none()));

        let first = cx.debug_bounds("score-row-header-0").unwrap();
        cx.simulate_click(
            point(
                first.origin.x + first.size.width + px(20.0),
                first.center().y,
            ),
            Modifiers::default(),
        );
        assert!(cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| clear.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| delete.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| editor.read(cx).selected_rows().is_none()));

        cx.simulate_click(first.center(), Modifiers::default());
        assert!(!cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| clear.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| delete.read(cx).is_disabled()));
        assert!(cx.debug_bounds("score-selected-row-header-0").is_some());

        cx.simulate_click(first.center(), Modifiers::default());
        assert!(cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| clear.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| delete.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| editor.read(cx).selected_rows().is_none()));

        cx.simulate_click(first.center(), Modifiers::default());
        let second = cx.debug_bounds("score-row-header-1").unwrap();
        cx.simulate_click(
            second.center(),
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
        assert!(!cx.update(|_, cx| clear.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| delete.read(cx).is_disabled()));
        assert!(cx.debug_bounds("score-selected-row-header-0").is_some());
        assert!(cx.debug_bounds("score-selected-row-header-1").is_some());
    }
}
