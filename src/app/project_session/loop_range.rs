use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::{
    part::PartName,
    playback::BeatRange,
    project::{ArrangementOccurrence, OccurrenceId},
    style as s,
    view::{
        button::{self, Button},
        dialog::error_message,
        range_selection_list::{self, RangeSelectionList, Row, SelectedRange},
        workspace,
    },
};

/// Loop intent for the project owner to resolve into playback bounds.
pub enum Request {
    SetSelection(LoopSelection),
}

/// User intent. Beat bounds are resolved only against the current arrangement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoopSelection {
    EntireArrangement,
    Occurrences(Vec<OccurrenceId>),
}

impl LoopSelection {
    pub fn resolve(&self, occurrences: &[ArrangementOccurrence]) -> Option<BeatRange> {
        let total = occurrences.last()?.last_beat();
        match self {
            Self::EntireArrangement => BeatRange::new(1, total, total).ok(),
            Self::Occurrences(ids) => {
                let mut matching = occurrences.iter().filter(|o| ids.contains(&o.id()));
                let first = matching.next()?;
                let last = matching.last().unwrap_or(first);
                BeatRange::new(first.first_beat(), last.last_beat(), total).ok()
            }
        }
    }
}

pub struct LoopWorkspace {
    occurrences: Vec<ArrangementOccurrence>,
    arrangement_beat_count: u64,
    active_part: Option<PartName>,
    arrangement_range: Entity<RangeSelectionList>,
    entire_arrangement_button: Entity<Button>,
}

impl EventEmitter<Request> for LoopWorkspace {}

impl LoopWorkspace {
    pub fn for_selection(
        occurrences: Vec<ArrangementOccurrence>,
        selection: &LoopSelection,
        cx: &mut Context<Self>,
    ) -> Self {
        let arrangement_beat_count = occurrences
            .last()
            .map_or(0, ArrangementOccurrence::last_beat);
        let selected = match selection {
            LoopSelection::EntireArrangement => {
                SelectedRange::new(0, occurrences.len().saturating_sub(1), occurrences.len())
            }
            LoopSelection::Occurrences(ids) => {
                let first = occurrences.iter().position(|o| ids.contains(&o.id()));
                let last = occurrences.iter().rposition(|o| ids.contains(&o.id()));
                first
                    .zip(last)
                    .and_then(|(first, last)| SelectedRange::new(first, last, occurrences.len()))
            }
        };
        let rows = occurrence_rows(&occurrences, None);
        let arrangement_range = cx.new(|cx| {
            RangeSelectionList::new(
                "loop-arrangement-list",
                "no arranged parts yet",
                rows,
                selected,
                cx,
            )
            .fill_height()
        });
        let entire_arrangement_button = cx.new(|_| {
            Button::new("loop-entire-arrangement", "entire arrangement")
                .disabled(occurrences.is_empty())
        });
        cx.subscribe(&arrangement_range, Self::on_arrangement_range_changed)
            .detach();
        cx.subscribe(
            &entire_arrangement_button,
            Self::on_entire_arrangement_clicked,
        )
        .detach();
        Self {
            occurrences,
            arrangement_beat_count,
            active_part: None,
            arrangement_range,
            entire_arrangement_button,
        }
    }

    pub(crate) fn arrangement_range(&self) -> Entity<RangeSelectionList> {
        self.arrangement_range.clone()
    }

    pub(crate) fn sync_active_part(
        &mut self,
        active_part: Option<PartName>,
        cx: &mut Context<Self>,
    ) {
        if self.active_part == active_part {
            return;
        }
        self.active_part = active_part;
        self.arrangement_range.update(cx, |range, cx| {
            range.sync_rows(
                occurrence_rows(&self.occurrences, self.active_part.as_ref()),
                cx,
            );
        });
        cx.notify();
    }

