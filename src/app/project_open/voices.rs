use std::path::PathBuf;

use gpui::{
    div, prelude::*, px, Context, CursorStyle, Entity, EventEmitter, MouseButton, MouseDownEvent,
    Window,
};

use crate::{
    project::{self, Project, Voice, VoiceType},
    style as s,
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
        field_group::field_group,
        text_input::TextInput,
    },
};

pub enum Event {
    Added(Project),
    Closed,
}

pub struct VoicesDialog {
    original_project: Project,
    project_directory: PathBuf,
    selected_voice: Option<usize>,
    adding_voice: bool,
    name: Entity<TextInput>,
    selected_voice_type: VoiceType,
    voice_type_buttons: VoiceTypeButtons,
    close_button: Entity<Button>,
    add_new_button: Entity<Button>,
    cancel_button: Entity<Button>,
    add_button: Entity<Button>,
    add_error: Option<String>,
}

impl EventEmitter<Event> for VoicesDialog {}

impl VoicesDialog {
    pub fn new(project: Project, project_directory: PathBuf, cx: &mut Context<Self>) -> Self {
        let selected_voice = (!project.voices.is_empty()).then_some(0);
        let name = cx.new(|cx| TextInput::new("", "lead", cx));
        let selected_voice_type = VoiceType::Sin;
        let voice_type_buttons = VoiceTypeButtons::new(selected_voice_type, cx);
        let close_button = cx.new(|_| Button::x("close-voices"));
        let add_new_button = cx.new(|_| Button::new("add-new-voice", "add new voice"));
        let cancel_button = cx.new(|_| Button::new("cancel-voices", "cancel"));
        let add_button = cx.new(|_| Button::new("confirm-add-voice", "add voice"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&add_new_button, Self::on_add_new_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&add_button, Self::on_add_clicked).detach();
        cx.subscribe(&voice_type_buttons.sin, Self::on_sin_clicked)
            .detach();
        cx.subscribe(&voice_type_buttons.saw, Self::on_saw_clicked)
            .detach();

        Self {
            original_project: project,
            project_directory,
            selected_voice,
            adding_voice: false,
            name,
            selected_voice_type,
            voice_type_buttons,
            close_button,
            add_new_button,
            cancel_button,
            add_button,
            add_error: None,
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
        self.adding_voice = true;
        self.add_error = None;
        cx.notify();
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.adding_voice = false;
        self.add_error = None;
        self.suppress_add_new_hover(cx);
        cx.notify();
    }

    fn on_add_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let result = add_voice_to_project(
            &self.original_project,
            &self.name.read(cx).value(),
            self.selected_voice_type,
        )
        .and_then(|project| {
            project::save_project(&self.project_directory, &project)?;
            Ok(project)
        });

        match result {
            Ok(project) => {
                self.selected_voice = project.voices.len().checked_sub(1);
                self.original_project = project.clone();
                self.adding_voice = false;
                self.name = cx.new(|cx| TextInput::new("", "lead", cx));
                self.add_error = None;
                self.suppress_add_new_hover(cx);
                cx.emit(Event::Added(project));
                cx.notify();
            }
            Err(error) => {
                self.add_error = Some(error.to_string());
                cx.notify();
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
        if self.selected_voice_type == voice_type {
            return;
        }

        self.selected_voice_type = voice_type;
        self.voice_type_buttons.set_selected(voice_type, cx);
        self.add_error = None;
        cx.notify();
    }

    fn select_voice(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.original_project.voices.len() || self.selected_voice == Some(index) {
            return;
        }

        self.selected_voice = Some(index);
        cx.notify();
    }

    fn suppress_add_new_hover(&self, cx: &mut Context<Self>) {
        self.add_new_button.update(cx, |button, cx| {
            button.suppress_hover_until_pointer_exit(cx);
        });
    }
}

impl Render for VoicesDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.adding_voice {
            self.add_voice_dialog()
        } else {
            self.voice_list_dialog(cx)
        }
    }
}

impl VoicesDialog {
    fn voice_list_dialog(&self, cx: &mut Context<Self>) -> gpui::Div {
        s::raised(
            div()
                .flex()
                .flex_col()
                .w(px(570.0))
                .bg(s::GRAY2)
                .child(title_bar("voices", Some(self.close_button.clone())))
                .child(
                    div()
                        .flex()
                        .gap(s::CONTENT_PADDING)
                        .p(s::CONTENT_PADDING)
                        .child(voice_list(
                            &self.original_project.voices,
                            self.selected_voice,
                            cx,
                        ))
                        .child(voice_details(
                            self.selected_voice
                                .and_then(|index| self.original_project.voices.get(index)),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .p(s::CONTENT_PADDING)
                        .pt(s::S0)
                        .child(self.add_new_button.clone()),
                ),
        )
    }

    fn add_voice_dialog(&self) -> gpui::Div {
        let form = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(field_group("voice name", self.name.clone()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_color(s::FIELD_LABEL_TEXT).child("voice type"))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .children(self.voice_type_buttons.entities()),
                    ),
            );

        let form = if let Some(error) = self.add_error.clone() {
            form.child(error_message(error))
        } else {
            form
        };

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(px(570.0))
                .bg(s::GRAY2)
                .child(title_bar("voices", Some(self.close_button.clone())))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(s::CONTENT_PADDING)
                        .p(s::CONTENT_PADDING)
                        .child(form)
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_3()
                                .child(self.cancel_button.clone())
                                .child(self.add_button.clone()),
                        ),
                ),
        )
    }
}

