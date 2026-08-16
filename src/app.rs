mod new_project;
mod open_project;
mod position_form;
mod project_open;
mod room_form;
mod tuning_system_editor;

use std::{
    cell::Cell,
    fmt, fs, io,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, OnceLock},
};

use gpui::{
    actions, div, img, prelude::*, px, AnyElement, App, Application, Context, Entity, Image,
    ImageFormat, KeyBinding, ObjectFit, SharedString, Window, WindowOptions,
};
use serde::{Deserialize, Serialize};

use crate::{
    project::{self, Project, ProjectOpened},
    style as s,
    view::{
        self,
        button::{self, Button},
        dialog::modal_overlay,
    },
};

use self::{new_project::NewProjectDialog, open_project::OpenProjectDialog};

actions!(ahess, [TogglePlayback, Undo, Redo]);

const AHESS_IMAGE_HEIGHT_RATIO: f32 = 1086.0 / 1448.0;
const APP_STATE_FILE: &str = ".ahess-ui-state.toml";
const FORCE_ERROR_VIEW: bool = false;

static AHESS_IMAGE: OnceLock<Arc<Image>> = OnceLock::new();

struct AhessApp {
    workspace_root: PathBuf,
    app_mode: AppMode,
}

enum AppMode {
    ProjectStart(ProjectStart),
    ProjectOpen(Entity<project_open::Model>),
    TuningSystems(Entity<tuning_system_editor::Model>),
    Error { message: String },
}

struct ProjectStart {
    project_start_mode: ProjectStartMode,
    new_project_dialog: Entity<NewProjectDialog>,
    open_project_dialog: Entity<OpenProjectDialog>,
    buttons: ProjectStartButtons,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ProjectStartMode {
    New,
    Existing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StoredAppMode {
    ProjectStart {
        project_start_mode: ProjectStartMode,
    },
    ProjectOpen {
        project: Box<Project>,
        project_directory: PathBuf,
        ui_state: project_open::UiState,
    },
    Error {
        message: String,
    },
}

impl AhessApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let restored_app_mode = restore_app_mode(&workspace_root);
        let app_mode = if FORCE_ERROR_VIEW {
            AppMode::Error {
                message: "preview error: failed to restore open project: invalid project config at projects/arc-light/project.toml".to_string(),
            }
        } else {
            AppMode::from_restored(restored_app_mode, &workspace_root, cx)
        };

        Self {
            workspace_root,
            app_mode,
        }
    }

    fn on_project_open_msg(
        &mut self,
        _: Entity<project_open::Model>,
        msg: &project_open::Msg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            project_open::Msg::CloseRequested => {
                self.set_project_start_mode(ProjectStartMode::Existing, cx);
            }
            project_open::Msg::UiStateChanged => self.persist_storage(cx),
        }
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

