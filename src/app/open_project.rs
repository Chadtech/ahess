use std::path::{Path, PathBuf};

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
        field_group::field_group,
        text_input::TextInput,
    },
};

pub struct OpenProjectDialog {
    workspace_root: PathBuf,
    view: DialogView,
}

enum DialogView {
    Browse(BrowseView),
    Duplicate {
        source: Box<ProjectEntry>,
        name: Entity<TextInput>,
        cancel_button: Entity<Button>,
        duplicate_button: Entity<Button>,
        form_error: Option<String>,
    },
}

struct BrowseView {
    projects: Vec<ProjectEntry>,
    selected_project: Option<usize>,
    open_button: Entity<Button>,
    duplicate_button: Entity<Button>,
    open_project_error: Option<String>,
}

impl BrowseView {
    fn selected_project(&self) -> Option<&ProjectEntry> {
        self.selected_project
            .and_then(|index| self.projects.get(index))
    }
}

impl EventEmitter<ProjectOpened> for OpenProjectDialog {}

impl OpenProjectDialog {
    pub fn new(workspace_root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Self {
        let workspace_root = workspace_root.into();
        let view = DialogView::Browse(Self::browse_view(&workspace_root, None, cx));

        Self {
            workspace_root,
            view,
        }
    }

    fn browse_view(
        workspace_root: &Path,
        preferred_project: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> BrowseView {
        let (projects, selected_project, open_project_error) = load_projects(workspace_root);
        let selected_project = preferred_project
            .and_then(|directory| {
                projects
                    .iter()
                    .position(|entry| entry.project_directory == directory)
            })
            .or(selected_project);
        let no_projects = projects.is_empty();
        let open_button =
            cx.new(|_| Button::new("open-selected-project", "open project").disabled(no_projects));
        let duplicate_button = cx.new(|_| {
            Button::new("duplicate-selected-project", "duplicate project").disabled(no_projects)
        });

        cx.subscribe(&open_button, Self::on_open_project_clicked)
            .detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_project_clicked)
            .detach();

        BrowseView {
            projects,
            selected_project,
            open_button,
            duplicate_button,
            open_project_error,
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let selected_directory = match &self.view {
            DialogView::Browse(view) => view
                .selected_project()
                .map(|entry| entry.project_directory.clone()),
            DialogView::Duplicate { source, .. } => Some(source.project_directory.clone()),
        };
        self.view = DialogView::Browse(Self::browse_view(
            &self.workspace_root,
            selected_directory.as_deref(),
            cx,
        ));
        cx.notify();
    }

    fn on_open_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let DialogView::Browse(view) = &mut self.view else {
            return;
        };
        let Some(project) = view.selected_project() else {
            view.open_project_error = Some("choose a project to open".to_string());
            cx.notify();
            return;
        };

        cx.emit(ProjectOpened {
            project_name: project.project.name.clone(),
            project_directory: project.project_directory.clone(),
        });
    }

    fn on_duplicate_project_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let DialogView::Browse(view) = &mut self.view else {
            return;
        };
        let Some(source) = view.selected_project().cloned() else {
            view.open_project_error = Some("choose a project to duplicate".to_string());
            cx.notify();
            return;
        };
        let suggested_name = format!("{} copy", source.project.name);
        let name = cx.new(|cx| TextInput::new("", suggested_name, cx));
        let cancel_button = cx.new(|_| Button::new("cancel-duplicate-project", "cancel"));
        let duplicate_button =
            cx.new(|_| Button::new("confirm-duplicate-project", "duplicate and open"));
        cx.subscribe(&cancel_button, Self::on_cancel_duplicate_clicked)
            .detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_confirmed)
            .detach();

