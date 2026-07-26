use std::{collections::BTreeMap, path::PathBuf};

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, MouseButton, MouseDownEvent,
    ScrollHandle, Window,
};

use crate::{
    pitch_system::{
        ExplicitPitchSystem, FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem,
        PitchSystem,
    },
    style as s,
    tuning_system::{self, TuningSystem, TuningSystemId, TuningSystemSource},
    view::{
        button::{self, Button},
        data_grid,
        dialog::destructive_confirmation,
        dropdown::{Dropdown, Selected},
        field_group::{compact_control_group, field_group},
        ordered_input_list, selection_list,
        status_bar::{self, Status},
        text_input::{Changed, TextInput},
        workspace_tile,
    },
};

pub enum Msg {
    CloseRequested,
}

pub struct Model {
    workspace_root: PathBuf,
    systems: Vec<TuningSystem>,
    selected_id: Option<TuningSystemId>,
    view: EditorView,
    close_button: Entity<Button>,
    new_button: Entity<Button>,
    duplicate_button: Entity<Button>,
    cancel_button: Entity<Button>,
    save_button: Entity<Button>,
    delete_button: Entity<Button>,
    cancel_delete_button: Entity<Button>,
    confirm_delete_button: Entity<Button>,
    add_row_button: Entity<Button>,
    remove_row_button: Entity<Button>,
    status: Status,
}