    fn on_tuning_systems_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let workspace_root = self.workspace_root.clone();
        let model = cx.new(move |cx| tuning_system_editor::Model::new(workspace_root, cx));
        cx.subscribe(&model, Self::on_tuning_system_editor_msg)
            .detach();
        self.app_mode = AppMode::TuningSystems(model);
        cx.notify();
    }

    fn on_tuning_system_editor_msg(
        &mut self,
        _: Entity<tuning_system_editor::Model>,
        msg: &tuning_system_editor::Msg,
        cx: &mut Context<Self>,
    ) {
        match msg {
            tuning_system_editor::Msg::CloseRequested => {
                self.set_project_start_mode(ProjectStartMode::New, cx);
            }
        }
    }

    fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        let AppMode::ProjectOpen(model) = &self.app_mode else {
            return;
        };
        model.update(cx, |model, cx| model.toggle_playback(cx));
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let AppMode::ProjectOpen(model) = &self.app_mode else {
            return;
        };
        model.update(cx, |model, cx| model.undo(cx));
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let AppMode::ProjectOpen(model) = &self.app_mode else {
            return;
        };
        model.update(cx, |model, cx| model.redo(cx));
    }

    fn on_new_project_opened(
        &mut self,
        _: Entity<NewProjectDialog>,
        project: &ProjectOpened,
        cx: &mut Context<Self>,
    ) {
        self.open_project(project, cx);
    }

    fn on_existing_project_opened(
        &mut self,
        _: Entity<OpenProjectDialog>,
        project: &ProjectOpened,
        cx: &mut Context<Self>,
    ) {
        self.open_project(project, cx);
    }

    fn open_project(&mut self, project: &ProjectOpened, cx: &mut Context<Self>) {
        self.app_mode = match project::load_project(&project.project_directory) {
            Ok(project) => {
                let model = cx.new(|cx| {
                    project_open::Model::new(
                        project.project,
                        project.project_directory,
                        self.workspace_root.clone(),
                        cx,
                    )
                });
                cx.subscribe(&model, Self::on_project_open_msg).detach();
                AppMode::ProjectOpen(model)
            }
            Err(error) => AppMode::Error {
                message: error.to_string(),
            },
        };
        self.persist_storage(cx);
    }

    fn set_project_start_mode(&mut self, mode: ProjectStartMode, cx: &mut Context<Self>) {
        let changed = match &self.app_mode {
            AppMode::ProjectStart(project_start) => project_start.project_start_mode != mode,
            AppMode::ProjectOpen(_) => true,
            AppMode::TuningSystems(_) => true,
            AppMode::Error { .. } => true,
        };

        match &mut self.app_mode {
            AppMode::ProjectStart(project_start) => {
                project_start.set_project_start_mode(mode, cx);
            }
            AppMode::ProjectOpen(_) | AppMode::TuningSystems(_) | AppMode::Error { .. } => {
                self.app_mode =
                    AppMode::ProjectStart(ProjectStart::new(&self.workspace_root, mode, cx));
            }
        }

        if changed {
            self.persist_storage(cx);
        }
    }

    fn persist_storage(&mut self, cx: &mut Context<Self>) {
        let Some(storage) = Storage::generate(self, cx) else {
            return;
        };

        if let Err(error) = save_storage(&self.workspace_root, &storage) {
            self.app_mode = AppMode::Error {
                message: error.to_string(),
            };
        }
        cx.notify();
    }
}

impl AppMode {
    fn from_restored(
        restored_app_mode: StoredAppMode,
        workspace_root: &Path,
        cx: &mut Context<AhessApp>,
    ) -> Self {
        match restored_app_mode {
            StoredAppMode::ProjectStart { project_start_mode } => {
                Self::ProjectStart(ProjectStart::new(workspace_root, project_start_mode, cx))
            }
            StoredAppMode::ProjectOpen {
                project,
                project_directory,
                ui_state,
            } => {
                let model = cx.new(|cx| {
                    project_open::Model::new_with_ui_state(
                        *project,
                        project_directory,
                        workspace_root.to_path_buf(),
                        ui_state,
                        cx,
                    )
                });
                cx.subscribe(&model, AhessApp::on_project_open_msg).detach();
                Self::ProjectOpen(model)
            }
            StoredAppMode::Error { message } => Self::Error { message },
        }
    }
}

impl ProjectStart {
    fn new(
        workspace_root: &Path,
        project_start_mode: ProjectStartMode,
        cx: &mut Context<AhessApp>,
    ) -> Self {
        let new_project_workspace_root = workspace_root.to_path_buf();
        let open_project_workspace_root = workspace_root.to_path_buf();
        let new_project_dialog =
            cx.new(move |cx| NewProjectDialog::new(new_project_workspace_root, cx));
        let open_project_dialog =
            cx.new(move |cx| OpenProjectDialog::new(open_project_workspace_root, cx));
        let buttons = ProjectStartButtons::new(cx, project_start_mode);

        cx.subscribe(&new_project_dialog, AhessApp::on_new_project_opened)
            .detach();
        cx.subscribe(&open_project_dialog, AhessApp::on_existing_project_opened)
            .detach();
        cx.subscribe(&buttons.new_project, AhessApp::on_new_project_clicked)
            .detach();
        cx.subscribe(
            &buttons.open_existing,
            AhessApp::on_existing_project_clicked,
        )
        .detach();
        cx.subscribe(&buttons.tuning_systems, AhessApp::on_tuning_systems_clicked)
            .detach();

        Self {
            project_start_mode,
            new_project_dialog,
            open_project_dialog,
            buttons,
        }
    }