        self.view = DialogView::Duplicate {
            source: Box::new(source),
            name,
            cancel_button,
            duplicate_button,
            form_error: None,
        };
        cx.notify();
    }

    fn on_cancel_duplicate_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let DialogView::Duplicate { source, .. } = &self.view else {
            return;
        };
        let preferred_project = source.project_directory.clone();
        self.view = DialogView::Browse(Self::browse_view(
            &self.workspace_root,
            Some(&preferred_project),
            cx,
        ));
        cx.notify();
    }

    fn on_duplicate_confirmed(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let DialogView::Duplicate { source, name, .. } = &self.view else {
            return;
        };
        let source = source.clone();
        let name = name.read(cx).value();

        match project::duplicate_project(&self.workspace_root, &source, &name) {
            Ok(duplicated) => cx.emit(ProjectOpened {
                project_name: duplicated.project.name,
                project_directory: duplicated.project_directory,
            }),
            Err(error) => {
                let DialogView::Duplicate { form_error, .. } = &mut self.view else {
                    unreachable!("the duplicate form cannot change while duplicating a project");
                };
                *form_error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        let DialogView::Browse(view) = &mut self.view else {
            return;
        };
        if index >= view.projects.len() || view.selected_project == Some(index) {
            return;
        }

        view.selected_project = Some(index);
        view.open_project_error = None;
        cx.notify();
    }
}

impl Render for OpenProjectDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            DialogView::Browse(view) => open_project_dialog(
                &view.projects,
                view.selected_project,
                view.selected_project(),
                view.duplicate_button.clone(),
                view.open_button.clone(),
                view.open_project_error.clone(),
                cx,
            ),
            DialogView::Duplicate {
                source,
                name,
                cancel_button,
                duplicate_button,
                form_error,
            } => duplicate_project_dialog(
                source,
                name.clone(),
                cancel_button.clone(),
                duplicate_button.clone(),
                form_error.clone(),
            ),
        }
    }
}

fn load_projects(workspace_root: &Path) -> (Vec<ProjectEntry>, Option<usize>, Option<String>) {
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
    duplicate_button: Entity<Button>,
    open_button: Entity<Button>,
    open_project_error: Option<String>,
    cx: &mut Context<OpenProjectDialog>,
) -> gpui::Div {
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
                    .child(duplicate_button)
                    .child(open_button),
            ),
    )
}

fn duplicate_project_dialog(
    source: &ProjectEntry,
    name: Entity<TextInput>,
    cancel_button: Entity<Button>,
    duplicate_button: Entity<Button>,
    form_error: Option<String>,
) -> gpui::Div {
    let form = div()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .text_color(s::TEXT_DEFAULT)
                .child(format!("copying {:?}", source.project.name)),
        )
        .child(field_group("new project name", name));
    let form = if let Some(error) = form_error {
        form.child(error_message(error))
    } else {
        form
    };

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S10)
            .bg(s::GRAY2)
            .child(title_bar("duplicate project", None))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(s::CONTENT_PADDING)
                    .p(s::CONTENT_PADDING)
                    .child(form)
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_3()
                            .child(cancel_button)
                            .child(duplicate_button),
                    ),
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
    } else if index.is_multiple_of(2) {
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

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::SystemTime};

    use gpui::TestAppContext;

    use super::{button, DialogView, OpenProjectDialog};
    use crate::{project, seed::Seed};

    #[gpui::test]
    fn duplicate_project_action_collects_a_name_and_creates_the_copy(cx: &mut TestAppContext) {
        let root = temp_root();
        project::create_project(
            &root,
            &project::Project::new("Original", 800, 0, Seed::new(1)),
        )
        .unwrap();
        let dialog_root = root.clone();
        let (dialog, cx) = cx.add_window_view(|_, cx| OpenProjectDialog::new(dialog_root, cx));

        let browse_duplicate_button = dialog.read_with(cx, |dialog, _| {
            let DialogView::Browse(view) = &dialog.view else {
                panic!("dialog should start in browse mode");
            };
            view.duplicate_button.clone()
        });
        dialog.update(cx, |dialog, cx| {
            dialog.on_duplicate_project_clicked(browse_duplicate_button, &button::Clicked, cx);
        });
        let (name, confirm_button) = dialog.read_with(cx, |dialog, _| {
            let DialogView::Duplicate {
                source,
                name,
                duplicate_button,
                ..
            } = &dialog.view
            else {
                panic!("duplicate action should open its naming form");
            };
            assert_eq!(source.project.name, "Original");
            (name.clone(), duplicate_button.clone())
        });
        name.update(cx, |name, cx| name.sync_value("Variation", cx));

        dialog.update(cx, |dialog, cx| {
            dialog.on_duplicate_confirmed(confirm_button, &button::Clicked, cx);
        });

        assert_eq!(
            project::load_project(root.join("projects").join("variation"))
                .unwrap()
                .project
                .name,
            "Variation"
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn temp_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ahess-open-project-duplicate-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
