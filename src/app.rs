use std::{borrow::Cow, path::PathBuf};

use gpui::{
    div, prelude::*, px, App, Application, Context, Entity, SharedString, Window, WindowOptions,
};

use crate::{project, seed::Seed, style as s, view};

use view::{
    button::{self, Button},
    text_input::TextInput,
};

const UNIFRAKTUR_MAGUNTIA: &[u8] = include_bytes!("../assets/fonts/UnifrakturMaguntia-Regular.ttf");

struct AhessApp {
    fields: NewProjectFields,
    buttons: Buttons,
    app_mode: AppMode,
    workspace_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppMode {
    ProjectStart {
        project_start_mode: ProjectStartMode,
        create_project_error: Option<String>,
    },
    ProjectOpen {
        project_name: String,
        project_directory: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
        cx.subscribe(&buttons.create, Self::on_create_project_clicked)
            .detach();

        let app = Self {
            fields,
            buttons,
            app_mode: AppMode::ProjectStart {
                project_start_mode,
                create_project_error: None,
            },
            workspace_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
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

    fn on_create_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        match self.create_project_from_fields(cx) {
            Ok(open_project) => {
                self.open_project(open_project);
            }
            Err(error) => {
                self.set_create_project_error(error);
            }
        }

        cx.notify();
    }

    fn set_project_start_mode(&mut self, mode: ProjectStartMode, cx: &mut Context<Self>) {
        let changed = match &self.app_mode {
            AppMode::ProjectStart {
                project_start_mode,
                create_project_error,
            } => *project_start_mode != mode || create_project_error.is_some(),
            AppMode::ProjectOpen { .. } => true,
        };

        self.app_mode = AppMode::ProjectStart {
            project_start_mode: mode,
            create_project_error: None,
        };
        self.buttons.set_project_start_mode(mode, cx);

        if changed {
            cx.notify();
        }
    }

    fn create_project_from_fields(
        &self,
        cx: &mut Context<Self>,
    ) -> Result<OpenProject, CreateProjectFormError> {
        let project_name = self.fields.project_name.read(cx).value().trim().to_string();
        let beat_length =
            parse_u32_field("beat length", &self.fields.beat_length.read(cx).value())?;
        let timing_variance = parse_u32_field("variance", &self.fields.variance.read(cx).value())?;
        let seed = parse_seed_field(&self.fields.seed.read(cx).value())?;
        let description = self.fields.description.read(cx).value();
        let project = project::Project::new(project_name, beat_length, timing_variance, seed)
            .with_description(description);
        let project_directory = project::create_project(&self.workspace_root, &project)?;

        Ok(OpenProject {
            project_name: project.name,
            project_directory,
        })
    }

    fn open_project(&mut self, open_project: OpenProject) {
        self.app_mode = AppMode::ProjectOpen {
            project_name: open_project.project_name,
            project_directory: open_project.project_directory,
        };
    }

    fn set_create_project_error(&mut self, error: CreateProjectFormError) {
        self.app_mode = AppMode::ProjectStart {
            project_start_mode: self.project_start_mode(),
            create_project_error: Some(error.to_string()),
        };
    }

    fn project_start_mode(&self) -> ProjectStartMode {
        match &self.app_mode {
            AppMode::ProjectStart {
                project_start_mode, ..
            } => *project_start_mode,
            AppMode::ProjectOpen { .. } => ProjectStartMode::New,
        }
    }

    fn project_title(&self) -> SharedString {
        match &self.app_mode {
            AppMode::ProjectStart { .. } => "".into(),
            AppMode::ProjectOpen { project_name, .. } => project_name.clone().into(),
        }
    }
}

struct OpenProject {
    project_name: String,
    project_directory: PathBuf,
}

#[derive(Debug)]
enum CreateProjectFormError {
    InvalidField(String),
    CreateProject(project::CreateProjectError),
}

impl std::fmt::Display for CreateProjectFormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidField(message) => write!(f, "{message}"),
            Self::CreateProject(error) => write!(f, "{error}"),
        }
    }
}

