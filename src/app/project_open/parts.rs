use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, MouseButton, MouseDownEvent, Window,
};

use crate::{
    part::{self, Part, PartName},
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

pub enum Event {
    AddRequested { name: String, length: u32 },
    DeleteRequested { name: PartName },
    Closed,
}

enum DialogView {
    List {
        add_new_button: Entity<Button>,
        delete_button: Entity<Button>,
        cancel_delete_button: Entity<Button>,
        confirm_delete_button: Entity<Button>,
        delete_error: Option<String>,
        confirming_delete: bool,
    },
    Add {
        name: Entity<TextInput>,
        length: Entity<TextInput>,
        cancel_button: Entity<Button>,
        add_button: Entity<Button>,
        form_error: Option<String>,
    },
}

pub struct PartsDialog {
    parts: Vec<Part>,
    selected_part: Option<PartName>,
    view: DialogView,
    close_button: Entity<Button>,
}

impl EventEmitter<Event> for PartsDialog {}

impl PartsDialog {
    pub fn new(parts: Vec<Part>, cx: &mut Context<Self>) -> Self {
        let selected_part = parts.first().map(|part| part.name.clone());
        let close_button = cx.new(|_| Button::x("close-parts"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        Self {
            parts,
            selected_part,
            view: Self::list_view(cx),
            close_button,
        }
    }

    fn list_view(cx: &mut Context<Self>) -> DialogView {
        let add_new_button = cx.new(|_| Button::new("add-new-part", "add new part"));
        let delete_button = cx.new(|_| Button::new("delete-part", "delete part"));
        let cancel_delete_button = cx.new(|_| Button::new("cancel-delete-part", "keep part"));
        let confirm_delete_button = cx.new(|_| Button::new("confirm-delete-part", "delete part"));

        cx.subscribe(&add_new_button, Self::on_add_new_clicked)
            .detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();
        cx.subscribe(&cancel_delete_button, Self::on_cancel_delete_clicked)
            .detach();
        cx.subscribe(&confirm_delete_button, Self::on_confirm_delete_clicked)
            .detach();

        DialogView::List {
            add_new_button,
            delete_button,
            cancel_delete_button,
            confirm_delete_button,
            delete_error: None,
            confirming_delete: false,
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
        cx.emit(Event::Closed);
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

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::list_view(cx);
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
            Ok(length) => cx.emit(Event::AddRequested { name, length }),
            Err(error) => {
                if let DialogView::Add { form_error, .. } = &mut self.view {
                    *form_error = Some(error);
                    cx.notify();
                }
            }
        }
    }

    fn on_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let DialogView::List {
            confirming_delete,
            delete_error,
            ..
        } = &mut self.view
        {
            *confirming_delete = true;
            *delete_error = None;
            cx.notify();
        }
    }

    fn on_cancel_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let DialogView::List {
            confirming_delete,
            delete_error,
            ..
        } = &mut self.view
        {
            *confirming_delete = false;
            *delete_error = None;
            cx.notify();
        }
    }

    fn on_confirm_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self.selected_part.clone() else {
            return;
        };

        cx.emit(Event::DeleteRequested { name });
    }

    fn select_part(&mut self, name: &PartName, cx: &mut Context<Self>) {
        let Some(part) = find_part(&self.parts, name) else {
            return;
        };
        if self.selected_part.as_ref() == Some(&part.name) {
            return;
        }

        self.selected_part = Some(part.name.clone());
        if let DialogView::List {
            confirming_delete,
            delete_error,
            ..
        } = &mut self.view
        {
            *confirming_delete = false;
            *delete_error = None;
        }
        cx.notify();
    }

    fn suppress_add_new_hover(&self, cx: &mut Context<Self>) {
        if let DialogView::List { add_new_button, .. } = &self.view {
            add_new_button.update(cx, |button, cx| {
                button.suppress_hover_until_pointer_exit(cx);
            });
        }
    }

    pub fn part_added(&mut self, parts: Vec<Part>, added: PartName, cx: &mut Context<Self>) {
        self.parts = parts;
        self.selected_part = Some(added);
        self.view = Self::list_view(cx);
        self.suppress_add_new_hover(cx);
        cx.notify();
    }

    pub fn add_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::Add { form_error, .. } = &mut self.view {
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
        cx.notify();
    }

    pub fn delete_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::List {
            confirming_delete,
            delete_error,
            ..
        } = &mut self.view
        {
            *confirming_delete = false;
            *delete_error = Some(error);
            cx.notify();
        }
    }
}

impl Render for PartsDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            DialogView::List {
                add_new_button,
                delete_button,
                cancel_delete_button,
                confirm_delete_button,
                delete_error,
                confirming_delete,
            } => list_detail_dialog(dialog::ListDetailArgs {
                title: "parts",
                close_button: self.close_button.clone(),
                list: part_list(&self.parts, self.selected_part.as_ref(), cx),
                details: part_details(
                    self.selected_part
                        .as_ref()
                        .and_then(|name| find_part(&self.parts, name)),
                    delete_button.clone(),
                    cancel_delete_button.clone(),
                    confirm_delete_button.clone(),
                    *confirming_delete,
                    delete_error.clone(),
                ),
                add_button: add_new_button.clone(),
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

    selection_list::list("no parts yet", rows)
}

fn part_list_row(
    index: usize,
    part: &Part,
    selected: bool,
    cx: &mut Context<PartsDialog>,
) -> gpui::Div {
    let part_name = part.name.clone();
    selection_list::row(index, selected, part.name.as_str().to_owned()).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
            dialog.select_part(&part_name, cx);
        }),
    )
}

fn part_details(
    part: Option<&Part>,
    delete_button: Entity<Button>,
    cancel_delete_button: Entity<Button>,
    confirm_delete_button: Entity<Button>,
    confirming_delete: bool,
    delete_error: Option<String>,
) -> gpui::Div {
    let details = match part {
        Some(part) => {
            let delete_actions = if confirming_delete {
                destructive_confirmation(
                    format!(
                        "delete {:?}? its csv file will be moved to the deleted folder.",
                        part.name.as_str()
                    ),
                    div()
                        .flex()
                        .gap_3()
                        .child(cancel_delete_button)
                        .child(confirm_delete_button),
                )
            } else {
                div().flex().child(delete_button)
            };
            let delete_actions = if let Some(error) = delete_error {
                delete_actions.child(error_message(error))
            } else {
                delete_actions
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
                                .child(div().text_color(s::TEXT_HEADER).child("file"))
                                .child(div().text_color(s::TEXT_DEFAULT).child(
                                    part::csv_file_name(&part.name).unwrap_or_else(|error| {
                                        format!("unable to derive filename: {error}")
                                    }),
                                )),
                        ),
                )
                .child(delete_actions)
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
        .min_h(px(0.0))
        .child(details)
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
    use super::parse_part_length;

    #[test]
    fn part_length_is_a_positive_whole_number() {
        assert_eq!(parse_part_length(" 16 ").unwrap(), 16);
        assert!(parse_part_length("").is_err());
        assert!(parse_part_length("0").is_err());
        assert!(parse_part_length("1.5").is_err());
    }
}
