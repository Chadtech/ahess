use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, MouseButton, MouseDownEvent,
    Window,
};

use crate::{
    acoustics::{AcousticScene, Point3Meters},
    app::position_form::PositionFields,
    style as s,
    view::{
        button::{self, Button},
        dialog::{destructive_dialog, error_message},
        field_group::field_group,
        selection_list,
        text_input::TextInput,
        workspace,
    },
    voice::{Voice, VoiceType},
    voice_name::VoiceName,
};

pub enum Msg {
    Change(Change),
    DeleteRequested { name: VoiceName },
}

pub enum Change {
    Add {
        name: String,
        voice_type: VoiceType,
        position: Point3Meters,
    },
    Edit {
        original_name: VoiceName,
        name: String,
        voice_type: VoiceType,
        position: Point3Meters,
    },
}

pub(super) enum Overlay {
    ConfirmDelete(Entity<DeleteDialog>),
}

impl Overlay {
    pub(super) fn element(&self) -> AnyElement {
        match self {
            Self::ConfirmDelete(dialog) => dialog.clone().into_any_element(),
        }
    }
}

pub(super) enum DeleteDialogMsg {
    Cancelled,
    Confirmed { name: VoiceName },
}

pub(super) struct DeleteDialog {
    name: VoiceName,
    cancel_button: Entity<Button>,
    confirm_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<DeleteDialogMsg> for DeleteDialog {}

impl DeleteDialog {
    pub(super) fn new(name: VoiceName, cx: &mut Context<Self>) -> Self {
        let cancel_button = cx.new(|_| Button::new("cancel-delete-voice", "keep voice"));
        let confirm_button = cx.new(|_| Button::new("confirm-delete-voice", "delete voice"));
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&confirm_button, Self::on_confirm_clicked)
            .detach();
        Self {
            name,
            cancel_button,
            confirm_button,
            error: None,
        }
    }

    pub(super) fn failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DeleteDialogMsg::Cancelled);
    }

    fn on_confirm_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DeleteDialogMsg::Confirmed {
            name: self.name.clone(),
        });
    }
}

impl Render for DeleteDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let actions = div()
            .flex()
            .flex_col()
            .gap(s::S3)
            .children(self.error.clone().map(error_message))
            .child(
                button::action_group([self.cancel_button.clone(), self.confirm_button.clone()])
                    .justify_end(),
            );
        destructive_dialog(
            "delete voice",
            None,
            format!("delete {:?}?", self.name.as_str()),
            actions,
        )
    }
}

enum View {
    List {
        add_new_button: Entity<Button>,
        edit_button: Entity<Button>,
    },
    Add {
        name: Entity<TextInput>,
        selected_voice_type: VoiceType,
        voice_type_buttons: VoiceTypeButtons,
        position: PositionFields,
        cancel_button: Entity<Button>,
        add_button: Entity<Button>,
        form_error: Option<String>,
    },
    Edit {
        name: Entity<TextInput>,
        selected_voice_type: VoiceType,
        voice_type_buttons: VoiceTypeButtons,
        position: PositionFields,
        cancel_button: Entity<Button>,
        save_button: Entity<Button>,
        delete_button: Entity<Button>,
        form_error: Option<String>,
    },
}

pub struct VoicesWorkspace {
    voices: Vec<Voice>,
    acoustic_scene: AcousticScene,
    selected_voice: Option<VoiceName>,
    view: View,
}

impl EventEmitter<Msg> for VoicesWorkspace {}

impl VoicesWorkspace {
    pub fn new(voices: Vec<Voice>, acoustic_scene: AcousticScene, cx: &mut Context<Self>) -> Self {
        let selected_voice = voices.first().map(|voice| voice.name.clone());

        Self {
            voices,
            acoustic_scene,
            selected_voice,
            view: Self::list_view(cx),
        }
    }

    pub fn has_draft(&self) -> bool {
        match &self.view {
            View::List { .. } => false,
            View::Add { .. } | View::Edit { .. } => true,
        }
    }

