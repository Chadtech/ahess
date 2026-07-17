use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, MouseButton, MouseDownEvent, Window,
};

use crate::{
    part::{Part, PartName},
    style as s,
    view::{
        button::{self, Button},
        dialog::{
            self, destructive_confirmation, error_message, list_detail_dialog,
            management_form_dialog,
        },
        field_group::field_group,
        selection_list,
        text_input::TextInput,
    },
};

pub enum Msg {
    AddRequested {
        name: String,
        length: u32,
    },
    DuplicateRequested {
        source: PartName,
        name: String,
    },
    DeleteRequested {
        name: PartName,
    },
    SequenceChangeRequested {
        sequence: Vec<PartName>,
        selected_occurrence: Option<usize>,
    },
    Closed,
}

struct ListView {
    add_new_button: Entity<Button>,
    duplicate_button: Entity<Button>,
    delete_button: Entity<Button>,
    delete_confirmation: Option<DeleteConfirmation>,
    add_to_arrangement_button: Entity<Button>,
    move_earlier_button: Entity<Button>,
    move_later_button: Entity<Button>,
    repeat_button: Entity<Button>,
    remove_occurrence_button: Entity<Button>,
    delete_error: Option<String>,
    arrangement_error: Option<String>,
}

#[derive(Clone)]
struct DeleteConfirmation {
    cancel_button: Entity<Button>,
    confirm_button: Entity<Button>,
}

enum DialogView {
    List(Box<ListView>),
    Add {
        name: Entity<TextInput>,
        length: Entity<TextInput>,
        cancel_button: Entity<Button>,
        add_button: Entity<Button>,
        form_error: Option<String>,
    },
    Duplicate {
        source: PartName,
        name: Entity<TextInput>,
        cancel_button: Entity<Button>,
        duplicate_button: Entity<Button>,
        form_error: Option<String>,
    },
}

pub struct PartsDialog {
    parts: Vec<Part>,
    sequence: Vec<PartName>,
    selected_part: Option<PartName>,
    selected_occurrence: Option<usize>,
    view: DialogView,
    close_button: Entity<Button>,
}

impl EventEmitter<Msg> for PartsDialog {}

