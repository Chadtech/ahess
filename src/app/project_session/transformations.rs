use super::*;
use crate::{
    pitch_system::Volume,
    view::{
        dropdown::{Dropdown, Selected},
        field_group::{compact_control_group, field_group},
        multi_selection_list::{self, MultiSelectionList},
        text_input::TextInput,
        workspace,
    },
};

#[derive(Clone, Debug)]
pub(super) enum Transformation {
    Transpose(std::num::NonZeroI32),
    Volume(VolumeRange),
    AdjustVolume(VolumeAdjustment),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct VolumeAdjustment(f64);

impl VolumeAdjustment {
    fn new(amount: f64) -> Result<Self, String> {
        if !amount.is_finite() || !(-100.0..=100.0).contains(&amount) {
            return Err("volume adjustment must be between -100 and 100 percentage points".into());
        }
        Ok(Self(amount))
    }
}

#[derive(Clone, Debug)]
pub(super) struct VolumeRange {
    min: f64,
    max: f64,
    mode: VolumeMode,
}
#[derive(Clone, Copy, Debug)]
enum VolumeMode {
    Relative,
    Absolute,
}

impl VolumeRange {
    fn new(min: f64, max: f64, mode: VolumeMode) -> Result<Self, String> {
        let floor = match mode {
            VolumeMode::Relative => -100.0,
            VolumeMode::Absolute => 0.0,
        };
        if !min.is_finite() || !max.is_finite() || min < floor || max > 100.0 || min > max {
            return Err(format!(
                "enter a minimum and maximum between {floor} and 100, in order"
            ));
        }
        Ok(Self { min, max, mode })
    }
}

pub(super) fn transform_score(
    score: &PartScore,
    project: &Project,
    transformation: &Transformation,
    seed: &mut u64,
) -> Result<PartScore, String> {
    let system = project.pitch_system();
    match transformation {
        Transformation::Transpose(steps) => map_score_cells(score, |value| {
            system
                .transpose_note(value, steps.get())
                .map_err(|e| e.to_string())
        }),
        Transformation::AdjustVolume(amount) => map_score_cells(score, |value| {
            let Some(note) = system.parse_note(value).map_err(|e| e.to_string())? else {
                return Ok(value.to_owned());
            };
            let byte = (f64::from(note.volume().as_byte()) + 255.0 * amount.0 / 100.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            system
                .with_note_volume(value, Volume::from_byte(byte))
                .map_err(|e| e.to_string())
        }),
        Transformation::Volume(range) => map_score_cells(score, |value| {
            let Some(note) = system.parse_note(value).map_err(|e| e.to_string())? else {
                return Ok(value.to_owned());
            };
            // SplitMix64: one draw per note, prepared once at Apply; redo restores bytes.
            *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = *seed;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            let unit = ((z ^ (z >> 31)) >> 11) as f64 / ((1_u64 << 53) as f64);
            let amount = range.min + unit * (range.max - range.min);
            let byte = match range.mode {
                VolumeMode::Relative => f64::from(note.volume().as_byte()) * (1.0 + amount / 100.0),
                VolumeMode::Absolute => 255.0 * amount / 100.0,
            }
            .round()
            .clamp(0.0, 255.0) as u8;
            system
                .with_note_volume(value, Volume::from_byte(byte))
                .map_err(|e| e.to_string())
        }),
    }
}

fn map_score_cells(
    score: &PartScore,
    mut transform: impl FnMut(&str) -> Result<String, String>,
) -> Result<PartScore, String> {
    let rows = score
        .rows()
        .iter()
        .enumerate()
        .map(|(row_index, row)| {
            row.iter()
                .enumerate()
                .map(|(column, value)| {
                    transform(value)
                        .map_err(|e| format!("beat {}, voice {}: {e}", row_index + 1, column + 1))
                })
                .collect::<Result<Vec<String>, String>>()
        })
        .collect::<Result<Vec<Vec<String>>, String>>()?;
    Ok(PartScore::from_rows(rows))
}

pub(super) struct Request {
    parts: Vec<PartName>,
    transformation: Transformation,
}
pub(super) struct TransformationsWorkspace {
    parts: Vec<PartName>,
    selection: Entity<MultiSelectionList>,
    kind: Entity<Dropdown>,
    direction: Entity<Dropdown>,
    mode: Entity<Dropdown>,
    steps: Entity<TextInput>,
    adjustment: Entity<TextInput>,
    min: Entity<TextInput>,
    max: Entity<TextInput>,
    apply: Entity<Button>,
    status: status_bar::Status,
}
impl EventEmitter<Request> for TransformationsWorkspace {}
impl TransformationsWorkspace {
    pub(super) fn new(
        parts: Vec<PartName>,
        selected: Vec<PartName>,
        cx: &mut Context<Self>,
    ) -> Self {
        let indices = parts
            .iter()
            .enumerate()
            .filter_map(|(i, name)| {
                selected
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(name))
                    .then_some(i)
            })
            .collect::<Vec<_>>();
        let selection = cx.new(|cx| {
            MultiSelectionList::new(
                parts.iter().map(|n| n.as_str().to_owned()).collect(),
                indices,
                cx,
            )
        });
        let kind = cx.new(|cx| {
            Dropdown::new(
                "transformation-kind",
                ["transpose", "randomize volume", "adjust volume"],
                0,
                cx,
            )
        });
        let direction =
            cx.new(|cx| Dropdown::new("transformation-direction", ["up", "down"], 0, cx));
        let mode = cx.new(|cx| {
            Dropdown::new(
                "transformation-volume-mode",
                ["relative to each note", "set new levels"],
                0,
                cx,
            )
        });
        let adjustment = cx.new(|cx| TextInput::new("-10", "signed percentage points", cx));
        let steps = cx.new(|cx| TextInput::new("1", "steps", cx));
        let min = cx.new(|cx| TextInput::new("-10", "minimum percent", cx));
        let max = cx.new(|cx| TextInput::new("10", "maximum percent", cx));
        let apply = cx.new(|_| {
            Button::new("apply-transformations", "apply")
                .variant(ButtonVariant::Primary)
                .disabled(selected.is_empty())
        });
        cx.subscribe(&apply, |this, _, _: &button::Clicked, cx| this.apply(cx))
            .detach();
        cx.subscribe(&kind, |this, _, _: &Selected, cx| {
            this.status = status_bar::Status::Empty;
            cx.notify();
        })
        .detach();
        cx.subscribe(&mode, |this, _, event: &Selected, cx| {
            let (min, max) = if event.index == 0 {
                ("-10", "10")
            } else {
                ("60", "80")
            };
            this.min.update(cx, |input, cx| input.sync_value(min, cx));
            this.max.update(cx, |input, cx| input.sync_value(max, cx));
            this.status = status_bar::Status::Empty;
            cx.notify();
        })
        .detach();
        cx.subscribe(
            &selection,
            |this, list, _: &multi_selection_list::Changed, cx| {
                this.apply.update(cx, |button, cx| {
                    let empty = list.read(cx).selected().next().is_none();
                    button.set_disabled(empty, cx)
                });
                this.status = status_bar::Status::Empty;
                cx.notify();
            },
        )
        .detach();
        Self {
            parts,
            selection,
            kind,
            direction,
            mode,
            steps,
            adjustment,
            min,
            max,
            apply,
            status: status_bar::Status::Empty,
        }
    }
    pub(super) fn sync_parts(&mut self, parts: Vec<PartName>, cx: &mut Context<Self>) {
        if self.parts == parts {
            return;
        }
        let selected = self
            .selection
            .read(cx)
            .selected()
            .map(|i| self.parts[i].clone())
            .collect::<Vec<_>>();
        let indices = parts
            .iter()
            .enumerate()
            .filter_map(|(i, name)| {
                selected
                    .iter()
                    .any(|old| old.eq_ignore_ascii_case(name))
                    .then_some(i)
            })
            .collect::<Vec<_>>();
        self.apply
            .update(cx, |button, cx| button.set_disabled(indices.is_empty(), cx));
        self.selection.update(cx, |list, cx| {
            list.sync_rows(
                parts.iter().map(|name| name.as_str().to_owned()).collect(),
                indices,
                cx,
            )
        });
        self.parts = parts;
        self.status = status_bar::Status::Empty;
        cx.notify();
    }
    fn specification(&self, cx: &App) -> Result<Transformation, String> {
        if self.selection.read(cx).selected().next().is_none() {
            return Err("select at least one part".into());
        }
        if self.kind.read(cx).selected_index() == 2 {
            let amount = self
                .adjustment
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .map_err(|_| "enter a signed volume adjustment, such as -10 or +10")?;
            return VolumeAdjustment::new(amount).map(Transformation::AdjustVolume);
        }
        if self.kind.read(cx).selected_index() == 0 {
            let steps = self
                .steps
                .read(cx)
                .value()
                .trim()
                .parse::<i32>()
                .ok()
                .filter(|steps| (1..=127).contains(steps))
                .ok_or("steps must be a whole number from 1 to 127")?;
            let signed = if self.direction.read(cx).selected_index() == 0 {
                steps
            } else {
                -steps
            };
            Ok(Transformation::Transpose(
                std::num::NonZeroI32::new(signed).unwrap(),
            ))
        } else {
            let min = self
                .min
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .map_err(|_| "minimum must be a percentage")?;
            let max = self
                .max
                .read(cx)
                .value()
                .trim()
                .parse::<f64>()
                .map_err(|_| "maximum must be a percentage")?;
            let mode = if self.mode.read(cx).selected_index() == 0 {
                VolumeMode::Relative
            } else {
                VolumeMode::Absolute
            };
            VolumeRange::new(min, max, mode).map(Transformation::Volume)
        }
    }
    fn apply(&mut self, cx: &mut Context<Self>) {
        match self.specification(cx) {
            Ok(transformation) => cx.emit(Request {
                parts: self
                    .selection
                    .read(cx)
                    .selected()
                    .map(|i| self.parts[i].clone())
                    .collect(),
                transformation,
            }),
            Err(error) => self.failed(error, cx),
        }
    }
    pub(super) fn failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.status = status_bar::Status::Error {
            message: error.into(),
            target: None,
        };
        cx.notify();
    }
}
impl Render for TransformationsWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut controls = div()
            .flex()
            .flex_col()
            .w(s::S9)
            .gap(s::S5)
            .child(compact_control_group("transformation", self.kind.clone()));
        if self.kind.read(cx).selected_index() == 0 {
            controls = controls
                .child(compact_control_group("direction", self.direction.clone()))
                .child(
                    field_group("steps", self.steps.clone())
                        .debug_selector(|| "transformation-steps".into()),
                );
        } else if self.kind.read(cx).selected_index() == 2 {
            controls = controls
                .child(
                    field_group("adjustment (percentage points)", self.adjustment.clone())
                        .debug_selector(|| "transformation-adjustment".into()),
                )
                .child("positive adds; negative subtracts")
                .child("levels are limited to 0–100%");
        } else {
            let relative = self.mode.read(cx).selected_index() == 0;
            controls = controls
                .child(compact_control_group("volume range", self.mode.clone()))
                .child(
                    field_group(
                        if relative {
                            "minimum change (%)"
                        } else {
                            "minimum volume (%)"
                        },
                        self.min.clone(),
                    )
                    .debug_selector(|| "transformation-minimum".into()),
                )
                .child(
                    field_group(
                        if relative {
                            "maximum change (%)"
                        } else {
                            "maximum volume (%)"
                        },
                        self.max.clone(),
                    )
                    .debug_selector(|| "transformation-maximum".into()),
                )
                .child("levels are limited to 0–100%");
        }
        let parts = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(s::S4)
            .child(format!(
                "parts · {} selected",
                self.selection.read(cx).selected().count()
            ))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(s::S0)
                    .child(self.selection.clone()),
            );
        workspace::tile(
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h(s::S0)
                        .p(s::CONTENT_PADDING)
                        .gap(s::S6)
                        .child(parts.w(s::S10).flex_none().flex_basis(gpui::Length::Auto))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_none()
                                .w(s::S9)
                                .gap(s::S5)
                                .child(controls.flex_none().flex_basis(gpui::Length::Auto))
                                .child(
                                    button::action_group([self.apply.clone()])
                                        .debug_selector(|| "transformation-actions".into()),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w(s::S0)
                                .max_w(s::S10)
                                .gap(s::S4)
                                .debug_selector(|| "transformation-instructions".into())
                                .child("click or drag across parts to include or exclude them")
                                .child("edits each selected part everywhere it appears"),
                        ),
                )
                .child(status_bar::bar(self.status.clone())),
        )
    }
}