    fn list_view(cx: &mut Context<Self>) -> View {
        let add_new_button = cx.new(|_| Button::new("add-new-voice", "add new voice"));
        let edit_button = cx.new(|_| Button::new("edit-voice", "edit voice"));

        cx.subscribe(&add_new_button, Self::on_add_new_clicked)
            .detach();
        cx.subscribe(&edit_button, Self::on_edit_clicked).detach();

        View::List {
            add_new_button,
            edit_button,
        }
    }

    fn add_view(acoustic_scene: &AcousticScene, cx: &mut Context<Self>) -> View {
        let name = cx.new(|cx| TextInput::new("", "lead", cx));
        let selected_voice_type = VoiceType::Sin;
        let voice_type_buttons = VoiceTypeButtons::new(selected_voice_type, cx);
        let position = PositionFields::new("add-voice", acoustic_scene.listener(), cx);
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let add_button = cx.new(|_| Button::new("confirm-add-voice", "add voice"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&add_button, Self::on_add_clicked).detach();
        Self::subscribe_voice_type_buttons(&voice_type_buttons, cx);

        View::Add {
            name,
            selected_voice_type,
            voice_type_buttons,
            position,
            cancel_button,
            add_button,
            form_error: None,
        }
    }

    fn edit_view(voice: &Voice, cx: &mut Context<Self>) -> View {
        let voice_name = voice.name.as_str().to_owned();
        let name = cx.new(move |cx| TextInput::new(voice_name, "lead", cx));
        let selected_voice_type = voice.voice_type;
        let voice_type_buttons = VoiceTypeButtons::new(selected_voice_type, cx);
        let position = PositionFields::new("edit-voice", voice.position(), cx);
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let save_button = cx.new(|_| Button::new("save-voice", "save changes"));
        let delete_button = cx.new(|_| Button::new("delete-voice", "delete voice"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();
        Self::subscribe_voice_type_buttons(&voice_type_buttons, cx);

        View::Edit {
            name,
            selected_voice_type,
            voice_type_buttons,
            position,
            cancel_button,
            save_button,
            delete_button,
            form_error: None,
        }
    }

    fn subscribe_voice_type_buttons(buttons: &VoiceTypeButtons, cx: &mut Context<Self>) {
        cx.subscribe(&buttons.sin, Self::on_sin_clicked).detach();
        cx.subscribe(&buttons.saw, Self::on_saw_clicked).detach();
        cx.subscribe(&buttons.harmonic_saw, Self::on_harmonic_saw_clicked)
            .detach();
        cx.subscribe(&buttons.surge_xt_piano, Self::on_surge_xt_piano_clicked)
            .detach();
        cx.subscribe(
            &buttons.surge_xt_distorted_guitar,
            Self::on_surge_xt_distorted_guitar_clicked,
        )
        .detach();
    }

    fn on_add_new_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::add_view(&self.acoustic_scene, cx);
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
        let Some(name) = self.selected_voice.clone() else {
            return;
        };

        cx.emit(Msg::DeleteRequested { name });
    }

    fn on_add_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let request = match &self.view {
            View::Add {
                name,
                selected_voice_type,
                position,
                ..
            } => position
                .position(&self.acoustic_scene, cx)
                .map(|position| (name.read(cx).value(), *selected_voice_type, position)),
            _ => return,
        };
        let (name, voice_type, position) = match request {
            Ok(request) => request,
            Err(error) => {
                if let View::Add { form_error, .. } = &mut self.view {
                    *form_error = Some(error);
                    cx.notify();
                }
                return;
            }
        };
        cx.emit(Msg::Change(Change::Add {
            name,
            voice_type,
            position,
        }));
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(original_name) = self.selected_voice.clone() else {
            return;
        };
        let request = match &self.view {
            View::Edit {
                name,
                selected_voice_type,
                position,
                ..
            } => position
                .position(&self.acoustic_scene, cx)
                .map(|position| (name.read(cx).value(), *selected_voice_type, position)),
            _ => return,
        };
        let (name, voice_type, position) = match request {
            Ok(request) => request,
            Err(error) => {
                if let View::Edit { form_error, .. } = &mut self.view {
                    *form_error = Some(error);
                    cx.notify();
                }
                return;
            }
        };
        cx.emit(Msg::Change(Change::Edit {
            original_name,
            name,
            voice_type,
            position,
        }));
    }

    fn on_sin_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.select_voice_type(VoiceType::Sin, cx);
    }

