use std::path::PathBuf;

use gpui::{div, prelude::*, px, Context, Entity, EventEmitter, SharedString, Window};

use crate::{
    project,
    seed::Seed,
    style as s,
    view::{
        button::{self, Button},
        text_input::TextInput,
    },
};

pub struct NewProjectDialog {
    fields: NewProjectFields,
    create_button: Entity<Button>,
    create_project_error: Option<String>,
    workspace_root: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectOpened {
    pub project_name: String,
    pub project_directory: PathBuf,
}

impl EventEmitter<ProjectOpened> for NewProjectDialog {}

impl NewProjectDialog {
    pub fn new(workspace_root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Self {
        let fields = NewProjectFields::new(cx);
        let create_button = cx.new(|_| Button::new("create-new-project", "create"));

        cx.subscribe(&create_button, Self::on_create_project_clicked)
            .detach();

        Self {
            fields,
            create_button,
            create_project_error: None,
            workspace_root: workspace_root.into(),
        }
    }

    fn on_create_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        match self.create_project_from_fields(cx) {
            Ok(project) => cx.emit(project),
            Err(error) => self.set_create_project_error(error, cx),
        }
    }

    fn create_project_from_fields(
        &self,
        cx: &mut Context<Self>,
    ) -> Result<ProjectOpened, CreateProjectFormError> {
        let project_name = self.fields.project_name.read(cx).value().trim().to_string();
        let beat_length =
            parse_u32_field("beat length", &self.fields.beat_length.read(cx).value())?;
        let timing_variance = parse_u32_field("variance", &self.fields.variance.read(cx).value())?;
        let seed = parse_seed_field(&self.fields.seed.read(cx).value())?;
        let description = self.fields.description.read(cx).value();
        let project = project::Project::new(project_name, beat_length, timing_variance, seed)
            .with_description(description);
        let project_directory = project::create_project(&self.workspace_root, &project)?;

        Ok(ProjectOpened {
            project_name: project.name,
            project_directory,
        })
    }

    fn set_create_project_error(&mut self, error: CreateProjectFormError, cx: &mut Context<Self>) {
        self.create_project_error = Some(error.to_string());
        cx.notify();
    }
}

impl Render for NewProjectDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        new_project_form(
            &self.fields,
            self.create_button.clone(),
            self.create_project_error.clone(),
        )
    }
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

struct NewProjectFields {
    project_name: Entity<TextInput>,
    beat_length: Entity<TextInput>,
    variance: Entity<TextInput>,
    seed: Entity<TextInput>,
    description: Entity<TextInput>,
}

impl NewProjectFields {
    fn new(cx: &mut Context<NewProjectDialog>) -> Self {
        Self {
            project_name: cx.new(|cx| TextInput::new("", "", cx)),
            beat_length: cx.new(|cx| TextInput::new("", "800", cx)),
            variance: cx.new(|cx| TextInput::new("", "", cx)),
            seed: cx.new(|cx| TextInput::new("", "1234", cx)),
            description: cx.new(|cx| TextInput::new("", "", cx)),
        }
    }
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

fn new_project_form(
    fields: &NewProjectFields,
    create_button: Entity<Button>,
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
                    .child(create_button),
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