impl Model {
    pub(super) fn on_transformation(
        &mut self,
        workspace: Entity<TransformationsWorkspace>,
        request: &Request,
        cx: &mut Context<Self>,
    ) {
        match self.apply_transformation(&request.parts, &request.transformation, cx) {
            Ok(()) => workspace.update(cx, |workspace, cx| {
                workspace.status = status_bar::Status::Message(
                    "transformation applied · undo is available in the project bar".into(),
                );
                cx.notify();
            }),
            Err(error) => workspace.update(cx, |workspace, cx| workspace.failed(error, cx)),
        }
        self.sync_history_buttons(cx);
        cx.notify();
    }
    pub(super) fn apply_transformation(
        &mut self,
        parts: &[PartName],
        transformation: &Transformation,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if parts.is_empty() {
            return Err("select at least one part".into());
        }
        if parts.iter().any(|name| self.project.part(name).is_none()) {
            return Err("a selected part no longer exists".into());
        }
        let before = self.collect_project_state_for_history_diff(cx)?;
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut scores = Vec::new();
        let mut affected = Vec::new();
        for (name, score, saved) in before.scores() {
            if parts.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                let updated = transform_score(score, &self.project, transformation, &mut seed)
                    .map_err(|e| format!("part {:?}: {e}", name.as_str()))?;
                let part = self
                    .project
                    .part(name)
                    .ok_or("selected part no longer exists")?;
                updated
                    .resolved_strikes(part, &self.project)
                    .map_err(|e| e.to_string())?;
                if &updated != score.as_ref() {
                    affected.push(name.clone());
                }
                let updated = Arc::new(updated);
                scores.push((name.clone(), updated.clone(), updated));
            } else {
                scores.push((name.clone(), score.clone(), saved.clone()));
            }
        }
        if affected.is_empty() {
            return Ok(());
        }
        let target = HistoryState::new(before.project.clone(), scores);
        self.restore_history_state(&target, false, &affected, cx)?;
        self.history.record_project(target);
        self.workspace_error = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        seed::Seed,
        voice::{Voice, VoiceType},
    };
    use gpui::TestAppContext;

    #[gpui::test]
    fn workspace_drag_selects_and_deselects_part_spans(cx: &mut TestAppContext) {
        let (workspace, cx) = cx.add_window_view(|_, cx| {
            TransformationsWorkspace::new(
                vec![
                    "intro".into(),
                    "theme".into(),
                    "bridge".into(),
                    "ending".into(),
                ],
                vec!["intro".into()],
                cx,
            )
        });
        let selected = |cx: &mut gpui::VisualTestContext| {
            cx.update(|_, cx| {
                workspace
                    .read(cx)
                    .selection
                    .read(cx)
                    .selected()
                    .collect::<Vec<_>>()
            })
        };
        let second = cx.debug_bounds("multi-selection-row-1").unwrap().center();
        let third = cx.debug_bounds("multi-selection-row-2").unwrap().center();
        let fourth = cx.debug_bounds("multi-selection-row-3").unwrap().center();
        cx.simulate_mouse_down(second, gpui::MouseButton::Left, Default::default());
        cx.simulate_mouse_move(fourth, Some(gpui::MouseButton::Left), Default::default());
        assert_eq!(selected(cx), [0, 1, 2, 3]);
        cx.simulate_mouse_move(third, Some(gpui::MouseButton::Left), Default::default());
        assert_eq!(selected(cx), [0, 1, 2]);
        cx.simulate_mouse_up(third, gpui::MouseButton::Left, Default::default());
        cx.simulate_mouse_move(fourth, None, Default::default());
        assert_eq!(selected(cx), [0, 1, 2]);
        cx.simulate_mouse_down(third, gpui::MouseButton::Left, Default::default());
        cx.simulate_mouse_move(second, Some(gpui::MouseButton::Left), Default::default());
        cx.simulate_mouse_up(second, gpui::MouseButton::Left, Default::default());
        assert_eq!(selected(cx), [0]);
    }

    #[gpui::test]
    fn workspace_toggles_parts_and_switches_volume_controls(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            TransformationsWorkspace::new(
                vec!["intro".into(), "theme".into(), "ending".into()],
                vec!["intro".into(), "intro".into()],
                cx,
            )
        });
        let actions = cx.debug_bounds("transformation-actions").unwrap();
        let instructions = cx.debug_bounds("transformation-instructions").unwrap();
        assert!(instructions.left() >= actions.right() + s::S6);
        for selector in [
            "transformation-kind-trigger",
            "transformation-direction-trigger",
            "transformation-steps",
        ] {
            let field = cx.debug_bounds(selector).unwrap();
            assert!(
                instructions.left() >= field.right() + s::S6,
                "instructions must be beside {selector}"
            );
        }
        assert_eq!(
            cx.update(|_, cx| dialog.read(cx).selection.read(cx).selected().count()),
            1
        );
        let third = cx.debug_bounds("multi-selection-row-2").unwrap();
        cx.simulate_click(third.center(), gpui::Modifiers::default());
        assert_eq!(
            cx.update(|_, cx| dialog
                .read(cx)
                .selection
                .read(cx)
                .selected()
                .collect::<Vec<_>>()),
            [0, 2]
        );
        let trigger = cx.debug_bounds("transformation-kind-trigger").unwrap();
        cx.simulate_click(trigger.center(), gpui::Modifiers::default());
        let option = cx.debug_bounds("transformation-kind-option-1").unwrap();
        cx.simulate_click(option.center(), gpui::Modifiers::default());
        assert!(cx
            .debug_bounds("transformation-volume-mode-trigger")
            .is_some());
        let actions = cx.debug_bounds("transformation-actions").unwrap();
        let instructions = cx.debug_bounds("transformation-instructions").unwrap();
        assert!(instructions.left() >= actions.right() + s::S6);
        for selector in [
            "transformation-kind-trigger",
            "transformation-volume-mode-trigger",
            "transformation-minimum",
            "transformation-maximum",
        ] {
            let field = cx.debug_bounds(selector).unwrap();
            assert!(
                instructions.left() >= field.right() + s::S6,
                "instructions must be beside {selector}"
            );
        }

        let trigger = cx
            .debug_bounds("transformation-volume-mode-trigger")
            .unwrap();
        cx.simulate_click(trigger.center(), gpui::Modifiers::default());
        let option = cx
            .debug_bounds("transformation-volume-mode-option-1")
            .unwrap();
        cx.simulate_click(option.center(), gpui::Modifiers::default());
        cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            assert_eq!(dialog.min.read(cx).value(), "60");
            assert_eq!(dialog.max.read(cx).value(), "80");
            assert!(dialog.specification(cx).is_ok());
        });
    }

    #[gpui::test]
    fn volume_adjustment_form_accepts_signed_amounts_and_stays_beside_instructions(
        cx: &mut TestAppContext,
    ) {
        let (workspace, cx) = cx.add_window_view(|_, cx| {
            TransformationsWorkspace::new(vec!["intro".into()], vec!["intro".into()], cx)
        });
        let trigger = cx.debug_bounds("transformation-kind-trigger").unwrap();
        cx.simulate_click(trigger.center(), Default::default());
        let option = cx.debug_bounds("transformation-kind-option-2").unwrap();
        cx.simulate_click(option.center(), Default::default());
        let field = cx.debug_bounds("transformation-adjustment").unwrap();
        let instructions = cx.debug_bounds("transformation-instructions").unwrap();
        assert!(instructions.left() >= field.right() + s::S6);
        workspace.update(cx, |workspace, cx| {
            for text in ["+10", "-10", "2.5"] {
                workspace
                    .adjustment
                    .update(cx, |input, cx| input.sync_value(text, cx));
                assert!(matches!(
                    workspace.specification(cx),
                    Ok(Transformation::AdjustVolume(_))
                ));
            }
            for text in ["", "NaN", "101"] {
                workspace
                    .adjustment
                    .update(cx, |input, cx| input.sync_value(text, cx));
                assert!(workspace.specification(cx).is_err());
            }
        });
    }

    #[gpui::test]
    fn transformations_workspace_retains_drafts_and_applies_without_a_modal(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "ahess-transformation-workspace-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let part = Part::new("intro", 1);
        let project = Project::new("test", 800, 0, Seed::new(1))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(vec![part.clone()]);
        let directory = project::create_project(&root, &project).unwrap();
        PartScore::from_rows(vec![vec!["C4".into()]])
            .save(&directory, &part, &project)
            .unwrap();
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project.clone(), directory.clone(), root.clone(), cx)
        });
        let (workspace, button) = cx.update(|_, cx| {
            (
                model.read(cx).workspace.transformations.clone(),
                model.read(cx).transformations_button.clone(),
            )
        });
        button.update(cx, |_, cx| cx.emit(button::Clicked));
        assert_eq!(
            cx.update(|_, cx| model.read(cx).workspace.section.kind()),
            WorkspaceSectionKind::Transformations
        );
        assert!(cx.update(|_, cx| model.read(cx).active_overlay().is_none()));
        let row = cx.debug_bounds("multi-selection-row-0").unwrap();
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        workspace.update(cx, |workspace, cx| {
            workspace
                .steps
                .update(cx, |input, cx| input.sync_value("2", cx))
        });
        model.update(cx, |model, cx| {
            model.set_workspace_section(WorkspaceSection::Score { overlay: None }, cx);
            model.set_workspace_section(WorkspaceSection::Transformations, cx);
            assert_eq!(model.workspace.transformations, workspace);
            assert_eq!(workspace.read(cx).steps.read(cx).value(), "2");
            assert_eq!(
                workspace
                    .read(cx)
                    .selection
                    .read(cx)
                    .selected()
                    .collect::<Vec<_>>(),
                [0]
            );
            let encoded = toml::to_string(&model.ui_state()).unwrap();
            assert_eq!(
                toml::from_str::<UiState>(&encoded).unwrap().workspace,
                WorkspaceSectionKind::Transformations
            );
        });
        let apply = cx.update(|_, cx| workspace.read(cx).apply.clone());
        apply.update(cx, |_, cx| cx.emit(button::Clicked));
        assert_eq!(
            PartScore::load(&directory, &part, project.voices())
                .unwrap()
                .rows()[0][0],
            "D4"
        );
        model.update(cx, |model, cx| {
            assert_eq!(
                model.workspace.section.kind(),
                WorkspaceSectionKind::Transformations
            );
            assert!(model.active_overlay().is_none());
            model.undo(cx);
        });
        assert_eq!(
            PartScore::load(&directory, &part, project.voices())
                .unwrap()
                .rows()[0][0],
            "C4"
        );
        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            PartScore::load(&directory, &part, project.voices())
                .unwrap()
                .rows()[0][0],
            "D4"
        );
        workspace.update(cx, |workspace, cx| {
            workspace.sync_parts(vec!["intro".into(), "theme".into()], cx);
            assert_eq!(
                workspace.selection.read(cx).selected().collect::<Vec<_>>(),
                [0]
            );
            workspace.sync_parts(vec!["renamed intro".into(), "theme".into()], cx);
            assert_eq!(workspace.selection.read(cx).selected().count(), 0);
            assert!(workspace.specification(cx).is_err());
            assert_eq!(workspace.steps.read(cx).value(), "2");
        });
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn volume_adjustment_preserves_six_character_radler_strikes() {
        use crate::pitch_system::{
            FrequencyHz, Interval, PeriodicNotation, PeriodicPitchSystem, PitchSystem,
        };

        let system = PitchSystem::periodic(
            PeriodicPitchSystem::new(
                "test",
                FrequencyHz::new(25.0).unwrap(),
                Interval::ratio(2, 1).unwrap(),
                vec![Interval::ratio(1, 1).unwrap()],
                PeriodicNotation::radler_digits(10).unwrap(),
            )
            .unwrap(),
        );
        let project = Project::new("test", 800, 0, Seed::new(1)).with_pitch_system(system);
        let source = PartScore::from_rows(vec![vec!["40ff80".into()]]);
        for (amount, expected) in [(20.0, "40ffB3"), (-20.0, "40ff4D"), (0.0, "40ff80")] {
            let change = Transformation::AdjustVolume(VolumeAdjustment::new(amount).unwrap());
            let result = transform_score(&source, &project, &change, &mut 1).unwrap();
            assert_eq!(result.rows()[0][0], expected);
        }
    }

    #[test]
    fn every_transformation_reports_the_invalid_cells_location() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let score =
            PartScore::from_rows(vec![vec!["C4".into()], vec!["".into(), "invalid".into()]]);
        for transformation in [
            Transformation::Transpose(std::num::NonZeroI32::new(1).unwrap()),
            Transformation::AdjustVolume(VolumeAdjustment::new(20.0).unwrap()),
            Transformation::Volume(VolumeRange::new(60.0, 80.0, VolumeMode::Absolute).unwrap()),
        ] {
            let error = transform_score(&score, &project, &transformation, &mut 1).unwrap_err();
            assert!(error.starts_with("beat 2, voice 2: "), "{error}");
            assert_eq!(error.matches("beat 2, voice 2").count(), 1);
        }
    }

    #[test]
    fn volume_adjustment_adds_fixed_points_and_preserves_notes_and_rests() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let source = PartScore::from_rows(vec![vec![
            "C4@80".into(),
            "D4@00".into(),
            "E4".into(),
            "rest".into(),
            "".into(),
        ]]);
        for (amount, expected) in [(20.0, [179, 51, 255]), (-20.0, [77, 0, 204])] {
            let change = Transformation::AdjustVolume(VolumeAdjustment::new(amount).unwrap());
            let mut seed = 123;
            let result = transform_score(&source, &project, &change, &mut seed).unwrap();
            assert_eq!(seed, 123);
            for (column, expected) in expected.into_iter().enumerate() {
                let original = project
                    .pitch_system()
                    .parse_note(&source.rows()[0][column])
                    .unwrap()
                    .unwrap();
                let changed = project
                    .pitch_system()
                    .parse_note(&result.rows()[0][column])
                    .unwrap()
                    .unwrap();
                assert_eq!(changed.pitch(), original.pitch());
                assert_eq!(changed.duration(), original.duration());
                assert_eq!(changed.volume().as_byte(), expected);
            }
            assert_eq!(&result.rows()[0][3..], &["rest", ""]);
        }
        let unchanged = Transformation::AdjustVolume(VolumeAdjustment::new(0.0).unwrap());
        assert_eq!(
            transform_score(&source, &project, &unchanged, &mut 1).unwrap(),
            source
        );
        for invalid in [f64::NAN, f64::INFINITY, -101.0, 101.0] {
            assert!(VolumeAdjustment::new(invalid).is_err());
        }
    }

    #[test]
    fn volume_ranges_preserve_rests_and_bound_each_note() {
        let project = Project::new("test", 800, 0, Seed::new(1));
        let source = PartScore::from_rows(vec![vec!["C4@80".into(), "rest".into(), "".into()]; 64]);
        let change =
            Transformation::Volume(VolumeRange::new(-10.0, 10.0, VolumeMode::Relative).unwrap());
        let output = transform_score(&source, &project, &change, &mut 1).unwrap();
        let levels = output
            .rows()
            .iter()
            .map(|row| {
                assert_eq!(&row[1..], &["rest", ""]);
                let note = project.pitch_system().parse_note(&row[0]).unwrap().unwrap();
                assert_eq!(
                    note.duration(),
                    crate::pitch_system::StrikeDuration::VoiceDefault
                );
                let level = note.volume().as_byte();
                assert!((115..=141).contains(&level));
                level
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert!(levels.len() > 10);
        for (min, max) in [
            (f64::NAN, 10.0),
            (20.0, 10.0),
            (-101.0, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert!(VolumeRange::new(min, max, VolumeMode::Relative).is_err());
        }
    }

    #[gpui::test]
    fn transformations_are_one_persisted_undo_and_exact_redo(cx: &mut TestAppContext) {
        let root = std::env::temp_dir().join(format!(
            "ahess-transformations-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let parts = vec![
            Part::new("intro", 2),
            Part::new("theme", 2),
            Part::new("ending", 2),
        ];
        let project = Project::new("test", 800, 0, Seed::new(1))
            .with_voices(vec![Voice::new(1, "lead", VoiceType::Saw)])
            .with_parts(parts.clone())
            .with_sequence(vec!["intro".into(), "theme".into(), "intro".into()]);
        let directory = project::create_project(&root, &project).unwrap();
        let original = PartScore::from_rows(vec![vec!["C4".into()], vec!["D4".into()]]);
        for part in &parts {
            original.save(&directory, part, &project).unwrap();
        }
        let (model, cx) = cx.add_window_view(|_, cx| {
            Model::new(project.clone(), directory.clone(), root.clone(), cx)
        });
        let selected = vec![
            parts[0].name.clone(),
            parts[1].name.clone(),
            parts[0].name.clone(),
        ];
        let change =
            Transformation::Volume(VolumeRange::new(60.0, 80.0, VolumeMode::Absolute).unwrap());
        model.update(cx, |model, cx| {
            model.apply_transformation(&selected, &change, cx).unwrap()
        });
        let changed = parts
            .iter()
            .map(|part| PartScore::load(&directory, part, project.voices()).unwrap())
            .collect::<Vec<_>>();
        assert_ne!(changed[0], original);
        assert_ne!(changed[1], original);
        assert_eq!(changed[2], original);
        model.update(cx, |model, cx| model.undo(cx));
        for part in &parts {
            assert_eq!(
                PartScore::load(&directory, part, project.voices()).unwrap(),
                original
            );
        }
        model.update(cx, |model, cx| model.redo(cx));
        for (part, expected) in parts.iter().zip(changed.iter()) {
            assert_eq!(
                &PartScore::load(&directory, part, project.voices()).unwrap(),
                expected
            );
        }
        model.update(cx, |model, cx| {
            assert_eq!(
                model.score_documents[0].document.read(cx).score(),
                &changed[0]
            );
            model.undo(cx);
            let transpose = Transformation::Transpose(std::num::NonZeroI32::new(1).unwrap());
            model
                .apply_transformation(&selected, &transpose, cx)
                .unwrap();
        });
        assert_eq!(
            PartScore::load(&directory, &parts[0], project.voices())
                .unwrap()
                .rows()[0][0],
            "C#4"
        );
        model.update(cx, |model, cx| {
            let before = model.history.current().clone();
            let transpose = Transformation::Transpose(std::num::NonZeroI32::new(127).unwrap());
            assert!(model
                .apply_transformation(&selected, &transpose, cx)
                .is_err());
            assert_eq!(&before, model.history.current());
        });
        assert_eq!(
            PartScore::load(&directory, &parts[0], project.voices())
                .unwrap()
                .rows()[0][0],
            "C#4"
        );
        let adjustment = Transformation::AdjustVolume(VolumeAdjustment::new(-20.0).unwrap());
        model.update(cx, |model, cx| {
            model
                .apply_transformation(&selected, &adjustment, cx)
                .unwrap()
        });
        for part in &parts[..2] {
            let changed = PartScore::load(&directory, part, project.voices()).unwrap();
            assert!(changed.rows().iter().all(|row| project
                .pitch_system()
                .parse_note(&row[0])
                .unwrap()
                .unwrap()
                .volume()
                .as_byte()
                == 204));
        }
        assert_eq!(
            PartScore::load(&directory, &parts[2], project.voices()).unwrap(),
            original
        );
        model.update(cx, |model, cx| model.undo(cx));
        assert_eq!(
            PartScore::load(&directory, &parts[0], project.voices())
                .unwrap()
                .rows()[0][0],
            "C#4"
        );
        model.update(cx, |model, cx| model.redo(cx));
        assert_eq!(
            PartScore::load(&directory, &parts[0], project.voices())
                .unwrap()
                .rows()[0][0],
            "C#4@CC"
        );
        let unrelated = model.update(cx, |model, cx| {
            model.score_document(&parts[2].name, cx).unwrap()
        });
        unrelated.update(cx, |document, cx| {
            document.update_cell(u64::MAX, 0, 0, "F4".into(), cx)
        });
        model.update(cx, |model, cx| {
            assert!(unrelated.read(cx).is_dirty());
            let transpose = Transformation::Transpose(std::num::NonZeroI32::new(1).unwrap());
            model
                .apply_transformation(&selected, &transpose, cx)
                .unwrap();
            assert!(
                unrelated.read(cx).is_dirty(),
                "batch must preserve unrelated pending autosaves"
            );
            model.undo(cx);
            assert!(
                unrelated.read(cx).is_dirty(),
                "undo must preserve unrelated pending autosaves"
            );
            assert_eq!(unrelated.read(cx).score().rows()[0][0], "F4");
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
