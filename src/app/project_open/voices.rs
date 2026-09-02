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
        text_input::{Changed, TextInput},
        workspace,
    },
    voice::{Voice, VoiceType, VoiceVolumeAdjustment},
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
        volume_adjustment: Option<VoiceVolumeAdjustment>,
    },
    Edit {
        original_name: VoiceName,
        name: String,
        voice_type: VoiceType,
        position: Point3Meters,
        volume_adjustment: Option<VoiceVolumeAdjustment>,
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
        voice_type_picker: VoiceTypePicker,
        position: PositionFields,
        volume_adjustment: Entity<TextInput>,
        cancel_button: Entity<Button>,
        add_button: Entity<Button>,
        form_error: Option<String>,
    },
    Edit {
        name: Entity<TextInput>,
        voice_type_picker: VoiceTypePicker,
        position: PositionFields,
        volume_adjustment: Entity<TextInput>,
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
        let voice_type_picker = VoiceTypePicker::new(VoiceType::Sin, cx);
        let position = PositionFields::new("add-voice", acoustic_scene.listener(), cx);
        let volume_adjustment = cx.new(|cx| TextInput::new("", "1.0", cx));
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let add_button = cx.new(|_| Button::new("confirm-add-voice", "add voice"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&add_button, Self::on_add_clicked).detach();

        View::Add {
            name,
            voice_type_picker,
            position,
            volume_adjustment,
            cancel_button,
            add_button,
            form_error: None,
        }
    }

    fn edit_view(voice: &Voice, cx: &mut Context<Self>) -> View {
        let voice_name = voice.name.as_str().to_owned();
        let name = cx.new(move |cx| TextInput::new(voice_name, "lead", cx));
        let voice_type_picker = VoiceTypePicker::new(voice.voice_type, cx);
        let position = PositionFields::new("edit-voice", voice.position(), cx);
        let saved_adjustment = voice
            .volume_adjustment()
            .map(|adjustment| adjustment.multiplier().to_string())
            .unwrap_or_default();
        let volume_adjustment = cx.new(move |cx| TextInput::new(saved_adjustment, "1.0", cx));
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let save_button = cx.new(|_| Button::new("save-voice", "save changes"));
        let delete_button = cx.new(|_| Button::new("delete-voice", "delete voice"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();

        View::Edit {
            name,
            voice_type_picker,
            position,
            volume_adjustment,
            cancel_button,
            save_button,
            delete_button,
            form_error: None,
        }
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
                voice_type_picker,
                position,
                volume_adjustment,
                ..
            } => position
                .position(&self.acoustic_scene, cx)
                .and_then(|position| {
                    parse_volume_adjustment(&volume_adjustment.read(cx).value()).map(|adjustment| {
                        (
                            name.read(cx).value(),
                            voice_type_picker.selected,
                            position,
                            adjustment,
                        )
                    })
                }),
            _ => return,
        };
        let (name, voice_type, position, volume_adjustment) = match request {
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
            volume_adjustment,
        }));
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(original_name) = self.selected_voice.clone() else {
            return;
        };
        let request = match &self.view {
            View::Edit {
                name,
                voice_type_picker,
                position,
                volume_adjustment,
                ..
            } => position
                .position(&self.acoustic_scene, cx)
                .and_then(|position| {
                    parse_volume_adjustment(&volume_adjustment.read(cx).value()).map(|adjustment| {
                        (
                            name.read(cx).value(),
                            voice_type_picker.selected,
                            position,
                            adjustment,
                        )
                    })
                }),
            _ => return,
        };
        let (name, voice_type, position, volume_adjustment) = match request {
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
            volume_adjustment,
        }));
    }

    fn on_voice_type_search_changed(
        &mut self,
        _: Entity<TextInput>,
        _: &Changed,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn select_voice_type(&mut self, voice_type: VoiceType, cx: &mut Context<Self>) {
        let (voice_type_picker, form_error) = match &mut self.view {
            View::Add {
                voice_type_picker,
                form_error,
                ..
            }
            | View::Edit {
                voice_type_picker,
                form_error,
                ..
            } => (voice_type_picker, form_error),
            View::List { .. } => return,
        };

        if voice_type_picker.selected == voice_type {
            return;
        }

        voice_type_picker.selected = voice_type;
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
                voice_type_picker,
                position,
                volume_adjustment,
                cancel_button,
                add_button,
                form_error,
                ..
            } => self.voice_form(
                name.clone(),
                voice_type_picker,
                position,
                volume_adjustment.clone(),
                form_error.clone(),
                button::action_group([cancel_button.clone(), add_button.clone()]).justify_end(),
                cx,
            ),
            View::Edit {
                name,
                voice_type_picker,
                position,
                volume_adjustment,
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
                    voice_type_picker,
                    position,
                    volume_adjustment.clone(),
                    form_error.clone(),
                    actions,
                    cx,
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
            list: voice_list(&self.voices, self.selected_voice.as_ref(), cx)
                .debug_selector(|| "voice-list-column".to_string()),
            details: voice_details(
                self.selected_voice
                    .as_ref()
                    .and_then(|name| find_voice(&self.voices, name)),
                edit_button,
            )
            .debug_selector(|| "voice-details-column".to_string()),
            auxiliary: None,
            footer: Some(add_new_button.into_any_element()),
        })
    }

    fn voice_form(
        &self,
        name: Entity<TextInput>,
        voice_type_picker: &VoiceTypePicker,
        position: &PositionFields,
        volume_adjustment: Entity<TextInput>,
        form_error: Option<String>,
        actions: gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let form = div()
            .id("voice-form-scroll")
            .debug_selector(|| "voice-form-scroll".to_string())
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .overflow_y_scroll()
            .gap_5()
            .child(field_group("voice name", name))
            .child(voice_type_picker.view(cx))
            .child(position.view(&self.acoustic_scene))
            .child(field_group(
                "volume adjustment (optional multiplier)",
                volume_adjustment,
            ));

        let form = if let Some(error) = form_error {
            form.child(error_message(error))
        } else {
            form
        };

        workspace::management_form(
            form,
            actions
                .flex_none()
                .debug_selector(|| "voice-form-actions".to_string()),
        )
    }
}