impl PartsDialog {
    pub fn new(parts: Vec<Part>, sequence: Vec<PartName>, cx: &mut Context<Self>) -> Self {
        let selected_part = parts.first().map(|part| part.name.clone());
        let selected_occurrence = (!sequence.is_empty()).then_some(0);
        let close_button = cx.new(|_| Button::x("close-parts"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        let dialog = Self {
            parts,
            sequence,
            selected_part,
            selected_occurrence,
            view: Self::list_view(cx),
            close_button,
        };
        dialog.sync_button_states(cx);
        dialog
    }

    fn list_view(cx: &mut Context<Self>) -> DialogView {
        let add_new_button = cx.new(|_| Button::new("add-new-part", "add new part"));
        let duplicate_button = cx.new(|_| Button::new("duplicate-part", "duplicate part"));
        let delete_button = cx.new(|_| Button::new("delete-part", "delete part"));
        let add_to_arrangement_button =
            cx.new(|_| Button::new("add-to-arrangement", "add to arrangement"));
        let move_earlier_button =
            cx.new(|_| Button::square("move-arrangement-earlier", "↑").disabled(true));
        let move_later_button =
            cx.new(|_| Button::square("move-arrangement-later", "↓").disabled(true));
        let repeat_button = cx.new(|_| Button::new("repeat-arrangement-part", "repeat"));
        let remove_occurrence_button = cx.new(|_| Button::new("remove-arrangement-part", "remove"));

        cx.subscribe(&add_new_button, Self::on_add_new_clicked)
            .detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_clicked)
            .detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();
        cx.subscribe(
            &add_to_arrangement_button,
            Self::on_add_to_arrangement_clicked,
        )
        .detach();
        cx.subscribe(&move_earlier_button, Self::on_move_earlier_clicked)
            .detach();
        cx.subscribe(&move_later_button, Self::on_move_later_clicked)
            .detach();
        cx.subscribe(&repeat_button, Self::on_repeat_clicked)
            .detach();
        cx.subscribe(
            &remove_occurrence_button,
            Self::on_remove_occurrence_clicked,
        )
        .detach();

        DialogView::List(Box::new(ListView {
            add_new_button,
            duplicate_button,
            delete_button,
            delete_confirmation: None,
            add_to_arrangement_button,
            move_earlier_button,
            move_later_button,
            repeat_button,
            remove_occurrence_button,
            delete_error: None,
            arrangement_error: None,
        }))
    }

    fn delete_confirmation(cx: &mut Context<Self>) -> DeleteConfirmation {
        let cancel_button = cx.new(|_| Button::new("cancel-delete-part", "keep part"));
        let confirm_button = cx.new(|_| Button::new("confirm-delete-part", "delete part"));
        cx.subscribe(&cancel_button, Self::on_cancel_delete_clicked)
            .detach();
        cx.subscribe(&confirm_button, Self::on_confirm_delete_clicked)
            .detach();

        DeleteConfirmation {
            cancel_button,
            confirm_button,
        }
    }

    fn duplicate_view(source: PartName, cx: &mut Context<Self>) -> DialogView {
        let placeholder = format!("{} copy", source.as_str());
        let name = cx.new(|cx| TextInput::new("", placeholder, cx));
        let cancel_button = cx.new(|_| Button::new("cancel-duplicate-part", "cancel"));
        let duplicate_button = cx.new(|_| Button::new("confirm-duplicate-part", "duplicate part"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_confirmed)
            .detach();

        DialogView::Duplicate {
            source,
            name,
            cancel_button,
            duplicate_button,
            form_error: None,
        }
    }

    fn add_view(cx: &mut Context<Self>) -> DialogView {
        let name = cx.new(|cx| TextInput::new("", "intro", cx));
        let length = cx.new(|cx| TextInput::new("16", "16", cx));
        let cancel_button = cx.new(|_| Button::new("cancel-add-part", "cancel"));
        let add_button = cx.new(|_| Button::new("confirm-add-part", "add part"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&add_button, Self::on_add_clicked).detach();

        DialogView::Add {
            name,
            length,
            cancel_button,
            add_button,
            form_error: None,
        }
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        cx.emit(Msg::Closed);
    }

    fn on_add_new_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::add_view(cx);
        cx.notify();
    }

    fn on_duplicate_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.selected_part.clone() else {
            return;
        };

        self.view = Self::duplicate_view(source, cx);
        cx.notify();
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        self.suppress_add_new_hover(cx);
        cx.notify();
    }

    fn on_add_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let DialogView::Add { name, length, .. } = &self.view else {
            return;
        };
        let name = name.read(cx).value();
        let length = length.read(cx).value();
        match parse_part_length(&length) {
            Ok(length) => cx.emit(Msg::AddRequested { name, length }),
            Err(error) => {
                if let DialogView::Add { form_error, .. } = &mut self.view {
                    *form_error = Some(error);
                    cx.notify();
                }
            }
        }
    }

    fn on_duplicate_confirmed(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let DialogView::Duplicate { source, name, .. } = &self.view else {
            return;
        };
        cx.emit(Msg::DuplicateRequested {
            source: source.clone(),
            name: name.read(cx).value(),
        });
    }