    fn on_saw_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        self.select_voice_type(VoiceType::Saw, cx);
    }

    fn on_harmonic_saw_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.select_voice_type(VoiceType::HarmonicSaw, cx);
    }

    fn on_surge_xt_piano_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.select_voice_type(VoiceType::SurgeXtPiano, cx);
    }

    fn on_surge_xt_distorted_guitar_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.select_voice_type(VoiceType::SurgeXtDistortedElectricGuitar, cx);
    }

    fn select_voice_type(&mut self, voice_type: VoiceType, cx: &mut Context<Self>) {
        let (selected_voice_type, voice_type_buttons, form_error) = match &mut self.view {
            View::Add {
                selected_voice_type,
                voice_type_buttons,
                form_error,
                ..
            }
            | View::Edit {
                selected_voice_type,
                voice_type_buttons,
                form_error,
                ..
            } => (selected_voice_type, voice_type_buttons, form_error),
            View::List { .. } => return,
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
        if let View::List { add_new_button, .. } = &self.view {
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
        if let View::Add { form_error, .. } = &mut self.view {
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
        if let View::Edit { form_error, .. } = &mut self.view {
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

    pub fn sync_project(
        &mut self,
        voices: Vec<Voice>,
        acoustic_scene: AcousticScene,
        cx: &mut Context<Self>,
    ) {
        self.voices = voices;
        self.acoustic_scene = acoustic_scene;
        if self
            .selected_voice
            .as_ref()
            .is_none_or(|selected| find_voice(&self.voices, selected).is_none())
        {
            self.selected_voice = self.voices.first().map(|voice| voice.name.clone());
        }
        cx.notify();
    }
}

impl Render for VoicesWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            View::List {
                add_new_button,
                edit_button,
            } => self.voice_list(add_new_button.clone(), edit_button.clone(), cx),
            View::Add {
                name,
                voice_type_buttons,
                position,
                cancel_button,
                add_button,
                form_error,
                ..
            } => self.voice_form(
                name.clone(),
                voice_type_buttons,
                position,
                form_error.clone(),
                button::action_group([cancel_button.clone(), add_button.clone()]).justify_end(),
            ),
            View::Edit {
                name,
                voice_type_buttons,
                position,
                cancel_button,
                save_button,
                delete_button,
                form_error,
                ..
            } => {
                let actions = div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(s::CONTENT_PADDING)
                    .child(div().flex().child(delete_button.clone()))
                    .child(button::action_group([
                        cancel_button.clone(),
                        save_button.clone(),
                    ]));

                self.voice_form(
                    name.clone(),
                    voice_type_buttons,
                    position,
                    form_error.clone(),
                    actions,
                )
            }
        }
    }
}

impl VoicesWorkspace {
    fn voice_list(
        &self,
        add_new_button: Entity<Button>,
        edit_button: Entity<Button>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        workspace::list_detail(workspace::ListDetailArgs {
            list: voice_list(&self.voices, self.selected_voice.as_ref(), cx),
            details: voice_details(
                self.selected_voice
                    .as_ref()
                    .and_then(|name| find_voice(&self.voices, name)),
                edit_button,
            ),
            auxiliary: None,
            footer: Some(add_new_button.into_any_element()),
        })
    }

    fn voice_form(
        &self,
        name: Entity<TextInput>,
        voice_type_buttons: &VoiceTypeButtons,
        position: &PositionFields,
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
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_3()
                            .children(voice_type_buttons.entities()),
                    ),
            )
            .child(position.view(&self.acoustic_scene));

        let form = if let Some(error) = form_error {
            form.child(error_message(error))
        } else {
            form
        };

        workspace::management_form(form, actions)
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
    cx: &mut Context<VoicesWorkspace>,
) -> gpui::Div {
    let rows = voices
        .iter()
        .enumerate()
        .map(|(index, voice)| voice_list_row(index, voice, selected_voice == Some(&voice.name), cx))
        .collect::<Vec<_>>();

    selection_list::list("voices-list-scroll", "no voices yet", rows)
}

