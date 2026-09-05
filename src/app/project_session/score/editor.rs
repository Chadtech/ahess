//! Score grid presentation, selection, and interaction with a shared document.

use super::document::{DocumentEvent, ScoreDocument};
use crate::{
    part::{Part, PartRowEdit, PartScore, ScoreRowIndex, ScoreRowRange},
    style as s,
    view::{
        action_menu::{self, ActionMenu},
        button::{self, Button},
        data_grid,
        dropdown::{self, Dropdown},
        text_input::{Changed, TextInput},
    },
};
use gpui::SharedString;
use gpui::{
    div, prelude::*, Context, Entity, EventEmitter, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Window,
};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_EDITOR_ID: AtomicU64 = AtomicU64::new(1);

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartLoopRequested {
    pub part_name: crate::part::PartName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditPartRequested {
    pub part_name: crate::part::PartName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EditSubdivisionRequested {
    pub part_name: crate::part::PartName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportRowsRequested {
    pub part_name: crate::part::PartName,
    pub rows: ScoreRowRange,
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

fn score_cell_background(part: &Part, row: usize) -> gpui::Rgba {
    if part.beat_starts_major_subdivision(row) {
        s::GREEN5
    } else if part.beat_is_highlighted(row) {
        s::GREEN4
    } else {
        s::GREEN3
    }
}

#[derive(Clone, Copy)]
pub(in crate::app::project_session) enum ScoreAction {
    EditPart,
    EditSubdivision,
    LoopPart,
    ExportRows,
    ClearRows,
    DeleteRows,
}

impl ScoreAction {
    const ALL: [Self; 6] = [
        Self::EditPart,
        Self::EditSubdivision,
        Self::LoopPart,
        Self::ExportRows,
        Self::ClearRows,
        Self::DeleteRows,
    ];

    pub(in crate::app::project_session) fn index(self) -> usize {
        self as usize
    }

    fn label(self) -> &'static str {
        match self {
            Self::EditPart => "edit part",
            Self::EditSubdivision => "edit subdivisions",
            Self::LoopPart => "loop part",
            Self::ExportRows => "export selected rows as part",
            Self::ClearRows => "clear selected rows",
            Self::DeleteRows => "delete selected rows",
        }
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
    action_menu: Entity<ActionMenu>,
    playing_row: Option<usize>,
    scroll_handle: data_grid::DataGridScrollHandle,
}

impl EventEmitter<PartSelected> for ScoreEditor {}
impl EventEmitter<RowEditRequested> for ScoreEditor {}
impl EventEmitter<PartLoopRequested> for ScoreEditor {}
impl EventEmitter<EditPartRequested> for ScoreEditor {}
impl EventEmitter<EditSubdivisionRequested> for ScoreEditor {}
impl EventEmitter<ExportRowsRequested> for ScoreEditor {}

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
        let part = document_state.part().clone();
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
        let cells = Self::build_cells(editor_id, &score, &part, cx);
        let insert_before_button = cx
            .new(move |_| Button::new(("insert-row-before", editor_id), "+ above").disabled(true));
        let insert_after_button =
            cx.new(move |_| Button::new(("insert-row-after", editor_id), "+ below").disabled(true));
        let action_labels = ScoreAction::ALL.map(ScoreAction::label);
        let action_menu = cx.new(move |cx| {
            let mut menu =
                ActionMenu::new(("score-actions", editor_id), "actions", action_labels, cx);
            for action in [
                ScoreAction::ExportRows,
                ScoreAction::ClearRows,
                ScoreAction::DeleteRows,
            ] {
                menu.set_disabled(action.index(), true, cx);
            }
            menu
        });

        cx.subscribe(&document, Self::on_document_event).detach();
        cx.subscribe(&part_dropdown, Self::on_part_selected)
            .detach();
        cx.subscribe(&insert_before_button, Self::on_insert_before_clicked)
            .detach();
        cx.subscribe(&insert_after_button, Self::on_insert_after_clicked)
            .detach();
        cx.subscribe(&action_menu, Self::on_action_selected)
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
            action_menu,
            playing_row: None,
            scroll_handle: data_grid::DataGridScrollHandle::compact(),
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
        part: &Part,
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
                        let background = score_cell_background(part, row_index);
                        let input = cx.new(|cx| {
                            TextInput::new(value.clone(), "", cx).with_background(background)
                        });
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
        self.sync_actions(cx);
        cx.notify();
    }

    fn sync_actions(&self, cx: &mut Context<Self>) {
        let selected = self.selected_rows();
        let no_selection = selected.is_none();
        let delete_disabled = selected.is_none_or(|rows| rows.len() == self.cells.len());
        for button in [&self.insert_before_button, &self.insert_after_button] {
            button.update(cx, |button, cx| button.set_disabled(no_selection, cx));
        }
        self.action_menu.update(cx, |menu, cx| {
            for action in [ScoreAction::ExportRows, ScoreAction::ClearRows] {
                menu.set_disabled(action.index(), no_selection, cx);
            }
            menu.set_disabled(ScoreAction::DeleteRows.index(), delete_disabled, cx);
        });
    }

    fn on_action_selected(
        &mut self,
        _: Entity<ActionMenu>,
        selected: &action_menu::Selected,
        cx: &mut Context<Self>,
    ) {
        match ScoreAction::ALL.get(selected.index).copied() {
            Some(ScoreAction::EditPart) => cx.emit(EditPartRequested {
                part_name: self.document.read(cx).part().name.clone(),
            }),
            Some(ScoreAction::EditSubdivision) => cx.emit(EditSubdivisionRequested {
                part_name: self.document.read(cx).part().name.clone(),
            }),
            Some(ScoreAction::LoopPart) => cx.emit(PartLoopRequested {
                part_name: self.document.read(cx).part().name.clone(),
            }),
            Some(ScoreAction::ExportRows) => {
                let Some(rows) = self.selected_rows() else {
                    return;
                };
                cx.emit(ExportRowsRequested {
                    part_name: self.document.read(cx).part().name.clone(),
                    rows,
                });
            }
            Some(ScoreAction::ClearRows) => self.clear_rows(cx),
            Some(ScoreAction::DeleteRows) => self.delete_rows(cx),
            None => {}
        }
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
        self.insert_before(cx);
    }

    fn on_insert_after_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.insert_after(cx);
    }

    fn insert_before(&mut self, cx: &mut Context<Self>) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        let Some(row) = ScoreRowIndex::new(rows.first(), self.cells.len()) else {
            return;
        };
        self.request_row_edit(PartRowEdit::InsertBefore(row), 0, cx);
    }

    fn insert_after(&mut self, cx: &mut Context<Self>) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        let Some(row) = ScoreRowIndex::new(rows.last(), self.cells.len()) else {
            return;
        };
        self.request_row_edit(PartRowEdit::InsertAfter(row), 0, cx);
    }

    fn clear_rows(&mut self, cx: &mut Context<Self>) {
        let Some(rows) = self.selected_rows() else {
            return;
        };
        let populated_cell_count = self.populated_cell_count(rows, cx);
        self.request_row_edit(PartRowEdit::Clear(rows), populated_cell_count, cx);
    }

    fn delete_rows(&mut self, cx: &mut Context<Self>) {
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
                edit,
            } => {
                if *source_editor != self.editor_id {
                    if let Some(cell) = self
                        .cells
                        .get(edit.row)
                        .and_then(|row| row.get(edit.column))
                    {
                        let value: SharedString = edit.after.clone().into();
                        cell.update(cx, |cell, cx| cell.sync_value(value, cx));
                    }
                }
            }
            DocumentEvent::RowsCleared => {
                let document = self.document.read(cx);
                let score = document.score().clone();
                let part = document.part().clone();
                self.cells = Self::build_cells(self.editor_id, &score, &part, cx);
            }
            DocumentEvent::StructureChanged {
                source_editor,
                selected_rows,
            } => {
                let document = self.document.read(cx);
                let score = document.score().clone();
                let part = document.part().clone();
                self.cells = Self::build_cells(self.editor_id, &score, &part, cx);
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
                self.sync_actions(cx);
            }
            DocumentEvent::Reset => {
                let document = self.document.read(cx);
                let score = document.score().clone();
                let part = document.part().clone();
                self.cells = Self::build_cells(self.editor_id, &score, &part, cx);
                self.drag_anchor = None;
                self.row_selection = None;
                self.sync_actions(cx);
            }
            DocumentEvent::HistoryRestored { structure_changed } => {
                let (score, part) = {
                    let document = self.document.read(cx);
                    (document.score().clone(), document.part().clone())
                };
                if *structure_changed {
                    self.cells = Self::build_cells(self.editor_id, &score, &part, cx);
                    self.drag_anchor = None;
                    self.row_selection = self.row_selection.and_then(|selection| {
                        AnchoredRowSelection::new(
                            selection.anchor.min(self.cells.len().saturating_sub(1)),
                            selection.head.min(self.cells.len().saturating_sub(1)),
                            self.cells.len(),
                        )
                    });
                    self.sync_actions(cx);
                } else {
                    for (row_index, row) in self.cells.iter().enumerate() {
                        let background = score_cell_background(&part, row_index);
                        for (column_index, cell) in row.iter().enumerate() {
                            let value: SharedString =
                                score.rows()[row_index][column_index].clone().into();
                            cell.update(cx, |cell, cx| {
                                cell.sync_value(value, cx);
                                cell.set_background(background, cx);
                            });
                        }
                    }
                }
            }
            DocumentEvent::PartSettingsChanged => {
                let part = self.document.read(cx).part().clone();
                for (row_index, row) in self.cells.iter().enumerate() {
                    let background = score_cell_background(&part, row_index);
                    for cell in row {
                        cell.update(cx, |cell, cx| cell.set_background(background, cx));
                    }
                }
            }
            DocumentEvent::Saved
            | DocumentEvent::RecoverySaved
            | DocumentEvent::SaveFailed
            | DocumentEvent::ProjectChanged => {}
        }
        cx.notify();
    }

    pub(in crate::app::project_session) fn reveal_issue(
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

    pub(in crate::app::project_session) fn set_playing_row(
        &mut self,
        row: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.playing_row != row {
            self.playing_row = row;
            cx.notify();
        }
    }

    #[cfg(test)]
    pub(in crate::app::project_session) fn playing_row(&self) -> Option<usize> {
        self.playing_row
    }

    #[cfg(test)]
    pub(in crate::app::project_session) fn actions(&self) -> Entity<ActionMenu> {
        self.action_menu.clone()
    }
}

