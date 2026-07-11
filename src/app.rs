use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use gpui::{
    div, img, prelude::*, px, App, Application, Context, Entity, Image, ImageFormat, ObjectFit,
    Pixels, SharedString, Window, WindowOptions,
};
use serde::{Deserialize, Serialize};

use crate::{
    new_project::NewProjectDialog,
    open_project::OpenProjectDialog,
    project::{self, ProjectOpened},
    style as s,
    view::{
        self,
        button::{self, Button},
    },
};

const PROJECT_PICKER_WIDTH: Pixels = px(430.0);
const AHESS_IMAGE_HEIGHT_RATIO: f32 = 1086.0 / 1448.0;
const APP_STATE_FILE: &str = ".ahess-ui-state.toml";
const FORCE_ERROR_VIEW: bool = false;

static AHESS_IMAGE: OnceLock<Arc<Image>> = OnceLock::new();

struct AhessApp {
    workspace_root: PathBuf,
    close_project_button: Entity<Button>,
    app_mode: AppMode,
}

enum AppMode {
    ProjectStart(ProjectStart),
    ProjectOpen {
        project_name: String,
        project_directory: PathBuf,
    },
    Error {
        message: String,
    },
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
        project_name: String,
        project_directory: PathBuf,
    },
    Error {
        message: String,
    },
}

impl AhessApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let close_project_button = cx.new(|_| Button::new("close-project", "close project"));
        let restored_app_mode = restore_app_mode(&workspace_root);
        let app_mode = if FORCE_ERROR_VIEW {
            AppMode::Error {
                message: "preview error: failed to restore open project: invalid project config at projects/arc-light/project.toml".to_string(),
            }
        } else {
            AppMode::from_restored(restored_app_mode, &workspace_root, cx)
        };

        cx.subscribe(&close_project_button, Self::on_close_project_clicked)
            .detach();

        Self {
            workspace_root,
            close_project_button,
            app_mode,
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

    fn on_close_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.set_project_start_mode(ProjectStartMode::Existing, cx);
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
        self.app_mode = AppMode::ProjectOpen {
            project_name: project.project_name.clone(),
            project_directory: project.project_directory.clone(),
        };
        self.persist_storage(cx);
    }

    fn set_project_start_mode(&mut self, mode: ProjectStartMode, cx: &mut Context<Self>) {
        let changed = match &self.app_mode {
            AppMode::ProjectStart(project_start) => project_start.project_start_mode != mode,
            AppMode::ProjectOpen { .. } => true,
            AppMode::Error { .. } => true,
        };

        match &mut self.app_mode {
            AppMode::ProjectStart(project_start) => {
                project_start.set_project_start_mode(mode, cx);
            }
            AppMode::ProjectOpen { .. } | AppMode::Error { .. } => {
                self.app_mode =
                    AppMode::ProjectStart(ProjectStart::new(&self.workspace_root, mode, cx));
            }
        }

        if changed {
            self.persist_storage(cx);
        }
    }

    fn project_title(&self) -> SharedString {
        match &self.app_mode {
            AppMode::ProjectStart(_) => "".into(),
            AppMode::ProjectOpen { project_name, .. } => project_name.clone().into(),
            AppMode::Error { .. } => "error".into(),
        }
    }

    fn persist_storage(&mut self, cx: &mut Context<Self>) {
        let Some(storage) = Storage::generate(self) else {
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
                project_name,
                project_directory,
            } => Self::ProjectOpen {
                project_name,
                project_directory,
            },
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
    fn generate(app: &AhessApp) -> Option<Self> {
        let storage = match &app.app_mode {
            AppMode::ProjectStart(project_start) => Storage::ProjectStart {
                project_start_mode: project_start.project_start_mode,
            },
            AppMode::ProjectOpen {
                project_directory, ..
            } => Storage::ProjectOpen {
                project_directory: project_directory
                    .strip_prefix(&app.workspace_root)
                    .unwrap_or(project_directory)
                    .to_path_buf(),
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
            Storage::ProjectOpen { project_directory } => {
                let project_directory = if project_directory.is_absolute() {
                    project_directory
                } else {
                    workspace_root.join(project_directory)
                };
                let project =
                    project::load_project(&project_directory).map_err(StorageError::LoadProject)?;

                Ok(StoredAppMode::ProjectOpen {
                    project_name: project.project.name,
                    project_directory: project.project_directory,
                })
            }
        }
    }
}

impl Render for AhessApp {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen = match &self.app_mode {
            AppMode::ProjectStart(project_start) => {
                project_start_screen(project_start).into_any_element()
            }
            AppMode::ProjectOpen {
                project_directory, ..
            } => project_workspace(project_directory).into_any_element(),
            AppMode::Error { message } => error_screen(message.clone().into()).into_any_element(),
        };
        let project_title = self.project_title();
        let close_project_button = matches!(self.app_mode, AppMode::ProjectOpen { .. })
            .then(|| self.close_project_button.clone());

        div()
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
                    .child(project_bar(project_title, close_project_button))
                    .child(screen),
            )
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    Application::new().run(|cx: &mut App| {
        view::text_input::bind_keys(cx);

        cx.open_window(WindowOptions::default(), |window, cx| {
            window.set_window_title("ahess");
            cx.new(AhessApp::new)
        })
        .unwrap();
    });

    Ok(())
}

fn project_bar(
    project_title: SharedString,
    close_project_button: Option<Entity<Button>>,
) -> impl IntoElement {
    let bar = div()
        .flex()
        .items_center()
        .justify_between()
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
                .child(div().text_color(s::TEXT_HEADER).child("ahess"))
                .child(div().text_color(s::TEXT_DEFAULT).child(project_title)),
        );

    if let Some(close_project_button) = close_project_button {
        bar.child(close_project_button)
    } else {
        bar
    }
}

struct ProjectStartButtons {
    new_project: Entity<Button>,
    open_existing: Entity<Button>,
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
    let image_width = PROJECT_PICKER_WIDTH - s::CONTENT_PADDING * 2.0;
    let image_height = image_width * AHESS_IMAGE_HEIGHT_RATIO;

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(PROJECT_PICKER_WIDTH)
            .bg(s::GRAY2)
            .child(
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

fn project_workspace(_project_directory: &PathBuf) -> gpui::Div {
    div().flex_1().min_h(px(0.0)).bg(s::GREEN2)
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
            .w(px(680.0)),
        )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        load_storage, restore_app_mode, save_storage, storage_path, ProjectStartMode, Storage,
        StoredAppMode,
    };
    use crate::{
        project::{self, Project},
        seed::Seed,
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
        let storage = Storage::ProjectOpen {
            project_directory: project_directory.strip_prefix(&root).unwrap().to_path_buf(),
        };

        save_storage(&root, &storage).unwrap();

        assert_eq!(
            restore_app_mode(&root),
            StoredAppMode::ProjectOpen {
                project_name: project.name,
                project_directory,
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
