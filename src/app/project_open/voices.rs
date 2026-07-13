use gpui::{
    div, prelude::*, px, Context, Entity, EventEmitter, MouseButton, MouseDownEvent, Window,
};

use crate::{
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
    voice::{Voice, VoiceType},
    voice_name::VoiceName,
};

pub enum Msg {
    AddRequested {
        name: String,
        voice_type: VoiceType,
    },
    EditRequested {
        original_name: VoiceName,
        name: String,
        voice_type: VoiceType,
    },
    DeleteRequested {
        name: VoiceName,
    },
    Closed,
}

enum DialogView {
    List {
        add_new_button: Entity<Button>,
        edit_button: Entity<Button>,
    },
    Add {
        name: Entity<TextInput>,
        selected_voice_type: VoiceType,
        voice_type_buttons: VoiceTypeButtons,
        cancel_button: Entity<Button>,
        add_button: Entity<Button>,
        form_error: Option<String>,
    },
    Edit {
        name: Entity<TextInput>,
        selected_voice_type: VoiceType,
        voice_type_buttons: VoiceTypeButtons,
        cancel_button: Entity<Button>,
        save_button: Entity<Button>,
        delete_button: Entity<Button>,
        cancel_delete_button: Entity<Button>,
        confirm_delete_button: Entity<Button>,
        form_error: Option<String>,
        delete_error: Option<String>,
        confirming_delete: bool,
    },
}

pub struct VoicesDialog {
    voices: Vec<Voice>,
    selected_voice: Option<VoiceName>,
    view: DialogView,
    close_button: Entity<Button>,
}

impl EventEmitter<Msg> for VoicesDialog {}