impl From<project::CreateProjectError> for CreateProjectFormError {
    fn from(error: project::CreateProjectError) -> Self {
        Self::CreateProject(error)
    }
}

fn parse_u32_field(label: &'static str, value: &str) -> Result<u32, CreateProjectFormError> {
    value.trim().parse::<u32>().map_err(|_| {
        CreateProjectFormError::InvalidField(format!("{label} must be a whole number"))
    })
}

fn parse_seed_field(value: &str) -> Result<Seed, CreateProjectFormError> {
    value.trim().parse::<u64>().map(Seed::new).map_err(|_| {
        CreateProjectFormError::InvalidField("seed must be a whole number".to_string())
    })
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
            project_name: cx.new(|cx| TextInput::new("", "", cx)),
            beat_length: cx.new(|cx| TextInput::new("", "800", cx)),
            variance: cx.new(|cx| TextInput::new("", "", cx)),
            seed: cx.new(|cx| TextInput::new("", "1234", cx)),
            description: cx.new(|cx| TextInput::new("", "", cx)),
        }
    }
}

impl Render for AhessApp {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen = match &self.app_mode {
            AppMode::ProjectStart {
                create_project_error,
                ..
            } => new_project_screen(&self.fields, &self.buttons, create_project_error.clone()),
            AppMode::ProjectOpen {
                project_directory, ..
            } => project_workspace(project_directory),
        };
        let project_title = self.project_title();

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
                    .child(project_bar(project_title))
                    .child(screen),
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

fn project_bar(project_title: SharedString) -> impl IntoElement {
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
                .child(div().text_color(s::GRAY5).child(project_title)),
        )
}

fn new_project_screen(
    fields: &NewProjectFields,
    buttons: &Buttons,
    create_project_error: Option<String>,
) -> gpui::Div {
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
                .child(new_project_dialog(fields, buttons, create_project_error)),
        )
}

fn project_workspace(_project_directory: &PathBuf) -> gpui::Div {
    div().flex_1().min_h(px(0.0)).bg(s::GREEN2)
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

fn new_project_dialog(
    fields: &NewProjectFields,
    buttons: &Buttons,
    create_project_error: Option<String>,
) -> impl IntoElement {
    let form_body = div()
        .flex()
        .flex_col()
        .gap_5()
        .p(s::S5)
        .child(text_field("project name", fields.project_name.clone()))
        .child(div().flex().gap_4().children([
            text_field("beat length (samples)", fields.beat_length.clone()),
            text_field("variance", fields.variance.clone()),
            text_field("seed", fields.seed.clone()),
        ]))
        .child(text_field("description", fields.description.clone()));
    let form_body = if let Some(error) = create_project_error {
        form_body.child(error_message(error.into()))
    } else {
        form_body
    };

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(px(570.0))
            .bg(s::GRAY2)
            .child(title_bar("new window", None))
            .child(form_body)
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

fn error_message(message: SharedString) -> gpui::Div {
    s::sunken(
        div()
            .bg(s::RED1)
            .text_color(s::WHITE)
            .p(s::S4)
            .child(message),
    )
    .overflow_hidden()
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

#[cfg(test)]
mod tests {
    use super::{parse_seed_field, parse_u32_field};
    use crate::seed::Seed;

    #[test]
    fn parses_decimal_number_fields() {
        assert_eq!(parse_u32_field("beat length", " 4000 ").unwrap(), 4000);
        assert!(parse_u32_field("beat length", "4.0").is_err());
    }

    #[test]
    fn parses_seed_as_a_whole_number() {
        assert_eq!(parse_seed_field(" 1234 ").unwrap(), Seed::new(1234));
        assert!(parse_seed_field("0x1234").is_err());
        assert!(parse_seed_field("12.34").is_err());
    }
}
