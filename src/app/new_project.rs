use std::path::PathBuf;

use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::{
    project::{self, ProjectOpened},
    seed::Seed,
    style as s,
    tuning_system::{self, TuningSystem},
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
        dropdown::Dropdown,
        field_group::{control_group, field_group},
        text_input::TextInput,
    },
};

pub struct NewProjectDialog {
    fields: NewProjectFields,
    tuning_systems: Vec<TuningSystem>,
    tuning_dropdown: Entity<Dropdown>,
    create_button: Entity<Button>,
    create_project_error: Option<String>,
    workspace_root: PathBuf,
}

impl EventEmitter<ProjectOpened> for NewProjectDialog {}

impl NewProjectDialog {
    pub fn new(workspace_root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Self {
        let workspace_root = workspace_root.into();
        let fields = NewProjectFields::new(cx);
        let (tuning_systems, create_project_error) =
            match tuning_system::list_tuning_systems(&workspace_root) {
                Ok(systems) => (systems, None),
                Err(error) => (
                    vec![TuningSystem::built_in_western()],
                    Some(format!("failed to load tuning systems: {error}")),
                ),
            };
        let tuning_options = tuning_systems
            .iter()
            .map(|system| system.name().to_string())
            .collect::<Vec<_>>();
        let tuning_dropdown =
            cx.new(|cx| Dropdown::new("new-project-tuning", tuning_options, 0, cx));
        let create_button = cx.new(|_| Button::new("create-new-project", "create"));

        cx.subscribe(&create_button, Self::on_create_project_clicked)
            .detach();

        Self {
            fields,
            tuning_systems,
            tuning_dropdown,
            create_button,
            create_project_error,
            workspace_root,
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
        let beat_length = parse_beat_length_field(&self.fields.beat_length.read(cx).value())?;
        let timing_variance = parse_u32_field("variance", &self.fields.variance.read(cx).value())?;
        let seed = parse_seed_field(&self.fields.seed.read(cx).value())?;
        let description = self.fields.description.read(cx).value();
        let tuning_system = self
            .tuning_systems
            .get(self.tuning_dropdown.read(cx).selected_index())
            .expect("the tuning dropdown always selects an available system");
        let project = project::Project::new(project_name, beat_length, timing_variance, seed)
            .with_description(description)
            .with_tuning_system(tuning_system);
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
            self.tuning_dropdown.clone(),
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

fn parse_beat_length_field(value: &str) -> Result<u32, CreateProjectFormError> {
    let beat_length = parse_u32_field("beat length", value)?;
    if beat_length == 0 {
        return Err(CreateProjectFormError::InvalidField(
            "beat length must be at least one sample".to_string(),
        ));
    }
    Ok(beat_length)
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

fn new_project_form(
    fields: &NewProjectFields,
    tuning_dropdown: Entity<Dropdown>,
    create_button: Entity<Button>,
    create_project_error: Option<String>,
) -> impl IntoElement {
    let form_body = div()
        .flex()
        .flex_col()
        .gap_5()
        .child(field_group("project name", fields.project_name.clone()))
        .child(control_group("tuning system", tuning_dropdown))
        .child(div().flex().gap_4().children([
            field_group("beat length (samples)", fields.beat_length.clone()),
            field_group("variance", fields.variance.clone()),
            field_group("seed", fields.seed.clone()),
        ]))
        .child(field_group("description", fields.description.clone()));
    let form_body = if let Some(error) = create_project_error {
        form_body.child(error_message(error))
    } else {
        form_body
    };

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S10)
            .bg(s::GRAY2)
            .child(title_bar("new project", None))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(s::CONTENT_PADDING)
                    .p(s::CONTENT_PADDING)
                    .child(form_body)
                    .child(div().flex().justify_end().child(create_button)),
            ),
    )
}

#[cfg(test)]
mod tests {
    use gpui::{px, size, TestAppContext};

    use super::{parse_beat_length_field, parse_seed_field, parse_u32_field};
    use crate::{seed::Seed, style as s};

    #[test]
    fn parses_decimal_number_fields() {
        assert_eq!(parse_u32_field("beat length", " 4000 ").unwrap(), 4000);
        assert!(parse_u32_field("beat length", "4.0").is_err());
        assert_eq!(parse_beat_length_field("800").unwrap(), 800);
        assert!(parse_beat_length_field("0").is_err());
    }

    #[test]
    fn parses_seed_as_a_whole_number() {
        assert_eq!(parse_seed_field(" 1234 ").unwrap(), Seed::new(1234));
        assert!(parse_seed_field("0x1234").is_err());
        assert!(parse_seed_field("12.34").is_err());
    }

    #[gpui::test]
    fn tuning_dropdown_uses_the_form_width_for_long_system_names(cx: &mut TestAppContext) {
        let (_, cx) =
            cx.add_window_view(|_, cx| super::NewProjectDialog::new(std::env::temp_dir(), cx));
        cx.simulate_resize(size(px(800.0), px(800.0)));
        cx.run_until_parked();

        let trigger = cx.debug_bounds("new-project-tuning-trigger").unwrap();

        assert!(trigger.size.width > s::S9);
    }
}