enum EditorView {
    BuiltIn,
    Form {
        original_id: Option<TuningSystemId>,
        draft: Box<TuningDraft>,
        deletion: DeletionState,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeletionState {
    Idle,
    Confirming,
}

struct TuningDraft {
    name: Entity<TextInput>,
    kind_dropdown: Entity<Dropdown>,
    kind: DraftKind,
}

enum DraftKind {
    Periodic(PeriodicDraft),
    Explicit(ExplicitDraft),
}

struct PeriodicDraft {
    fundamental: Entity<TextInput>,
    period: Entity<TextInput>,
    notation_dropdown: Entity<Dropdown>,
    notation: DraftNotation,
    degrees: Vec<Entity<TextInput>>,
    scroll_handle: ScrollHandle,
}

enum DraftNotation {
    RadlerDigits { place_value: Entity<TextInput> },
    WesternTwelveTone,
}

struct ExplicitDraft {
    pitches: Vec<Vec<Entity<TextInput>>>,
    scroll_handle: data_grid::DataGridScrollHandle,
}

impl EventEmitter<Msg> for Model {}

impl Model {
    pub fn new(workspace_root: PathBuf, cx: &mut Context<Self>) -> Self {
        let (systems, status) = match tuning_system::list_tuning_systems(&workspace_root) {
            Ok(systems) => (systems, Status::Empty),
            Err(error) => (
                vec![TuningSystem::built_in_western()],
                Status::Error {
                    message: format!("failed to load tuning systems: {error}").into(),
                    target: None,
                },
            ),
        };
        let selected_id = systems.first().map(|system| system.id().clone());
        let close_button = cx.new(|_| Button::new("close-tuning-editor", "back to projects"));
        let new_button = cx.new(|_| Button::new("new-tuning-system", "new tuning system"));
        let duplicate_button = cx.new(|_| Button::new("duplicate-tuning-system", "duplicate"));
        let cancel_button = cx.new(|_| Button::new("cancel-tuning-edit", "cancel"));
        let save_button = cx.new(|_| Button::new("save-tuning-system", "save changes"));
        let delete_button = cx.new(|_| Button::new("delete-tuning-system", "delete"));
        let cancel_delete_button = cx.new(|_| Button::new("cancel-delete-tuning", "keep tuning"));
        let confirm_delete_button =
            cx.new(|_| Button::new("confirm-delete-tuning", "delete tuning"));
        let add_row_button = cx.new(|_| Button::new("add-tuning-row", "add row"));
        let remove_row_button = cx.new(|_| Button::new("remove-tuning-row", "remove last row"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&new_button, Self::on_new_clicked).detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();
        cx.subscribe(&cancel_delete_button, Self::on_cancel_delete_clicked)
            .detach();
        cx.subscribe(&confirm_delete_button, Self::on_confirm_delete_clicked)
            .detach();
        cx.subscribe(&add_row_button, Self::on_add_row_clicked)
            .detach();
        cx.subscribe(&remove_row_button, Self::on_remove_row_clicked)
            .detach();

        Self {
            workspace_root,
            systems,
            selected_id,
            view: EditorView::BuiltIn,
            close_button,
            new_button,
            duplicate_button,
            cancel_button,
            save_button,
            delete_button,
            cancel_delete_button,
            confirm_delete_button,
            add_row_button,
            remove_row_button,
            status,
        }
    }

    pub fn bar_actions(&self) -> Vec<AnyElement> {
        vec![self.close_button.clone().into_any_element()]
    }

    fn selected_system(&self) -> Option<&TuningSystem> {
        let id = self.selected_id.as_ref()?;
        self.systems.iter().find(|system| system.id() == id)
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        cx.emit(Msg::CloseRequested);
    }

    fn on_new_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.selected_id = None;
        self.view = EditorView::Form {
            original_id: None,
            draft: Box::new(TuningDraft::new_periodic("", cx)),
            deletion: DeletionState::Idle,
        };
        self.status = Status::Message("creating a new periodic tuning system".into());
        self.sync_row_buttons(cx);
        cx.notify();
    }

    fn on_duplicate_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(system) = self.selected_system().cloned() else {
            return;
        };
        self.selected_id = None;
        self.view = EditorView::Form {
            original_id: None,
            draft: Box::new(TuningDraft::from_system(
                format!("{} copy", system.name()),
                system.pitch_system(),
                cx,
            )),
            deletion: DeletionState::Idle,
        };
        self.status = Status::Message("edit the copy, then save it as a new tuning".into());
        self.sync_row_buttons(cx);
        cx.notify();
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let selected = match &self.view {
            EditorView::Form {
                original_id: Some(id),
                ..
            } => Some(id.clone()),
            _ => self.systems.first().map(|system| system.id().clone()),
        };
        if let Some(id) = selected {
            self.select_system(id, cx);
        }
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let EditorView::Form {
            original_id, draft, ..
        } = &self.view
        else {
            return;
        };
        let pitch_system = match draft.pitch_system(cx) {
            Ok(system) => system,
            Err(message) => {
                self.status = Status::Error {
                    message: message.into(),
                    target: None,
                };
                cx.notify();
                return;
            }
        };
        let result = match original_id {
            Some(id) => tuning_system::update_tuning_system(&self.workspace_root, id, pitch_system),
            None => tuning_system::create_tuning_system(&self.workspace_root, pitch_system),
        };
        match result {
            Ok(saved) => {
                let saved_id = saved.id().clone();
                match tuning_system::list_tuning_systems(&self.workspace_root) {
                    Ok(systems) => {
                        self.systems = systems;
                        self.select_system(saved_id, cx);
                        self.status = Status::Message("tuning system saved".into());
                    }
                    Err(error) => {
                        self.status = Status::Error {
                            message: format!(
                                "saved, but failed to refresh tuning systems: {error}"
                            )
                            .into(),
                            target: None,
                        }
                    }
                }
            }
            Err(error) => {
                self.status = Status::Error {
                    message: error.to_string().into(),
                    target: None,
                }
            }
        }
        cx.notify();
    }

    fn on_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let EditorView::Form { deletion, .. } = &mut self.view {
            *deletion = DeletionState::Confirming;
            self.status = Status::Warning("confirm deletion or keep the tuning system".into());
            cx.notify();
        }
    }