fn find_voice<'a>(voices: &'a [Voice], name: &VoiceName) -> Option<&'a Voice> {
    voices
        .iter()
        .find(|voice| voice.name.eq_ignore_ascii_case(name))
}

fn parse_volume_adjustment(value: &str) -> Result<Option<VoiceVolumeAdjustment>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    let multiplier = value
        .parse::<f64>()
        .map_err(|_| "voice volume adjustment must be a decimal number".to_string())?;
    VoiceVolumeAdjustment::new(multiplier)
        .map(Some)
        .map_err(|error| error.to_string())
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
        Some(voice) => {
            let voice_type_details = voice.details();
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
                        .child(detail_field("about", voice_type_details.description))
                        .child(detail_field("source", voice_type_details.source))
                        .child(detail_field("fidelity", voice_type_details.fidelity))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_color(s::TEXT_HEADER).child("volume adjustment"))
                                .child(div().text_color(s::TEXT_DEFAULT).child(
                                    voice.volume_adjustment().map_or_else(
                                        || "default (1×)".to_string(),
                                        |adjustment| format!("{}×", adjustment.multiplier()),
                                    ),
                                )),
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
                .child(div().flex().child(edit_button))
        }
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
        .min_w(s::S0)
        .min_h(px(0.0))
        .child(details)
}

fn detail_field(label: &'static str, value: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_color(s::TEXT_HEADER).child(label))
        .child(div().text_color(s::TEXT_DEFAULT).child(value))
}

struct VoiceTypePicker {
    search: Entity<TextInput>,
    selected: VoiceType,
}

impl VoiceTypePicker {
    fn new(selected: VoiceType, cx: &mut Context<VoicesWorkspace>) -> Self {
        let search = cx.new(|cx| TextInput::new("", "search voice types", cx));
        cx.subscribe(&search, VoicesWorkspace::on_voice_type_search_changed)
            .detach();
        Self { search, selected }
    }