fn voice_list_row(
    index: usize,
    voice: &Voice,
    selected: bool,
    cx: &mut Context<VoicesWorkspace>,
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
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_color(s::TEXT_HEADER).child("position"))
                            .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                                "X {}, Y {}, Z {} meters",
                                voice.position().x(),
                                voice.position().y(),
                                voice.position().z(),
                            ))),
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
    harmonic_saw: Entity<Button>,
    surge_xt_piano: Entity<Button>,
    surge_xt_distorted_guitar: Entity<Button>,
}

impl VoiceTypeButtons {
    fn new(selected: VoiceType, cx: &mut Context<VoicesWorkspace>) -> Self {
        Self {
            sin: voice_type_button("voice-type-sin", VoiceType::Sin, selected, cx),
            saw: voice_type_button("voice-type-saw", VoiceType::Saw, selected, cx),
            harmonic_saw: voice_type_button(
                "voice-type-harmonic-saw",
                VoiceType::HarmonicSaw,
                selected,
                cx,
            ),
            surge_xt_piano: voice_type_button(
                "voice-type-surge-xt-piano",
                VoiceType::SurgeXtPiano,
                selected,
                cx,
            ),
            surge_xt_distorted_guitar: voice_type_button(
                "voice-type-surge-xt-distorted-guitar",
                VoiceType::SurgeXtDistortedElectricGuitar,
                selected,
                cx,
            ),
        }
    }

    fn entities(&self) -> Vec<Entity<Button>> {
        vec![
            self.sin.clone(),
            self.saw.clone(),
            self.harmonic_saw.clone(),
            self.surge_xt_piano.clone(),
            self.surge_xt_distorted_guitar.clone(),
        ]
    }

    fn set_selected(&self, selected: VoiceType, cx: &mut Context<VoicesWorkspace>) {
        set_button_selected(&self.sin, selected == VoiceType::Sin, cx);
        set_button_selected(&self.saw, selected == VoiceType::Saw, cx);
        set_button_selected(&self.harmonic_saw, selected == VoiceType::HarmonicSaw, cx);
        set_button_selected(
            &self.surge_xt_piano,
            selected == VoiceType::SurgeXtPiano,
            cx,
        );
        set_button_selected(
            &self.surge_xt_distorted_guitar,
            selected == VoiceType::SurgeXtDistortedElectricGuitar,
            cx,
        );
    }
}

fn voice_type_button(
    id: &'static str,
    voice_type: VoiceType,
    selected: VoiceType,
    cx: &mut Context<VoicesWorkspace>,
) -> Entity<Button> {
    cx.new(|_| Button::new(id, voice_type.label()).depressed(voice_type == selected))
}

fn set_button_selected(button: &Entity<Button>, selected: bool, cx: &mut Context<VoicesWorkspace>) {
    button.update(cx, |button, cx| button.set_depressed(selected, cx));
}

#[cfg(test)]
mod tests {
    use gpui::{px, size, TestAppContext};

    use super::{View, VoicesWorkspace};
    use crate::{
        acoustics::{AcousticScene, Point3Meters, RectangularRoom},
        view::button,
        voice::{Voice, VoiceType},
    };