    fn on_cancel_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let EditorView::Form { deletion, .. } = &mut self.view {
            *deletion = DeletionState::Idle;
            self.status = Status::Empty;
            cx.notify();
        }
    }

    fn on_confirm_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let EditorView::Form {
            original_id: Some(id),
            ..
        } = &self.view
        else {
            return;
        };
        match tuning_system::delete_tuning_system(&self.workspace_root, id) {
            Ok(()) => match tuning_system::list_tuning_systems(&self.workspace_root) {
                Ok(systems) => {
                    self.systems = systems;
                    let id = self.systems[0].id().clone();
                    self.select_system(id, cx);
                    self.status = Status::Message("tuning system deleted".into());
                }
                Err(error) => {
                    self.status = Status::Error {
                        message: format!("deleted, but failed to refresh tuning systems: {error}")
                            .into(),
                        target: None,
                    }
                }
            },
            Err(error) => {
                if let EditorView::Form { deletion, .. } = &mut self.view {
                    *deletion = DeletionState::Idle;
                }
                self.status = Status::Error {
                    message: error.to_string().into(),
                    target: None,
                };
            }
        }
        cx.notify();
    }

    fn on_add_row_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let EditorView::Form { draft, .. } = &mut self.view {
            draft.add_row(cx);
            self.status = Status::Empty;
            self.sync_row_buttons(cx);
            cx.notify();
        }
    }

    fn on_remove_row_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let EditorView::Form { draft, .. } = &mut self.view {
            draft.remove_last_row();
            self.status = Status::Empty;
            self.sync_row_buttons(cx);
            cx.notify();
        }
    }

    fn on_kind_selected(
        &mut self,
        _: Entity<Dropdown>,
        selected: &Selected,
        cx: &mut Context<Self>,
    ) {
        let EditorView::Form { draft, .. } = &mut self.view else {
            return;
        };
        let requested_periodic = selected.index == 0;
        if requested_periodic == matches!(draft.kind, DraftKind::Periodic(_)) {
            return;
        }
        draft.kind = if requested_periodic {
            DraftKind::Periodic(PeriodicDraft::new(cx))
        } else {
            DraftKind::Explicit(ExplicitDraft::new(cx))
        };
        self.status = Status::Warning("changing kind reset the kind-specific fields".into());
        self.sync_row_buttons(cx);
        cx.notify();
    }

    fn on_notation_selected(
        &mut self,
        _: Entity<Dropdown>,
        selected: &Selected,
        cx: &mut Context<Self>,
    ) {
        let EditorView::Form { draft, .. } = &mut self.view else {
            return;
        };
        let DraftKind::Periodic(periodic) = &mut draft.kind else {
            return;
        };
        periodic.notation = if selected.index == 0 {
            DraftNotation::RadlerDigits {
                place_value: new_input("10", "10", cx),
            }
        } else {
            DraftNotation::WesternTwelveTone
        };
        self.status = Status::Empty;
        cx.notify();
    }

    fn on_field_changed(&mut self, _: Entity<TextInput>, _: &Changed, cx: &mut Context<Self>) {
        if !matches!(self.status, Status::Empty | Status::Message(_)) {
            self.status = Status::Empty;
        }
        cx.notify();
    }

    fn select_system(&mut self, id: TuningSystemId, cx: &mut Context<Self>) {
        let Some(system) = self
            .systems
            .iter()
            .find(|system| system.id() == &id)
            .cloned()
        else {
            return;
        };
        self.selected_id = Some(id.clone());
        self.view = match system.source() {
            TuningSystemSource::BuiltIn => EditorView::BuiltIn,
            TuningSystemSource::User => EditorView::Form {
                original_id: Some(id),
                draft: Box::new(TuningDraft::from_system(
                    system.name(),
                    system.pitch_system(),
                    cx,
                )),
                deletion: DeletionState::Idle,
            },
        };
        self.status = Status::Empty;
        self.sync_row_buttons(cx);
        cx.notify();
    }

    fn sync_row_buttons(&self, cx: &mut Context<Self>) {
        let (row_count, add_label, remove_label) = match &self.view {
            EditorView::Form { draft, .. } => {
                let row_count = draft.row_count();
                match &draft.kind {
                    DraftKind::Periodic(_) => (row_count, "add degree", "remove last degree"),
                    DraftKind::Explicit(_) => (row_count, "add pitch", "remove last pitch"),
                }
            }
            EditorView::BuiltIn => (0, "add row", "remove last row"),
        };
        self.add_row_button.update(cx, |button, cx| {
            button.set_label(add_label, cx);
        });
        self.remove_row_button.update(cx, |button, cx| {
            button.set_label(remove_label, cx);
            button.set_disabled(row_count <= 1, cx);
        });
    }

    fn select_system_from_list(&mut self, id: TuningSystemId, cx: &mut Context<Self>) {
        if self.selected_id.as_ref() == Some(&id) {
            return;
        }
        self.select_system(id, cx);
    }
}

impl Render for Model {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list = self.tuning_list(cx);
        let details = match &self.view {
            EditorView::BuiltIn => self.builtin_details(),
            EditorView::Form {
                original_id,
                draft,
                deletion,
            } => self.form_details(original_id.as_ref(), draft, *deletion, cx),
        };
        let workspace = div()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .debug_selector(|| "tuning-editor-workspace".to_string())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(s::S9)
                    .min_h(px(0.0))
                    .debug_selector(|| "tuning-editor-sidebar".to_string())
                    .child(list)
                    .child(
                        button::action_group([
                            self.new_button.clone(),
                            self.duplicate_button.clone(),
                        ])
                        .pt(s::S4),
                    ),
            )
            .child(details);
        let tile = workspace_tile::tile("tuning system editor", workspace)
            .debug_selector(|| "tuning-editor-tile".to_string());

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .bg(s::GREEN2)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .p(s::CONTENT_PADDING)
                    .child(tile),
            )
            .child(status_bar::bar(self.status.clone()))
    }
}