    fn on_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let should_start = matches!(
            &self.view,
            DialogView::List(view) if view.delete_confirmation.is_none()
        );
        if !should_start {
            return;
        }
        let confirmation = Self::delete_confirmation(cx);
        if let DialogView::List(view) = &mut self.view {
            view.delete_confirmation = Some(confirmation);
            view.delete_error = None;
            cx.notify();
        }
    }

    fn on_cancel_delete_clicked(
        &mut self,
        button: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let is_current = matches!(
            &self.view,
            DialogView::List(view)
                if view.delete_confirmation.as_ref().is_some_and(|confirmation| {
                    confirmation.cancel_button == button
                })
        );
        if !is_current {
            return;
        }
        if let DialogView::List(view) = &mut self.view {
            view.delete_confirmation = None;
            view.delete_error = None;
            cx.notify();
        }
    }

    fn on_confirm_delete_clicked(
        &mut self,
        button: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let is_current = matches!(
            &self.view,
            DialogView::List(view)
                if view.delete_confirmation.as_ref().is_some_and(|confirmation| {
                    confirmation.confirm_button == button
                })
        );
        if !is_current {
            return;
        }
        let Some(name) = self.selected_part.clone() else {
            return;
        };

        cx.emit(Msg::DeleteRequested { name });
    }

    fn on_add_to_arrangement_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(part_name) = self.selected_part.clone() else {
            return;
        };
        let (sequence, selected_occurrence) =
            sequence_with_inserted_part(&self.sequence, part_name, self.selected_occurrence);
        cx.emit(Msg::SequenceChangeRequested {
            sequence,
            selected_occurrence: Some(selected_occurrence),
        });
    }

    fn on_move_earlier_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_occurrence) = self.selected_occurrence else {
            return;
        };
        let Some((sequence, selected_occurrence)) =
            sequence_with_moved_part(&self.sequence, selected_occurrence, -1)
        else {
            return;
        };
        cx.emit(Msg::SequenceChangeRequested {
            sequence,
            selected_occurrence: Some(selected_occurrence),
        });
    }

    fn on_move_later_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_occurrence) = self.selected_occurrence else {
            return;
        };
        let Some((sequence, selected_occurrence)) =
            sequence_with_moved_part(&self.sequence, selected_occurrence, 1)
        else {
            return;
        };
        cx.emit(Msg::SequenceChangeRequested {
            sequence,
            selected_occurrence: Some(selected_occurrence),
        });
    }

    fn on_repeat_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_occurrence) = self.selected_occurrence else {
            return;
        };
        let Some((sequence, selected_occurrence)) =
            sequence_with_repeated_part(&self.sequence, selected_occurrence)
        else {
            return;
        };
        cx.emit(Msg::SequenceChangeRequested {
            sequence,
            selected_occurrence: Some(selected_occurrence),
        });
    }

    fn on_remove_occurrence_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_occurrence) = self.selected_occurrence else {
            return;
        };
        let Some((sequence, selected_occurrence)) =
            sequence_with_removed_part(&self.sequence, selected_occurrence)
        else {
            return;
        };
        cx.emit(Msg::SequenceChangeRequested {
            sequence,
            selected_occurrence,
        });
    }

    fn select_part(&mut self, name: &PartName, cx: &mut Context<Self>) {
        let Some(part) = find_part(&self.parts, name) else {
            return;
        };
        if self.selected_part.as_ref() == Some(&part.name) {
            return;
        }

        self.selected_part = Some(part.name.clone());
        if let DialogView::List(view) = &mut self.view {
            view.delete_confirmation = None;
            view.delete_error = None;
            view.arrangement_error = None;
        }
        self.sync_button_states(cx);
        cx.notify();
    }

    fn select_occurrence(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.sequence.len() || self.selected_occurrence == Some(index) {
            return;
        }

        self.selected_occurrence = Some(index);
        if let DialogView::List(view) = &mut self.view {
            view.arrangement_error = None;
        }
        self.sync_button_states(cx);
        cx.notify();
    }

    fn sync_button_states(&self, cx: &mut Context<Self>) {
        let DialogView::List(view) = &self.view else {
            return;
        };
        let can_move_earlier = self.selected_occurrence.is_some_and(|index| index > 0);
        let can_move_later = self
            .selected_occurrence
            .is_some_and(|index| index + 1 < self.sequence.len());
        let can_delete = self.selected_part.as_ref().is_some_and(|selected| {
            !self
                .sequence
                .iter()
                .any(|name| name.eq_ignore_ascii_case(selected))
        });
        view.delete_button.update(cx, |button, cx| {
            button.set_disabled(!can_delete, cx);
        });
        view.move_earlier_button.update(cx, |button, cx| {
            button.set_disabled(!can_move_earlier, cx);
        });
        view.move_later_button.update(cx, |button, cx| {
            button.set_disabled(!can_move_later, cx);
        });
    }

    fn suppress_add_new_hover(&self, cx: &mut Context<Self>) {
        if let DialogView::List(view) = &self.view {
            view.add_new_button.update(cx, |button, cx| {
                button.suppress_hover_until_pointer_exit(cx);
            });
        }
    }

    pub fn part_added(&mut self, parts: Vec<Part>, added: PartName, cx: &mut Context<Self>) {
        self.parts = parts;
        self.selected_part = Some(added);
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        self.suppress_add_new_hover(cx);
        cx.notify();
    }

    pub fn add_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::Add { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn duplicate_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::Duplicate { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn part_deleted(&mut self, parts: Vec<Part>, deleted: &PartName, cx: &mut Context<Self>) {
        let deleted_index = self
            .parts
            .iter()
            .position(|part| part.name.eq_ignore_ascii_case(deleted));
        self.parts = parts;
        self.selected_part = deleted_index
            .and_then(|index| {
                self.parts
                    .get(index.min(self.parts.len().saturating_sub(1)))
            })
            .map(|part| part.name.clone());
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        cx.notify();
    }

    pub fn delete_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::List(view) = &mut self.view {
            view.delete_confirmation = None;
            view.delete_error = Some(error);
            cx.notify();
        }
    }

    pub fn sequence_changed(
        &mut self,
        sequence: Vec<PartName>,
        selected_occurrence: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.sequence = sequence;
        self.selected_occurrence = selected_occurrence.filter(|index| *index < self.sequence.len());
        self.sync_button_states(cx);
        if let DialogView::List(view) = &mut self.view {
            view.arrangement_error = None;
        }
        cx.notify();
    }

    pub fn sequence_change_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::List(view) = &mut self.view {
            view.arrangement_error = Some(error);
            cx.notify();
        }
    }
}

