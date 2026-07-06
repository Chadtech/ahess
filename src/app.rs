use std::borrow::Cow;

use gpui::{div, prelude::*, px, App, Application, Context, Entity, Window, WindowOptions};

use crate::{style as s, view};

use view::{
    button::{self, Button},
    text_input::TextInput,
};

const UNIFRAKTUR_MAGUNTIA: &[u8] = include_bytes!("../assets/fonts/UnifrakturMaguntia-Regular.ttf");

struct AhessApp {
    fields: NewProjectFields,
    buttons: Buttons,
    project_start_mode: ProjectStartMode,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ProjectStartMode {
    New,
    Existing,
}

impl AhessApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let project_start_mode = ProjectStartMode::New;
        let fields = NewProjectFields::new(cx);
        let buttons = Buttons::new(cx, project_start_mode);

        cx.subscribe(&buttons.new_project, Self::on_new_project_clicked)
            .detach();
        cx.subscribe(&buttons.open_existing, Self::on_existing_project_clicked)
            .detach();

        let app = Self {
            fields,
            buttons,
            project_start_mode,
        };

        app
    }

    fn on_new_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.set_project_start_mode(ProjectStartMode::New, cx);
    }

    fn on_existing_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.set_project_start_mode(ProjectStartMode::Existing, cx);
    }

    fn set_project_start_mode(&mut self, mode: ProjectStartMode, cx: &mut Context<Self>) {
        let changed = self.project_start_mode != mode;

        self.project_start_mode = mode;
        self.buttons.set_project_start_mode(mode, cx);

        if changed {
            cx.notify();
        }
    }
}

struct Buttons {
    new_project: Entity<Button>,
    open_existing: Entity<Button>,
    create: Entity<Button>,
}

impl Buttons {
    fn new(cx: &mut Context<AhessApp>, mode: ProjectStartMode) -> Self {
        Self {
            new_project: cx.new(|_| {
                Button::new("new-project", "new project").depressed(mode == ProjectStartMode::New)
            }),
            open_existing: cx.new(|_| {
                Button::new("open-existing", "open existing")
                    .depressed(mode == ProjectStartMode::Existing)
            }),
            create: cx.new(|_| Button::new("create-new-project", "create")),
        }
    }

    fn set_project_start_mode(&self, mode: ProjectStartMode, cx: &mut Context<AhessApp>) {
        self.new_project.update(cx, |button, cx| {
            button.set_depressed(mode == ProjectStartMode::New, cx);
        });
        self.open_existing.update(cx, |button, cx| {
            button.set_depressed(mode == ProjectStartMode::Existing, cx);
        });
    }
}

struct NewProjectFields {
    project_name: Entity<TextInput>,
    beat_length: Entity<TextInput>,
    variance: Entity<TextInput>,
    seed: Entity<TextInput>,
    description: Entity<TextInput>,
}

impl NewProjectFields {
    fn new(cx: &mut Context<AhessApp>) -> Self {
        Self {
            project_name: cx.new(|cx| TextInput::new("arc-light sketch", "project name", cx)),
            beat_length: cx.new(|cx| TextInput::new("4000", "beat length", cx)),
            variance: cx.new(|cx| TextInput::new("100", "variance", cx)),
            seed: cx.new(|cx| TextInput::new("0x1234", "seed", cx)),
            description: cx.new(|cx| TextInput::new("first generated sketch", "description", cx)),
        }
    }
}

impl Render for AhessApp {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .font_family(s::FONT)
            .bg(s::GREEN2)
            .text_color(s::GRAY6)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(status_bar())
                    .child(new_project_screen(&self.fields, &self.buttons)),
            )
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    Application::new().run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(UNIFRAKTUR_MAGUNTIA)])
            .expect("failed to load UnifrakturMaguntia font");

        view::text_input::bind_keys(cx);

        cx.open_window(WindowOptions::default(), |window, cx| {
            window.set_window_title("ahess");
            cx.new(AhessApp::new)
        })
        .unwrap();
    });

    Ok(())
}

fn status_bar() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .border_b_2()
        .border_color(s::GRAY1)
        .bg(s::GRAY2)
        .px(s::S5)
        .py(s::S4)
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().text_color(s::GRAY4).child("ahess"))
                .child(div().text_color(s::GRAY5).child("no project open")),
        )
}

fn new_project_screen(fields: &NewProjectFields, buttons: &Buttons) -> impl IntoElement {
    div()
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .bg(s::GREEN2)
        .p(s::S7)
        .child(
            div()
                .flex()
                .size_full()
                .items_start()
                .justify_between()
                .gap_3()
                .child(project_picker_dialog(buttons))
                .child(new_project_dialog(fields, buttons)),
        )
}

fn title_bar(title: &'static str, close_button: Option<Entity<Button>>) -> gpui::Div {
    let title_bar = div()
        .flex()
        .items_center()
        .justify_between()
        .bg(s::GRAY5)
        .text_color(s::GREEN1)
        .p(s::S3)
        .px(s::S4)
        .child(title);

    if let Some(close_button) = close_button {
        title_bar.child(close_button)
    } else {
        title_bar
    }
}

fn project_picker_dialog(buttons: &Buttons) -> impl IntoElement {
    s::raised(
        div().flex().flex_col().w(px(430.0)).bg(s::GRAY2).child(
            div()
                .flex()
                .flex_col()
                .gap_5()
                .p(s::S5)
                .child(
                    s::sunken(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .min_h(px(240.0))
                            .bg(s::GREEN3)
                            .p(s::S6)
                            .child(
                                div()
                                    .font_family(s::DISPLAY_FONT)
                                    .text_color(s::YELLOW6)
                                    .text_size(px(92.0))
                                    .line_height(px(92.0))
                                    .child("ahess"),
                            ),
                    )
                    .overflow_hidden(),
                )
                .child(
                    div()
                        .flex()
                        .justify_center()
                        .gap_5()
                        .child(buttons.new_project.clone())
                        .child(buttons.open_existing.clone()),
                ),
        ),
    )
}

fn new_project_dialog(fields: &NewProjectFields, buttons: &Buttons) -> impl IntoElement {
    s::raised(
        div()
            .flex()
            .flex_col()
            .w(px(570.0))
            .bg(s::GRAY2)
            .child(title_bar("new window", None))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .p(s::S5)
                    .child(text_field("project name", fields.project_name.clone()))
                    .child(div().flex().gap_4().children([
                        text_field("beat length", fields.beat_length.clone()),
                        text_field("variance", fields.variance.clone()),
                        text_field("seed", fields.seed.clone()),
                    ]))
                    .child(text_field("description", fields.description.clone())),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_3()
                    .p(s::S4)
                    .child(buttons.create.clone()),
            ),
    )
}

fn text_field(label: &'static str, input: Entity<TextInput>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .child(div().text_color(s::GRAY5).child(label))
        .child(s::sunken(input).overflow_hidden())
}