impl VoicesDialog {
    pub fn new(voices: Vec<Voice>, cx: &mut Context<Self>) -> Self {
        let selected_voice = voices.first().map(|voice| voice.name.clone());
        let close_button = cx.new(|_| Button::x("close-voices"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        Self {
            voices,
            selected_voice,
            view: Self::list_view(cx),
            close_button,
        }
    }

    fn list_view(cx: &mut Context<Self>) -> DialogView {
        let add_new_button = cx.new(|_| Button::new("add-new-voice", "add new voice"));
        let edit_button = cx.new(|_| Button::new("edit-voice", "edit voice"));

        cx.subscribe(&add_new_button, Self::on_add_new_clicked)
            .detach();
        cx.subscribe(&edit_button, Self::on_edit_clicked).detach();

        DialogView::List {
            add_new_button,
            edit_button,
        }
    }

    fn add_view(cx: &mut Context<Self>) -> DialogView {
        let name = cx.new(|cx| TextInput::new("", "lead", cx));
        let selected_voice_type = VoiceType::Sin;
        let voice_type_buttons = VoiceTypeButtons::new(selected_voice_type, cx);
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let add_button = cx.new(|_| Button::new("confirm-add-voice", "add voice"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&add_button, Self::on_add_clicked).detach();
        Self::subscribe_voice_type_buttons(&voice_type_buttons, cx);

        DialogView::Add {
            name,
            selected_voice_type,
            voice_type_buttons,
            cancel_button,
            add_button,
            form_error: None,
        }
    }

    fn edit_view(voice: &Voice, cx: &mut Context<Self>) -> DialogView {
        let voice_name = voice.name.as_str().to_owned();
        let name = cx.new(move |cx| TextInput::new(voice_name, "lead", cx));
        let selected_voice_type = voice.voice_type;
        let voice_type_buttons = VoiceTypeButtons::new(selected_voice_type, cx);
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let save_button = cx.new(|_| Button::new("save-voice", "save changes"));
        let delete_button = cx.new(|_| Button::new("delete-voice", "delete voice"));
        let cancel_delete_button = cx.new(|_| Button::new("cancel-delete-voice", "keep voice"));
        let confirm_delete_button = cx.new(|_| Button::new("confirm-delete-voice", "delete voice"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();
        cx.subscribe(&cancel_delete_button, Self::on_cancel_delete_clicked)
            .detach();
        cx.subscribe(&confirm_delete_button, Self::on_confirm_delete_clicked)
            .detach();
        Self::subscribe_voice_type_buttons(&voice_type_buttons, cx);

        DialogView::Edit {
            name,
            selected_voice_type,
            voice_type_buttons,
            cancel_button,
            save_button,
            delete_button,
            cancel_delete_button,
            confirm_delete_button,
            form_error: None,
            delete_error: None,
            confirming_delete: false,
        }
    }

    fn subscribe_voice_type_buttons(buttons: &VoiceTypeButtons, cx: &mut Context<Self>) {
        cx.subscribe(&buttons.sin, Self::on_sin_clicked).detach();
        cx.subscribe(&buttons.saw, Self::on_saw_clicked).detach();
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

    fn on_edit_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(voice) = self
            .selected_voice
            .as_ref()
            .and_then(|name| find_voice(&self.voices, name))
            .cloned()
        else {
            return;
        };

        self.view = Self::edit_view(&voice, cx);
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

    fn on_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        if let DialogView::Edit {
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
        if let DialogView::Edit {
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
        let Some(name) = self.selected_voice.clone() else {
            return;
        };

        cx.emit(Msg::DeleteRequested { name });
    }

    fn on_add_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let DialogView::Add {
            name,
            selected_voice_type,
            ..
        } = &self.view
        else {
            return;
        };
        cx.emit(Msg::AddRequested {
            name: name.read(cx).value(),
            voice_type: *selected_voice_type,
        });
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(original_name) = self.selected_voice.clone() else {
            return;
        };
        let DialogView::Edit {
            name,
            selected_voice_type,
            ..
        } = &self.view
        else {
            return;
        };
        cx.emit(Msg::EditRequested {
            original_name,
            name: name.read(cx).value(),
            voice_type: *selected_voice_type,
        });
    }

    fn on_sin_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.select_voice_type(VoiceType::Sin, cx);
    }

    fn on_saw_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.select_voice_type(VoiceType::Saw, cx);
    }

    fn select_voice_type(&mut self, voice_type: VoiceType, cx: &mut Context<Self>) {
        let (selected_voice_type, voice_type_buttons, form_error) = match &mut self.view {
            DialogView::Add {
                selected_voice_type,
                voice_type_buttons,
                form_error,
                ..
            }
            | DialogView::Edit {
                selected_voice_type,
                voice_type_buttons,
                form_error,
                ..
            } => (selected_voice_type, voice_type_buttons, form_error),
            DialogView::List { .. } => return,
        };

        if *selected_voice_type == voice_type {
            return;
        }

        *selected_voice_type = voice_type;
        voice_type_buttons.set_selected(voice_type, cx);
        *form_error = None;
        cx.notify();
    }

    fn select_voice(&mut self, name: &VoiceName, cx: &mut Context<Self>) {
        let Some(voice) = find_voice(&self.voices, name) else {
            return;
        };
        if self.selected_voice.as_ref() == Some(&voice.name) {
            return;
        }

        self.selected_voice = Some(voice.name.clone());
        cx.notify();
    }

    fn suppress_add_new_hover(&self, cx: &mut Context<Self>) {
        if let DialogView::List { add_new_button, .. } = &self.view {
            add_new_button.update(cx, |button, cx| {
                button.suppress_hover_until_pointer_exit(cx);
            });
        }
    }

    pub fn voice_added(&mut self, voices: Vec<Voice>, added: VoiceName, cx: &mut Context<Self>) {
        self.voices = voices;
        self.selected_voice = Some(added);
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

    pub fn voice_edited(&mut self, voices: Vec<Voice>, edited: VoiceName, cx: &mut Context<Self>) {
        self.voices = voices;
        self.selected_voice = Some(edited);
        self.view = Self::list_view(cx);
        cx.notify();
    }

    pub fn edit_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::Edit { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn voice_deleted(
        &mut self,
        voices: Vec<Voice>,
        deleted: &VoiceName,
        cx: &mut Context<Self>,
    ) {
        let deleted_index = self
            .voices
            .iter()
            .position(|voice| voice.name.eq_ignore_ascii_case(deleted));
        self.voices = voices;
        self.selected_voice = deleted_index
            .and_then(|index| {
                self.voices
                    .get(index.min(self.voices.len().saturating_sub(1)))
            })
            .map(|voice| voice.name.clone());
        self.view = Self::list_view(cx);
        cx.notify();
    }

    pub fn delete_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let DialogView::Edit {
            delete_error,
            confirming_delete,
            ..
        } = &mut self.view
        {
            *delete_error = Some(error);
            *confirming_delete = false;
            cx.notify();
        }
    }
}

impl Render for VoicesDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            DialogView::List {
                add_new_button,
                edit_button,
            } => self.voice_list_dialog(add_new_button.clone(), edit_button.clone(), cx),
            DialogView::Add {
                name,
                voice_type_buttons,
                cancel_button,
                add_button,
                form_error,
                ..
            } => self.voice_form_dialog(
                "add voice",
                name.clone(),
                voice_type_buttons,
                form_error.clone(),
                div()
                    .flex()
                    .justify_end()
                    .gap_3()
                    .child(cancel_button.clone())
                    .child(add_button.clone()),
            ),
            DialogView::Edit {
                name,
                voice_type_buttons,
                cancel_button,
                save_button,
                delete_button,
                cancel_delete_button,
                confirm_delete_button,
                form_error,
                delete_error,
                confirming_delete,
                ..
            } => {
                let actions = div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(s::CONTENT_PADDING)
                    .child(self.delete_voice_actions(
                        delete_button.clone(),
                        cancel_delete_button.clone(),
                        confirm_delete_button.clone(),
                        *confirming_delete,
                        delete_error.clone(),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(cancel_button.clone())
                            .child(save_button.clone()),
                    );

                self.voice_form_dialog(
                    "edit voice",
                    name.clone(),
                    voice_type_buttons,
                    form_error.clone(),
                    actions,
                )
            }
        }
    }
}

impl VoicesDialog {
    fn voice_list_dialog(
        &self,
        add_new_button: Entity<Button>,
        edit_button: Entity<Button>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        list_detail_dialog(dialog::ListDetailArgs {
            title: "voices",
            close_button: self.close_button.clone(),
            list: voice_list(&self.voices, self.selected_voice.as_ref(), cx),
            details: voice_details(
                self.selected_voice
                    .as_ref()
                    .and_then(|name| find_voice(&self.voices, name)),
                edit_button,
            ),
            add_button: add_new_button,
        })
    }

    fn voice_form_dialog(
        &self,
        title: &'static str,
        name: Entity<TextInput>,
        voice_type_buttons: &VoiceTypeButtons,
        form_error: Option<String>,
        actions: gpui::Div,
    ) -> gpui::Div {
        let form = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(field_group("voice name", name))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(s::FIELD_LABEL_TEXT).child("voice type"))
                    .child(div().flex().gap_3().children(voice_type_buttons.entities())),
            );

        let form = if let Some(error) = form_error {
            form.child(error_message(error))
        } else {
            form
        };

        management_form_dialog(title, self.close_button.clone(), form, actions)
    }

    fn delete_voice_actions(
        &self,
        delete_button: Entity<Button>,
        cancel_delete_button: Entity<Button>,
        confirm_delete_button: Entity<Button>,
        confirming_delete: bool,
        delete_error: Option<String>,
    ) -> gpui::Div {
        let actions = if confirming_delete {
            destructive_confirmation(
                self.selected_voice
                    .as_ref()
                    .map(|name| format!("delete {:?}?", name.as_str()))
                    .unwrap_or_else(|| "delete this voice?".to_string()),
                div()
                    .flex()
                    .gap_3()
                    .child(cancel_delete_button)
                    .child(confirm_delete_button),
            )
        } else {
            div().flex().child(delete_button)
        };

        if let Some(error) = delete_error {
            actions.child(error_message(error))
        } else {
            actions
        }
    }
}

fn find_voice<'a>(voices: &'a [Voice], name: &VoiceName) -> Option<&'a Voice> {
    voices
        .iter()
        .find(|voice| voice.name.eq_ignore_ascii_case(name))
}