fn voice_list(
    voices: &[Voice],
    selected_voice: Option<usize>,
    cx: &mut Context<VoicesDialog>,
) -> gpui::Div {
    let rows = voices
        .iter()
        .enumerate()
        .map(|(index, voice)| voice_list_row(index, voice, selected_voice == Some(index), cx))
        .collect::<Vec<_>>();

    let list_body = div().flex().flex_col().min_h(px(200.0)).bg(s::GREEN3);
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
        .w(px(250.0))
        .child(s::sunken(list_body).overflow_hidden())
}

fn voice_list_row(
    index: usize,
    voice: &Voice,
    selected: bool,
    cx: &mut Context<VoicesDialog>,
) -> gpui::Div {
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
        .child(voice.name.clone())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
                dialog.select_voice(index, cx);
            }),
        )
}

fn voice_details(voice: Option<&Voice>) -> gpui::Div {
    let details = match voice {
        Some(voice) => div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_color(s::TEXT_DEFAULT).child(voice.name.clone()))
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
        .min_h(px(200.0))
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
enum AddVoiceError {
    InvalidField(String),
    SaveProject(project::SaveProjectError),
}

impl std::fmt::Display for AddVoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidField(message) => write!(f, "{message}"),
            Self::SaveProject(error) => write!(f, "{error}"),
        }
    }
}

impl From<project::SaveProjectError> for AddVoiceError {
    fn from(error: project::SaveProjectError) -> Self {
        Self::SaveProject(error)
    }
}

fn add_voice_to_project(
    project: &Project,
    name: &str,
    voice_type: VoiceType,
) -> Result<Project, AddVoiceError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AddVoiceError::InvalidField(
            "voice name must not be empty".to_string(),
        ));
    }

    if project
        .voices
        .iter()
        .any(|voice| voice.name.eq_ignore_ascii_case(name))
    {
        return Err(AddVoiceError::InvalidField(format!(
            "a voice named {name:?} already exists"
        )));
    }

    let mut updated_project = project.clone();
    updated_project.add_voice(Voice::new(name, voice_type));
    Ok(updated_project)
}

#[cfg(test)]
mod tests {
    use super::add_voice_to_project;
    use crate::{
        project::{Project, VoiceType},
        seed::Seed,
    };

    #[test]
    fn adds_voices_in_column_order() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project = add_voice_to_project(&project, " lead ", VoiceType::Saw).unwrap();
        let project = add_voice_to_project(&project, "bass", VoiceType::Sin).unwrap();

        assert_eq!(project.voices[0].name, "lead");
        assert_eq!(project.voices[0].voice_type, VoiceType::Saw);
        assert_eq!(project.voices[1].name, "bass");
        assert_eq!(project.voices[1].voice_type, VoiceType::Sin);
    }

    #[test]
    fn voice_names_must_be_present_and_unique() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let project = add_voice_to_project(&project, "lead", VoiceType::Sin).unwrap();

        assert!(add_voice_to_project(&project, " ", VoiceType::Saw).is_err());
        assert!(add_voice_to_project(&project, "LEAD", VoiceType::Saw).is_err());
    }
}