    pub(crate) fn sync_occurrences(
        &mut self,
        occurrences: Vec<ArrangementOccurrence>,
        cx: &mut Context<Self>,
    ) {
        if self.occurrences == occurrences {
            return;
        }

        self.arrangement_beat_count = occurrences
            .last()
            .map_or(0, ArrangementOccurrence::last_beat);
        self.arrangement_range.update(cx, |range, cx| {
            range.sync_rows(occurrence_rows(&occurrences, self.active_part.as_ref()), cx);
        });
        self.entire_arrangement_button.update(cx, |button, cx| {
            button.set_disabled(occurrences.is_empty(), cx)
        });
        self.occurrences = occurrences;
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn occurrence_names(&self) -> Vec<&str> {
        self.occurrences
            .iter()
            .map(|occurrence| occurrence.part_name().as_str())
            .collect()
    }

    fn on_arrangement_range_changed(
        &mut self,
        _: Entity<RangeSelectionList>,
        _: &range_selection_list::Changed,
        cx: &mut Context<Self>,
    ) {
        let ids = self
            .arrangement_range
            .read(cx)
            .selected_range()
            .map(|selected| {
                self.occurrences[selected.first()..=selected.last()]
                    .iter()
                    .map(ArrangementOccurrence::id)
                    .collect()
            })
            .unwrap_or_default();
        cx.emit(Request::SetSelection(LoopSelection::Occurrences(ids)));
        cx.notify();
    }

    fn on_entire_arrangement_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.arrangement_range.update(cx, |range, cx| {
            range.sync_selected_range(
                SelectedRange::new(
                    0,
                    self.occurrences.len().saturating_sub(1),
                    self.occurrences.len(),
                ),
                cx,
            );
        });
        cx.emit(Request::SetSelection(LoopSelection::EntireArrangement));
        cx.notify();
    }

    fn arrangement_summary(&self, cx: &Context<Self>) -> Option<String> {
        let selected = self.arrangement_range.read(cx).selected_range()?;
        let first = self.occurrences.get(selected.first())?;
        let last = self.occurrences.get(selected.last())?;
        let beat_count = last.last_beat() - first.first_beat() + 1;
        let beat_label = singular_or_plural(beat_count, "beat", "beats");
        if selected.first() == selected.last() {
            Some(format!(
                "loop: part {} · {} · {beat_count} {beat_label}",
                first.index() + 1,
                first.part_name().as_str()
            ))
        } else {
            Some(format!(
                "loop: parts {}–{} · {beat_count} {beat_label}",
                first.index() + 1,
                last.index() + 1
            ))
        }
    }

    fn arrangement_controls(&self, cx: &Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(s::S4)
            .child(
                div()
                    .text_color(s::TEXT_DEFAULT)
                    .child("click a part or drag across a contiguous range"),
            )
            .child(div().flex().child(self.entire_arrangement_button.clone()))
            .children(self.arrangement_summary(cx).map(|summary| {
                div()
                    .text_color(s::TEXT_DEFAULT)
                    .debug_selector(|| "loop-selection-summary".to_string())
                    .child(summary)
            }))
    }

    fn arrangement_panel(&self) -> gpui::Div {
        let part_count = self.occurrences.len();
        let part_label = singular_or_plural(part_count, "part", "parts");
        let beat_label = singular_or_plural(self.arrangement_beat_count, "beat", "beats");
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w(s::S0)
            .min_w(s::S0)
            .min_h(s::S0)
            .debug_selector(|| "loop-arrangement-column".to_string())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb(s::S4)
                    .child(div().text_color(s::TEXT_HEADER).child("arrangement"))
                    .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                        "{part_count} {part_label}, {} {beat_label}",
                        self.arrangement_beat_count
                    ))),
            )
            .child(self.arrangement_range.clone())
    }
}

fn occurrence_rows(
    occurrences: &[ArrangementOccurrence],
    active_part: Option<&PartName>,
) -> Vec<Row> {
    occurrences
        .iter()
        .map(|occurrence| {
            let active = active_part.is_some_and(|active_part| {
                occurrence.part_name().eq_ignore_ascii_case(active_part)
            });
            Row::new(
                format!(
                    "{}. {}",
                    occurrence.index() + 1,
                    occurrence.part_name().as_str()
                ),
                "",
                "",
            )
            .with_indicator(if active { "→" } else { "" })
        })
        .collect()
}

impl Render for LoopWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut controls = self.arrangement_controls(cx);
        if self.arrangement_beat_count == 0 {
            controls = controls.child(error_message(
                "add at least one part to the arrangement before setting a loop",
            ));
        }

        let controls_column = controls
            .flex_none()
            .flex_basis(s::S9)
            .w(s::S9)
            .min_w(s::S9)
            .debug_selector(|| "loop-controls-column".to_string());

        workspace::tile(
            div()
                .flex()
                .flex_1()
                .min_h(s::S0)
                .gap(s::CONTENT_PADDING)
                .p(s::CONTENT_PADDING)
                .debug_selector(|| "loop-workspace".to_string())
                .child(controls_column)
                .child(self.arrangement_panel()),
        )
    }
}

