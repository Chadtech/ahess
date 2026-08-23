use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::{
    audio_build::{
        planned_build_files, AudioBuildResult, BuildSampleRate, PlannedBuildFile, BUILD_DIRECTORY,
    },
    project::Project,
    style as s,
    view::{
        button::{self, Button},
        dropdown::{self, Dropdown},
        field_group::compact_control_group,
        status_bar, workspace,
    },
};

pub(crate) enum Msg {
    Requested {
        request_id: u64,
        sample_rate: BuildSampleRate,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BuildState {
    Ready,
    Building {
        request_id: u64,
        project_changed: bool,
    },
    Built(AudioBuildResult),
    Failed(String),
    Stale,
}

pub(crate) struct BuildWorkspace {
    project: Project,
    sample_rate: BuildSampleRate,
    sample_rate_dropdown: Entity<Dropdown>,
    build_button: Entity<Button>,
    state: BuildState,
    next_request_id: u64,
}

impl EventEmitter<Msg> for BuildWorkspace {}

impl BuildWorkspace {
    pub(crate) fn new(project: Project, cx: &mut Context<Self>) -> Self {
        let sample_rate = BuildSampleRate::DEFAULT;
        let sample_rate_dropdown = cx.new(|cx| {
            Dropdown::new(
                "audio-build-sample-rate",
                BuildSampleRate::ALL.map(BuildSampleRate::label),
                sample_rate.index(),
                cx,
            )
        });
        let build_button = cx.new(|_| Button::new("build-audio", "build"));
        cx.subscribe(&sample_rate_dropdown, Self::on_sample_rate_selected)
            .detach();
        cx.subscribe(&build_button, Self::on_build_clicked).detach();

        Self {
            project,
            sample_rate,
            sample_rate_dropdown,
            build_button,
            state: BuildState::Ready,
            next_request_id: 1,
        }
    }

    pub(crate) fn sync_project(&mut self, project: Project, cx: &mut Context<Self>) {
        if self.project == project {
            return;
        }
        self.project = project;
        self.mark_project_changed(cx);
    }

    pub(crate) fn mark_project_changed(&mut self, cx: &mut Context<Self>) {
        self.state = match &self.state {
            BuildState::Ready => BuildState::Ready,
            BuildState::Failed(_) | BuildState::Stale => BuildState::Stale,
            BuildState::Built(_) => BuildState::Stale,
            BuildState::Building {
                request_id,
                project_changed: _,
            } => BuildState::Building {
                request_id: *request_id,
                project_changed: true,
            },
        };
        cx.notify();
    }

    pub(crate) fn build_finished(
        &mut self,
        request_id: u64,
        result: Result<AudioBuildResult, String>,
        cx: &mut Context<Self>,
    ) {
        let (active_request, project_changed) = match &self.state {
            BuildState::Building {
                request_id,
                project_changed,
            } => (*request_id, *project_changed),
            BuildState::Ready
            | BuildState::Built(_)
            | BuildState::Failed(_)
            | BuildState::Stale => return,
        };
        if request_id != active_request {
            return;
        }

        self.state = if project_changed {
            BuildState::Stale
        } else {
            match result {
                Ok(result) => BuildState::Built(result),
                Err(error) => BuildState::Failed(error),
            }
        };
        self.build_button.update(cx, |button, cx| {
            button.set_disabled(false, cx);
        });
        cx.notify();
    }

    fn on_sample_rate_selected(
        &mut self,
        _: Entity<Dropdown>,
        selected: &dropdown::Selected,
        cx: &mut Context<Self>,
    ) {
        let Some(sample_rate) = BuildSampleRate::from_index(selected.index) else {
            return;
        };
        if self.sample_rate == sample_rate {
            return;
        }
        self.sample_rate = sample_rate;
        self.mark_project_changed(cx);
    }

    fn on_build_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        if matches!(self.state, BuildState::Building { .. }) {
            return;
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        self.state = BuildState::Building {
            request_id,
            project_changed: false,
        };
        self.build_button.update(cx, |button, cx| {
            button.set_disabled(true, cx);
        });
        cx.emit(Msg::Requested {
            request_id,
            sample_rate: self.sample_rate,
        });
        cx.notify();
    }

    fn status(&self) -> status_bar::Status {
        match &self.state {
            BuildState::Ready => status_bar::Status::Empty,
            BuildState::Building { .. } => status_bar::Status::Message("building…".into()),
            BuildState::Built(result) => status_bar::Status::Message(
                format!(
                    "built {} files · {} · {:.2} seconds · {}",
                    result.file_count,
                    sample_rate_label(result.sample_rate),
                    result.duration_seconds(),
                    result.directory.display()
                )
                .into(),
            ),
            BuildState::Failed(error) => status_bar::Status::Error {
                message: error.clone().into(),
                target: None,
            },
            BuildState::Stale => status_bar::Status::Warning(
                "project or build settings changed · build again".into(),
            ),
        }
    }
}

impl Render for BuildWorkspace {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let files = planned_build_files(&self.project);
        let controls = div()
            .flex()
            .flex_col()
            .flex_none()
            .w(s::S9)
            .max_w_full()
            .gap(s::CONTENT_PADDING)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(s::S4)
                    .child(div().text_color(s::TEXT_HEADER).child("project build"))
                    .child("renders the entire arrangement, independent of the playback loop")
                    .child("stereo 32-bit float wav with acoustic tails")
                    .child("one score json per voice with grid and ahess timing"),
            )
            .child(compact_control_group(
                "sample rate",
                self.sample_rate_dropdown.clone(),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(s::S3)
                    .child(div().text_color(s::TEXT_HEADER).child("output folder"))
                    .child(format!("{BUILD_DIRECTORY}/")),
            )
            .child(button::action_group([self.build_button.clone()]));
        let output = output_file_panel(files);
        let content = div()
            .flex()
            .flex_1()
            .min_h(s::S0)
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .debug_selector(|| "build-workspace".to_string())
            .child(controls)
            .child(output);
        let tile = workspace::tile(content);

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w(s::S0)
            .min_h(s::S0)
            .overflow_hidden()
            .bg(s::GREEN2)
            .child(div().flex().flex_1().min_h(s::S0).child(tile))
            .child(status_bar::bar(self.status()).debug_selector(|| "build-status-bar".to_string()))
    }
}

fn output_file_panel(files: Vec<PlannedBuildFile>) -> gpui::Div {
    let rows = files.into_iter().map(|file| {
        div()
            .flex()
            .justify_between()
            .gap(s::CONTENT_PADDING)
            .px(s::S4)
            .py(s::S3)
            .child(div().text_color(s::TEXT_DEFAULT).child(file.label))
            .child(
                div()
                    .min_w(s::S0)
                    .truncate()
                    .text_color(s::TEXT_HEADER)
                    .child(file.file_name),
            )
    });

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(s::S0)
        .min_h(s::S0)
        .gap(s::S3)
        .child(div().text_color(s::TEXT_HEADER).child("files"))
        .child(
            s::sunken(
                div()
                    .id("audio-build-files")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(s::S0)
                    .overflow_y_scroll()
                    .bg(s::GREEN3)
                    .children(rows),
            )
            .flex()
            .flex_1()
            .min_h(s::S6)
            .overflow_hidden(),
        )
}

fn sample_rate_label(sample_rate: u32) -> String {
    BuildSampleRate::ALL
        .into_iter()
        .find(|candidate| candidate.hz() == sample_rate)
        .map(|candidate| candidate.label().to_string())
        .unwrap_or_else(|| format!("{sample_rate} Hz"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{px, size, TestAppContext};

    use super::{sample_rate_label, BuildState, BuildWorkspace};
    use crate::{audio_build::AudioBuildResult, project::Project, seed::Seed, style as s};

    #[test]
    fn known_sample_rates_use_compact_labels() {
        assert_eq!(sample_rate_label(44_100), "44.1 kHz");
        assert_eq!(sample_rate_label(48_000), "48 kHz");
        assert_eq!(sample_rate_label(12_345), "12345 Hz");
    }

    #[gpui::test]
    fn a_project_change_during_render_marks_the_completed_build_stale(cx: &mut TestAppContext) {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let (workspace, cx) = cx.add_window_view(|_, cx| BuildWorkspace::new(project, cx));

        workspace.update(cx, |workspace, cx| {
            workspace.on_build_clicked(
                workspace.build_button.clone(),
                &crate::view::button::Clicked,
                cx,
            );
            workspace.mark_project_changed(cx);
            workspace.build_finished(
                1,
                Ok(AudioBuildResult {
                    directory: PathBuf::from("build"),
                    file_count: 2,
                    frame_count: 48_000,
                    sample_rate: 48_000,
                }),
                cx,
            );
        });

        cx.update(|_, cx| {
            let workspace = workspace.read(cx);
            assert_eq!(workspace.state, BuildState::Stale);
            assert!(!workspace.build_button.read(cx).is_disabled());
        });
    }

    #[gpui::test]
    fn build_status_bar_stays_at_the_workspace_bottom(cx: &mut TestAppContext) {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let (_, cx) = cx.add_window_view(|_, cx| BuildWorkspace::new(project, cx));
        cx.simulate_resize(size(px(900.0), px(600.0)));
        cx.run_until_parked();

        let workspace = cx.debug_bounds("build-workspace").unwrap();
        let status = cx.debug_bounds("build-status-bar").unwrap();
        assert_eq!(status.size.height, s::S6);
        assert_eq!(status.origin.x, px(0.0));
        assert_eq!(status.size.width, px(900.0));
        assert!(workspace.bottom() <= status.top());
        assert_eq!(status.bottom(), px(600.0));
    }
}