impl Render for PartsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            DialogView::List(view) => list_detail_dialog(dialog::ListDetailArgs {
                title: "parts",
                close_button: self.close_button.clone(),
                list: dialog::column_with_actions(
                    part_list(&self.parts, self.selected_part.as_ref(), cx),
                    div()
                        .flex()
                        .debug_selector(|| "part-list-actions".to_string())
                        .child(view.add_new_button.clone()),
                ),
                details: part_details(
                    self.selected_part
                        .as_ref()
                        .and_then(|name| find_part(&self.parts, name)),
                    PartDetailsButtons {
                        duplicate: view.duplicate_button.clone(),
                        delete: view.delete_button.clone(),
                        delete_confirmation: view.delete_confirmation.clone(),
                        add_to_arrangement: view.add_to_arrangement_button.clone(),
                    },
                    view.delete_error.clone(),
                    &self.sequence,
                ),
                auxiliary: Some(
                    arrangement_panel(
                        &self.parts,
                        &self.sequence,
                        self.selected_occurrence,
                        view.move_earlier_button.clone(),
                        view.move_later_button.clone(),
                        view.repeat_button.clone(),
                        view.remove_occurrence_button.clone(),
                        view.arrangement_error.clone(),
                        cx,
                    )
                    .into_any_element(),
                ),
                footer: None,
            }),
            DialogView::Add {
                name,
                length,
                cancel_button,
                add_button,
                form_error,
            } => {
                let form = div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(field_group("part name", name.clone()))
                    .child(field_group("length in beats", length.clone()));
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                management_form_dialog(
                    "add part",
                    self.close_button.clone(),
                    form,
                    div()
                        .flex()
                        .justify_end()
                        .gap_3()
                        .child(cancel_button.clone())
                        .child(add_button.clone()),
                )
            }
            DialogView::Duplicate {
                source,
                name,
                cancel_button,
                duplicate_button,
                form_error,
            } => {
                let form = div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_color(s::TEXT_DEFAULT)
                            .child(format!("copying {:?}", source.as_str())),
                    )
                    .child(field_group("new part name", name.clone()));
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                management_form_dialog(
                    "duplicate part",
                    self.close_button.clone(),
                    form,
                    div()
                        .flex()
                        .justify_end()
                        .gap_3()
                        .child(cancel_button.clone())
                        .child(duplicate_button.clone()),
                )
            }
        }
    }
}

fn part_list(
    parts: &[Part],
    selected_part: Option<&PartName>,
    cx: &mut Context<PartsDialog>,
) -> gpui::Div {
    let rows = parts
        .iter()
        .enumerate()
        .map(|(index, part)| part_list_row(index, part, selected_part == Some(&part.name), cx))
        .collect::<Vec<_>>();

    selection_list::list("parts-list-scroll", "no parts yet", rows)
        .w_full()
        .debug_selector(|| "parts-list-column".to_string())
}

fn part_list_row(
    index: usize,
    part: &Part,
    selected: bool,
    cx: &mut Context<PartsDialog>,
) -> gpui::Div {
    let part_name = part.name.clone();
    selection_list::row(index, selected, part.name.as_str().to_owned())
        .debug_selector(move || format!("part-list-row-{index}"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
                dialog.select_part(&part_name, cx);
            }),
        )
}

struct PartDetailsButtons {
    duplicate: Entity<Button>,
    delete: Entity<Button>,
    delete_confirmation: Option<DeleteConfirmation>,
    add_to_arrangement: Entity<Button>,
}

