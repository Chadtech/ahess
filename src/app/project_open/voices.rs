use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, Context, CursorStyle, Entity, EventEmitter, MouseButton, MouseDownEvent,
    Pixels, Window,
};

use crate::{
    project::{self, Project, Voice, VoiceType},
    style as s,
    view::{
        button::{self, Button},
        dialog::{destructive_confirmation, error_message, title_bar},
        field_group::field_group,
        text_input::TextInput,
    },
    voice_name::VoiceName,
};

pub enum Event {
    Updated(Project),
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

const DIALOG_WIDTH: Pixels = px(800.0);
const DIALOG_HEIGHT: Pixels = px(500.0);

pub struct VoicesDialog {
    original_project: Project,
    project_directory: PathBuf,
    selected_voice: Option<VoiceName>,
    view: DialogView,
    close_button: Entity<Button>,
}

impl EventEmitter<Event> for VoicesDialog {}

impl VoicesDialog {
    pub fn new(project: Project, project_directory: PathBuf, cx: &mut Context<Self>) -> Self {
        let selected_voice = project.voices.first().map(|voice| voice.name.clone());
        let close_button = cx.new(|_| Button::x("close-voices"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();

        Self {
            original_project: project,
            project_directory,
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

    fn on_edit_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(voice) = self
            .selected_voice
            .as_ref()
            .and_then(|name| self.original_project.voice(name))
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
        let Some(name) = self.selected_voice.as_ref() else {
            return;
        };
        let Some(index) = self
            .original_project
            .voices
            .iter()
            .position(|voice| &voice.name == name)
        else {
            return;
        };
        let Some(project) = delete_voice_from_project(&self.original_project, name) else {
            return;
        };

        match project::save_project(&self.project_directory, &project) {
            Ok(()) => {
                self.selected_voice = project
                    .voices
                    .get(index.min(project.voices.len().saturating_sub(1)))
                    .map(|voice| voice.name.clone());
                self.original_project = project.clone();
                self.view = Self::list_view(cx);
                cx.emit(Event::Updated(project));
                cx.notify();
            }
            Err(error) => {
                if let DialogView::Edit {
                    delete_error,
                    confirming_delete,
                    ..
                } = &mut self.view
                {
                    *delete_error = Some(error.to_string());
                    *confirming_delete = false;
                    cx.notify();
                }
            }
        }
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
        let result = add_voice_to_project(
            &self.original_project,
            &name.read(cx).value(),
            *selected_voice_type,
        )
        .and_then(|project| {
            project::save_project(&self.project_directory, &project)?;
            Ok(project)
        });

        match result {
            Ok(project) => {
                self.selected_voice = project.voices.last().map(|voice| voice.name.clone());
                self.original_project = project.clone();
                self.view = Self::list_view(cx);
                self.suppress_add_new_hover(cx);
                cx.emit(Event::Updated(project));
                cx.notify();
            }
            Err(error) => {
                if let DialogView::Add { form_error, .. } = &mut self.view {
                    *form_error = Some(error.to_string());
                    cx.notify();
                }
            }
        }
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
        let result = edit_voice_in_project(
            &self.original_project,
            &original_name,
            &name.read(cx).value(),
            *selected_voice_type,
        )
        .and_then(|project| {
            project::save_project(&self.project_directory, &project)?;
            Ok(project)
        });

        match result {
            Ok(project) => {
                let edited_index = self
                    .original_project
                    .voices
                    .iter()
                    .position(|voice| voice.name.eq_ignore_ascii_case(&original_name));
                self.selected_voice = edited_index
                    .and_then(|index| project.voices.get(index))
                    .map(|voice| voice.name.clone());
                self.original_project = project.clone();
                self.view = Self::list_view(cx);
                cx.emit(Event::Updated(project));
                cx.notify();
            }
            Err(error) => {
                if let DialogView::Edit { form_error, .. } = &mut self.view {
                    *form_error = Some(error.to_string());
                    cx.notify();
                }
            }
        }
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
        let Some(voice) = self.original_project.voice(name) else {
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
        s::raised(
            div()
                .flex()
                .flex_col()
                .w(DIALOG_WIDTH)
                .h(DIALOG_HEIGHT)
                .bg(s::GRAY2)
                .child(title_bar("voices", Some(self.close_button.clone())))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h(px(0.0))
                        .gap(s::CONTENT_PADDING)
                        .p(s::CONTENT_PADDING)
                        .child(voice_list(
                            &self.original_project.voices,
                            self.selected_voice.as_ref(),
                            cx,
                        ))
                        .child(voice_details(
                            self.selected_voice
                                .as_ref()
                                .and_then(|name| self.original_project.voice(name)),
                            edit_button,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .p(s::CONTENT_PADDING)
                        .pt(s::S0)
                        .child(add_new_button),
                ),
        )
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

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(DIALOG_WIDTH)
                .h(DIALOG_HEIGHT)
                .bg(s::GRAY2)
                .child(title_bar(title, Some(self.close_button.clone())))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .justify_between()
                        .gap(s::CONTENT_PADDING)
                        .p(s::CONTENT_PADDING)
                        .child(form)
                        .child(actions),
                ),
        )
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

    let list_body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .bg(s::GREEN3);
    let list_body = if rows.is_empty() {
        list_body.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(s::TEXT_DEFAULT)
                .child("no voices yet"),
        )
    } else {
        list_body.children(rows)
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .w(s::S9)
        .child(s::sunken(list_body).flex().flex_1().overflow_hidden())
}

fn voice_list_row(
    index: usize,
    voice: &Voice,
    selected: bool,
    cx: &mut Context<VoicesDialog>,
) -> gpui::Div {
    let voice_name = voice.name.clone();
    let background = if selected {
        s::GREEN4
    } else if index.is_multiple_of(2) {
        s::GREEN2
    } else {
        s::GREEN3
    };
    let name_color = if selected { s::GRAY6 } else { s::GRAY5 };

    div()
        .bg(background)
        .p(s::S4)
        .text_color(name_color)
        .cursor(CursorStyle::PointingHand)
        .child(voice.name.as_str().to_owned())
        .on_mouse_down(
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

#[derive(Debug)]
enum VoiceFormError {
    InvalidField(String),
    SaveProject(project::SaveProjectError),
}

impl std::fmt::Display for VoiceFormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidField(message) => write!(f, "{message}"),
            Self::SaveProject(error) => write!(f, "{error}"),
        }
    }
}

impl From<project::SaveProjectError> for VoiceFormError {
    fn from(error: project::SaveProjectError) -> Self {
        Self::SaveProject(error)
    }
}

fn add_voice_to_project(
    project: &Project,
    name: &str,
    voice_type: VoiceType,
) -> Result<Project, VoiceFormError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(VoiceFormError::InvalidField(
            "voice name must not be empty".to_string(),
        ));
    }

    let name = VoiceName::new(name);
    if project.voice(&name).is_some() {
        return Err(VoiceFormError::InvalidField(format!(
            "a voice named {:?} already exists",
            name.as_str()
        )));
    }

    let mut updated_project = project.clone();
    updated_project.add_voice(Voice::new(name, voice_type));
    Ok(updated_project)
}

fn edit_voice_in_project(
    project: &Project,
    original_name: &VoiceName,
    name: &str,
    voice_type: VoiceType,
) -> Result<Project, VoiceFormError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(VoiceFormError::InvalidField(
            "voice name must not be empty".to_string(),
        ));
    }

