use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, App, Context, Entity, EventEmitter, PathPromptOptions, Window};

use crate::{
    app::room_form::RoomFields,
    convolution::{self, WavMetadata},
    pitch_system::PitchSystem,
    project::{self, BeatDurationMillis, FrequencyVariance, Project, VoiceConvolutionChange},
    seed::Seed,
    style as s,
    tuning_system::{self, TuningSystem},
    view::{
        button::{self, Button},
        dialog::error_message,
        dropdown::{self, Dropdown},
        field_group::{control_group, field_group},
        file_import::file_import,
        text_input::TextInput,
        workspace,
    },
};

pub enum ProjectSettingsMsg {
    Saved(Box<Project>),
}

pub struct ProjectSettingsWorkspace {
    original_project: Project,
    project_directory: PathBuf,
    workspace_root: PathBuf,
    fields: ProjectSettingsFields,
    tuning_options: Vec<ProjectTuningOption>,
    original_tuning_index: usize,
    choose_impulse_response_button: Entity<Button>,
    remove_impulse_response_button: Entity<Button>,
    save_button: Entity<Button>,
    save_error: Option<String>,
}

impl EventEmitter<ProjectSettingsMsg> for ProjectSettingsWorkspace {}

impl ProjectSettingsWorkspace {
    pub fn new(
        project: Project,
        project_directory: PathBuf,
        workspace_root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tuning_options, original_tuning_index, tuning_error) =
            project_tuning_options(&workspace_root, &project);
        let fields = ProjectSettingsFields::new(
            &project,
            &project_directory,
            tuning_options.iter().map(ProjectTuningOption::name),
            original_tuning_index,
            cx,
        );
        let room_kind = fields.room.kind();
        let has_impulse_response = fields.impulse_response.is_some();
        let choose_impulse_response_button = cx.new(|_| {
            Button::new(
                "choose-project-impulse-response",
                if has_impulse_response {
                    "replace wav"
                } else {
                    "choose wav"
                },
            )
        });
        let remove_impulse_response_button = cx.new(|_| {
            Button::new("remove-project-impulse-response", "remove").disabled(!has_impulse_response)
        });
        let save_button = cx.new(|_| Button::new("save-project-settings", "save changes"));

        cx.subscribe(&save_button, Self::on_save_clicked).detach();
        cx.subscribe(&room_kind, Self::on_room_kind_selected)
            .detach();
        cx.subscribe(
            &choose_impulse_response_button,
            Self::on_choose_impulse_response_clicked,
        )
        .detach();
        cx.subscribe(
            &remove_impulse_response_button,
            Self::on_remove_impulse_response_clicked,
        )
        .detach();

        Self {
            original_project: project,
            project_directory,
            workspace_root,
            fields,
            tuning_options,
            original_tuning_index,
            choose_impulse_response_button,
            remove_impulse_response_button,
            save_button,
            save_error: tuning_error,
        }
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let convolution_change = self
            .fields
            .impulse_response
            .change(self.original_project.voice_convolution().is_some());
        let result = self.project_from_fields(cx).and_then(|project| {
            validate_project_name_unique(
                &self.workspace_root,
                &self.project_directory,
                &project.name,
            )?;
            project::save_project_with_voice_convolution(
                &self.project_directory,
                project,
                convolution_change,
            )
            .map_err(ProjectSettingsFormError::from)
        });

