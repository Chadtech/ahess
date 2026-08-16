use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use gpui::{
    div, prelude::*, AnyElement, AsyncApp, Context, Entity, EventEmitter, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, Task, WeakEntity, Window,
};

use crate::{
    part::{
        Part, PartRowEdit, PartRowEditError, PartScore, ScoreError, ScoreRowIndex, ScoreRowRange,
        SubdivisionPattern,
    },
    project::Project,
    style as s,
    view::{
        action_menu::{self, ActionMenu},
        button::{self, Button},
        data_grid,
        dialog::{destructive_dialog, error_message, title_bar},
        dropdown::{self, Dropdown},
        field_group::field_group,
        text_input::{Changed, TextInput},
    },
};

static NEXT_EDITOR_ID: AtomicU64 = AtomicU64::new(1);
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(750);
const AUTOSAVE_MAX_DELAY: Duration = Duration::from_secs(5);

pub(super) enum Overlay {
    ExportRows(Entity<ExportRowsDialog>),
    RowEdit(Entity<RowEditConfirmation>),
    Subdivision(Entity<SubdivisionDialog>),
}

impl Overlay {
    pub(super) fn element(&self) -> AnyElement {
        match self {
            Self::ExportRows(dialog) => dialog.clone().into_any_element(),
            Self::RowEdit(dialog) => dialog.clone().into_any_element(),
            Self::Subdivision(dialog) => dialog.clone().into_any_element(),
        }
    }
}

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
    parse_issue_cache: ParseIssueCache,
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

struct ParseIssueCache {
    issues: Vec<ParseIssue>,
    invalid_cells: data_grid::InvalidCells,
}

impl ParseIssueCache {
    fn collect(project: &Project, score: &PartScore) -> Self {
        let issues = score
            .rows()
            .iter()
            .enumerate()
            .flat_map(|(row, values)| {
                values
                    .iter()
                    .enumerate()
                    .filter_map(move |(column, value)| {
                        project
                            .pitch_system()
                            .resolve_cell(value)
                            .err()
                            .map(|source| ParseIssue {
                                row,
                                column,
                                voice: project
                                    .voices()
                                    .get(column)
                                    .map(|voice| voice.name.as_str().to_string())
                                    .unwrap_or_else(|| format!("column {}", column + 1)),
                                message: source.to_string(),
                            })
                    })
            })
            .collect::<Vec<_>>();
        let invalid_cells = issues
            .iter()
            .map(|issue| (issue.row, issue.column))
            .collect();
        Self {
            issues,
            invalid_cells,
        }
    }
}