    let Some(index) = project
        .voices
        .iter()
        .position(|voice| voice.name.eq_ignore_ascii_case(original_name))
    else {
        return Err(VoiceFormError::InvalidField(
            "the voice no longer exists".to_string(),
        ));
    };
    let name = VoiceName::new(name);
    if project
        .voices
        .iter()
        .enumerate()
        .any(|(other_index, voice)| other_index != index && voice.name.eq_ignore_ascii_case(&name))
    {
        return Err(VoiceFormError::InvalidField(format!(
            "a voice named {:?} already exists",
            name.as_str()
        )));
    }

    let mut updated_project = project.clone();
    updated_project.voices[index] = Voice::new(name, voice_type);
    Ok(updated_project)
}

fn delete_voice_from_project(project: &Project, name: &VoiceName) -> Option<Project> {
    let mut updated_project = project.clone();
    updated_project.remove_voice(name)?;
    Some(updated_project)
}

#[cfg(test)]
mod tests {
    use super::{add_voice_to_project, delete_voice_from_project, edit_voice_in_project};
    use crate::{
        project::{Project, Voice, VoiceType},
        seed::Seed,
        voice_name::VoiceName,
    };

    #[test]
    fn adds_voices_in_column_order() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project = add_voice_to_project(&project, " lead ", VoiceType::Saw).unwrap();
        let project = add_voice_to_project(&project, "bass", VoiceType::Sin).unwrap();

        assert_eq!(project.voices[0].name.as_str(), "lead");
        assert_eq!(project.voices[0].voice_type, VoiceType::Saw);
        assert_eq!(project.voices[1].name.as_str(), "bass");
        assert_eq!(project.voices[1].voice_type, VoiceType::Sin);
    }

    #[test]
    fn voice_names_must_be_present_and_unique() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project = add_voice_to_project(&project, "lead", VoiceType::Sin).unwrap();

        assert!(add_voice_to_project(&project, " ", VoiceType::Saw).is_err());
        assert!(add_voice_to_project(&project, "LEAD", VoiceType::Saw).is_err());
    }

    #[test]
    fn edits_a_voice_without_changing_its_column_order() {
        let project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![
            Voice::new("lead", VoiceType::Saw),
            Voice::new("bass", VoiceType::Sin),
        ]);

        let project = edit_voice_in_project(
            &project,
            &VoiceName::new("LEAD"),
            " melody ",
            VoiceType::Sin,
        )
        .unwrap();

        assert_eq!(
            project.voices,
            vec![
                Voice::new("melody", VoiceType::Sin),
                Voice::new("bass", VoiceType::Sin),
            ]
        );
    }

    #[test]
    fn edited_voice_names_must_be_present_and_unique() {
        let project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![
            Voice::new("lead", VoiceType::Saw),
            Voice::new("bass", VoiceType::Sin),
        ]);

        assert!(
            edit_voice_in_project(&project, &VoiceName::new("lead"), " ", VoiceType::Sin,).is_err()
        );
        assert!(
            edit_voice_in_project(&project, &VoiceName::new("lead"), "BASS", VoiceType::Sin,)
                .is_err()
        );
    }

    #[test]
    fn deletes_the_voice_with_the_selected_name() {
        let project = Project::new("test", 800, 0, Seed::new(1)).with_voices(vec![
            Voice::new("lead", VoiceType::Saw),
            Voice::new("bass", VoiceType::Sin),
        ]);

        let project = delete_voice_from_project(&project, &VoiceName::new("LEAD")).unwrap();

        assert_eq!(project.voices, vec![Voice::new("bass", VoiceType::Sin)]);
        assert!(delete_voice_from_project(&project, &VoiceName::new("missing")).is_none());
    }
}