impl Model {
    fn tuning_list(&self, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self
            .systems
            .iter()
            .enumerate()
            .map(|(index, system)| {
                let id = system.id().clone();
                let selected = self.selected_id.as_ref() == Some(system.id());
                selection_list::row(index, selected, system.name().to_string()).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |model, _: &MouseDownEvent, _: &mut Window, cx| {
                        model.select_system_from_list(id.clone(), cx);
                    }),
                )
            })
            .collect();
        selection_list::list("tuning-system-list", "no tuning systems", rows)
    }

    fn builtin_details(&self) -> gpui::Div {
        let Some(system) = self.selected_system() else {
            return empty_details("select or create a tuning system");
        };
        let kind = match system.pitch_system() {
            PitchSystem::Periodic(_) => "periodic",
            PitchSystem::Explicit(_) => "explicit",
        };
        div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
        .bg(s::GRAY2)
        .p(s::CONTENT_PADDING)
        .gap_5()
        .child(
            div()
                .text_color(s::TEXT_HEADER)
                .child(system.name().to_string()),
        )
        .child(detail("stable id", system.id().as_str().to_string()))
        .child(detail("kind", kind))
        .child(
            div()
                .text_color(s::TEXT_DEFAULT)
                .child("this built-in compatibility tuning is read-only; duplicate it to make an editable copy"),
        )
        .debug_selector(|| "tuning-editor-details".to_string())
    }

    fn form_details(
        &self,
        original_id: Option<&TuningSystemId>,
        draft: &TuningDraft,
        deletion: DeletionState,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let title = match original_id {
            Some(_) => "edit tuning system",
            None => "new tuning system",
        };
        let identity = original_id.map(|id| detail("stable id", id.as_str().to_string()));
        let (kind_settings, tuning_values) = self.kind_sections(draft, cx);
        let primary_actions =
            button::action_group([self.cancel_button.clone(), self.save_button.clone()])
                .debug_selector(|| "tuning-primary-actions".to_string());
        let settings_actions = match (original_id, deletion) {
            (Some(id), DeletionState::Confirming) => destructive_confirmation(
                format!("delete tuning system {:?}?", id.as_str()),
                button::action_group([
                    self.cancel_delete_button.clone(),
                    self.confirm_delete_button.clone(),
                ]),
            ),
            (Some(_), DeletionState::Idle) => div()
                .flex()
                .items_end()
                .justify_end()
                .gap(s::CONTENT_PADDING)
                .child(
                    div()
                        .child(self.delete_button.clone())
                        .debug_selector(|| "tuning-delete-action".to_string()),
                )
                .child(primary_actions),
            (None, _) => div().flex().justify_end().child(primary_actions),
        }
        .debug_selector(|| "tuning-settings-actions".to_string());
        let settings_fields = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(div().text_color(s::TEXT_HEADER).child(title))
            .children(identity)
            .child(
                field_group("name", draft.name.clone())
                    .flex_none()
                    .w(s::S10)
                    .max_w_full(),
            )
            .child(compact_control_group("kind", draft.kind_dropdown.clone()))
            .child(kind_settings);
        let settings = div()
            .flex()
            .flex_col()
            .flex_none()
            .justify_between()
            .w(s::S10)
            .max_w_full()
            .min_h(px(0.0))
            .debug_selector(|| "tuning-settings-column".to_string())
            .child(settings_fields)
            .child(settings_actions);
        let form = div()
            .flex()
            .flex_1()
            .min_w(s::S0)
            .min_h(px(0.0))
            .gap(s::CONTENT_PADDING)
            .child(settings)
            .child(tuning_values);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(s::S0)
            .min_h(s::S0)
            .overflow_hidden()
            .bg(s::GRAY2)
            .p(s::CONTENT_PADDING)
            .pb(s::S0)
            .gap(s::CONTENT_PADDING)
            .child(form)
            .debug_selector(|| "tuning-editor-details".to_string())
    }

    fn kind_sections(&self, draft: &TuningDraft, cx: &Context<Self>) -> (gpui::Div, gpui::Div) {
        match &draft.kind {
            DraftKind::Periodic(periodic) => {
                let notation_specific = match &periodic.notation {
                    DraftNotation::RadlerDigits { place_value } => Some(
                        field_group("notation place value", place_value.clone())
                            .flex_none()
                            .w(s::S8)
                            .max_w_full(),
                    ),
                    DraftNotation::WesternTwelveTone => None,
                };
                let settings = div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div().flex().flex_wrap().gap_4().children([
                            field_group("fundamental (hz)", periodic.fundamental.clone())
                                .flex_none()
                                .w(s::S9)
                                .max_w_full(),
                            field_group("repeating period", periodic.period.clone())
                                .flex_none()
                                .w(s::S9)
                                .max_w_full(),
                        ]),
                    )
                    .child(compact_control_group(
                        "notation",
                        periodic.notation_dropdown.clone(),
                    ))
                    .children(notation_specific);
                let values = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .w(s::S0)
                    .max_w(s::S10)
                    .min_w(s::S0)
                    .min_h(px(0.0))
                    .gap_3()
                    .debug_selector(|| "tuning-values-column".to_string())
                    .child(
                        ordered_input_list::editable(
                            "tuning-degree-intervals",
                            "intervals by degree",
                            &periodic.degrees,
                            &periodic.invalid_degrees(cx),
                            &periodic.scroll_handle,
                        )
                        .debug_selector(|| "tuning-degree-interval-list".to_string()),
                    )
                    .child(self.row_actions());
                (settings, values)
            }
            DraftKind::Explicit(explicit) => {
                let settings = div();
                let values = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .w(s::S0)
                    .max_w(s::S10)
                    .min_w(s::S0)
                    .min_h(px(0.0))
                    .gap_3()
                    .debug_selector(|| "tuning-values-column".to_string())
                    .child(
                        div()
                            .text_color(s::FIELD_LABEL_TEXT)
                            .child("notation and frequencies"),
                    )
                    .child(data_grid::editable(
                        "explicit-pitches-grid",
                        vec!["token".to_string(), "frequency (hz)".to_string()],
                        &explicit.pitches,
                        &explicit.invalid_cells(cx),
                        None,
                        &explicit.scroll_handle,
                    ))
                    .child(self.row_actions());
                (settings, values)
            }
        }
    }

    fn row_actions(&self) -> gpui::Div {
        div().flex().w_full().justify_end().child(
            button::action_group([self.add_row_button.clone(), self.remove_row_button.clone()])
                .debug_selector(|| "tuning-row-action-buttons".to_string()),
        )
    }
}

