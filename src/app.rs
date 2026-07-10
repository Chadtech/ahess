use std::{borrow::Cow, path::PathBuf};

use gpui::{
    div, prelude::*, px, App, Application, Context, Entity, SharedString, Window, WindowOptions,
};

use crate::{
    new_project::{NewProjectDialog, ProjectOpened},
    style as s,
    view::{
        self,
        button::{self, Button},
    },
};

const UNIFRAKTUR_MAGUNTIA: &[u8] = include_bytes!("../assets/fonts/UnifrakturMaguntia-Regular.ttf");

struct AhessApp {
    new_project_dialog: Entity<NewProjectDialog>,
    project_start_buttons: ProjectStartButtons,
    app_mode: AppMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AppMode {
    ProjectStart {
        project_start_mode: ProjectStartMode,
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
        let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let new_project_dialog = cx.new(move |cx| NewProjectDialog::new(workspace_root, cx));
        let project_start_buttons = ProjectStartButtons::new(cx, project_start_mode);

        cx.subscribe(&new_project_dialog, Self::on_project_opened)
            .detach();
        cx.subscribe(
            &project_start_buttons.new_project,
            Self::on_new_project_clicked,
        )
        .detach();
        cx.subscribe(
            &project_start_buttons.open_existing,
            Self::on_existing_project_clicked,
        )
        .detach();

        Self {
            new_project_dialog,
            project_start_buttons,
            app_mode: AppMode::ProjectStart { project_start_mode },
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

    fn on_project_opened(
        &mut self,
        _: Entity<NewProjectDialog>,
        project: &ProjectOpened,
        cx: &mut Context<Self>,
    ) {
        self.app_mode = AppMode::ProjectOpen {
            project_name: project.project_name.clone(),
            project_directory: project.project_directory.clone(),
        };
        cx.notify();
    }

    fn set_project_start_mode(&mut self, mode: ProjectStartMode, cx: &mut Context<Self>) {
        let changed = match &self.app_mode {
            AppMode::ProjectStart { project_start_mode } => *project_start_mode != mode,
            AppMode::ProjectOpen { .. } => true,
        };

        self.app_mode = AppMode::ProjectStart {
            project_start_mode: mode,
        };
        self.project_start_buttons.set_project_start_mode(mode, cx);

        if changed {
            cx.notify();
        }
    }

    fn project_title(&self) -> SharedString {
        match &self.app_mode {
            AppMode::ProjectStart { .. } => "".into(),
            AppMode::ProjectOpen { project_name, .. } => project_name.clone().into(),
        }
    }
}

impl Render for AhessApp {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let screen = match &self.app_mode {
            AppMode::ProjectStart { .. } => {
                project_start_screen(&self.new_project_dialog, &self.project_start_buttons)
                    .into_any_element()
            }
            AppMode::ProjectOpen {
                project_directory, ..
            } => project_workspace(project_directory).into_any_element(),
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

fn project_start_screen(
    new_project_dialog: &Entity<NewProjectDialog>,
    buttons: &ProjectStartButtons,
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
                .child(new_project_dialog.clone()),
        )
}

fn project_picker_dialog(buttons: &ProjectStartButtons) -> impl IntoElement {
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

fn project_workspace(_project_directory: &PathBuf) -> gpui::Div {
    div().flex_1().min_h(px(0.0)).bg(s::GREEN2)
}