    fn set_project_start_mode(&mut self, mode: ProjectStartMode, cx: &mut Context<AhessApp>) {
        self.project_start_mode = mode;
        self.buttons.set_project_start_mode(mode, cx);

        if mode == ProjectStartMode::Existing {
            self.open_project_dialog.update(cx, |dialog, cx| {
                dialog.refresh(cx);
            });
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
enum Storage {
    ProjectStart {
        project_start_mode: ProjectStartMode,
    },
    ProjectOpen {
        project_directory: PathBuf,
        #[serde(default)]
        ui_state: project_open::UiState,
    },
}

#[derive(Debug)]
enum StorageError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Serialize {
        source: toml::ser::Error,
    },
    LoadProject(project::LoadProjectError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read storage at {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(f, "failed to write storage at {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "invalid storage at {}: {source}", path.display())
            }
            Self::Serialize { source } => write!(f, "failed to serialize storage: {source}"),
            Self::LoadProject(error) => write!(f, "failed to restore open project: {error}"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Serialize { source } => Some(source),
            Self::LoadProject(error) => Some(error),
        }
    }
}

fn restore_app_mode(workspace_root: &Path) -> StoredAppMode {
    match load_storage(workspace_root).and_then(|storage| {
        storage
            .map(|storage| storage.into_restored_app_mode(workspace_root))
            .transpose()
    }) {
        Ok(Some(app_mode)) => app_mode,
        Ok(None) => StoredAppMode::ProjectStart {
            project_start_mode: ProjectStartMode::New,
        },
        Err(error) => StoredAppMode::Error {
            message: error.to_string(),
        },
    }
}

fn load_storage(workspace_root: &Path) -> Result<Option<Storage>, StorageError> {
    let path = storage_path(workspace_root);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(StorageError::Read { path, source }),
    };
    let storage = toml::from_str::<Storage>(&contents)
        .map_err(|source| StorageError::Parse { path, source })?;

    Ok(Some(storage))
}

fn save_storage(workspace_root: &Path, storage: &Storage) -> Result<(), StorageError> {
    let path = storage_path(workspace_root);
    let contents =
        toml::to_string_pretty(storage).map_err(|source| StorageError::Serialize { source })?;

    fs::write(&path, contents).map_err(|source| StorageError::Write { path, source })
}

fn storage_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(APP_STATE_FILE)
}

impl Storage {
    fn generate(app: &AhessApp, cx: &Context<AhessApp>) -> Option<Self> {
        let storage = match &app.app_mode {
            AppMode::ProjectStart(project_start) => Storage::ProjectStart {
                project_start_mode: project_start.project_start_mode,
            },
            AppMode::ProjectOpen(model) => {
                let model = model.read(cx);
                let project_directory = model.project_directory();
                Storage::ProjectOpen {
                    project_directory: project_directory
                        .strip_prefix(&app.workspace_root)
                        .unwrap_or(project_directory)
                        .to_path_buf(),
                    ui_state: model.ui_state(),
                }
            }
            AppMode::TuningSystems(_) => Storage::ProjectStart {
                project_start_mode: ProjectStartMode::New,
            },
            AppMode::Error { .. } => return None,
        };

        Some(storage)
    }

    fn into_restored_app_mode(self, workspace_root: &Path) -> Result<StoredAppMode, StorageError> {
        match self {
            Storage::ProjectStart { project_start_mode } => {
                Ok(StoredAppMode::ProjectStart { project_start_mode })
            }
            Storage::ProjectOpen {
                project_directory,
                ui_state,
            } => {
                let project_directory = if project_directory.is_absolute() {
                    project_directory
                } else {
                    workspace_root.join(project_directory)
                };
                let project =
                    project::load_project(&project_directory).map_err(StorageError::LoadProject)?;

                Ok(StoredAppMode::ProjectOpen {
                    project: Box::new(project.project),
                    project_directory: project.project_directory,
                    ui_state,
                })
            }
        }
    }
}

impl Render for AhessApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = match &self.app_mode {
            AppMode::ProjectStart(project_start) => {
                AppFrame::new(project_start_screen(project_start), "")
            }
            AppMode::ProjectOpen(model) => {
                let project_model = model.read(cx);
                AppFrame::new(model.clone(), project_model.project().name.clone())
                    .with_actions(project_model.bar_actions())
                    .with_overlay(project_model.active_overlay())
            }
            AppMode::TuningSystems(model) => AppFrame::new(model.clone(), "tuning systems")
                .with_actions(model.read(cx).bar_actions()),
            AppMode::Error { message } => {
                AppFrame::new(error_screen(message.clone().into()), "error")
            }
        };

        let AppFrame {
            content,
            project_title,
            actions,
            overlay,
        } = frame;

        let content = div()
            .relative()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .children(content);

