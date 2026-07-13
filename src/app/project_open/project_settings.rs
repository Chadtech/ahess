use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::{
    project::{self, Project},
    seed::Seed,
    style as s,
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
        field_group::field_group,
        text_input::TextInput,
    },
};

pub enum ProjectSettingsMsg {
    Saved(Project),
    Closed,
}

pub struct ProjectSettingsDialog {
    original_project: Project,
    project_directory: PathBuf,
    workspace_root: PathBuf,
    fields: ProjectSettingsFields,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    save_button: Entity<Button>,
    keep_editing_button: Entity<Button>,
    discard_button: Entity<Button>,
    save_error: Option<String>,
    confirming_discard: bool,
}

impl EventEmitter<ProjectSettingsMsg> for ProjectSettingsDialog {}

impl ProjectSettingsDialog {
    pub fn new(
        project: Project,
        project_directory: PathBuf,
        workspace_root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields = ProjectSettingsFields::new(&project, cx);
        let close_button = cx.new(|_| Button::x("close-project-settings"));
        let cancel_button = cx.new(|_| Button::new("cancel-project-settings", "cancel"));
        let save_button = cx.new(|_| Button::new("save-project-settings", "save changes"));
        let keep_editing_button =
            cx.new(|_| Button::new("keep-editing-project-settings", "keep editing"));
        let discard_button = cx.new(|_| Button::new("discard-project-settings", "discard"));

        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&cancel_button, Self::on_close_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();
        cx.subscribe(&keep_editing_button, Self::on_keep_editing_clicked)
            .detach();
        cx.subscribe(&discard_button, Self::on_discard_clicked)
            .detach();

        Self {
            original_project: project,
            project_directory,
            workspace_root,
            fields,
            close_button,
            cancel_button,
            save_button,
            keep_editing_button,
            discard_button,
            save_error: None,
            confirming_discard: false,
        }
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if self.is_dirty(cx) {
            self.confirming_discard = true;
            self.save_error = None;
            cx.notify();
        } else {
            cx.emit(ProjectSettingsMsg::Closed);
        }
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let result = self.project_from_fields(cx).and_then(|project| {
            validate_project_name_unique(
                &self.workspace_root,
                &self.project_directory,
                &project.name,
            )?;
            project::save_project(&self.project_directory, &project)?;
            Ok(project)
        });

        match result {
            Ok(project) => {
                self.original_project = project.clone();
                cx.emit(ProjectSettingsMsg::Saved(project));
            }
            Err(error) => {
                self.save_error = Some(error.to_string());
                self.confirming_discard = false;
                cx.notify();
            }
        }
    }

    fn on_keep_editing_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.confirming_discard = false;
        cx.notify();
    }

    fn on_discard_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ProjectSettingsMsg::Closed);
    }

    fn project_from_fields(
        &self,
        cx: &mut Context<Self>,
    ) -> Result<Project, ProjectSettingsFormError> {
        let name = self.fields.project_name.read(cx).value().trim().to_string();
        if project::project_directory_name(&name).is_none() {
            return Err(ProjectSettingsFormError::InvalidField(
                "project name must contain a letter or number".to_string(),
            ));
        }

        let beat_length =
            parse_u32_field("beat length", &self.fields.beat_length.read(cx).value())?;
        if beat_length == 0 {
            return Err(ProjectSettingsFormError::InvalidField(
                "beat length must be greater than zero".to_string(),
            ));
        }

        let timing_variance = parse_u32_field("variance", &self.fields.variance.read(cx).value())?;
        let seed = parse_seed_field(&self.fields.seed.read(cx).value())?;
        let description = self.fields.description.read(cx).value();

        let mut project = self.original_project.clone();
        project.name = name;
        project.beat_length = beat_length;
        project.timing_variance = timing_variance;
        project.seed = seed;
        project.description = description;

        Ok(project)
    }

    fn is_dirty(&self, cx: &mut Context<Self>) -> bool {
        self.fields.project_name.read(cx).value() != self.original_project.name
            || self.fields.description.read(cx).value() != self.original_project.description
            || self.fields.beat_length.read(cx).value()
                != self.original_project.beat_length.to_string()
            || self.fields.variance.read(cx).value()
                != self.original_project.timing_variance.to_string()
            || self.fields.seed.read(cx).value() != self.original_project.seed.value().to_string()
    }
}

impl Render for ProjectSettingsDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        project_settings_dialog(
            &self.fields,
            self.close_button.clone(),
            self.cancel_button.clone(),
            self.save_button.clone(),
            self.keep_editing_button.clone(),
            self.discard_button.clone(),
            self.save_error.clone(),
            self.confirming_discard,
        )
    }
}

#[derive(Debug)]
enum ProjectSettingsFormError {
    InvalidField(String),
    ListProjects(project::ListProjectsError),
    SaveProject(project::SaveProjectError),
}

impl std::fmt::Display for ProjectSettingsFormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidField(message) => write!(f, "{message}"),
            Self::ListProjects(error) => write!(f, "{error}"),
            Self::SaveProject(error) => write!(f, "{error}"),
        }
    }
}