fn part_details(
    part: Option<&Part>,
    buttons: PartDetailsButtons,
    delete_error: Option<String>,
    sequence: &[PartName],
) -> gpui::Div {
    let PartDetailsButtons {
        duplicate,
        delete,
        delete_confirmation,
        add_to_arrangement,
    } = buttons;
    let details = match part {
        Some(part) => {
            let occurrence_count = sequence
                .iter()
                .filter(|name| name.eq_ignore_ascii_case(&part.name))
                .count();
            let actions =
                if let Some(confirmation) = delete_confirmation.filter(|_| occurrence_count == 0) {
                    destructive_confirmation(
                        format!(
                            "delete {:?}? its csv file will be moved to the deleted folder.",
                            part.name.as_str()
                        ),
                        div()
                            .flex()
                            .gap_3()
                            .child(confirmation.cancel_button)
                            .child(confirmation.confirm_button),
                    )
                    .debug_selector(|| "part-details-actions".to_string())
                } else {
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap_3()
                        .debug_selector(|| "part-details-actions".to_string())
                        .child(
                            div()
                                .debug_selector(|| "add-to-arrangement-control".to_string())
                                .child(add_to_arrangement),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    div()
                                        .debug_selector(|| "duplicate-part-control".to_string())
                                        .child(duplicate),
                                )
                                .child(
                                    div()
                                        .debug_selector(|| "delete-part-control".to_string())
                                        .child(delete),
                                ),
                        )
                };
            let actions = if let Some(error) = delete_error {
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(actions)
                    .child(error_message(error))
            } else {
                actions
            };
            let beat_label = if part.length == 1 { "beat" } else { "beats" };

            div()
                .flex()
                .flex_col()
                .flex_1()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .text_color(s::TEXT_DEFAULT)
                                .child(part.name.as_str().to_owned()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_color(s::TEXT_HEADER).child("length"))
                                .child(
                                    div()
                                        .text_color(s::TEXT_DEFAULT)
                                        .child(format!("{} {beat_label}", part.length)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_color(s::TEXT_HEADER).child("arrangement"))
                                .child(div().text_color(s::TEXT_DEFAULT).child(
                                    match occurrence_count {
                                        0 => "not used".to_string(),
                                        1 => "used once".to_string(),
                                        count => format!("used {count} times"),
                                    },
                                )),
                        ),
                )
                .child(actions)
        }
        None => div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(s::TEXT_DEFAULT)
            .child("add a part to get started"),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w(s::S0)
        .min_w(s::S0)
        .min_h(px(0.0))
        .debug_selector(|| "part-details-column".to_string())
        .child(details)
}

#[allow(clippy::too_many_arguments)]
fn arrangement_panel(
    parts: &[Part],
    sequence: &[PartName],
    selected_occurrence: Option<usize>,
    move_earlier_button: Entity<Button>,
    move_later_button: Entity<Button>,
    repeat_button: Entity<Button>,
    remove_occurrence_button: Entity<Button>,
    arrangement_error: Option<String>,
    cx: &mut Context<PartsDialog>,
) -> gpui::Div {
    let rows = sequence
        .iter()
        .enumerate()
        .map(|(index, part_name)| {
            arrangement_row(index, part_name, selected_occurrence == Some(index), cx)
        })
        .collect::<Vec<_>>();
    let total_beats = sequence
        .iter()
        .filter_map(|name| find_part(parts, name))
        .map(|part| part.length)
        .sum::<u32>();
    let part_label = if sequence.len() == 1 { "part" } else { "parts" };
    let beat_label = if total_beats == 1 { "beat" } else { "beats" };
    let has_selection = selected_occurrence.is_some_and(|index| index < sequence.len());

    let movement_actions = div()
        .flex()
        .items_center()
        .gap_3()
        .debug_selector(|| "arrangement-movement-actions".to_string())
        .child(div().text_color(s::TEXT_HEADER).child("move"))
        .child(move_earlier_button)
        .child(move_later_button);
    let occurrence_actions = div()
        .flex()
        .gap_3()
        .debug_selector(|| "arrangement-occurrence-actions".to_string())
        .child(repeat_button)
        .child(remove_occurrence_button);
    let action_row = div()
        .flex()
        .gap(s::S4)
        .debug_selector(|| "arrangement-actions".to_string())
        .child(movement_actions)
        .when(has_selection, |actions| actions.child(occurrence_actions));
    let actions = div().flex().flex_col().gap_3().pt(s::S4).child(action_row);
    let actions = if let Some(error) = arrangement_error {
        actions.child(error_message(error))
    } else {
        actions
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .w(s::S9)
        .debug_selector(|| "arrangement-column".to_string())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .pb(s::S4)
                .child(div().text_color(s::TEXT_HEADER).child("arrangement"))
                .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                    "{} {part_label}, {total_beats} {beat_label}",
                    sequence.len()
                ))),
        )
        .child(
            selection_list::list("arrangement-list-scroll", "no arranged parts yet", rows)
                .w_full()
                .debug_selector(|| "arrangement-list".to_string()),
        )
        .child(actions)
}

fn arrangement_row(
    index: usize,
    part_name: &PartName,
    selected: bool,
    cx: &mut Context<PartsDialog>,
) -> gpui::Div {
    selection_list::row(
        index,
        selected,
        format!("{}. {}", index + 1, part_name.as_str()),
    )
    .debug_selector(move || format!("arrangement-occurrence-{index}"))
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
            dialog.select_occurrence(index, cx);
        }),
    )
}