    #[gpui::test]
    fn harmonic_saw_can_be_selected_for_a_new_voice(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            VoicesWorkspace::new(Vec::new(), AcousticScene::default(), cx)
        });

        dialog.update(cx, |dialog, cx| {
            dialog.view = VoicesWorkspace::add_view(&dialog.acoustic_scene, cx);
            let harmonic_saw_button = match &dialog.view {
                View::Add {
                    voice_type_buttons, ..
                } => voice_type_buttons.harmonic_saw.clone(),
                _ => panic!("add view must contain voice type buttons"),
            };

            dialog.on_harmonic_saw_clicked(harmonic_saw_button, &button::Clicked, cx);

            let View::Add {
                selected_voice_type,
                ..
            } = &dialog.view
            else {
                panic!("voice type selection must keep the add view open");
            };
            assert_eq!(*selected_voice_type, VoiceType::HarmonicSaw);
        });
    }

    #[gpui::test]
    fn surge_xt_piano_can_be_selected_for_a_new_voice(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            VoicesWorkspace::new(Vec::new(), AcousticScene::default(), cx)
        });

        dialog.update(cx, |dialog, cx| {
            dialog.view = VoicesWorkspace::add_view(&dialog.acoustic_scene, cx);
            let surge_xt_piano_button = match &dialog.view {
                View::Add {
                    voice_type_buttons, ..
                } => voice_type_buttons.surge_xt_piano.clone(),
                _ => panic!("add view must contain voice type buttons"),
            };

            dialog.on_surge_xt_piano_clicked(surge_xt_piano_button, &button::Clicked, cx);

            let View::Add {
                selected_voice_type,
                ..
            } = &dialog.view
            else {
                panic!("voice type selection must keep the add view open");
            };
            assert_eq!(*selected_voice_type, VoiceType::SurgeXtPiano);
        });
    }

    #[gpui::test]
    fn surge_xt_distorted_guitar_can_be_selected_for_a_new_voice(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            VoicesWorkspace::new(Vec::new(), AcousticScene::default(), cx)
        });

        dialog.update(cx, |dialog, cx| {
            dialog.view = VoicesWorkspace::add_view(&dialog.acoustic_scene, cx);
            let guitar_button = match &dialog.view {
                View::Add {
                    voice_type_buttons, ..
                } => voice_type_buttons.surge_xt_distorted_guitar.clone(),
                _ => panic!("add view must contain voice type buttons"),
            };

            dialog.on_surge_xt_distorted_guitar_clicked(guitar_button, &button::Clicked, cx);

            let View::Add {
                selected_voice_type,
                ..
            } = &dialog.view
            else {
                panic!("voice type selection must keep the add view open");
            };
            assert_eq!(
                *selected_voice_type,
                VoiceType::SurgeXtDistortedElectricGuitar
            );
        });
    }

    #[gpui::test]
    fn add_voice_position_starts_at_the_listener(cx: &mut TestAppContext) {
        let room = RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap();
        let scene = AcousticScene::new(room.center(), Some(room)).unwrap();
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| VoicesWorkspace::new(Vec::new(), scene, cx));
        cx.simulate_resize(size(px(800.0), px(800.0)));
        cx.run_until_parked();

        dialog.update(cx, |dialog, cx| {
            dialog.view = VoicesWorkspace::add_view(&dialog.acoustic_scene, cx);
            cx.notify();
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("add-voice-position-x").is_some());
        assert!(cx.debug_bounds("add-voice-position-y").is_some());
        assert!(cx.debug_bounds("add-voice-position-z").is_some());
        let position = cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            let View::Add { position, .. } = &dialog.view else {
                panic!("add button must show the add form");
            };
            position.position(&dialog.acoustic_scene, cx).unwrap()
        });
        assert_eq!(position, Point3Meters::new(4.0, 5.0, 1.5).unwrap());
    }

    #[gpui::test]
    fn edit_voice_position_starts_at_the_saved_position(cx: &mut TestAppContext) {
        let scene = AcousticScene::default();
        let saved_position = Point3Meters::new(-2.0, 4.5, 1.0).unwrap();
        let voice = Voice::new(1, "lead", VoiceType::Saw).with_position(saved_position);
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| VoicesWorkspace::new(vec![voice], scene, cx));
        cx.simulate_resize(size(px(800.0), px(800.0)));
        cx.run_until_parked();

        dialog.update(cx, |dialog, cx| {
            let voice = dialog.voices[0].clone();
            dialog.view = VoicesWorkspace::edit_view(&voice, cx);
            cx.notify();
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("edit-voice-position-x").is_some());
        assert!(cx.debug_bounds("edit-voice-position-y").is_some());
        assert!(cx.debug_bounds("edit-voice-position-z").is_some());
        let position = cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            let View::Edit { position, .. } = &dialog.view else {
                panic!("edit button must show the edit form");
            };
            position.position(&dialog.acoustic_scene, cx).unwrap()
        });
        assert_eq!(position, saved_position);
    }
}