    fn view(&self, cx: &mut Context<VoicesWorkspace>) -> gpui::Div {
        let query = self.search.read(cx).value();
        let rows = matching_voice_types(&query)
            .enumerate()
            .map(|(index, voice_type)| voice_type_row(index, voice_type, self.selected, cx))
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(s::S3)
            .child(div().text_color(s::FIELD_LABEL_TEXT).child("voice type"))
            .child(selection_list::searchable(
                "voice-type-list-scroll",
                self.search.clone(),
                "no voice types match",
                rows,
            ))
            .child(voice_type_summary(self.selected))
    }
}

fn voice_type_summary(voice_type: VoiceType) -> gpui::Div {
    let details = voice_type.details();
    div()
        .flex()
        .flex_col()
        .gap(s::S3)
        .child(detail_field("about", details.description))
        .child(detail_field("source", details.source))
        .child(detail_field("fidelity", details.fidelity))
}

fn matching_voice_types(query: &str) -> impl Iterator<Item = VoiceType> + '_ {
    let terms = query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    VoiceType::ALL.into_iter().filter(move |voice_type| {
        let label = voice_type.label().to_lowercase();
        let details = voice_type.details();
        let searchable = format!(
            "{label} {} {} {}",
            details.description, details.source, details.fidelity
        )
        .to_lowercase();
        terms.iter().all(|term| {
            if term.chars().count() == 1 {
                label.split_whitespace().any(|word| word == term)
            } else {
                searchable.contains(term)
            }
        })
    })
}

fn voice_type_row(
    index: usize,
    voice_type: VoiceType,
    selected: VoiceType,
    cx: &mut Context<VoicesWorkspace>,
) -> gpui::Div {
    selection_list::row(index, voice_type == selected, voice_type.label())
        .debug_selector(move || format!("voice-type-{}", voice_type.config_value()))
        .hover(|style| style.bg(s::GREEN5).text_color(s::TEXT_HOVERED))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |workspace, _: &MouseDownEvent, _: &mut Window, cx| {
                workspace.select_voice_type(voice_type, cx);
            }),
        )
}

#[cfg(test)]
mod tests {
    use gpui::{px, size, Modifiers, MouseButton, TestAppContext};

    use super::{matching_voice_types, parse_volume_adjustment, View, VoicesWorkspace};
    use crate::{
        acoustics::{AcousticScene, Point3Meters, RectangularRoom},
        voice::{Voice, VoiceType, VoiceVolumeAdjustment},
    };

    #[test]
    fn volume_adjustment_field_is_optional_and_validated() {
        assert_eq!(parse_volume_adjustment("  ").unwrap(), None);
        assert_eq!(
            parse_volume_adjustment("1.5").unwrap(),
            Some(VoiceVolumeAdjustment::new(1.5).unwrap())
        );
        assert!(parse_volume_adjustment("loud").is_err());
        assert!(parse_volume_adjustment("0").is_err());
        assert!(parse_volume_adjustment("-1").is_err());
    }

    #[test]
    fn voice_type_search_is_case_insensitive_and_matches_all_terms() {
        assert_eq!(
            matching_voice_types("XT guitar").collect::<Vec<_>>(),
            vec![VoiceType::SurgeXtDistortedElectricGuitar]
        );
        assert_eq!(
            matching_voice_types("saw").collect::<Vec<_>>(),
            vec![
                VoiceType::Saw,
                VoiceType::HarmonicSaw,
                VoiceType::CtpianoHiSaw,
                VoiceType::CtpianoLoSaw,
                VoiceType::RadlerDullSaw,
            ]
        );
        assert_eq!(
            matching_voice_types("bell a").collect::<Vec<_>>(),
            vec![VoiceType::NoitechBellA]
        );
        assert_eq!(
            matching_voice_types("bell b").collect::<Vec<_>>(),
            vec![VoiceType::NoitechBellB]
        );
        assert_eq!(
            matching_voice_types("home_clap_1.wav").collect::<Vec<_>>(),
            vec![VoiceType::NoitechBellL, VoiceType::NoitechBellM]
        );
        assert!(matching_voice_types("pipe organ").next().is_none());
    }