impl Render for ScoreEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let document = self.document.read(cx);
        let column_labels = document
            .project()
            .voices()
            .iter()
            .map(|voice| voice.name.as_str().to_string())
            .collect::<Vec<_>>();
        let has_voices = !column_labels.is_empty();
        let invalid_cells = document.invalid_cells().clone();
        let row_labels = (0..self.cells.len())
            .map(|row| document.part().beat_label(row))
            .collect::<Vec<_>>();
        let score_actions = button::action_group([
            div()
                .debug_selector(|| "insert-row-before-control".to_string())
                .child(self.insert_before_button.clone()),
            div()
                .debug_selector(|| "insert-row-after-control".to_string())
                .child(self.insert_after_button.clone()),
            div()
                .debug_selector(|| "score-actions-control".to_string())
                .child(self.action_menu.clone()),
        ])
        .flex_none()
        .items_center()
        .justify_end();
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
            .child(score_actions);
        let selected_rows = self.selected_rows();
        let editor = cx.entity();
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
                row_labels,
                selected_rows,
                self.playing_row,
                &self.scroll_handle,
                move |row, header| {
                    let mouse_down_editor = editor.clone();
                    let mouse_move_editor = editor.clone();
                    let mouse_up_editor = editor.clone();
                    let mouse_up_out_editor = editor.clone();
                    header
                        .debug_selector(move || format!("score-row-header-{row}"))
                        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                            mouse_down_editor.update(cx, |editor, cx| {
                                editor.on_row_mouse_down(row, event, window, cx);
                            });
                        })
                        .on_mouse_move(move |event, window, cx| {
                            mouse_move_editor.update(cx, |editor, cx| {
                                editor.on_row_mouse_move(row, event, window, cx);
                            });
                        })
                        .on_mouse_up(MouseButton::Left, move |event, window, cx| {
                            mouse_up_editor.update(cx, |editor, cx| {
                                editor.on_row_mouse_up(event, window, cx);
                            });
                        })
                        .on_mouse_up_out(MouseButton::Left, move |event, window, cx| {
                            mouse_up_out_editor.update(cx, |editor, cx| {
                                editor.on_row_mouse_up(event, window, cx);
                            });
                        })
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
    use super::*;
    use crate::{
        part::{Part, PartScore},
        project::{Project, Voice, VoiceType},
        seed::Seed,
    };
    use gpui::{
        div, point, px, size, AppContext, Context, Entity, Modifiers, ScrollDelta,
        ScrollWheelEvent, TestAppContext, Window,
    };
    use std::path::PathBuf;
    struct ScoreEditorHost {
        editor: Entity<ScoreEditor>,
    }

    impl Render for ScoreEditorHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().size_full().child(self.editor.clone())
        }
    }

    #[gpui::test]
    fn score_cells_alternate_backgrounds_by_subdivision_group(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 6)
            .with_subdivision_pattern(Some("2".parse().unwrap()))
            .with_major_subdivision(Some("4".parse().unwrap()));
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec![String::new()]; 6]);
        let part_name = part.name.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| {
            let document = cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part, score));
            ScoreEditor::new(0, document, vec![part_name], cx)
        });

        let backgrounds = cx.update(|_, cx| {
            editor
                .read(cx)
                .cells
                .iter()
                .map(|row| row[0].read(cx).background())
                .collect::<Vec<_>>()
        });

        assert_eq!(
            backgrounds,
            [
                crate::style::GREEN5,
                crate::style::GREEN3,
                crate::style::GREEN4,
                crate::style::GREEN4,
                crate::style::GREEN5,
                crate::style::GREEN3,
            ]
        );
    }

    #[gpui::test]
    fn changing_part_settings_updates_cell_groups_without_clearing_dirty_score(
        cx: &mut TestAppContext,
    ) {
        let part = Part::new("part-a", 6);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec![String::new()]; 6]);
        let part_name = part.name.clone();
        let updated_part = part
            .clone()
            .with_subdivision_pattern(Some("2".parse().unwrap()));
        let updated_project = project.clone().with_parts(vec![updated_part.clone()]);
        let (editor, cx) = cx.add_window_view(move |_, cx| {
            let document = cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part, score));
            ScoreEditor::new(0, document, vec![part_name], cx)
        });
        let document = cx.update(|_, cx| editor.read(cx).document.clone());
        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
            document.part_settings_changed(updated_project, updated_part, cx);
        });

        let backgrounds = cx.update(|_, cx| {
            editor
                .read(cx)
                .cells
                .iter()
                .map(|row| row[0].read(cx).background())
                .collect::<Vec<_>>()
        });

        assert!(cx.update(|_, cx| document.read(cx).is_dirty()));
        assert_eq!(
            backgrounds,
            [
                crate::style::GREEN3,
                crate::style::GREEN3,
                crate::style::GREEN4,
                crate::style::GREEN4,
                crate::style::GREEN3,
                crate::style::GREEN3,
            ]
        );
    }

    #[gpui::test]
    fn score_grid_only_renders_visible_rows(cx: &mut TestAppContext) {
        let row_count = 200;
        let part = Part::new("part-a", row_count);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec![String::new()]; row_count as usize]);
        let part_name = part.name.clone();
        let (_, cx) = cx.add_window_view(move |_, cx| {
            let document = cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part, score));
            let editor = cx.new(|cx| ScoreEditor::new(0, document, vec![part_name], cx));
            ScoreEditorHost { editor }
        });
        cx.simulate_resize(size(px(600.0), px(300.0)));
        cx.run_until_parked();

        let first = cx.debug_bounds("score-row-header-0").unwrap();
        assert!(cx.debug_bounds("score-row-header-199").is_none());

        cx.simulate_event(ScrollWheelEvent {
            position: first.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-10_000.0))),
            ..Default::default()
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("score-row-header-199").is_some());
    }

    #[gpui::test]
    fn score_actions_stay_compact_and_follow_the_row_selection(cx: &mut TestAppContext) {
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
        let (insert_before, insert_after, action_menu) = cx.update(|_, cx| {
            let editor = editor.read(cx);
            (
                editor.insert_before_button.clone(),
                editor.insert_after_button.clone(),
                editor.action_menu.clone(),
            )
        });

        assert!(cx.debug_bounds("insert-row-before-control").is_some());
        assert!(cx.debug_bounds("insert-row-after-control").is_some());
        assert!(cx.debug_bounds("score-actions-control").is_some());
        assert!(cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| {
            let menu = action_menu.read(cx);
            menu.is_disabled(ScoreAction::ExportRows.index())
                && menu.is_disabled(ScoreAction::ClearRows.index())
                && menu.is_disabled(ScoreAction::DeleteRows.index())
                && !menu.is_disabled(ScoreAction::EditPart.index())
                && !menu.is_disabled(ScoreAction::EditSubdivision.index())
                && !menu.is_disabled(ScoreAction::LoopPart.index())
        }));
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
        assert!(cx.update(|_, cx| {
            let menu = action_menu.read(cx);
            menu.is_disabled(ScoreAction::ExportRows.index())
                && menu.is_disabled(ScoreAction::ClearRows.index())
                && menu.is_disabled(ScoreAction::DeleteRows.index())
        }));
        assert!(cx.update(|_, cx| editor.read(cx).selected_rows().is_none()));

        cx.simulate_click(first.center(), Modifiers::default());
        assert!(!cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| {
            let menu = action_menu.read(cx);
            !menu.is_disabled(ScoreAction::ExportRows.index())
                && !menu.is_disabled(ScoreAction::ClearRows.index())
                && !menu.is_disabled(ScoreAction::DeleteRows.index())
        }));
        assert!(cx.debug_bounds("score-selected-row-header-0").is_some());

        cx.simulate_click(first.center(), Modifiers::default());
        assert!(cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| {
            let menu = action_menu.read(cx);
            menu.is_disabled(ScoreAction::ExportRows.index())
                && menu.is_disabled(ScoreAction::ClearRows.index())
                && menu.is_disabled(ScoreAction::DeleteRows.index())
        }));
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
        assert!(!cx.update(|_, cx| insert_before.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| insert_after.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| {
            let menu = action_menu.read(cx);
            !menu.is_disabled(ScoreAction::ExportRows.index())
                && !menu.is_disabled(ScoreAction::ClearRows.index())
                && menu.is_disabled(ScoreAction::DeleteRows.index())
        }));
        assert!(cx.debug_bounds("score-selected-row-header-0").is_some());
        assert!(cx.debug_bounds("score-selected-row-header-1").is_some());
    }
}
