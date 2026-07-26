use gpui::{div, prelude::*, App, Context, Entity, EventEmitter, Window};

use crate::{
    playback::BeatRange,
    project::ArrangementOccurrence,
    style as s,
    view::{
        button::{self, Button},
        dialog::error_message,
        dropdown::{self, Dropdown},
        field_group::{compact_control_group, field_group},
        range_selection_list::{self, RangeSelectionList, Row, SelectedRange},
        text_input::{Changed as TextChanged, TextInput},
        workspace,
    },
};

pub enum Msg {
    Applied(BeatRange),
    ResetRequested,
}

// Remember to add any new modes to ALL_SELECTION_MODES below
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionMode {
    ArrangementParts,
    ExactBeats,
}

const ALL_SELECTION_MODES: &[SelectionMode] =
    &[SelectionMode::ArrangementParts, SelectionMode::ExactBeats];

impl SelectionMode {
    fn dropdown_index(self) -> usize {
        match self {
            Self::ArrangementParts => 0,
            Self::ExactBeats => 1,
        }
    }

    fn from_dropdown_index(index: usize) -> Option<Self> {
        ALL_SELECTION_MODES
            .iter()
            .find(|&&mode| mode.dropdown_index() == index)
            .copied()
    }
}

pub struct LoopWorkspace {
    occurrences: Vec<ArrangementOccurrence>,
    arrangement_beat_count: u64,
    applied_range: Option<BeatRange>,
    selection_mode: SelectionMode,
    selection_mode_dropdown: Entity<Dropdown>,
    arrangement_range: Entity<RangeSelectionList>,
    entire_arrangement_button: Entity<Button>,
    from_beat: Entity<TextInput>,
    to_beat: Entity<TextInput>,
    cancel_button: Entity<Button>,
    apply_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<Msg> for LoopWorkspace {}

impl LoopWorkspace {
    pub fn new(
        occurrences: Vec<ArrangementOccurrence>,
        range: Option<BeatRange>,
        cx: &mut Context<Self>,
    ) -> Self {
        let arrangement_beat_count = occurrences
            .last()
            .map_or(0, ArrangementOccurrence::last_beat);
        let exact_boundary_range = range
            .and_then(|range| selected_occurrence_range_for_exact_boundaries(&occurrences, range));
        let selected_occurrences = exact_boundary_range
            .or_else(|| {
                range.and_then(|range| selected_occurrence_range_covering(&occurrences, range))
            })
            .or_else(|| {
                SelectedRange::new(0, occurrences.len().saturating_sub(1), occurrences.len())
            });
        let selection_mode = if range.is_some() && exact_boundary_range.is_none() {
            SelectionMode::ExactBeats
        } else {
            SelectionMode::ArrangementParts
        };
        let selection_mode_dropdown = cx.new(|cx| {
            Dropdown::new(
                "loop-selection-mode",
                ["arranged parts", "exact beats"],
                selection_mode.dropdown_index(),
                cx,
            )
        });
        let rows = occurrences
            .iter()
            .map(|occurrence| {
                let beat_label = singular_or_plural(occurrence.length(), "beat", "beats");
                Row::new(
                    format!(
                        "{}. {}",
                        occurrence.index() + 1,
                        occurrence.part_name().as_str()
                    ),
                    format!("{} {beat_label}", occurrence.length()),
                    format!(
                        "beats {}–{}",
                        occurrence.first_beat(),
                        occurrence.last_beat()
                    ),
                )
            })
            .collect();
        let arrangement_range = cx.new(|cx| {
            RangeSelectionList::new(
                "loop-arrangement-list",
                "no arranged parts yet",
                rows,
                selected_occurrences,
                cx,
            )
            .fill_height()
        });
        let entire_arrangement_button = cx.new(|_| {
            Button::new("loop-entire-arrangement", "entire arrangement")
                .disabled(occurrences.is_empty())
        });
        let first = range.map(BeatRange::first).unwrap_or(1);
        let last = range
            .map(BeatRange::last)
            .unwrap_or(arrangement_beat_count.max(1));
        let from_beat = cx.new(|cx| TextInput::new(first.to_string(), "", cx));
        let to_beat = cx.new(|cx| TextInput::new(last.to_string(), "", cx));
        let cancel_button = cx.new(|_| Button::new("reset-loop-range", "reset"));
        let apply_button =
            cx.new(|_| Button::new("apply-loop-range", "apply").disabled(occurrences.is_empty()));

        cx.subscribe(&selection_mode_dropdown, Self::on_selection_mode_changed)
            .detach();
        cx.subscribe(&arrangement_range, Self::on_arrangement_range_changed)
            .detach();
        cx.subscribe(
            &entire_arrangement_button,
            Self::on_entire_arrangement_clicked,
        )
        .detach();
        cx.subscribe(&from_beat, Self::on_exact_beat_changed)
            .detach();
        cx.subscribe(&to_beat, Self::on_exact_beat_changed).detach();
        cx.subscribe(&cancel_button, Self::on_reset_clicked)
            .detach();
        cx.subscribe(&apply_button, Self::on_apply_clicked).detach();

        Self {
            occurrences,
            arrangement_beat_count,
            applied_range: range,
            selection_mode,
            selection_mode_dropdown,
            arrangement_range,
            entire_arrangement_button,
            from_beat,
            to_beat,
            cancel_button,
            apply_button,
            error: None,
        }
    }