impl From<project::SaveProjectError> for ProjectSettingsFormError {
    fn from(error: project::SaveProjectError) -> Self {
        Self::SaveProject(error)
    }
}

impl From<project::ListProjectsError> for ProjectSettingsFormError {
    fn from(error: project::ListProjectsError) -> Self {
        Self::ListProjects(error)
    }
}

fn validate_project_name_unique(
    workspace_root: &Path,
    current_project_directory: &Path,
    project_name: &str,
) -> Result<(), ProjectSettingsFormError> {
    let duplicate = project::list_projects(workspace_root)?
        .into_iter()
        .find(|entry| {
            entry.project_directory != current_project_directory
                && entry.project.name.eq_ignore_ascii_case(project_name)
        });

    if duplicate.is_some() {
        Err(ProjectSettingsFormError::InvalidField(format!(
            "a project named {project_name:?} already exists"
        )))
    } else {
        Ok(())
    }
}

fn parse_u32_field(label: &'static str, value: &str) -> Result<u32, ProjectSettingsFormError> {
    value.trim().parse::<u32>().map_err(|_| {
        ProjectSettingsFormError::InvalidField(format!("{label} must be a whole number"))
    })
}

fn parse_seed_field(value: &str) -> Result<Seed, ProjectSettingsFormError> {
    value.trim().parse::<u64>().map(Seed::new).map_err(|_| {
        ProjectSettingsFormError::InvalidField("seed must be a whole number".to_string())
    })
}

struct ProjectSettingsFields {
    project_name: Entity<TextInput>,
    description: Entity<TextInput>,
    beat_length: Entity<TextInput>,
    variance: Entity<TextInput>,
    seed: Entity<TextInput>,
}

impl ProjectSettingsFields {
    fn new(project: &Project, cx: &mut Context<ProjectSettingsDialog>) -> Self {
        Self {
            project_name: cx.new(|cx| TextInput::new(project.name.clone(), "", cx)),
            description: cx.new(|cx| TextInput::new(project.description.clone(), "", cx)),
            beat_length: cx.new(|cx| TextInput::new(project.beat_length.to_string(), "", cx)),
            variance: cx.new(|cx| TextInput::new(project.timing_variance.to_string(), "", cx)),
            seed: cx.new(|cx| TextInput::new(project.seed.value().to_string(), "", cx)),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn project_settings_dialog(
    fields: &ProjectSettingsFields,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    save_button: Entity<Button>,
    keep_editing_button: Entity<Button>,
    discard_button: Entity<Button>,
    save_error: Option<String>,
    confirming_discard: bool,
) -> impl IntoElement {
    let form = div()
        .flex()
        .flex_col()
        .gap_5()
        .child(field_group("project name", fields.project_name.clone()))
        .child(field_group("description", fields.description.clone()))
        .child(section_label("generation settings"))
        .child(div().flex().gap_4().children([
            field_group("beat length (samples)", fields.beat_length.clone()),
            field_group("variance", fields.variance.clone()),
            field_group("seed", fields.seed.clone()),
        ]));

    let form = if let Some(error) = save_error {
        form.child(error_message(error))
    } else {
        form
    };

    let actions = if confirming_discard {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child("discard unsaved changes?")
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(keep_editing_button)
                    .child(discard_button),
            )
    } else {
        div()
            .flex()
            .justify_end()
            .gap_3()
            .child(cancel_button)
            .child(save_button)
    };

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S10)
            .bg(s::GRAY2)
            .child(title_bar("project settings", Some(close_button)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(s::CONTENT_PADDING)
                    .p(s::CONTENT_PADDING)
                    .child(form)
                    .child(actions),
            ),
    )
}

fn section_label(label: &'static str) -> gpui::Div {
    div().text_color(s::TEXT_HEADER).child(label)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{parse_seed_field, parse_u32_field, validate_project_name_unique};
    use crate::{
        project::{self, Project},
        seed::Seed,
    };

    #[test]
    fn parses_project_setting_number_fields() {
        assert_eq!(parse_u32_field("beat length", " 4000 ").unwrap(), 4000);
        assert!(parse_u32_field("beat length", "4.0").is_err());
        assert_eq!(parse_seed_field(" 99 ").unwrap(), Seed::new(99));
        assert!(parse_seed_field("0x99").is_err());
    }

    #[test]
    fn project_name_must_be_unique_except_for_the_current_project() {
        let root = temp_root("unique-project-name");
        let current_directory =
            project::create_project(&root, &Project::new("Current", 800, 0, Seed::new(1))).unwrap();
        project::create_project(&root, &Project::new("Other", 800, 0, Seed::new(2))).unwrap();

        assert!(validate_project_name_unique(&root, &current_directory, "Current").is_ok());
        assert!(validate_project_name_unique(&root, &current_directory, "current").is_ok());
        assert!(validate_project_name_unique(&root, &current_directory, "OTHER").is_err());

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