fn sequence_with_inserted_part(
    sequence: &[PartName],
    part_name: PartName,
    selected_occurrence: Option<usize>,
) -> (Vec<PartName>, usize) {
    let insertion_index = selected_occurrence
        .filter(|index| *index < sequence.len())
        .map_or(sequence.len(), |index| index + 1);
    let mut updated = sequence.to_vec();
    updated.insert(insertion_index, part_name);
    (updated, insertion_index)
}

fn sequence_with_moved_part(
    sequence: &[PartName],
    selected_occurrence: usize,
    offset: isize,
) -> Option<(Vec<PartName>, usize)> {
    if selected_occurrence >= sequence.len() {
        return None;
    }
    let target = selected_occurrence.checked_add_signed(offset)?;
    if target >= sequence.len() {
        return None;
    }

    let mut updated = sequence.to_vec();
    updated.swap(selected_occurrence, target);
    Some((updated, target))
}

fn sequence_with_repeated_part(
    sequence: &[PartName],
    selected_occurrence: usize,
) -> Option<(Vec<PartName>, usize)> {
    let repeated = sequence.get(selected_occurrence)?.clone();
    let repeated_index = selected_occurrence + 1;
    let mut updated = sequence.to_vec();
    updated.insert(repeated_index, repeated);
    Some((updated, repeated_index))
}

fn sequence_with_removed_part(
    sequence: &[PartName],
    selected_occurrence: usize,
) -> Option<(Vec<PartName>, Option<usize>)> {
    sequence.get(selected_occurrence)?;
    let mut updated = sequence.to_vec();
    updated.remove(selected_occurrence);
    let selected_occurrence =
        (!updated.is_empty()).then(|| selected_occurrence.min(updated.len().saturating_sub(1)));
    Some((updated, selected_occurrence))
}

fn find_part<'a>(parts: &'a [Part], name: &PartName) -> Option<&'a Part> {
    parts
        .iter()
        .find(|part| part.name.eq_ignore_ascii_case(name))
}