    pub fn is_dirty(&self, cx: &App) -> bool {
        match self.selected_beat_range(cx) {
            Ok(range) => Some(range) != self.applied_range,
            Err(_) => self.applied_range.is_some() || self.arrangement_beat_count > 0,
        }
    }

    pub fn applied(&mut self, range: BeatRange, cx: &mut Context<Self>) {
        self.applied_range = Some(range);
        self.error = None;
        cx.notify();
    }

    fn on_selection_mode_changed(
        &mut self,
        _: Entity<Dropdown>,
        selected: &dropdown::Selected,
        cx: &mut Context<Self>,
    ) {
        let Some(selection_mode) = SelectionMode::from_dropdown_index(selected.index) else {
            return;
        };
        if selection_mode == SelectionMode::ExactBeats {
            if let Some(range) = self.selected_arrangement_beat_range(cx) {
                self.from_beat.update(cx, |input, cx| {
                    input.sync_value(range.first().to_string(), cx);
                });
                self.to_beat.update(cx, |input, cx| {
                    input.sync_value(range.last().to_string(), cx);
                });
            }
        }
        self.selection_mode = selection_mode;
        self.error = None;
        cx.notify();
    }

    fn on_arrangement_range_changed(
        &mut self,
        _: Entity<RangeSelectionList>,
        _: &range_selection_list::Changed,
        cx: &mut Context<Self>,
    ) {
        if self.selection_mode != SelectionMode::ArrangementParts {
            self.selection_mode = SelectionMode::ArrangementParts;
            self.selection_mode_dropdown
                .update(cx, |dropdown, cx| dropdown.set_selected_index(0, cx));
        }
        self.error = None;
        cx.notify();
    }

    fn on_entire_arrangement_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.arrangement_range.update(cx, |range, cx| {
            range.select_all(cx);
        });
    }

    fn on_exact_beat_changed(
        &mut self,
        _: Entity<TextInput>,
        _: &TextChanged,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        cx.notify();
    }