    #[gpui::test]
    fn searchable_voice_type_list_filters_and_selects_a_row(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            VoicesWorkspace::new(Vec::new(), AcousticScene::default(), cx)
        });
        cx.simulate_resize(size(px(800.0), px(800.0)));

        dialog.update(cx, |dialog, cx| {
            dialog.view = VoicesWorkspace::add_view(&dialog.acoustic_scene, cx);
            let View::Add {
                voice_type_picker, ..
            } = &dialog.view
            else {
                panic!("add view must contain a voice type picker");
            };
            voice_type_picker
                .search
                .update(cx, |search, cx| search.sync_value("clarinet", cx));
            cx.notify();
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("voice-type-surge-xt-clarinet").is_some());
        assert!(cx.debug_bounds("voice-type-sin").is_none());

        let clarinet = cx.debug_bounds("voice-type-surge-xt-clarinet").unwrap();
        cx.simulate_mouse_down(clarinet.center(), MouseButton::Left, Modifiers::default());

        dialog.read_with(cx, |dialog, _| {
            let View::Add {
                voice_type_picker, ..
            } = &dialog.view
            else {
                panic!("voice type selection must keep the add view open");
            };
            assert_eq!(voice_type_picker.selected, VoiceType::SurgeXtClarinet);
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

    #[gpui::test]
    fn edit_voice_volume_starts_at_the_saved_adjustment(cx: &mut TestAppContext) {
        let scene = AcousticScene::default();
        let voice = Voice::new(1, "lead", VoiceType::Saw)
            .with_volume_adjustment(Some(VoiceVolumeAdjustment::new(1.5).unwrap()));
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| VoicesWorkspace::new(vec![voice], scene, cx));

        dialog.update(cx, |dialog, cx| {
            let voice = dialog.voices[0].clone();
            dialog.view = VoicesWorkspace::edit_view(&voice, cx);
        });

        let value = cx.update(|_, cx| {
            let View::Edit {
                volume_adjustment, ..
            } = &dialog.read(cx).view
            else {
                panic!("edit button must show the edit form");
            };
            volume_adjustment.read(cx).value()
        });
        assert_eq!(value, "1.5");
    }

    #[gpui::test]
    fn edit_voice_actions_remain_visible_when_the_form_is_taller_than_the_window(
        cx: &mut TestAppContext,
    ) {
        let scene = AcousticScene::default();
        let voice = Voice::new(1, "lead", VoiceType::NoitechBellG);
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| VoicesWorkspace::new(vec![voice], scene, cx));
        cx.simulate_resize(size(px(800.0), px(520.0)));

        dialog.update(cx, |dialog, cx| {
            let voice = dialog.voices[0].clone();
            dialog.view = VoicesWorkspace::edit_view(&voice, cx);
            cx.notify();
        });
        cx.run_until_parked();

        let scroll = cx.debug_bounds("voice-form-scroll").unwrap();
        let actions = cx.debug_bounds("voice-form-actions").unwrap();
        assert!(scroll.bottom() <= actions.top());
        assert!(actions.bottom() <= px(520.0));
    }

    #[gpui::test]
    fn voice_list_and_details_keep_equal_widths_across_selections(cx: &mut TestAppContext) {
        let voices = vec![
            Voice::new(1, "short details", VoiceType::Sin),
            Voice::new(2, "long details", VoiceType::GamelanMetallophone),
        ];
        let long_details_name = voices[1].name.clone();
        let (workspace, cx) = cx.add_window_view(move |_, cx| {
            VoicesWorkspace::new(voices, AcousticScene::default(), cx)
        });
        cx.simulate_resize(size(px(800.0), px(700.0)));
        cx.run_until_parked();

        let short_list = cx.debug_bounds("voice-list-column").unwrap();
        let short_details = cx.debug_bounds("voice-details-column").unwrap();
        assert_eq!(short_list.size.width, short_details.size.width);

        workspace.update(cx, |workspace, cx| {
            workspace.select_voice(&long_details_name, cx);
        });
        cx.run_until_parked();

        let long_list = cx.debug_bounds("voice-list-column").unwrap();
        let long_details = cx.debug_bounds("voice-details-column").unwrap();
        assert_eq!(long_list.size.width, long_details.size.width);
        assert_eq!(long_list.size.width, short_list.size.width);
        assert_eq!(long_details.size.width, short_details.size.width);
    }
}