impl TuningDraft {
    fn new_periodic(name: impl Into<String>, cx: &mut Context<Model>) -> Self {
        let name = new_input(name.into(), "slendro sketch", cx);
        let kind_dropdown = new_kind_dropdown(0, cx);
        Self {
            name,
            kind_dropdown,
            kind: DraftKind::Periodic(PeriodicDraft::new(cx)),
        }
    }

    fn from_system(name: impl Into<String>, system: &PitchSystem, cx: &mut Context<Model>) -> Self {
        let name = new_input(name.into(), "tuning name", cx);
        match system {
            PitchSystem::Periodic(system) => Self {
                name,
                kind_dropdown: new_kind_dropdown(0, cx),
                kind: DraftKind::Periodic(PeriodicDraft::from_system(system, cx)),
            },
            PitchSystem::Explicit(system) => Self {
                name,
                kind_dropdown: new_kind_dropdown(1, cx),
                kind: DraftKind::Explicit(ExplicitDraft::from_system(system, cx)),
            },
        }
    }

    fn pitch_system(&self, cx: &Context<Model>) -> Result<PitchSystem, String> {
        let name = self.name.read(cx).value();
        match &self.kind {
            DraftKind::Periodic(periodic) => periodic.pitch_system(name, cx),
            DraftKind::Explicit(explicit) => explicit.pitch_system(name, cx),
        }
    }

    fn add_row(&mut self, cx: &mut Context<Model>) {
        match &mut self.kind {
            DraftKind::Periodic(periodic) => {
                periodic.degrees.push(new_input("", "3/2 or 700c", cx))
            }
            DraftKind::Explicit(explicit) => explicit
                .pitches
                .push(vec![new_input("", "ember", cx), new_input("", "197.3", cx)]),
        }
    }

    fn remove_last_row(&mut self) {
        match &mut self.kind {
            DraftKind::Periodic(periodic) if periodic.degrees.len() > 1 => {
                periodic.degrees.pop();
            }
            DraftKind::Explicit(explicit) if explicit.pitches.len() > 1 => {
                explicit.pitches.pop();
            }
            _ => {}
        }
    }