fn singular_or_plural<T>(count: T, singular: &'static str, plural: &'static str) -> &'static str
where
    T: PartialEq + From<u8>,
{
    if count == T::from(1) {
        singular
    } else {
        plural
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        prelude::*, px, size, Context, Entity, Modifiers, MouseButton, TestAppContext, Window,
    };

    use super::{LoopSelection, LoopWorkspace, Request};
    use crate::{
        part::Part, playback::BeatRange, project, project::Project, seed::Seed, style,
        view::range_selection_list::SelectedRange,
    };

    fn loop_project() -> Project {
        Project::new("loops", 800, 0, Seed::new(1))
            .with_parts(vec![Part::new("a", 4), Part::new("b", 8)])
            .with_sequence(vec!["a".into(), "b".into(), "a".into()])
    }

    #[test]
    fn occurrence_loop_follows_length_changes_and_surviving_endpoints() {
        let mut project = loop_project();
        let occurrences = project.arrangement_occurrences();
        let selection =
            super::LoopSelection::Occurrences(vec![occurrences[1].id(), occurrences[2].id()]);
        project.parts[0].length = 7;
        assert_eq!(
            selection.resolve(&project.arrangement_occurrences()),
            BeatRange::new(8, 22, 22).ok()
        );
        let before = project.clone();
        project.set_sequence(vec!["a".into(), "a".into()]);
        project.retain_occurrence_ids(&before, &[Some(0), Some(2)]);
        assert_eq!(
            selection.resolve(&project.arrangement_occurrences()),
            BeatRange::new(8, 14, 14).ok()
        );
        let before = project.clone();
        project.set_sequence(vec!["a".into()]);
        project.retain_occurrence_ids(&before, &[Some(0)]);
        assert_eq!(selection.resolve(&project.arrangement_occurrences()), None);
    }

    #[test]
    fn occurrence_loop_tracks_a_repeated_instance_through_insert_and_move() {
        let mut project = loop_project();
        let selection =
            super::LoopSelection::Occurrences(vec![project.arrangement_occurrences()[2].id()]);
        let before = project.clone();
        project.set_sequence(vec!["a".into(), "a".into(), "b".into(), "a".into()]);
        project.retain_occurrence_ids(&before, &[Some(0), None, Some(1), Some(2)]);
        assert_eq!(
            selection.resolve(&project.arrangement_occurrences()),
            BeatRange::new(17, 20, 20).ok()
        );
        let before = project.clone();
        project.set_sequence(vec!["a".into(), "a".into(), "a".into(), "b".into()]);
        project.retain_occurrence_ids(&before, &[Some(3), Some(0), Some(1), Some(2)]);
        assert_eq!(
            selection.resolve(&project.arrangement_occurrences()),
            BeatRange::new(1, 4, 20).ok()
        );
    }

    #[test]
    fn entire_arrangement_and_explicit_full_span_have_different_intent() {
        let mut project = loop_project();
        let range = BeatRange::new(1, 16, 16).unwrap();
        let selected = LoopSelection::Occurrences(
            project
                .arrangement_occurrences()
                .iter()
                .map(|o| o.id())
                .collect(),
        );
        project.set_sequence(vec!["a".into(), "b".into(), "a".into(), "b".into()]);
        let occurrences = project.arrangement_occurrences();
        assert_eq!(selected.resolve(&occurrences), Some(range));
        assert_eq!(
            super::LoopSelection::EntireArrangement.resolve(&occurrences),
            BeatRange::new(1, 24, 24).ok()
        );
        project.parts[0].length = 2;
        let occurrences = project.arrangement_occurrences();
        assert_eq!(
            selected.resolve(&occurrences),
            BeatRange::new(1, 12, 20).ok()
        );
    }

    #[gpui::test]
    fn rebuilding_loop_editor_preserves_occurrence_selection(cx: &mut TestAppContext) {
        let occurrences = loop_project().arrangement_occurrences();
        let selected = LoopSelection::Occurrences(vec![occurrences[1].id(), occurrences[2].id()]);
        let workspace =
            cx.new(|cx| LoopWorkspace::for_selection(occurrences.clone(), &selected, cx));
        assert_eq!(
            cx.read(|cx| workspace
                .read(cx)
                .arrangement_range
                .read(cx)
                .selected_range()),
            SelectedRange::new(1, 2, 3)
        );
        let missing = super::LoopSelection::Occurrences(vec![loop_project()
            .arrangement_occurrences()[0]
            .id()]);
        let workspace = cx.new(|cx| LoopWorkspace::for_selection(occurrences, &missing, cx));
        assert_eq!(
            cx.read(|cx| workspace
                .read(cx)
                .arrangement_range
                .read(cx)
                .selected_range()),
            None
        );
    }

    struct LoopWorkspaceHost {
        workspace: Entity<LoopWorkspace>,
        applied_ranges: Vec<BeatRange>,
    }

    impl LoopWorkspaceHost {
        fn new(cx: &mut Context<Self>) -> Self {
            let workspace = cx.new(|cx| {
                LoopWorkspace::for_selection(occurrences(), &LoopSelection::EntireArrangement, cx)
            });
            cx.subscribe(&workspace, |host, _, request, _cx| {
                let Request::SetSelection(range) = request;
                host.applied_ranges.push(
                    range
                        .resolve(&host.workspace.read(_cx).occurrences)
                        .unwrap(),
                );
            })
            .detach();
            Self {
                workspace,
                applied_ranges: Vec::new(),
            }
        }
    }

    impl Render for LoopWorkspaceHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.workspace.clone()
        }
    }

    fn occurrences() -> Vec<project::ArrangementOccurrence> {
        Project::new("test", 800, 0, Seed::new(1))
            .with_parts(vec![
                Part::new("intro", 8),
                Part::new("verse", 16),
                Part::new("chorus", 8),
            ])
            .with_sequence(vec!["intro".into(), "verse".into(), "chorus".into()])
            .arrangement_occurrences()
    }

    fn many_occurrences() -> Vec<project::ArrangementOccurrence> {
        let parts = (1..=28)
            .map(|index| Part::new(format!("part-{index}"), 16))
            .collect();
        Project::new("test", 800, 0, Seed::new(1))
            .with_parts(parts)
            .arrangement_occurrences()
    }

    #[gpui::test]
    fn loop_workspace_fits_its_arrangement_picker_and_controls(cx: &mut TestAppContext) {
        let (_dialog, cx) = cx.add_window_view(|_, cx| {
            LoopWorkspace::for_selection(many_occurrences(), &LoopSelection::EntireArrangement, cx)
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let workspace = cx.debug_bounds("loop-workspace").unwrap();
        let controls = cx.debug_bounds("loop-controls-column").unwrap();
        let arrangement = cx.debug_bounds("loop-arrangement-column").unwrap();
        let list = cx.debug_bounds("loop-arrangement-list").unwrap();

        assert!(workspace.size.width > style::S11);
        assert!(workspace.size.height > style::S10);
        assert!(workspace.origin.x >= px(0.0));
        assert!(workspace.origin.y >= px(0.0));
        assert!(workspace.origin.x + workspace.size.width <= px(1_200.0));
        assert!(
            workspace.origin.y + workspace.size.height <= px(700.0),
            "workspace bounds: {workspace:?}"
        );
        assert!(controls.origin.x < arrangement.origin.x);
        assert!(
            list.size.width > controls.size.width,
            "controls: {controls:?}, arrangement: {arrangement:?}, list: {list:?}"
        );
        assert!(list.size.height > style::S9);
        assert!(list.origin.x + list.size.width <= workspace.origin.x + workspace.size.width);
        assert!(cx.debug_bounds("apply-loop-range").is_none());
        assert!(cx.debug_bounds("loop-selection-mode").is_none());
        assert!(cx.debug_bounds("loop-range-fields").is_none());
    }

    #[gpui::test]
    fn clicking_and_dragging_applies_each_selected_part_range(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| LoopWorkspaceHost::new(cx));
        cx.run_until_parked();
        let second = cx.debug_bounds("loop-arrangement-list-row-1").unwrap();
        let third = cx.debug_bounds("loop-arrangement-list-row-2").unwrap();

        cx.simulate_mouse_down(second.center(), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            third.center(),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(third.center(), MouseButton::Left, Modifiers::default());
        cx.run_until_parked();

        assert_eq!(
            cx.update(|_, cx| host.read(cx).applied_ranges.clone()),
            vec![
                BeatRange::new(9, 24, 32).unwrap(),
                BeatRange::new(9, 32, 32).unwrap(),
            ]
        );
    }
}