        match result {
            Ok(project) => {
                self.original_project = project.clone();
                self.original_tuning_index = self.fields.tuning.read(cx).selected_index();
                cx.emit(ProjectSettingsMsg::Saved(Box::new(project)));
            }
            Err(error) => {
                self.save_error = Some(error.to_string());
                cx.notify();
            }
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

    fn on_choose_impulse_response_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("choose impulse response wav".into()),
        });
        cx.spawn(async move |dialog, cx| {
            let selected_path = match selection.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    dialog
                        .update(cx, |dialog, cx| {
                            dialog.save_error =
                                Some(format!("failed to open the file chooser: {error}"));
                            cx.notify();
                        })
                        .ok();
                    return;
                }
            };
            let Some(selected_path) = selected_path else {
                return;
            };
            let path_for_validation = selected_path.clone();
            let validation = cx
                .background_executor()
                .spawn(async move { convolution::inspect_wav_file(&path_for_validation) })
                .await;
            dialog
                .update(cx, |dialog, cx| {
                    dialog.apply_impulse_response_selection(selected_path, validation, cx)
                })
                .ok();
        })
        .detach();
    }

    fn on_remove_impulse_response_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.fields.impulse_response = ImpulseResponseSelection::None;
        self.save_error = None;
        self.sync_impulse_response_buttons(cx);
        cx.notify();
    }

    fn apply_impulse_response_selection(
        &mut self,
        path: PathBuf,
        validation: Result<WavMetadata, convolution::ImpulseResponseError>,
        cx: &mut Context<Self>,
    ) {
        match validation {
            Ok(metadata) => {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("impulse-response.wav")
                    .to_string();
                self.fields.impulse_response = ImpulseResponseSelection::Imported {
                    source_path: path,
                    name,
                    metadata,
                };
                self.save_error = None;
                self.sync_impulse_response_buttons(cx);
            }
            Err(error) => self.save_error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn sync_impulse_response_buttons(&self, cx: &mut Context<Self>) {
        let has_impulse_response = self.fields.impulse_response.is_some();
        self.choose_impulse_response_button
            .update(cx, |button, cx| {
                button.set_label(
                    if has_impulse_response {
                        "replace wav"
                    } else {
                        "choose wav"
                    },
                    cx,
                );
            });
        self.remove_impulse_response_button
            .update(cx, |button, cx| {
                button.set_disabled(!has_impulse_response, cx);
            });
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

        let beat_duration_millis = BeatDurationMillis::new(parse_u32_field(
            "beat duration",
            &self.fields.beat_duration.read(cx).value(),
        )?)
        .map_err(|error| ProjectSettingsFormError::InvalidField(error.to_string()))?;

        let timing_variance =
            parse_u32_field("timing variance", &self.fields.variance.read(cx).value())?;
        let frequency_variance =
            parse_frequency_variance_field(&self.fields.frequency_variance.read(cx).value())?;
        let seed = parse_seed_field(&self.fields.seed.read(cx).value())?;
        let description = self.fields.description.read(cx).value();

        let mut project = self.original_project.clone();
        project.name = name;
        project.beat_duration_millis = beat_duration_millis;
        project.timing_variance = timing_variance;
        project.set_frequency_variance(frequency_variance);
        project.seed = seed;
        project.description = description;
        match self
            .tuning_options
            .get(self.fields.tuning.read(cx).selected_index())
            .expect("the tuning dropdown always selects an available option")
        {
            ProjectTuningOption::Library(system) => project.set_tuning_system(system),
            ProjectTuningOption::Embedded(pitch_system) => {
                project = project.with_pitch_system(pitch_system.clone());
            }
        }
        let room = self
            .fields
            .room
            .room(cx)
            .map_err(ProjectSettingsFormError::InvalidField)?;
        if room != self.original_project.acoustic_scene().room() {
            project
                .set_centered_room(room)
                .map_err(|error| ProjectSettingsFormError::InvalidField(error.to_string()))?;
        }

        Ok(project)
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        self.fields.project_name.read(cx).value() != self.original_project.name
            || self.fields.description.read(cx).value() != self.original_project.description
            || self.fields.beat_duration.read(cx).value()
                != self.original_project.beat_duration_millis.get().to_string()
            || self.fields.variance.read(cx).value()
                != self.original_project.timing_variance.to_string()
            || self.fields.frequency_variance.read(cx).value()
                != self
                    .original_project
                    .frequency_variance()
                    .ratio()
                    .to_string()
            || self.fields.seed.read(cx).value() != self.original_project.seed.value().to_string()
            || self.fields.tuning.read(cx).selected_index() != self.original_tuning_index
            || self
                .fields
                .room
                .is_dirty(self.original_project.acoustic_scene().room(), cx)
            || self
                .fields
                .impulse_response
                .is_dirty(self.original_project.voice_convolution().is_some())
    }

    pub fn sync_project(&mut self, project: Project, cx: &mut Context<Self>) {
        let preserve_draft = self.is_dirty(cx);
        self.original_project = project.clone();
        if !preserve_draft {
            let (tuning_options, original_tuning_index, tuning_error) =
                project_tuning_options(&self.workspace_root, &project);
            let fields = ProjectSettingsFields::new(
                &project,
                &self.project_directory,
                tuning_options.iter().map(ProjectTuningOption::name),
                original_tuning_index,
                cx,
            );
            let room_kind = fields.room.kind();
            cx.subscribe(&room_kind, Self::on_room_kind_selected)
                .detach();
            self.fields = fields;
            self.tuning_options = tuning_options;
            self.original_tuning_index = original_tuning_index;
            self.save_error = tuning_error;
            self.sync_impulse_response_buttons(cx);
        }
        cx.notify();
    }
}

impl Render for ProjectSettingsWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let room_form = self.fields.room.view(self.fields.room.is_enabled(cx));
        let impulse_response = file_import(
            self.fields.impulse_response.summary(),
            self.choose_impulse_response_button.clone(),
            self.remove_impulse_response_button.clone(),
        )
        .debug_selector(|| "project-settings-impulse-response".to_string());
        project_settings_workspace(
            &self.fields,
            room_form,
            impulse_response,
            self.save_button.clone(),
            self.save_error.clone(),
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

fn parse_frequency_variance_field(
    value: &str,
) -> Result<FrequencyVariance, ProjectSettingsFormError> {
    let ratio = value.trim().parse::<f64>().map_err(|_| {
        ProjectSettingsFormError::InvalidField(
            "frequency variance must be a decimal from 0 up to but not including 1".to_string(),
        )
    })?;
    FrequencyVariance::new(ratio)
        .map_err(|error| ProjectSettingsFormError::InvalidField(error.to_string()))
}

fn parse_seed_field(value: &str) -> Result<Seed, ProjectSettingsFormError> {
    value.trim().parse::<u64>().map(Seed::new).map_err(|_| {
        ProjectSettingsFormError::InvalidField("seed must be a whole number".to_string())
    })
}

enum ImpulseResponseSelection {
    None,
    Existing {
        name: String,
        metadata: WavMetadata,
    },
    Imported {
        source_path: PathBuf,
        name: String,
        metadata: WavMetadata,
    },
}

impl ImpulseResponseSelection {
    fn from_project(project: &Project, project_directory: &Path) -> Self {
        let Some(spec) = project.voice_convolution() else {
            return Self::None;
        };
        let metadata = convolution::inspect_project_asset(project_directory, spec)
            .expect("loaded project impulse responses have already been validated");
        Self::Existing {
            name: spec.file_name().to_string(),
            metadata,
        }
    }

    fn is_some(&self) -> bool {
        match self {
            Self::None => false,
            Self::Existing { .. } | Self::Imported { .. } => true,
        }
    }

    fn is_dirty(&self, original_has_impulse_response: bool) -> bool {
        match self {
            Self::None => original_has_impulse_response,
            Self::Existing { .. } => !original_has_impulse_response,
            Self::Imported { .. } => true,
        }
    }

    fn change(&self, original_has_impulse_response: bool) -> VoiceConvolutionChange {
        match self {
            Self::None if original_has_impulse_response => VoiceConvolutionChange::Remove,
            Self::None | Self::Existing { .. } => VoiceConvolutionChange::Keep,
            Self::Imported { source_path, .. } => {
                VoiceConvolutionChange::Import(source_path.clone())
            }
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::None => "no wav selected".to_string(),
            Self::Existing { name, metadata } | Self::Imported { name, metadata, .. } => {
                let duration_seconds = metadata.duration_seconds();
                let duration = if duration_seconds < 1.0 {
                    format!("{:.0} ms", duration_seconds * 1_000.0)
                } else {
                    format!("{duration_seconds:.2} s")
                };
                format!(
                    "{name} · mono · {}-bit · {} hz · {duration}",
                    metadata.bits_per_sample(),
                    metadata.sample_rate()
                )
            }
        }
    }
}

struct ProjectSettingsFields {
    project_name: Entity<TextInput>,
    description: Entity<TextInput>,
    beat_duration: Entity<TextInput>,
    variance: Entity<TextInput>,
    frequency_variance: Entity<TextInput>,
    seed: Entity<TextInput>,
    tuning: Entity<Dropdown>,
    impulse_response: ImpulseResponseSelection,
    room: RoomFields,
}

impl ProjectSettingsFields {
    fn new(
        project: &Project,
        project_directory: &Path,
        tuning_options: impl IntoIterator<Item = impl Into<gpui::SharedString>>,
        selected_tuning: usize,
        cx: &mut Context<ProjectSettingsWorkspace>,
    ) -> Self {
        Self {
            project_name: cx.new(|cx| TextInput::new(project.name.clone(), "", cx)),
            description: cx.new(|cx| TextInput::new(project.description.clone(), "", cx)),
            beat_duration: cx
                .new(|cx| TextInput::new(project.beat_duration_millis.get().to_string(), "", cx)),
            variance: cx.new(|cx| TextInput::new(project.timing_variance.to_string(), "", cx)),
            frequency_variance: cx
                .new(|cx| TextInput::new(project.frequency_variance().ratio().to_string(), "", cx)),
            seed: cx.new(|cx| TextInput::new(project.seed.value().to_string(), "", cx)),
            tuning: cx.new(|cx| {
                Dropdown::new(
                    "project-settings-tuning",
                    tuning_options,
                    selected_tuning,
                    cx,
                )
            }),
            impulse_response: ImpulseResponseSelection::from_project(project, project_directory),
            room: RoomFields::new(
                "project-settings",
                "project-settings-room-kind",
                project.acoustic_scene().room(),
                cx,
            ),
        }
    }
}

fn project_settings_workspace(
    fields: &ProjectSettingsFields,
    room_form: gpui::Div,
    impulse_response: gpui::Div,
    save_button: Entity<Button>,
    save_error: Option<String>,
) -> impl IntoElement {
    let project_column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(s::S0)
        .gap_5()
        .child(field_group("project name", fields.project_name.clone()))
        .child(field_group("description", fields.description.clone()))
        .child(control_group("tuning system", fields.tuning.clone()))
        .child(section_label("generation settings"))
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
        .debug_selector(|| "project-settings-project-column".to_string());

    let acoustics_column = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(s::S0)
        .gap_5()
        .child(section_label("voice convolution"))
        .child("applied independently to every voice before stereo positioning")
        .child(control_group("impulse response wav", impulse_response))
        .child(room_form)
        .debug_selector(|| "project-settings-acoustics-column".to_string());

    let form = div()
        .flex()
        .gap(s::CONTENT_PADDING)
        .child(project_column)
        .child(acoustics_column);

    let feedback = save_error.map(error_message);

    let actions = button::action_group([save_button]).justify_end();

    let settings = div()
        .flex()
        .flex_col()
        .gap(s::CONTENT_PADDING)
        .child(form)
        .children(feedback);

    workspace::tile(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(s::S0)
            .justify_between()
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .debug_selector(|| "project-settings-workspace".to_string())
            .child(settings)
            .child(actions),
    )
}

fn section_label(label: &'static str) -> gpui::Div {
    div().text_color(s::TEXT_HEADER).child(label)
}

#[derive(Clone)]
enum ProjectTuningOption {
    Library(TuningSystem),
    Embedded(PitchSystem),
}

impl ProjectTuningOption {
    fn name(&self) -> String {
        match self {
            Self::Library(system) => system.name().to_string(),
            Self::Embedded(system) => format!("{} (embedded legacy tuning)", system.name()),
        }
    }
}

fn project_tuning_options(
    workspace_root: &Path,
    project: &Project,
) -> (Vec<ProjectTuningOption>, usize, Option<String>) {
    match tuning_system::list_tuning_systems(workspace_root) {
        Ok(systems) => {
            let mut options = systems
                .into_iter()
                .map(ProjectTuningOption::Library)
                .collect::<Vec<_>>();
            let selected = project.tuning_system_id().and_then(|selected_id| {
                options.iter().position(|option| match option {
                    ProjectTuningOption::Library(system) => system.id() == selected_id,
                    ProjectTuningOption::Embedded(_) => false,
                })
            });
            match selected {
                Some(index) => (options, index, None),
                None => {
                    options.insert(
                        0,
                        ProjectTuningOption::Embedded(project.pitch_system().clone()),
                    );
                    (options, 0, None)
                }
            }
        }
        Err(error) => (
            vec![ProjectTuningOption::Embedded(
                project.pitch_system().clone(),
            )],
            0,
            Some(format!("failed to load tuning systems: {error}")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use gpui::{px, size, TestAppContext};

    use super::{
        parse_frequency_variance_field, parse_seed_field, parse_u32_field,
        validate_project_name_unique, ProjectSettingsWorkspace,
    };
    use crate::{
        acoustics::{Point3Meters, RectangularRoom},
        convolution,
        project::{self, Project},
        seed::Seed,
        style as s,
        view::button,
    };

    #[test]
    fn parses_project_setting_number_fields() {
        assert_eq!(parse_u32_field("beat length", " 4000 ").unwrap(), 4000);
        assert!(parse_u32_field("beat length", "4.0").is_err());
        assert_eq!(
            parse_frequency_variance_field(" 0.025 ").unwrap().ratio(),
            0.025
        );
        assert!(parse_frequency_variance_field("1.0").is_err());
        assert!(parse_frequency_variance_field("-0.1").is_err());
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

    #[gpui::test]
    fn project_settings_apply_frequency_variance_as_a_decimal(cx: &mut TestAppContext) {
        let root = temp_root("frequency-variance");
        let project = Project::new("test project", 800, 0, Seed::new(1));
        let project_directory = project::create_project(&root, &project).unwrap();
        let root_for_view = root.clone();
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            ProjectSettingsWorkspace::new(project, project_directory, root_for_view, cx)
        });

        let updated = cx.update(|_, cx| {
            let frequency_variance = dialog.read(cx).fields.frequency_variance.clone();
            frequency_variance.update(cx, |input, cx| input.sync_value("0.037", cx));
            dialog
                .update(cx, |dialog, cx| dialog.project_from_fields(cx))
                .unwrap()
        });

        assert_eq!(updated.frequency_variance().ratio(), 0.037);
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn tuning_dropdown_uses_the_form_width_for_long_system_names(cx: &mut TestAppContext) {
        let root = temp_root("tuning-dropdown-width");
        let project = Project::new("test project", 800, 0, Seed::new(1));
        let project_directory = project::create_project(&root, &project).unwrap();
        let root_for_view = root.clone();
        let (_, cx) = cx.add_window_view(move |_, cx| {
            ProjectSettingsWorkspace::new(project, project_directory, root_for_view, cx)
        });
        cx.simulate_resize(size(px(800.0), px(800.0)));
        cx.run_until_parked();

        let trigger = cx.debug_bounds("project-settings-tuning-trigger").unwrap();

        assert!(trigger.size.width > s::S9);
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn configured_room_uses_the_full_two_column_project_workspace(cx: &mut TestAppContext) {
        let root = temp_root("configured-room-visible");
        let mut project = Project::new("test project", 800, 0, Seed::new(1));
        project
            .set_centered_room(Some(RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap()))
            .unwrap();
        let project_directory = project::create_project(&root, &project).unwrap();
        let root_for_view = root.clone();
        let (_, cx) = cx.add_window_view(move |_, cx| {
            ProjectSettingsWorkspace::new(project, project_directory, root_for_view, cx)
        });
        cx.simulate_resize(size(px(1400.0), px(900.0)));
        cx.run_until_parked();

        let workspace = cx.debug_bounds("project-settings-workspace").unwrap();
        let project_column = cx.debug_bounds("project-settings-project-column").unwrap();
        let acoustics_column = cx
            .debug_bounds("project-settings-acoustics-column")
            .unwrap();

        assert!(workspace.size.width > s::S11);
        assert!(workspace.size.height > px(800.0));
        assert_eq!(project_column.origin.y, acoustics_column.origin.y);
        assert_eq!(project_column.size.width, acoustics_column.size.width);
        assert!(project_column.origin.x + project_column.size.width < acoustics_column.origin.x);
        assert!(cx.debug_bounds("project-settings-room-width").is_some());
        assert!(cx.debug_bounds("project-settings-room-length").is_some());
        assert!(cx.debug_bounds("project-settings-room-height").is_some());
        assert!(cx
            .debug_bounds("project-settings-room-reflection-gain")
            .is_some());

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn enabling_a_room_in_project_settings_builds_a_centered_scene(cx: &mut TestAppContext) {
        let root = temp_root("enable-room");
        let project = Project::new("test project", 800, 0, Seed::new(1));
        let project_directory = project::create_project(&root, &project).unwrap();
        let root_for_view = root.clone();
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            ProjectSettingsWorkspace::new(project, project_directory, root_for_view, cx)
        });

        let updated = cx.update(|_, cx| {
            let room_kind = dialog.read(cx).fields.room.kind();
            room_kind.update(cx, |kind, cx| {
                kind.set_selected_index(1, cx);
            });
            dialog
                .update(cx, |dialog, cx| dialog.project_from_fields(cx))
                .unwrap()
        });

        assert_eq!(
            updated.acoustic_scene().listener(),
            Point3Meters::new(4.0, 5.0, 1.5).unwrap()
        );
        assert_eq!(
            updated.acoustic_scene().room(),
            Some(RectangularRoom::new(8.0, 10.0, 3.0, 0.25).unwrap())
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn valid_impulse_response_selection_is_shown_and_enables_remove(cx: &mut TestAppContext) {
        let root = temp_root("impulse-response-selection");
        let source_path = root.join("small hall.wav");
        fs::write(&source_path, mono_wav_bytes(48_000, &[0, 0, 1, 0])).unwrap();
        let project = Project::new("test project", 800, 0, Seed::new(1));
        let project_directory = project::create_project(&root, &project).unwrap();
        let root_for_view = root.clone();
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            ProjectSettingsWorkspace::new(project, project_directory, root_for_view, cx)
        });
        let metadata = convolution::inspect_wav_file(&source_path).unwrap();

        dialog.update(cx, |dialog, cx| {
            dialog.apply_impulse_response_selection(source_path, Ok(metadata), cx)
        });
        cx.simulate_resize(size(px(800.0), px(900.0)));
        cx.run_until_parked();

        assert!(cx
            .debug_bounds("project-settings-impulse-response")
            .is_some());
        assert!(dialog.update(cx, |dialog, cx| dialog.is_dirty(cx)));
        assert!(!cx.update(|_, cx| {
            dialog
                .read(cx)
                .remove_impulse_response_button
                .read(cx)
                .is_disabled()
        }));
        assert!(cx.update(|_, cx| {
            dialog
                .read(cx)
                .fields
                .impulse_response
                .summary()
                .contains("small hall.wav · mono · 16-bit · 48000 hz")
        }));
        fs::remove_dir_all(root).unwrap();
    }

    #[gpui::test]
    fn saving_settings_imports_the_selected_wav_into_the_project(cx: &mut TestAppContext) {
        let root = temp_root("save-impulse-response");
        let source_path = root.join("room.wav");
        fs::write(&source_path, mono_wav_bytes(44_100, &[0, 0])).unwrap();
        let project = Project::new("test project", 800, 0, Seed::new(1));
        let project_directory = project::create_project(&root, &project).unwrap();
        let project_directory_for_view = project_directory.clone();
        let root_for_view = root.clone();
        let (dialog, cx) = cx.add_window_view(move |_, cx| {
            ProjectSettingsWorkspace::new(project, project_directory_for_view, root_for_view, cx)
        });
        let metadata = convolution::inspect_wav_file(&source_path).unwrap();
        dialog.update(cx, |dialog, cx| {
            dialog.apply_impulse_response_selection(source_path, Ok(metadata), cx)
        });

        let save_button = cx.update(|_, cx| dialog.read(cx).save_button.clone());
        dialog.update(cx, |dialog, cx| {
            dialog.on_save_clicked(save_button, &button::Clicked, cx)
        });

        let loaded = project::load_project(&project_directory).unwrap().project;
        let spec = loaded.voice_convolution().unwrap();
        assert_eq!(spec.file_name(), "room.wav");
        assert!(project_directory.join(spec.file()).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    fn mono_wav_bytes(sample_rate: u32, data: &[u8]) -> Vec<u8> {
        let channels = 1_u16;
        let bits = 16_u16;
        let block_align = channels * (bits / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let riff_size = 36 + data.len() as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
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
