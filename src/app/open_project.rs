use std::path::PathBuf;

use gpui::{
    div, prelude::*, Context, CursorStyle, Entity, EventEmitter, MouseButton, MouseDownEvent,
    SharedString, Window,
};

use crate::{
    project::{self, ProjectEntry, ProjectOpened},
    style as s,
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
    },
};

pub struct OpenProjectDialog {
    projects: Vec<ProjectEntry>,
    selected_project: Option<usize>,
    open_button: Entity<Button>,
    open_project_error: Option<String>,
    workspace_root: PathBuf,
}

impl EventEmitter<ProjectOpened> for OpenProjectDialog {}

impl OpenProjectDialog {
    pub fn new(workspace_root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Self {
        let workspace_root = workspace_root.into();
        let open_button = cx.new(|_| Button::new("open-selected-project", "open project"));
        let (projects, selected_project, open_project_error) = load_projects(&workspace_root);

        cx.subscribe(&open_button, Self::on_open_project_clicked)
            .detach();

        Self {
            projects,
            selected_project,
            open_button,
            open_project_error,
            workspace_root,
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let selected_directory = self
            .selected_project()
            .map(|entry| entry.project_directory.clone());
        let (projects, selected_project, open_project_error) = load_projects(&self.workspace_root);

        self.projects = projects;
        self.selected_project = selected_directory
            .and_then(|directory| {
                self.projects
                    .iter()
                    .position(|entry| entry.project_directory == directory)
            })
            .or(selected_project);
        self.open_project_error = open_project_error;
        cx.notify();
    }

    fn on_open_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.selected_project() else {
            self.open_project_error = Some("choose a project to open".to_string());
            cx.notify();
            return;
        };

        cx.emit(ProjectOpened {
            project_name: project.project.name.clone(),
            project_directory: project.project_directory.clone(),
        });
    }

    fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.projects.len() || self.selected_project == Some(index) {
            return;
        }

        self.selected_project = Some(index);
        self.open_project_error = None;
        cx.notify();
    }

    fn selected_project(&self) -> Option<&ProjectEntry> {
        self.selected_project
            .and_then(|index| self.projects.get(index))
    }
}

impl Render for OpenProjectDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        open_project_dialog(
            &self.projects,
            self.selected_project,
            self.selected_project(),
            self.open_button.clone(),
            self.open_project_error.clone(),
            cx,
        )
    }
}

fn load_projects(workspace_root: &PathBuf) -> (Vec<ProjectEntry>, Option<usize>, Option<String>) {
    match project::list_projects(workspace_root) {
        Ok(projects) => {
            let selected_project = (!projects.is_empty()).then_some(0);
            (projects, selected_project, None)
        }
        Err(error) => (Vec::new(), None, Some(error.to_string())),
    }
}

fn open_project_dialog(
    projects: &[ProjectEntry],
    selected_project: Option<usize>,
    selected_project_entry: Option<&ProjectEntry>,
    open_button: Entity<Button>,
    open_project_error: Option<String>,
    cx: &mut Context<OpenProjectDialog>,
) -> impl IntoElement {
    let body = div()
        .flex()
        .gap_5()
        .p(s::CONTENT_PADDING)
        .child(project_list(projects, selected_project, cx))
        .child(project_details(selected_project_entry));

    let body = if let Some(error) = open_project_error {
        body.child(error_message(error))
    } else {
        body
    };

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S10)
            .bg(s::GRAY2)
            .child(title_bar("open project", None))
            .child(body)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_3()
                    .p(s::CONTENT_PADDING)
                    .child(open_button),
            ),
    )
}

fn project_list(
    projects: &[ProjectEntry],
    selected_project: Option<usize>,
    cx: &mut Context<OpenProjectDialog>,
) -> gpui::Div {
    let rows = projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            project_list_row(index, project, selected_project == Some(index), cx)
        })
        .collect::<Vec<_>>();

    let list_body = div().flex().flex_col().min_h(s::S9).bg(s::GREEN3);

    let list_body = if rows.is_empty() {
        list_body.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(s::GREEN6)
                .child("no projects found"),
        )
    } else {
        list_body.children(rows)
    };

    div()
        .flex()
        .flex_col()
        .gap_1()
        .w(s::S9)
        .child(div().text_color(s::TEXT_HEADER).child("projects"))
        .child(s::sunken(list_body).overflow_hidden())
}

fn project_list_row(
    index: usize,
    project: &ProjectEntry,
    selected: bool,
    cx: &mut Context<OpenProjectDialog>,
) -> gpui::Div {
    let background = if selected {
        s::GREEN4
    } else if index % 2 == 0 {
        s::GREEN2
    } else {
        s::GREEN3
    };
    let name_color = if selected { s::GRAY6 } else { s::GRAY5 };

    div()
        .flex()
        .flex_col()
        .bg(background)
        .p(s::S4)
        .cursor(CursorStyle::PointingHand)
        .child(
            div()
                .text_color(name_color)
                .child(project.project.name.clone()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
                dialog.select_project(index, cx);
            }),
        )
}

fn project_details(project: Option<&ProjectEntry>) -> gpui::Div {
    let details = match project {
        Some(project) => selected_project_details(project),
        None => div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(s::GREEN6)
            .child("select a project"),
    };

    div()
        .flex()
        .flex_col()
        .gap_4()
        .flex_1()
        .min_h(s::S9)
        .child(details)
}

fn selected_project_details(project: &ProjectEntry) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .text_color(s::TEXT_DEFAULT)
                .child(project.project.name.clone()),
        )
        .child(detail_row(
            "description",
            project.project.description.clone(),
        ))
        .child(div().flex().gap_4().children([
            metric("beat length", project.project.beat_length.to_string()),
            metric("variance", project.project.timing_variance.to_string()),
            metric("seed", project.project.seed.value().to_string()),
        ]))
}

fn detail_row(label: &'static str, value: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_color(s::TEXT_HEADER).child(label))
        .child(div().text_color(s::TEXT_DEFAULT).child(value.into()))
}

fn metric(label: &'static str, value: impl Into<SharedString>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .child(div().text_color(s::TEXT_HEADER).child(label))
        .child(div().text_color(s::TEXT_DEFAULT).child(value.into()))
}