        let app_frame = div()
            .relative()
            .size_full()
            .font_family(s::FONT)
            .text_size(s::TEXT_SIZE)
            .line_height(s::TEXT_LINE_HEIGHT)
            .bg(s::GREEN2)
            .text_color(s::TEXT_DEFAULT)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .child(project_bar(project_title, actions))
                    .child(content),
            );

        if let Some(overlay) = overlay {
            app_frame.child(modal_overlay(overlay))
        } else {
            app_frame
        }
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let startup_error = Rc::new(Cell::new(None));
    let startup_error_from_callback = Rc::clone(&startup_error);

    Application::new().run(move |cx: &mut App| {
        #[cfg(target_os = "macos")]
        crate::macos::set_application_icon();

        bind_keys(cx);

        let app = cx.new(AhessApp::new);
        let window_app = app.clone();
        if let Err(error) = cx.open_window(WindowOptions::default(), move |window, _| {
            window.set_window_title("ahess");
            window_app
        }) {
            startup_error_from_callback.set(Some(io::Error::other(error)));
            cx.quit();
            return;
        }
        register_actions(app, cx);
    });

    match startup_error.take() {
        Some(error) => Err(Box::new(error)),
        None => Ok(()),
    }
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("secondary-p", TogglePlayback, None),
        KeyBinding::new("secondary-z", Undo, None),
        KeyBinding::new("secondary-shift-z", Redo, None),
    ]);
    view::text_input::bind_keys(cx);
}

fn register_actions(app: Entity<AhessApp>, cx: &mut App) {
    let undo_app = app.downgrade();
    let redo_app = app.downgrade();
    let app = app.downgrade();
    cx.on_action(move |_: &TogglePlayback, cx| {
        app.update(cx, |app, cx| app.toggle_playback(cx)).ok();
    });
    cx.on_action(move |_: &Undo, cx| {
        undo_app.update(cx, |app, cx| app.undo(cx)).ok();
    });
    cx.on_action(move |_: &Redo, cx| {
        redo_app.update(cx, |app, cx| app.redo(cx)).ok();
    });
}

struct AppFrame {
    content: Vec<AnyElement>,
    project_title: SharedString,
    actions: Vec<AnyElement>,
    overlay: Option<AnyElement>,
}

impl AppFrame {
    fn new(content: impl IntoElement, project_title: impl Into<SharedString>) -> Self {
        Self {
            content: vec![content.into_any_element()],
            project_title: project_title.into(),
            actions: Vec::new(),
            overlay: None,
        }
    }

    fn with_actions(mut self, actions: Vec<AnyElement>) -> Self {
        self.actions = actions;
        self
    }

    fn with_overlay(mut self, overlay: Option<AnyElement>) -> Self {
        self.overlay = overlay;
        self
    }
}

fn project_bar(project_title: SharedString, actions: Vec<AnyElement>) -> impl IntoElement {
    let bar = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(s::S5)
        .border_b_2()
        .border_color(s::GRAY1)
        .bg(s::GRAY2)
        .px(s::S5)
        .py(s::S4)
        .child(
            div()
                .flex()
                .flex_1()
                .min_w(s::S0)
                .items_center()
                .gap_3()
                .child(div().flex_none().text_color(s::TEXT_HEADER).child("ahess"))
                .child(
                    div()
                        .min_w(s::S0)
                        .truncate()
                        .text_color(s::TEXT_DEFAULT)
                        .child(project_title),
                ),
        );

    if actions.is_empty() {
        bar
    } else {
        bar.child(
            div()
                .flex()
                .flex_none()
                .gap(s::S5)
                .my(-s::S3)
                .children(actions),
        )
    }
}

struct ProjectStartButtons {
    new_project: Entity<Button>,
    open_existing: Entity<Button>,
    tuning_systems: Entity<Button>,
}