fn parse_part_length(value: &str) -> Result<u32, String> {
    let length = value
        .trim()
        .parse::<u32>()
        .map_err(|_| "part length must be a whole number".to_string())?;
    if length == 0 {
        return Err("part length must be at least one beat".to_string());
    }

    Ok(length)
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, size, ScrollDelta, ScrollWheelEvent, TestAppContext};

    use super::{
        parse_part_length, sequence_with_inserted_part, sequence_with_moved_part,
        sequence_with_removed_part, sequence_with_repeated_part, DialogView, PartsDialog,
    };
    use crate::{
        part::{Part, PartName},
        view::button,
    };

    #[test]
    fn part_length_is_a_positive_whole_number() {
        assert_eq!(parse_part_length(" 16 ").unwrap(), 16);
        assert!(parse_part_length("").is_err());
        assert!(parse_part_length("0").is_err());
        assert!(parse_part_length("1.5").is_err());
    }

    #[test]
    fn selected_parts_are_inserted_after_the_selected_occurrence() {
        let sequence = names(["part-a", "part-b", "part-b"]);

        let (updated, selected) = sequence_with_inserted_part(&sequence, "bridge".into(), Some(0));

        assert_eq!(
            name_strings(&updated),
            ["part-a", "bridge", "part-b", "part-b"]
        );
        assert_eq!(selected, 1);

        let (updated, selected) = sequence_with_inserted_part(&sequence, "bridge".into(), None);
        assert_eq!(
            name_strings(&updated),
            ["part-a", "part-b", "part-b", "bridge"]
        );
        assert_eq!(selected, 3);
    }

    #[test]
    fn arrangement_occurrences_can_move_repeat_and_be_removed() {
        let sequence = names(["part-a", "part-b", "bridge"]);

        let (moved, selected) = sequence_with_moved_part(&sequence, 1, -1).unwrap();
        assert_eq!(name_strings(&moved), ["part-b", "part-a", "bridge"]);
        assert_eq!(selected, 0);
        assert!(sequence_with_moved_part(&sequence, 0, -1).is_none());
        assert!(sequence_with_moved_part(&sequence, 2, 1).is_none());

        let (repeated, selected) = sequence_with_repeated_part(&sequence, 1).unwrap();
        assert_eq!(
            name_strings(&repeated),
            ["part-a", "part-b", "part-b", "bridge"]
        );
        assert_eq!(selected, 2);

        let (removed, selected) = sequence_with_removed_part(&sequence, 2).unwrap();
        assert_eq!(name_strings(&removed), ["part-a", "part-b"]);
        assert_eq!(selected, Some(1));
    }

    #[gpui::test]
    fn parts_dialog_renders_part_list_details_and_arrangement_columns(cx: &mut TestAppContext) {
        let parts = vec![
            Part::new("part-a", 16),
            Part::new("part-b", 8),
            Part::new("bridge", 12),
        ];
        let sequence = names(["part-a", "part-b", "part-b"]);
        let (dialog, cx) = cx.add_window_view(|_, cx| PartsDialog::new(parts, sequence, cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let part_list = cx.debug_bounds("parts-list-column").unwrap();
        let details = cx.debug_bounds("part-details-column").unwrap();
        let arrangement = cx.debug_bounds("arrangement-column").unwrap();
        let arrangement_list = cx.debug_bounds("arrangement-list").unwrap();
        let part_list_actions = cx.debug_bounds("part-list-actions").unwrap();
        let part_details_actions = cx.debug_bounds("part-details-actions").unwrap();
        let arrangement_actions = cx.debug_bounds("arrangement-actions").unwrap();
        let movement_actions = cx.debug_bounds("arrangement-movement-actions").unwrap();
        let occurrence_actions = cx.debug_bounds("arrangement-occurrence-actions").unwrap();

        assert!(part_list.size.width > px(0.0));
        assert!(details.size.width > px(0.0));
        assert!(arrangement.size.width > px(0.0));
        assert_eq!(arrangement_list.size.width, arrangement.size.width);
        assert!(part_list.origin.x < details.origin.x);
        assert!(details.origin.x < arrangement.origin.x);
        assert_eq!(part_list_actions.origin.x, part_list.origin.x);
        assert!(part_list.origin.y + part_list.size.height < part_list_actions.origin.y);
        assert!(part_details_actions.origin.y < part_list_actions.origin.y);
        assert_eq!(part_list_actions.origin.y, arrangement_actions.origin.y);
        assert_eq!(
            part_list_actions.origin.y + part_list_actions.size.height,
            part_details_actions.origin.y + part_details_actions.size.height
        );
        assert_eq!(movement_actions.origin.y, occurrence_actions.origin.y);
        assert!(movement_actions.origin.x < occurrence_actions.origin.x);
        assert!(
            occurrence_actions.origin.x + occurrence_actions.size.width
                <= arrangement.origin.x + arrangement.size.width,
            "movement {:?}, occurrence {:?}, arrangement {:?}",
            movement_actions,
            occurrence_actions,
            arrangement
        );
        assert!(
            (part_list.size.width / details.size.width - 1.0).abs() < 0.01,
            "part list {:?}, details {:?}, arrangement {:?}",
            part_list.size.width,
            details.size.width,
            arrangement.size.width
        );
        assert!(
            (details.size.width / arrangement.size.width - 1.0).abs() < 0.01,
            "part list {:?}, details {:?}, arrangement {:?}",
            part_list.size.width,
            details.size.width,
            arrangement.size.width
        );
        assert!(cx.debug_bounds("arrangement-occurrence-2").is_some());
        let add_to_arrangement = cx.debug_bounds("add-to-arrangement-control").unwrap();
        let duplicate_part = cx.debug_bounds("duplicate-part-control").unwrap();
        let delete_part_control = cx.debug_bounds("delete-part-control").unwrap();
        assert!(add_to_arrangement.origin.y < duplicate_part.origin.y);
        assert_eq!(duplicate_part.origin.y, delete_part_control.origin.y);
        assert!(
            add_to_arrangement.size.width < details.size.width
                && duplicate_part.size.width < details.size.width
        );

        dialog.update(cx, |dialog, cx| {
            dialog.select_part(&PartName::new("bridge"), cx);
        });
        cx.run_until_parked();

        assert_eq!(
            cx.debug_bounds("parts-list-column").unwrap().size.width,
            part_list.size.width
        );
        assert_eq!(
            cx.debug_bounds("part-details-column").unwrap().size.width,
            details.size.width
        );
        assert_eq!(
            cx.debug_bounds("arrangement-column").unwrap().size.width,
            arrangement.size.width
        );

        let (delete_part, move_earlier, move_later) = cx.update(|_, cx| {
            let DialogView::List(view) = &dialog.read(cx).view else {
                panic!("parts dialog should show its list view");
            };
            (
                view.delete_button.clone(),
                view.move_earlier_button.clone(),
                view.move_later_button.clone(),
            )
        });
        assert!(cx.debug_bounds("delete-part-control").is_some());
        assert!(cx.debug_bounds("delete-part-control").unwrap().size.width < details.size.width);
        assert!(!cx.update(|_, cx| delete_part.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| move_earlier.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| move_later.read(cx).is_disabled()));

        let last_occurrence = cx.debug_bounds("arrangement-occurrence-2").unwrap();
        cx.simulate_click(last_occurrence.center(), Default::default());

        assert!(!cx.update(|_, cx| move_earlier.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| move_later.read(cx).is_disabled()));

        dialog.update(cx, |dialog, cx| {
            dialog.select_part(&PartName::new("part-b"), cx);
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("delete-part-control").is_some());
        assert!(cx.debug_bounds("delete-part-control").unwrap().size.width < details.size.width);
        assert!(cx.update(|_, cx| delete_part.read(cx).is_disabled()));

        dialog.update(cx, |dialog, cx| {
            dialog.sequence_changed(Vec::new(), None, cx);
        });
        cx.run_until_parked();

        assert!(!cx.update(|_, cx| delete_part.read(cx).is_disabled()));
    }

    #[gpui::test]
    fn part_and_arrangement_lists_scroll_independently(cx: &mut TestAppContext) {
        let parts = (0..24)
            .map(|index| Part::new(format!("part-{index}"), 16))
            .collect::<Vec<_>>();
        let sequence = parts.iter().map(|part| part.name.clone()).collect();
        let (_, cx) = cx.add_window_view(|_, cx| PartsDialog::new(parts, sequence, cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let parts_list = cx.debug_bounds("parts-list-column").unwrap();
        let arrangement_list = cx.debug_bounds("arrangement-list").unwrap();
        let last_part_before = cx.debug_bounds("part-list-row-23").unwrap();
        let last_occurrence_before = cx.debug_bounds("arrangement-occurrence-23").unwrap();

        cx.simulate_event(ScrollWheelEvent {
            position: parts_list.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            ..Default::default()
        });

        let last_part_after = cx.debug_bounds("part-list-row-23").unwrap();
        assert!(last_part_after.origin.y < last_part_before.origin.y);
        assert_eq!(
            cx.debug_bounds("arrangement-occurrence-23").unwrap(),
            last_occurrence_before
        );

        cx.simulate_event(ScrollWheelEvent {
            position: arrangement_list.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            ..Default::default()
        });

        assert!(
            cx.debug_bounds("arrangement-occurrence-23")
                .unwrap()
                .origin
                .y
                < last_occurrence_before.origin.y
        );
    }

    #[gpui::test]
    fn duplicate_part_action_opens_a_new_name_form(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            PartsDialog::new(vec![Part::new("intro", 16)], Vec::new(), cx)
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let duplicate = cx.debug_bounds("duplicate-part-control").unwrap();
        cx.simulate_click(duplicate.center(), Default::default());

        cx.update(|_, cx| {
            let DialogView::Duplicate { source, name, .. } = &dialog.read(cx).view else {
                panic!("duplicate action should open its form");
            };
            assert_eq!(source.as_str(), "intro");
            assert_eq!(name.read(cx).value(), "");
        });
    }

    #[gpui::test]
    fn delete_confirmation_owns_its_controls_only_while_active(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            PartsDialog::new(vec![Part::new("intro", 16)], Vec::new(), cx)
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        cx.update(|_, cx| {
            let DialogView::List(view) = &dialog.read(cx).view else {
                panic!("parts dialog should show its list view");
            };
            assert!(view.delete_confirmation.is_none());
        });
        let delete = cx.debug_bounds("delete-part-control").unwrap();
        cx.simulate_click(delete.center(), Default::default());
        cx.run_until_parked();

        let cancel_button = cx.update(|_, cx| {
            let DialogView::List(view) = &dialog.read(cx).view else {
                panic!("parts dialog should show its list view");
            };
            let confirmation = view.delete_confirmation.as_ref().unwrap();
            confirmation.cancel_button.clone()
        });

        cancel_button.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.run_until_parked();

        cx.update(|_, cx| {
            let DialogView::List(view) = &dialog.read(cx).view else {
                panic!("parts dialog should show its list view");
            };
            assert!(view.delete_confirmation.is_none());
        });
    }

    fn names<const N: usize>(names: [&str; N]) -> Vec<PartName> {
        names.into_iter().map(PartName::from).collect()
    }

    fn name_strings(names: &[PartName]) -> Vec<&str> {
        names.iter().map(PartName::as_str).collect()
    }
}