    fn row_count(&self) -> usize {
        match &self.kind {
            DraftKind::Periodic(periodic) => periodic.degrees.len(),
            DraftKind::Explicit(explicit) => explicit.pitches.len(),
        }
    }
}

impl PeriodicDraft {
    fn new(cx: &mut Context<Model>) -> Self {
        Self {
            fundamental: new_input("25", "25", cx),
            period: new_input("2/1", "2/1", cx),
            notation_dropdown: new_notation_dropdown(0, cx),
            notation: DraftNotation::RadlerDigits {
                place_value: new_input("10", "10", cx),
            },
            degrees: vec![new_input("1/1", "1/1", cx)],
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn from_system(system: &PeriodicPitchSystem, cx: &mut Context<Model>) -> Self {
        let (notation_index, notation) = match system.notation() {
            PeriodicNotation::RadlerDigits { place_value } => (
                0,
                DraftNotation::RadlerDigits {
                    place_value: new_input(place_value.get().to_string(), "10", cx),
                },
            ),
            PeriodicNotation::WesternTwelveTone => (1, DraftNotation::WesternTwelveTone),
        };
        Self {
            fundamental: new_input(system.fundamental().as_hz().to_string(), "25", cx),
            period: new_input(system.period().config_value(), "2/1", cx),
            notation_dropdown: new_notation_dropdown(notation_index, cx),
            notation,
            degrees: system
                .degrees()
                .iter()
                .map(|degree| new_input(degree.config_value(), "1/1", cx))
                .collect(),
            scroll_handle: ScrollHandle::new(),
        }
    }

    fn pitch_system(&self, name: String, cx: &Context<Model>) -> Result<PitchSystem, String> {
        let fundamental = FrequencyHz::from_config(&self.fundamental.read(cx).value())
            .map_err(|error| error.to_string())?;
        let period = Interval::from_config(&self.period.read(cx).value())
            .map_err(|error| format!("repeating period: {error}"))?;
        let degrees = self
            .degrees
            .iter()
            .enumerate()
            .map(|(index, degree)| {
                Interval::from_config(&degree.read(cx).value())
                    .map_err(|error| format!("degree {}: {error}", index + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let notation = match &self.notation {
            DraftNotation::RadlerDigits { place_value } => {
                let value = place_value
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| {
                        "notation place value must be a positive whole number".to_string()
                    })?;
                PeriodicNotation::radler_digits(value).map_err(|error| error.to_string())?
            }
            DraftNotation::WesternTwelveTone => PeriodicNotation::WesternTwelveTone,
        };
        PeriodicPitchSystem::new(name, fundamental, period, degrees, notation)
            .map(PitchSystem::periodic)
            .map_err(|error| error.to_string())
    }

    fn invalid_degrees(&self, cx: &Context<Model>) -> Vec<usize> {
        self.degrees
            .iter()
            .enumerate()
            .filter_map(|(index, degree)| {
                Interval::from_config(&degree.read(cx).value())
                    .err()
                    .map(|_| index)
            })
            .collect()
    }
}

impl ExplicitDraft {
    fn new(cx: &mut Context<Model>) -> Self {
        Self {
            pitches: vec![vec![new_input("", "ember", cx), new_input("", "197.3", cx)]],
            scroll_handle: data_grid::DataGridScrollHandle::new(),
        }
    }

    fn from_system(system: &ExplicitPitchSystem, cx: &mut Context<Model>) -> Self {
        Self {
            pitches: system
                .pitches()
                .iter()
                .map(|(token, frequency)| {
                    vec![
                        new_input(token, "ember", cx),
                        new_input(frequency.as_hz().to_string(), "197.3", cx),
                    ]
                })
                .collect(),
            scroll_handle: data_grid::DataGridScrollHandle::new(),
        }
    }

    fn pitch_system(&self, name: String, cx: &Context<Model>) -> Result<PitchSystem, String> {
        let mut pitches = BTreeMap::new();
        for (index, row) in self.pitches.iter().enumerate() {
            let token = row[0].read(cx).value();
            let frequency = FrequencyHz::from_config(&row[1].read(cx).value())
                .map_err(|error| format!("pitch row {}: {error}", index + 1))?;
            if pitches.insert(token.clone(), frequency).is_some() {
                return Err(format!(
                    "pitch row {} duplicates token {token:?}",
                    index + 1
                ));
            }
        }
        ExplicitPitchSystem::new(name, pitches)
            .map(PitchSystem::explicit)
            .map_err(|error| error.to_string())
    }

    fn invalid_cells(&self, cx: &Context<Model>) -> data_grid::InvalidCells {
        let tokens = self
            .pitches
            .iter()
            .map(|row| row[0].read(cx).value())
            .collect::<Vec<_>>();
        self.pitches
            .iter()
            .enumerate()
            .flat_map(|(row_index, row)| {
                let token = &tokens[row_index];
                let token_invalid = token.trim().is_empty()
                    || token.trim() != token
                    || tokens[..row_index].contains(token);
                let frequency_invalid = FrequencyHz::from_config(&row[1].read(cx).value()).is_err();
                [
                    token_invalid.then_some((row_index, 0)),
                    frequency_invalid.then_some((row_index, 1)),
                ]
                .into_iter()
                .flatten()
            })
            .collect()
    }
}

fn new_input(
    value: impl Into<String>,
    placeholder: impl Into<String>,
    cx: &mut Context<Model>,
) -> Entity<TextInput> {
    let value = value.into();
    let placeholder = placeholder.into();
    let input = cx.new(move |cx| TextInput::new(value, placeholder, cx));
    cx.subscribe(&input, Model::on_field_changed).detach();
    input
}

fn new_kind_dropdown(selected: usize, cx: &mut Context<Model>) -> Entity<Dropdown> {
    let dropdown =
        cx.new(move |cx| Dropdown::new("tuning-kind", ["periodic", "explicit"], selected, cx));
    cx.subscribe(&dropdown, Model::on_kind_selected).detach();
    dropdown
}

fn new_notation_dropdown(selected: usize, cx: &mut Context<Model>) -> Entity<Dropdown> {
    let dropdown = cx.new(move |cx| {
        Dropdown::new(
            "tuning-notation",
            ["radler digits", "western twelve-tone"],
            selected,
            cx,
        )
    });
    cx.subscribe(&dropdown, Model::on_notation_selected)
        .detach();
    dropdown
}

fn detail(label: impl Into<gpui::SharedString>, value: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_color(s::TEXT_HEADER).child(label.into()))
        .child(div().text_color(s::TEXT_DEFAULT).child(value.into()))
}

fn empty_details(message: impl Into<gpui::SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_1()
        .min_w(s::S0)
        .min_h(s::S0)
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(s::GRAY2)
        .text_color(s::TEXT_DEFAULT)
        .child(message.into())
        .debug_selector(|| "tuning-editor-details".to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use gpui::{px, size, TestAppContext};

    use super::{EditorView, Model};
    use crate::{style as s, tuning_system, view::button};

    #[gpui::test]
    fn full_page_editor_creates_and_selects_a_reusable_tuning(cx: &mut TestAppContext) {
        let root = temp_root("create");
        let root_for_view = root.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| Model::new(root_for_view, cx));

        cx.update(|_, cx| {
            let new_button = editor.read(cx).new_button.clone();
            editor.update(cx, |editor, cx| {
                editor.on_new_clicked(new_button, &button::Clicked, cx);
                let EditorView::Form { draft, .. } = &editor.view else {
                    panic!("new tuning action must open the editor form");
                };
                draft.name.update(cx, |name, cx| {
                    name.sync_value("my slendro", cx);
                });
            });
            let save_button = editor.read(cx).save_button.clone();
            editor.update(cx, |editor, cx| {
                editor.on_save_clicked(save_button, &button::Clicked, cx);
            });
        });

        let systems = tuning_system::list_tuning_systems(&root).unwrap();
        assert_eq!(systems.len(), 2);
        assert_eq!(systems[1].id().as_str(), "my-slendro");
        assert_eq!(
            cx.update(|_, cx| editor.read(cx).selected_id.clone()),
            Some(systems[1].id().clone())
        );
        cx.simulate_resize(size(px(1_200.0), px(800.0)));
        cx.run_until_parked();
        let settings = cx.debug_bounds("tuning-settings-column").unwrap();
        let actions = cx.debug_bounds("tuning-settings-actions").unwrap();
        let delete = cx.debug_bounds("tuning-delete-action").unwrap();
        let primary = cx.debug_bounds("tuning-primary-actions").unwrap();
        let values = cx.debug_bounds("tuning-values-column").unwrap();
        let row_actions = cx.debug_bounds("tuning-row-action-buttons").unwrap();
        let settings_right = settings.origin.x + settings.size.width;
        let values_right = values.origin.x + values.size.width;

        assert!(actions.origin.x >= settings.origin.x);
        assert!(actions.origin.x + actions.size.width <= settings_right);
        assert!(delete.origin.x + delete.size.width < primary.origin.x);
        assert_eq!(primary.origin.x + primary.size.width, settings_right);
        assert_eq!(row_actions.origin.x + row_actions.size.width, values_right);
        assert_eq!(
            primary.origin.y + primary.size.height,
            settings.origin.y + settings.size.height
        );
        assert_eq!(
            row_actions.origin.y + row_actions.size.height,
            values.origin.y + values.size.height
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn editor_starts_on_the_builtin_read_only_tuning(cx: &mut TestAppContext) {
        let root = temp_root("builtin");
        let root_for_view = root.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| Model::new(root_for_view, cx));

        assert!(cx.update(|_, cx| matches!(editor.read(cx).view, EditorView::BuiltIn)));
        assert_eq!(cx.update(|_, cx| editor.read(cx).systems.len()), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn detail_panels_match_the_sidebar_height(cx: &mut TestAppContext) {
        let root = temp_root("layout");
        let root_for_view = root.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| Model::new(root_for_view, cx));
        cx.simulate_resize(size(px(1_200.0), px(800.0)));
        cx.run_until_parked();

        let sidebar = cx.debug_bounds("tuning-editor-sidebar").unwrap();
        let details = cx.debug_bounds("tuning-editor-details").unwrap();

        assert_eq!(details.origin.y, sidebar.origin.y);
        assert_eq!(details.size.height, sidebar.size.height);

        let new_button = cx.update(|_, cx| editor.read(cx).new_button.clone());
        editor.update(cx, |editor, cx| {
            editor.on_new_clicked(new_button, &button::Clicked, cx);
        });
        cx.run_until_parked();

        let sidebar = cx.debug_bounds("tuning-editor-sidebar").unwrap();
        let details = cx.debug_bounds("tuning-editor-details").unwrap();
        let kind_dropdown = cx.debug_bounds("tuning-kind-trigger").unwrap();
        let notation_dropdown = cx.debug_bounds("tuning-notation-trigger").unwrap();
        let settings = cx.debug_bounds("tuning-settings-column").unwrap();
        let values = cx.debug_bounds("tuning-values-column").unwrap();
        assert_eq!(details.origin.y, sidebar.origin.y);
        assert_eq!(details.size.height, sidebar.size.height);
        assert!(kind_dropdown.size.width <= s::S9);
        assert!(notation_dropdown.size.width <= s::S9);
        assert!(settings.origin.x + settings.size.width < values.origin.x);
        assert_eq!(settings.origin.y, values.origin.y);
        assert_eq!(settings.size.height, values.size.height);
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn periodic_intervals_are_a_compact_ordered_list(cx: &mut TestAppContext) {
        let root = temp_root("periodic-list");
        let root_for_view = root.clone();
        let (editor, cx) = cx.add_window_view(move |_, cx| Model::new(root_for_view, cx));
        let new_button = cx.update(|_, cx| editor.read(cx).new_button.clone());
        let add_button = cx.update(|_, cx| editor.read(cx).add_row_button.clone());

        editor.update(cx, |editor, cx| {
            editor.on_new_clicked(new_button, &button::Clicked, cx);
            editor.on_add_row_clicked(add_button.clone(), &button::Clicked, cx);
            editor.on_add_row_clicked(add_button, &button::Clicked, cx);
        });
        cx.simulate_resize(size(px(1_200.0), px(800.0)));
        cx.run_until_parked();

        let list = cx.debug_bounds("tuning-degree-interval-list").unwrap();
        let first = cx.debug_bounds("ordered-input-item-0").unwrap();
        let second = cx.debug_bounds("ordered-input-item-1").unwrap();
        let third = cx.debug_bounds("ordered-input-item-2").unwrap();
        let label = cx.debug_bounds("ordered-input-label").unwrap();
        let first_field = cx.debug_bounds("ordered-input-field-0").unwrap();

        assert!(list.size.width <= s::S9);
        assert_eq!(label.origin.x, first_field.origin.x);
        assert!(cx.debug_bounds("ordered-input-item-label-0").is_none());
        assert_eq!(first.origin.x, second.origin.x);
        assert_eq!(second.origin.x, third.origin.x);
        assert!(first.origin.y + first.size.height < second.origin.y);
        assert!(second.origin.y + second.size.height < third.origin.y);
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_root(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ahess-tuning-ui-{test_name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