impl ProjectStartButtons {
    fn new(cx: &mut Context<AhessApp>, mode: ProjectStartMode) -> Self {
        Self {
            new_project: cx.new(|_| {
                Button::new("new-project", "new project").depressed(mode == ProjectStartMode::New)
            }),
            open_existing: cx.new(|_| {
                Button::new("open-existing", "open existing")
                    .depressed(mode == ProjectStartMode::Existing)
            }),
            tuning_systems: cx.new(|_| Button::new("tuning-systems", "tuning system editor")),
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

fn project_start_screen(project_start: &ProjectStart) -> gpui::Div {
    let project_dialog = match project_start.project_start_mode {
        ProjectStartMode::New => project_start.new_project_dialog.clone().into_any_element(),
        ProjectStartMode::Existing => project_start.open_project_dialog.clone().into_any_element(),
    };

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
                .child(project_picker_dialog(&project_start.buttons))
                .child(project_dialog),
        )
}

fn project_picker_dialog(buttons: &ProjectStartButtons) -> impl IntoElement {
    let image_width = s::S10 - s::CONTENT_PADDING * 2.0;
    let image_height = image_width * AHESS_IMAGE_HEIGHT_RATIO;

    s::raised(
        div().flex().flex_col().w(s::S10).bg(s::GRAY2).child(
            div()
                .flex()
                .flex_col()
                .gap(s::CONTENT_PADDING)
                .p(s::CONTENT_PADDING)
                .child(
                    s::sunken(img(ahess_image()).size_full().object_fit(ObjectFit::Fill))
                        .w(image_width)
                        .h(image_height)
                        .overflow_hidden(),
                )
                .child(
                    button::action_group([
                        buttons.new_project.clone(),
                        buttons.open_existing.clone(),
                        buttons.tuning_systems.clone(),
                    ])
                    .justify_center(),
                ),
        ),
    )
}

fn ahess_image() -> Arc<Image> {
    AHESS_IMAGE
        .get_or_init(|| {
            Arc::new(Image::from_bytes(
                ImageFormat::Png,
                include_bytes!("../ahess_image.png").to_vec(),
            ))
        })
        .clone()
}

fn error_screen(message: SharedString) -> gpui::Div {
    div()
        .flex()
        .flex_1()
        .min_h(px(0.0))
        .items_center()
        .justify_center()
        .bg(s::GREEN2)
        .p(s::S7)
        .child(
            s::raised(
                div()
                    .flex()
                    .flex_col()
                    .bg(s::RED1)
                    .text_color(s::WHITE)
                    .child(
                        div()
                            .bg(s::GRAY5)
                            .text_color(s::DIALOG_TITLE_TEXT)
                            .p(s::S3)
                            .px(s::S4)
                            .child("error"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .p(s::CONTENT_PADDING)
                            .child(
                                "ahess experienced a critical error that prevented it from starting.",
                            )
                            .child(
                                s::sunken(
                                    div()
                                        .w_full()
                                        .bg(s::GREEN3)
                                        .text_color(s::TEXT_DEFAULT)
                                        .p(s::CONTENT_PADDING)
                                        .child(message),
                                )
                                .overflow_hidden(),
                            ),
                    ),
            )
            .w(s::S10),
        )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use gpui::{div, prelude::*, Context, Entity, FocusHandle, TestAppContext, Window};

    use super::project_open::{UiState, WorkspaceSectionKind};
    use super::{
        bind_keys, load_storage, restore_app_mode, save_storage, storage_path, ProjectStartMode,
        Redo, Storage, StoredAppMode, TogglePlayback, Undo,
    };
    use crate::{
        project::{self, Project},
        seed::Seed,
        view::text_input::TextInput,
    };

    #[test]
    fn storage_round_trips_the_project_start_mode() {
        let root = temp_root("storage-start-mode");
        let storage = Storage::ProjectStart {
            project_start_mode: ProjectStartMode::Existing,
        };

        save_storage(&root, &storage).unwrap();

        assert_eq!(load_storage(&root).unwrap(), Some(storage));
        assert!(fs::read_to_string(storage_path(&root))
            .unwrap()
            .contains("project_start_mode = \"existing\""));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn storage_restores_an_open_project_from_its_project_config() {
        let root = temp_root("storage-open-project");
        let project = Project::new("Arc Light Sketch", 4000, 100, Seed::new(1234))
            .with_description("saved project");
        let project_directory = project::create_project(&root, &project).unwrap();
        let ui_state = UiState {
            workspace: WorkspaceSectionKind::Parts,
            score_pane_count: 2,
            open_score_parts: vec!["intro".to_string(), "verse".to_string()],
            active_score_pane: 1,
            score_arrangement_visible: false,
        };
        let storage = Storage::ProjectOpen {
            project_directory: project_directory.strip_prefix(&root).unwrap().to_path_buf(),
            ui_state: ui_state.clone(),
        };

        save_storage(&root, &storage).unwrap();

        assert_eq!(
            restore_app_mode(&root),
            StoredAppMode::ProjectOpen {
                project: Box::new(project),
                project_directory,
                ui_state,
            }
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_open_project_storage_uses_the_default_ui_state() {
        let root = temp_root("legacy-open-project-storage");
        let project = Project::new("Arc Light Sketch", 4000, 100, Seed::new(1234));
        let project_directory = project::create_project(&root, &project).unwrap();
        fs::write(
            storage_path(&root),
            format!(
                "[ProjectOpen]\nproject_directory = {:?}\n",
                project_directory.strip_prefix(&root).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(
            restore_app_mode(&root),
            StoredAppMode::ProjectOpen {
                project: Box::new(project),
                project_directory,
                ui_state: UiState::default(),
            }
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_storage_defaults_to_new_project_mode() {
        let root = temp_root("missing-storage");

        assert_eq!(load_storage(&root).unwrap(), None);
        assert_eq!(
            restore_app_mode(&root),
            StoredAppMode::ProjectStart {
                project_start_mode: ProjectStartMode::New,
            }
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_storage_restores_to_error_mode() {
        let root = temp_root("invalid-storage");

        fs::write(storage_path(&root), "app_mode =").unwrap();

        assert!(matches!(
            restore_app_mode(&root),
            StoredAppMode::Error { message } if message.contains("invalid storage")
        ));

        fs::remove_dir_all(root).unwrap();
    }

    struct PlaybackShortcutHost {
        project_focus: FocusHandle,
        text_input_focus: FocusHandle,
        toggle_count: usize,
    }

    impl Render for PlaybackShortcutHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().track_focus(&self.project_focus).child(
                div()
                    .key_context("TextInput")
                    .track_focus(&self.text_input_focus),
            )
        }
    }

    #[gpui::test]
    fn command_p_toggles_playback_with_or_without_focus(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (host, cx) = cx.add_window_view(|_, cx| PlaybackShortcutHost {
            project_focus: cx.focus_handle(),
            text_input_focus: cx.focus_handle(),
            toggle_count: 0,
        });
        let weak_host = host.downgrade();
        cx.update(move |_, cx| {
            cx.on_action(move |_: &TogglePlayback, cx| {
                weak_host.update(cx, |host, _| host.toggle_count += 1).ok();
            });
        });

        cx.update(|window, cx| host.read(cx).project_focus.focus(window));
        cx.simulate_keystrokes("secondary-p");
        assert_eq!(cx.update(|_, cx| host.read(cx).toggle_count), 1);

        cx.update(|window, cx| host.read(cx).text_input_focus.focus(window));
        cx.simulate_keystrokes("secondary-p");
        assert_eq!(cx.update(|_, cx| host.read(cx).toggle_count), 2);

        cx.update(|window, _| window.blur());
        cx.simulate_keystrokes("secondary-p");
        assert_eq!(cx.update(|_, cx| host.read(cx).toggle_count), 3);

        cx.simulate_keystrokes("space");
        assert_eq!(cx.update(|_, cx| host.read(cx).toggle_count), 3);
    }

    struct HistoryShortcutHost {
        input: Entity<TextInput>,
        undo_count: usize,
        redo_count: usize,
    }

    impl Render for HistoryShortcutHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().child(self.input.clone())
        }
    }

    #[gpui::test]
    fn command_z_reaches_global_history_while_a_text_input_is_focused(cx: &mut TestAppContext) {
        cx.update(bind_keys);
        let (host, cx) = cx.add_window_view(|_, cx| HistoryShortcutHost {
            input: cx.new(|cx| TextInput::new("score", "", cx)),
            undo_count: 0,
            redo_count: 0,
        });
        let undo_host = host.downgrade();
        let redo_host = host.downgrade();
        cx.update(move |_, cx| {
            cx.on_action(move |_: &Undo, cx| {
                undo_host.update(cx, |host, _| host.undo_count += 1).ok();
            });
            cx.on_action(move |_: &Redo, cx| {
                redo_host.update(cx, |host, _| host.redo_count += 1).ok();
            });
        });

        cx.update(|window, cx| host.read(cx).input.read(cx).focus(window));
        cx.simulate_keystrokes("secondary-z secondary-shift-z");
        assert_eq!(cx.update(|_, cx| host.read(cx).undo_count), 1);
        assert_eq!(cx.update(|_, cx| host.read(cx).redo_count), 1);
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