    fn on_reset_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        cx.emit(Msg::ResetRequested);
    }

    fn on_apply_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        match self.selected_beat_range(cx) {
            Ok(range) => cx.emit(Msg::Applied(range)),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn selected_beat_range(&self, cx: &App) -> Result<BeatRange, String> {
        match self.selection_mode {
            SelectionMode::ArrangementParts => {
                self.selected_arrangement_beat_range(cx).ok_or_else(|| {
                    "add at least one part to the arrangement before setting a loop".to_string()
                })
            }
            SelectionMode::ExactBeats => parse_beat(&self.from_beat.read(cx).value(), "from beat")
                .and_then(|first| {
                    parse_beat(&self.to_beat.read(cx).value(), "to beat").map(|last| (first, last))
                })
                .and_then(|(first, last)| {
                    BeatRange::new(first, last, self.arrangement_beat_count)
                        .map_err(|error| error.to_string())
                }),
        }
    }

    fn selected_arrangement_beat_range(&self, cx: &App) -> Option<BeatRange> {
        let selected = self.arrangement_range.read(cx).selected_range()?;
        beat_range_for_occurrences(&self.occurrences, selected)
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

    fn exact_summary(&self, cx: &Context<Self>) -> String {
        let parsed = parse_beat(&self.from_beat.read(cx).value(), "from beat").and_then(|first| {
            parse_beat(&self.to_beat.read(cx).value(), "to beat").and_then(|last| {
                BeatRange::new(first, last, self.arrangement_beat_count)
                    .map_err(|error| error.to_string())
            })
        });
        match parsed {
            Ok(range) => {
                let beat_count = range.last() - range.first() + 1;
                let beat_label = singular_or_plural(beat_count, "beat", "beats");
                format!(
                    "loop: beats {}–{} · {beat_count} {beat_label}",
                    range.first(),
                    range.last()
                )
            }
            Err(_) => "loop: enter an exact beat range".to_string(),
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

    fn exact_controls(&self, cx: &Context<Self>) -> gpui::Div {
        let beat_label = singular_or_plural(self.arrangement_beat_count, "beat", "beats");
        div()
            .flex()
            .flex_col()
            .gap(s::CONTENT_PADDING)
            .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                "the arrangement contains {} {beat_label}; both ends of the loop are included",
                self.arrangement_beat_count
            )))
            .child(
                div()
                    .flex()
                    .gap(s::S4)
                    .debug_selector(|| "loop-range-fields".to_string())
                    .child(field_group("from beat", self.from_beat.clone()))
                    .child(field_group("to beat", self.to_beat.clone())),
            )
            .child(
                div()
                    .text_color(s::TEXT_DEFAULT)
                    .debug_selector(|| "loop-selection-summary".to_string())
                    .child(self.exact_summary(cx)),
            )
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

impl Render for LoopWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_controls = match self.selection_mode {
            SelectionMode::ArrangementParts => self.arrangement_controls(cx),
            SelectionMode::ExactBeats => self.exact_controls(cx),
        };
        let mut controls = div()
            .flex()
            .flex_col()
            .gap(s::CONTENT_PADDING)
            .child(compact_control_group(
                "select by",
                self.selection_mode_dropdown.clone(),
            ))
            .child(mode_controls);
        if self.arrangement_beat_count == 0 {
            controls = controls.child(error_message(
                "add at least one part to the arrangement before setting a loop",
            ));
        } else if let Some(error) = &self.error {
            controls = controls.child(error_message(error.clone()));
        }

        let actions = div()
            .flex()
            .justify_end()
            .gap(s::S3)
            .debug_selector(|| "loop-range-actions".to_string())
            .child(self.cancel_button.clone())
            .child(self.apply_button.clone());
        let controls_column = workspace::column_with_actions(controls, actions)
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

fn selected_occurrence_range_for_exact_boundaries(
    occurrences: &[ArrangementOccurrence],
    range: BeatRange,
) -> Option<SelectedRange> {
    let first = occurrences
        .iter()
        .position(|occurrence| occurrence.first_beat() == range.first())?;
    let last = occurrences
        .iter()
        .position(|occurrence| occurrence.last_beat() == range.last())?;
    SelectedRange::new(first, last, occurrences.len())
}

fn selected_occurrence_range_covering(
    occurrences: &[ArrangementOccurrence],
    range: BeatRange,
) -> Option<SelectedRange> {
    let first = occurrences.iter().position(|occurrence| {
        (occurrence.first_beat()..=occurrence.last_beat()).contains(&range.first())
    })?;
    let last = occurrences.iter().position(|occurrence| {
        (occurrence.first_beat()..=occurrence.last_beat()).contains(&range.last())
    })?;
    SelectedRange::new(first, last, occurrences.len())
}

fn beat_range_for_occurrences(
    occurrences: &[ArrangementOccurrence],
    selected: SelectedRange,
) -> Option<BeatRange> {
    let first = occurrences.get(selected.first())?.first_beat();
    let last = occurrences.get(selected.last())?.last_beat();
    let arrangement_beat_count = occurrences
        .last()
        .map_or(0, ArrangementOccurrence::last_beat);
    BeatRange::new(first, last, arrangement_beat_count).ok()
}

fn parse_beat(value: &str, label: &str) -> Result<u64, String> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("{label} must be a whole number"))
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
    use gpui::{px, size, TestAppContext};

    use super::{LoopWorkspace, SelectionMode};
    use crate::{
        part::Part, playback::BeatRange, project::Project, seed::Seed,
        view::range_selection_list::SelectedRange,
    };

    fn occurrences() -> Vec<crate::project::ArrangementOccurrence> {
        Project::new("test", 800, 0, Seed::new(1))
            .with_parts(vec![
                Part::new("intro", 8),
                Part::new("verse", 16),
                Part::new("chorus", 8),
            ])
            .with_sequence(vec!["intro".into(), "verse".into(), "chorus".into()])
            .arrangement_occurrences()
    }

    fn many_occurrences() -> Vec<crate::project::ArrangementOccurrence> {
        let parts = (1..=28)
            .map(|index| Part::new(format!("part-{index}"), 16))
            .collect();
        Project::new("test", 800, 0, Seed::new(1))
            .with_parts(parts)
            .arrangement_occurrences()
    }

    #[gpui::test]
    fn loop_workspace_fits_its_arrangement_picker_and_actions(cx: &mut TestAppContext) {
        let range = BeatRange::new(1, 448, 448).unwrap();
        let (_dialog, cx) =
            cx.add_window_view(|_, cx| LoopWorkspace::new(many_occurrences(), Some(range), cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let workspace = cx.debug_bounds("loop-workspace").unwrap();
        let controls = cx.debug_bounds("loop-controls-column").unwrap();
        let arrangement = cx.debug_bounds("loop-arrangement-column").unwrap();
        let list = cx.debug_bounds("loop-arrangement-list").unwrap();
        let actions = cx.debug_bounds("loop-range-actions").unwrap();

        assert!(workspace.size.width > crate::style::S11);
        assert!(workspace.size.height > crate::style::S10);
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
        assert!(list.size.height > crate::style::S9);
        assert!(list.origin.x + list.size.width <= workspace.origin.x + workspace.size.width);
        assert!(actions.origin.x + actions.size.width <= controls.origin.x + controls.size.width);
        assert!(
            actions.origin.y + actions.size.height <= workspace.origin.y + workspace.size.height
        );
    }

    #[gpui::test]
    fn part_aligned_ranges_open_in_arrangement_mode(cx: &mut TestAppContext) {
        let range = BeatRange::new(9, 32, 32).unwrap();
        let (dialog, cx) =
            cx.add_window_view(|_, cx| LoopWorkspace::new(occurrences(), Some(range), cx));

        let (mode, selected) = cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            (
                dialog.selection_mode,
                dialog.arrangement_range.read(cx).selected_range(),
            )
        });
        assert_eq!(mode, SelectionMode::ArrangementParts);
        assert_eq!(selected, SelectedRange::new(1, 2, 3));
    }

    #[gpui::test]
    fn partial_part_ranges_open_in_exact_mode(cx: &mut TestAppContext) {
        let range = BeatRange::new(10, 30, 32).unwrap();
        let (dialog, cx) =
            cx.add_window_view(|_, cx| LoopWorkspace::new(occurrences(), Some(range), cx));

        let (mode, selected_range) = dialog.update(cx, |dialog, cx| {
            (
                dialog.selection_mode,
                dialog.selected_beat_range(cx).unwrap(),
            )
        });
        assert_eq!(mode, SelectionMode::ExactBeats);
        assert_eq!(selected_range, range);
    }

    #[gpui::test]
    fn exact_mode_keeps_the_same_wide_two_column_layout(cx: &mut TestAppContext) {
        let range = BeatRange::new(2, 447, 448).unwrap();
        let (_dialog, cx) =
            cx.add_window_view(|_, cx| LoopWorkspace::new(many_occurrences(), Some(range), cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let workspace = cx.debug_bounds("loop-workspace").unwrap();
        let controls = cx.debug_bounds("loop-controls-column").unwrap();
        let arrangement = cx.debug_bounds("loop-arrangement-column").unwrap();
        let list = cx.debug_bounds("loop-arrangement-list").unwrap();
        let fields = cx.debug_bounds("loop-range-fields").unwrap();

        assert!(workspace.size.width > crate::style::S11);
        assert!(workspace.size.height > crate::style::S10);
        assert_eq!(controls.size.width, crate::style::S9);
        assert!(controls.origin.x < arrangement.origin.x);
        assert!(list.size.height > crate::style::S9);
        assert!(fields.origin.x + fields.size.width <= controls.origin.x + controls.size.width);
    }

    #[gpui::test]
    fn selected_parts_resolve_to_their_inclusive_beat_range(cx: &mut TestAppContext) {
        let range = BeatRange::new(9, 32, 32).unwrap();
        let (dialog, cx) =
            cx.add_window_view(|_, cx| LoopWorkspace::new(occurrences(), Some(range), cx));

        assert_eq!(
            dialog.update(cx, |dialog, cx| dialog.selected_beat_range(cx).unwrap()),
            range
        );
    }
}
