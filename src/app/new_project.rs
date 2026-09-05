use std::path::PathBuf;

use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::{
    app::room_form::RoomFields,
    project::{self, BeatDurationMillis, FrequencyVariance, OpenProjectRequest},
    seed::Seed,
    style as s,
    tuning_system::{self, TuningSystem},
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
        dropdown::{self, Dropdown},
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

impl EventEmitter<OpenProjectRequest> for NewProjectDialog {}

impl NewProjectDialog {
    pub fn new(workspace_root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Self {
        let workspace_root = workspace_root.into();
        let fields = NewProjectFields::new(cx);
        let room_kind = fields.room.kind();
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
        cx.subscribe(&room_kind, Self::on_room_kind_selected)
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

    fn on_room_kind_selected(
        &mut self,
        _: Entity<Dropdown>,
        _: &dropdown::Selected,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn create_project_from_fields(
        &self,
        cx: &mut Context<Self>,
    ) -> Result<OpenProjectRequest, CreateProjectFormError> {
        let project_name = self.fields.project_name.read(cx).value().trim().to_string();
        let beat_duration = parse_beat_duration_field(&self.fields.beat_duration.read(cx).value())?;
        let timing_variance =
            parse_u32_field("timing variance", &self.fields.variance.read(cx).value())?;
        let frequency_variance =
            parse_frequency_variance_field(&self.fields.frequency_variance.read(cx).value())?;
        let seed = parse_seed_field(&self.fields.seed.read(cx).value())?;
        let description = self.fields.description.read(cx).value();
        let tuning_system = self
            .tuning_systems
            .get(self.tuning_dropdown.read(cx).selected_index())
            .expect("the tuning dropdown always selects an available system");
        let room = self
            .fields
            .room
            .room(cx)
            .map_err(CreateProjectFormError::InvalidField)?;
        let mut project = project::Project::new(project_name, beat_duration, timing_variance, seed)
            .with_frequency_variance(frequency_variance)
            .with_description(description)
            .with_tuning_system(tuning_system);
        project
            .set_centered_room(room)
            .map_err(|error| CreateProjectFormError::InvalidField(error.to_string()))?;
        let project_directory = project::create_project(&self.workspace_root, &project)?;

        Ok(OpenProjectRequest {
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let room_form = self.fields.room.view(self.fields.room.is_enabled(cx));
        new_project_form(
            &self.fields,
            self.tuning_dropdown.clone(),
            room_form,
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

fn parse_beat_duration_field(value: &str) -> Result<BeatDurationMillis, CreateProjectFormError> {
    let milliseconds = parse_u32_field("beat duration", value)?;
    BeatDurationMillis::new(milliseconds)
        .map_err(|error| CreateProjectFormError::InvalidField(error.to_string()))
}

fn parse_frequency_variance_field(
    value: &str,
) -> Result<FrequencyVariance, CreateProjectFormError> {
    let ratio = value.trim().parse::<f64>().map_err(|_| {
        CreateProjectFormError::InvalidField(
            "frequency variance must be a decimal from 0 up to but not including 1".to_string(),
        )
    })?;
    FrequencyVariance::new(ratio)
        .map_err(|error| CreateProjectFormError::InvalidField(error.to_string()))
}

fn parse_seed_field(value: &str) -> Result<Seed, CreateProjectFormError> {
    value.trim().parse::<u64>().map(Seed::new).map_err(|_| {
        CreateProjectFormError::InvalidField("seed must be a whole number".to_string())
    })
}

struct NewProjectFields {
    project_name: Entity<TextInput>,
    beat_duration: Entity<TextInput>,
    variance: Entity<TextInput>,
    frequency_variance: Entity<TextInput>,
    seed: Entity<TextInput>,
    description: Entity<TextInput>,
    room: RoomFields,
}

impl NewProjectFields {
    fn new(cx: &mut Context<NewProjectDialog>) -> Self {
        Self {
            project_name: cx.new(|cx| TextInput::new("", "", cx)),
            beat_duration: cx.new(|cx| TextInput::new("", "17", cx)),
            variance: cx.new(|cx| TextInput::new("", "", cx)),
            frequency_variance: cx.new(|cx| TextInput::new("", "", cx)),
            seed: cx.new(|cx| TextInput::new("", "1234", cx)),
            description: cx.new(|cx| TextInput::new("", "", cx)),
            room: RoomFields::new("new-project", "new-project-room-kind", None, cx),
        }
    }
}

fn new_project_form(
    fields: &NewProjectFields,
    tuning_dropdown: Entity<Dropdown>,
    room_form: gpui::Div,
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
            field_group("beat duration (ms)", fields.beat_duration.clone()),
            field_group("timing variance (samples)", fields.variance.clone()),
        ]))
        .child(div().flex().gap_4().children([
            field_group(
                "frequency variance (decimal)",
                fields.frequency_variance.clone(),
            ),
            field_group("seed", fields.seed.clone()),
        ]))
        .child(field_group("description", fields.description.clone()))
        .child(room_form);
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
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use gpui::{px, size, Modifiers, TestAppContext};

    use super::{
        parse_beat_duration_field, parse_frequency_variance_field, parse_seed_field,
        parse_u32_field,
    };
    use crate::{project, seed::Seed, style as s};

    #[test]
    fn parses_decimal_number_fields() {
        assert_eq!(parse_u32_field("beat length", " 4000 ").unwrap(), 4000);
        assert!(parse_u32_field("beat length", "4.0").is_err());
        assert_eq!(parse_beat_duration_field("17").unwrap().get(), 17);
        assert!(parse_beat_duration_field("0").is_err());
        assert_eq!(
            parse_frequency_variance_field(" 0.025 ").unwrap().ratio(),
            0.025
        );
        assert!(parse_frequency_variance_field("1.0").is_err());
        assert!(parse_frequency_variance_field("-0.1").is_err());
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

    #[gpui::test]
    fn room_fields_create_a_centered_rectangular_acoustic_scene(cx: &mut TestAppContext) {
        let root = temp_root("new-project-room");
        let root_for_view = root.clone();
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| super::NewProjectDialog::new(root_for_view, cx));
        cx.simulate_resize(size(px(800.0), px(800.0)));
        cx.run_until_parked();

        assert!(cx.debug_bounds("new-project-room-width").is_none());
        let trigger = cx.debug_bounds("new-project-room-kind-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        let rectangular_room = cx.debug_bounds("new-project-room-kind-option-1").unwrap();
        cx.simulate_click(rectangular_room.center(), Modifiers::default());
        assert!(cx.debug_bounds("new-project-room-width").is_some());

        let opened = cx.update(|_, cx| {
            let fields = &dialog.read(cx).fields;
            let values = [
                (fields.project_name.clone(), "room project"),
                (fields.beat_duration.clone(), "17"),
                (fields.variance.clone(), "0"),
                (fields.frequency_variance.clone(), "0.025"),
                (fields.seed.clone(), "1"),
            ];
            for (input, value) in values {
                input.update(cx, |input, cx| input.sync_value(value, cx));
            }
            dialog
                .update(cx, |dialog, cx| dialog.create_project_from_fields(cx))
                .unwrap()
        });
        let project = project::load_project(opened.project_directory)
            .unwrap()
            .project;
        let room = project.acoustic_scene().room().unwrap();

        assert_eq!(
            (room.width(), room.length(), room.height()),
            (8.0, 10.0, 3.0)
        );
        assert_eq!(
            project.acoustic_scene().listener(),
            crate::acoustics::Point3Meters::new(4.0, 5.0, 1.5).unwrap()
        );
        assert_eq!(project.frequency_variance().ratio(), 0.025);

        fs::remove_dir_all(root).unwrap();
    }

    fn temp_root(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ahess-{test_name}-{}-{unique}", std::process::id()));

        fs::create_dir_all(&root).unwrap();
        root
    }
}