#[derive(Clone, Debug)]
pub enum DocumentEvent {
    CellChanged {
        source_editor: u64,
        edit: ScoreCellEdit,
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
    HistoryRestored {
        structure_changed: bool,
    },
    ProjectChanged,
    PartSettingsChanged,
}

impl EventEmitter<DocumentEvent> for ScoreDocument {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ScoreCellEdit {
    pub(super) row: usize,
    pub(super) column: usize,
    pub(super) before: String,
    pub(super) after: String,
}

impl ScoreCellEdit {
    pub(super) fn reversed(&self) -> Self {
        Self {
            row: self.row,
            column: self.column,
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

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

impl ScoreDocument {
    pub fn new(project: Project, project_directory: PathBuf, part: Part, score: PartScore) -> Self {
        let parse_issue_cache = ParseIssueCache::collect(&project, &score);
        Self {
            project,
            project_directory,
            part,
            score,
            parse_issue_cache,
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

    pub fn parse_issues(&self) -> &[ParseIssue] {
        &self.parse_issue_cache.issues
    }

    fn invalid_cells(&self) -> &data_grid::InvalidCells {
        &self.parse_issue_cache.invalid_cells
    }

    fn refresh_parse_issues(&mut self) {
        self.parse_issue_cache = ParseIssueCache::collect(&self.project, &self.score);
    }

    fn set_score(&mut self, score: PartScore) {
        self.score = score;
        self.refresh_parse_issues();
    }

    fn set_project(&mut self, project: Project) {
        self.project = project;
        self.refresh_parse_issues();
    }

    fn replace_content(&mut self, project: Project, part: Part, score: PartScore) {
        self.project = project;
        self.part = part;
        self.score = score;
        self.refresh_parse_issues();
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

        let edit = ScoreCellEdit {
            row,
            column,
            before: cell.clone(),
            after: value,
        };
        *cell = edit.after.clone();
        self.set_score(PartScore::from_rows(rows));
        self.dirty = true;
        self.last_save_error = None;
        self.schedule_autosave(cx);
        cx.emit(DocumentEvent::CellChanged {
            source_editor,
            edit,
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

        self.set_score(score);
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
        self.replace_content(project, part, score);
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
        self.replace_content(project, part, score);
        self.dirty = false;
        self.last_save_error = None;
        self.save_state = SaveState::Idle;
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::Reset);
        cx.notify();
    }

    pub fn restore_history_content(
        &mut self,
        project: Project,
        part: Part,
        score: PartScore,
        has_recovery: bool,
        cx: &mut Context<Self>,
    ) {
        let structure_changed = self.score.rows().len() != score.rows().len()
            || self
                .score
                .rows()
                .iter()
                .zip(score.rows())
                .any(|(current, restored)| current.len() != restored.len());
        self.replace_content(project, part, score);
        self.dirty = has_recovery;
        self.last_save_error = None;
        self.save_state = if has_recovery {
            SaveState::RecoverySaved
        } else {
            SaveState::Saved
        };
        self.pending_autosave_since = None;
        self.autosave_task.take();
        cx.emit(DocumentEvent::HistoryRestored { structure_changed });
        cx.notify();
    }

    pub fn project_settings_changed(&mut self, project: Project, cx: &mut Context<Self>) {
        self.set_project(project);
        self.last_save_error = None;
        cx.emit(DocumentEvent::ProjectChanged);
        cx.notify();
    }

    pub fn part_settings_changed(&mut self, project: Project, part: Part, cx: &mut Context<Self>) {
        self.project = project;
        self.part = part;
        self.refresh_parse_issues();
        self.last_save_error = None;
        cx.emit(DocumentEvent::PartSettingsChanged);
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
        let actions = button::action_group([
            div()
                .debug_selector(|| "cancel-row-edit-control".to_string())
                .child(self.cancel_button.clone()),
            div()
                .debug_selector(|| "confirm-row-edit-control".to_string())
                .child(self.confirm_button.clone()),
        ])
        .justify_end();
        destructive_dialog("confirm row change", None, self.message(), actions)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubdivisionDialogMsg {
    Confirmed {
        part_name: crate::part::PartName,
        subdivision_pattern: Option<SubdivisionPattern>,
    },
    Cancelled,
}

pub struct SubdivisionDialog {
    part_name: crate::part::PartName,
    subdivision_pattern: Entity<TextInput>,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    save_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<SubdivisionDialogMsg> for SubdivisionDialog {}

impl SubdivisionDialog {
    pub fn new(part: &Part, cx: &mut Context<Self>) -> Self {
        let value = part
            .subdivision_pattern()
            .map(ToString::to_string)
            .unwrap_or_default();
        let subdivision_pattern = cx.new(|cx| TextInput::new(value, "4 or 4, 3, 3", cx));
        let close_button = cx.new(|_| Button::x("close-score-subdivision"));
        let cancel_button = cx.new(|_| Button::new("cancel-score-subdivision", "cancel"));
        let save_button = cx.new(|_| Button::new("save-score-subdivision", "save pattern"));

        cx.subscribe(&close_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();

        Self {
            part_name: part.name.clone(),
            subdivision_pattern,
            close_button,
            cancel_button,
            save_button,
            error: None,
        }
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(SubdivisionDialogMsg::Cancelled);
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        match parse_optional_subdivision_pattern(&self.subdivision_pattern.read(cx).value()) {
            Ok(subdivision_pattern) => cx.emit(SubdivisionDialogMsg::Confirmed {
                part_name: self.part_name.clone(),
                subdivision_pattern,
            }),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn save_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }
}

impl Render for SubdivisionDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let content =
            div()
                .flex()
                .flex_col()
                .gap(s::CONTENT_PADDING)
                .p(s::CONTENT_PADDING)
                .child(
                    div()
                        .text_color(s::TEXT_DEFAULT)
                        .child(format!("editing {:?}", self.part_name.as_str())),
                )
                .child(field_group(
                    "subdivision pattern (optional)",
                    self.subdivision_pattern.clone(),
                ))
                .child(div().text_color(s::TEXT_DEFAULT).child(
                    "use comma-separated beat groups; leave blank for sequential beat numbers",
                ))
                .children(self.error.clone().map(error_message))
                .child(
                    button::action_group([self.cancel_button.clone(), self.save_button.clone()])
                        .justify_end(),
                );

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .child(title_bar(
                    "edit subdivision pattern",
                    Some(self.close_button.clone()),
                ))
                .child(content),
        )
    }
}

fn parse_optional_subdivision_pattern(value: &str) -> Result<Option<SubdivisionPattern>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    value
        .parse::<SubdivisionPattern>()
        .map(Some)
        .map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportRowsConfirmed {
    pub part_name: crate::part::PartName,
    pub rows: ScoreRowRange,
    pub new_part_name: String,
}

pub enum ExportRowsDialogMsg {
    Confirmed(ExportRowsConfirmed),
    Cancelled,
}

pub struct ExportRowsDialog {
    request: ExportRowsRequested,
    name: Entity<TextInput>,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    export_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<ExportRowsDialogMsg> for ExportRowsDialog {}

impl ExportRowsDialog {
    pub fn new(request: ExportRowsRequested, cx: &mut Context<Self>) -> Self {
        let placeholder = format!("{} excerpt", request.part_name.as_str());
        let name = cx.new(|cx| TextInput::new("", placeholder, cx));
        let close_button = cx.new(|_| Button::x("close-export-score-rows"));
        let cancel_button = cx.new(|_| Button::new("cancel-export-score-rows", "cancel"));
        let export_button = cx.new(|_| Button::new("confirm-export-score-rows", "export as part"));

        cx.subscribe(&close_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&export_button, Self::on_export_clicked)
            .detach();

        Self {
            request,
            name,
            close_button,
            cancel_button,
            export_button,
            error: None,
        }
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ExportRowsDialogMsg::Cancelled);
    }

    fn on_export_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ExportRowsDialogMsg::Confirmed(ExportRowsConfirmed {
            part_name: self.request.part_name.clone(),
            rows: self.request.rows,
            new_part_name: self.name.read(cx).value(),
        }));
    }

    pub fn export_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    fn selection_description(&self) -> String {
        if self.request.rows.len() == 1 {
            format!(
                "export beat {} from {:?} into a new part",
                self.request.rows.first() + 1,
                self.request.part_name.as_str()
            )
        } else {
            format!(
                "export beats {}–{} from {:?} into a new part",
                self.request.rows.first() + 1,
                self.request.rows.last() + 1,
                self.request.part_name.as_str()
            )
        }
    }
}

impl Render for ExportRowsDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let actions =
            button::action_group([self.cancel_button.clone(), self.export_button.clone()])
                .justify_end();
        let content = div()
            .flex()
            .flex_col()
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .child(
                div()
                    .text_color(s::TEXT_DEFAULT)
                    .child(self.selection_description()),
            )
            .child(field_group("new part name", self.name.clone()))
            .children(self.error.clone().map(error_message))
            .child(actions);

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .child(title_bar(
                    "export selected rows",
                    Some(self.close_button.clone()),
                ))
                .child(content),
        )
    }
}

#[derive(Clone, Copy)]
pub(super) enum ScoreAction {
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

    pub(super) fn index(self) -> usize {
        self as usize
    }

    fn label(self) -> &'static str {
        match self {
            Self::EditPart => "edit part",
            Self::EditSubdivision => "edit subdivision pattern",
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
                        let background = if part.beat_is_highlighted(row_index) {
                            s::GREEN4
                        } else {
                            s::GREEN3
                        };
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
                        let background = if part.beat_is_highlighted(row_index) {
                            s::GREEN4
                        } else {
                            s::GREEN3
                        };
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
                    let background = if part.beat_is_highlighted(row_index) {
                        s::GREEN4
                    } else {
                        s::GREEN3
                    };
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

    #[cfg(test)]
    pub(super) fn actions(&self) -> Entity<ActionMenu> {
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
    use std::{collections::BTreeMap, path::PathBuf};

    use gpui::{
        div, point, prelude::*, px, size, AppContext, Context, Entity, Modifiers, ScrollDelta,
        ScrollWheelEvent, TestAppContext, Window,
    };

    use super::{
        parse_optional_subdivision_pattern, DocumentEvent, ScoreAction, ScoreCellEdit,
        ScoreDocument, ScoreEditor,
    };
    use crate::{
        part::{Part, PartScore, ScoreRowRange},
        pitch_system::{ExplicitPitchSystem, FrequencyHz, PitchSystem},
        project::{Project, Voice, VoiceType},
        seed::Seed,
        view::button,
    };

    #[test]
    fn subdivision_dialog_patterns_are_optional_positive_whole_number_lists() {
        assert!(parse_optional_subdivision_pattern("  ").unwrap().is_none());
        assert_eq!(
            parse_optional_subdivision_pattern(" 4, 3,3 ")
                .unwrap()
                .unwrap()
                .subdivisions()
                .collect::<Vec<_>>(),
            [4, 3, 3]
        );
        assert!(parse_optional_subdivision_pattern("4,,3").is_err());
        assert!(parse_optional_subdivision_pattern("4, 0").is_err());
        assert!(parse_optional_subdivision_pattern("4, 1.5").is_err());
    }

    #[gpui::test]
    fn subdivision_dialog_starts_with_the_current_pattern_and_keeps_invalid_input_open(
        cx: &mut TestAppContext,
    ) {
        let part =
            Part::new("intro", 10).with_subdivision_pattern(Some("4, 3, 3".parse().unwrap()));
        let (dialog, cx) = cx.add_window_view(|_, cx| super::SubdivisionDialog::new(&part, cx));
        let (input, save_button) = cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            assert_eq!(dialog.subdivision_pattern.read(cx).value(), "4, 3, 3");
            (
                dialog.subdivision_pattern.clone(),
                dialog.save_button.clone(),
            )
        });
        input.update(cx, |input, cx| input.sync_value("4,,3", cx));
        dialog.update(cx, |dialog, cx| {
            dialog.on_save_clicked(save_button, &button::Clicked, cx);
        });

        assert!(cx.update(|_, cx| dialog.read(cx).error.is_some()));
    }

    struct ScoreEditorHost {
        editor: Entity<ScoreEditor>,
    }

    impl Render for ScoreEditorHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().flex().size_full().child(self.editor.clone())
        }
    }

    struct DocumentEventHost {
        document: Entity<ScoreDocument>,
        last_cell_edit: Option<ScoreCellEdit>,
    }

    impl DocumentEventHost {
        fn new(document: Entity<ScoreDocument>, cx: &mut Context<Self>) -> Self {
            cx.subscribe(&document, Self::on_document_event).detach();
            Self {
                document,
                last_cell_edit: None,
            }
        }

        fn on_document_event(
            &mut self,
            _: Entity<ScoreDocument>,
            event: &DocumentEvent,
            _: &mut Context<Self>,
        ) {
            if let DocumentEvent::CellChanged { edit, .. } = event {
                self.last_cell_edit = Some(edit.clone());
            }
        }
    }

    impl Render for DocumentEventHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    #[gpui::test]
    fn cell_changes_emit_the_exact_domain_edit(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 1);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec!["C4".to_string()]]);
        let (host, cx) = cx.add_window_view(|_, cx| {
            let document = cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part, score));
            DocumentEventHost::new(document, cx)
        });
        let document = cx.update(|_, cx| host.read(cx).document.clone());

        document.update(cx, |document, cx| {
            document.update_cell(7, 0, 0, "D4".to_string(), cx);
        });

        assert_eq!(
            cx.update(|_, cx| host.read(cx).last_cell_edit.clone()),
            Some(ScoreCellEdit {
                row: 0,
                column: 0,
                before: "C4".to_string(),
                after: "D4".to_string(),
            })
        );
    }

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
    fn score_cells_alternate_backgrounds_by_subdivision_group(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 6).with_subdivision_pattern(Some("2".parse().unwrap()));
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
    fn cached_parse_issues_follow_document_mutations(cx: &mut TestAppContext) {
        let part = Part::new("part-a", 2);
        let project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let score = PartScore::from_rows(vec![vec!["H4".to_string()], vec![String::new()]]);
        let part_for_document = part.clone();
        let document =
            cx.new(|_| ScoreDocument::new(project, PathBuf::new(), part_for_document, score));

        assert_eq!(cx.update(|cx| document.read(cx).parse_issues().len()), 1);
        assert!(cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "C4".to_string(), cx);
        });
        assert!(cx.update(|cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "H4".to_string(), cx);
        });
        assert_eq!(cx.update(|cx| document.read(cx).parse_issues().len()), 1);
        assert!(cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document
                .clear_rows(ScoreRowRange::new(0, 0, 2).unwrap(), cx)
                .unwrap();
        });
        assert!(cx.update(|cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        document.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "ember".to_string(), cx);
        });
        assert_eq!(cx.update(|cx| document.read(cx).parse_issues().len()), 1);
        assert!(cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));

        let pitch_system = PitchSystem::explicit(
            ExplicitPitchSystem::new(
                "embers",
                BTreeMap::from([("ember".to_string(), FrequencyHz::new(197.3).unwrap())]),
            )
            .unwrap(),
        );
        let explicit_project = Project::new("test project", 20_000, 32, Seed::new(12))
            .with_pitch_system(pitch_system)
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part]);
        document.update(cx, |document, cx| {
            document.project_settings_changed(explicit_project, cx);
        });
        assert!(cx.update(|cx| document.read(cx).parse_issues().is_empty()));
        assert!(!cx.update(|cx| document.read(cx).invalid_cells().contains(0, 0)));
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