fn voice_list(
    voices: &[Voice],
    selected_voice: Option<&VoiceName>,
    cx: &mut Context<VoicesDialog>,
) -> gpui::Div {
    let rows = voices
        .iter()
        .enumerate()
        .map(|(index, voice)| voice_list_row(index, voice, selected_voice == Some(&voice.name), cx))
        .collect::<Vec<_>>();

    selection_list::list("no voices yet", rows)
}

fn voice_list_row(
    index: usize,
    voice: &Voice,
    selected: bool,
    cx: &mut Context<VoicesDialog>,
) -> gpui::Div {
    let voice_name = voice.name.clone();
    selection_list::row(index, selected, voice.name.as_str().to_owned()).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
            dialog.select_voice(&voice_name, cx);
        }),
    )
}

fn voice_details(voice: Option<&Voice>, edit_button: Entity<Button>) -> gpui::Div {
    let details = match voice {
        Some(voice) => div()
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
                            .child(voice.name.as_str().to_owned()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_color(s::TEXT_HEADER).child("voice type"))
                            .child(
                                div()
                                    .text_color(s::TEXT_DEFAULT)
                                    .child(voice.voice_type.label()),
                            ),
                    ),
            )
            .child(div().flex().child(edit_button)),
        None => div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(s::TEXT_DEFAULT)
            .child("add a voice to get started"),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .child(details)
}

struct VoiceTypeButtons {
    sin: Entity<Button>,
    saw: Entity<Button>,
}

impl VoiceTypeButtons {
    fn new(selected: VoiceType, cx: &mut Context<VoicesDialog>) -> Self {
        Self {
            sin: voice_type_button("voice-type-sin", VoiceType::Sin, selected, cx),
            saw: voice_type_button("voice-type-saw", VoiceType::Saw, selected, cx),
        }
    }

    fn entities(&self) -> Vec<Entity<Button>> {
        vec![self.sin.clone(), self.saw.clone()]
    }

    fn set_selected(&self, selected: VoiceType, cx: &mut Context<VoicesDialog>) {
        set_button_selected(&self.sin, selected == VoiceType::Sin, cx);
        set_button_selected(&self.saw, selected == VoiceType::Saw, cx);
    }
}

fn voice_type_button(
    id: &'static str,
    voice_type: VoiceType,
    selected: VoiceType,
    cx: &mut Context<VoicesDialog>,
) -> Entity<Button> {
    cx.new(|_| Button::new(id, voice_type.label()).depressed(voice_type == selected))
}

fn set_button_selected(button: &Entity<Button>, selected: bool, cx: &mut Context<VoicesDialog>) {
    button.update(cx, |button, cx| button.set_depressed(selected, cx));
}
