//! Transactional mini-tracker for one score cell's authored note events.
use crate::{
    pitch_system::PitchSystem,
    score_cell::{self, BeatOffset, CellEvent},
    style as s,
    view::{
        attack_control::{self, AttackControl},
        button::{self, Button},
        data_grid::{self, DataGridScrollHandle, InvalidCells},
        dialog::{error_message, title_bar},
        dropdown::{self, Dropdown},
        text_input::{Changed, TextInput},
    },
};
use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};
use std::collections::BTreeMap;

pub enum NoteDetailsMsg {
    Confirmed(String),
    Cancelled,
}
pub struct NoteDetailsDialog {
    system: PitchSystem,
    title: String,
    original: String,
    original_events: String,
    draft: BTreeMap<i32, [String; 4]>,
    voice_attack: Option<crate::voice::AttackSharpness>,
    attack: Entity<AttackControl>,
    inherit: Entity<Button>,
    selection: Option<(usize, usize)>,
    positions: Vec<i32>,
    cells: Vec<Vec<Entity<TextInput>>>,
    step: Entity<Dropdown>,
    close: Entity<Button>,
    cancel: Entity<Button>,
    done: Entity<Button>,
    scroll: DataGridScrollHandle,
    error: Option<String>,
    invalid: InvalidCells,
    initial_focus: Option<Entity<TextInput>>,
    focus_subscriptions: Vec<gpui::Subscription>,
}
impl EventEmitter<NoteDetailsMsg> for NoteDetailsDialog {}
const STEPS: [i32; 10] = [24, 12, 6, 32, 16, 8, 4, 3, 2, 1];
impl NoteDetailsDialog {
    pub fn new(
        system: PitchSystem,
        title: String,
        original: String,
        events: Vec<CellEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        let original_events = score_cell::encode(&events);
        let draft = events
            .iter()
            .map(|e| {
                (
                    e.offset.ticks(),
                    [
                        system.pitch_text(e.note.pitch()),
                        score_cell::duration_text(e.note.duration()),
                        format!("{:02X}", e.note.volume().as_byte()),
                        e.attack_sharpness
                            .map(|a| a.percent().to_string())
                            .unwrap_or_default(),
                    ],
                )
            })
            .collect();
        let selected = STEPS
            .iter()
            .position(|step| events.iter().all(|e| e.offset.ticks() % step == 0))
            .unwrap_or(STEPS.len() - 1);
        let step = cx.new(|cx| {
            Dropdown::new(
                "note-detail-step",
                STEPS
                    .iter()
                    .map(|step| score_cell::fraction(*step))
                    .collect::<Vec<_>>(),
                selected,
                cx,
            )
        });
        let attack = cx.new(|cx| AttackControl::new(Default::default(), cx));
        let inherit = cx.new(|_| Button::new("inherit-note-attack", "use voice default"));
        cx.subscribe(&attack, |this, _, event: &attack_control::Changed, cx| {
            this.apply_attack(Some(event.0), cx);
        })
        .detach();
        cx.subscribe(&inherit, |this, _, _: &button::Clicked, cx| {
            this.apply_attack(None, cx);
        })
        .detach();
        let close = cx.new(|_| Button::x("close-note-details"));
        let cancel = cx.new(|_| Button::new("cancel-note-details", "cancel"));
        let done = cx.new(|_| Button::new("save-note-details", "done"));
        cx.subscribe(&step, |this, _, event: &dropdown::Selected, cx| {
            this.read_draft(cx);
            this.rebuild(STEPS[event.index], cx);
        })
        .detach();
        for button in [&close, &cancel] {
            cx.subscribe(button, |_, _, _: &button::Clicked, cx| {
                cx.emit(NoteDetailsMsg::Cancelled)
            })
            .detach();
        }
        cx.subscribe(&done, |this, _, _: &button::Clicked, cx| this.save(cx))
            .detach();
        let mut this = Self {
            system,
            title,
            original,
            original_events,
            draft,
            voice_attack: None,
            attack,
            inherit,
            selection: None,
            positions: Vec::new(),
            cells: Vec::new(),
            step,
            close,
            cancel,
            done,
            scroll: DataGridScrollHandle::new(),
            error: None,
            invalid: InvalidCells::default(),
            initial_focus: None,
            focus_subscriptions: Vec::new(),
        };
        this.rebuild(STEPS[selected], cx);
        this
    }
    pub fn set_voice_attack(
        &mut self,
        attack: Option<crate::voice::AttackSharpness>,
        cx: &mut Context<Self>,
    ) {
        self.read_draft(cx);
        self.voice_attack = attack;
        self.rebuild(STEPS[self.step.read(cx).selected_index()], cx);
        self.attack.update(cx, |control, cx| {
            control.sync(attack.unwrap_or_default(), cx)
        });
        cx.notify();
    }
    fn select_row(&mut self, row: usize, extend: bool, cx: &mut Context<Self>) {
        self.selection = if extend {
            Some((self.selection.map_or(row, |(anchor, _)| anchor), row))
        } else if self.selection == Some((row, row)) {
            None
        } else {
            Some((row, row))
        };
        let value = self.cells[row][3]
            .read(cx)
            .value()
            .parse::<u8>()
            .ok()
            .and_then(|v| crate::voice::AttackSharpness::new(v).ok())
            .or(self.voice_attack)
            .unwrap_or_default();
        self.attack
            .update(cx, |control, cx| control.sync(value, cx));
        cx.notify();
    }
    fn apply_attack(
        &mut self,
        attack: Option<crate::voice::AttackSharpness>,
        cx: &mut Context<Self>,
    ) {
        let Some((a, b)) = self.selection else {
            return;
        };
        for row in a.min(b)..=a.max(b) {
            if !self.cells[row][0].read(cx).value().trim().is_empty() {
                let value = attack.map(|a| a.percent().to_string()).unwrap_or_default();
                self.cells[row][3].update(cx, |input, cx| input.sync_value(value, cx));
            }
        }
        if attack.is_none() {
            self.attack.update(cx, |control, cx| {
                control.sync(self.voice_attack.unwrap_or_default(), cx)
            });
        }
        self.error = None;
        self.invalid = InvalidCells::default();
        cx.notify();
    }
    fn read_draft(&mut self, cx: &Context<Self>) {
        for (offset, row) in self.positions.iter().zip(&self.cells) {
            self.draft.insert(
                *offset,
                [
                    row[0].read(cx).value(),
                    row[1].read(cx).value(),
                    row[2].read(cx).value(),
                    row[3].read(cx).value(),
                ],
            );
        }
    }
    fn rebuild(&mut self, step: i32, cx: &mut Context<Self>) {
        // Keep occupied off-grid rows visible when selecting a coarser step.
        let mut positions = (-96..96).step_by(step as usize).collect::<Vec<_>>();
        positions.extend(
            self.draft
                .iter()
                .filter(|(_, fields)| fields.iter().any(|v| !v.trim().is_empty()))
                .map(|(p, _)| *p),
        );
        positions.sort_unstable();
        positions.dedup();
        self.cells = positions
            .iter()
            .map(|offset| {
                let values = self.draft.get(offset).cloned().unwrap_or_default();
                let background = if *offset == 0 { s::GREEN5 } else { s::GREEN3 };
                let row: Vec<_> = values
                    .into_iter()
                    .map(|value| {
                        cx.new(|cx| TextInput::new(value, "", cx).with_background(background))
                    })
                    .collect();
                let duration = row[1].clone();
                let volume = row[2].clone();
                cx.subscribe(&row[0], move |this, input, _: &Changed, cx| {
                    if !input.read(cx).value().trim().is_empty() {
                        if duration.read(cx).value().trim().is_empty() {
                            duration.update(cx, |v, cx| v.sync_value("default", cx));
                        }
                        if volume.read(cx).value().trim().is_empty() {
                            volume.update(cx, |v, cx| v.sync_value("FF", cx));
                        }
                    }
                    this.error = None;
                    this.invalid = InvalidCells::default();
                    cx.notify();
                })
                .detach();
                row
            })
            .collect();
        for row in 0..self.cells.len() {
            for column in 0..if self.voice_attack.is_some() { 4 } else { 3 } {
                let previous = self.cells[row.saturating_sub(1)][column].clone();
                let next = self.cells[(row + 1).min(self.cells.len() - 1)][column].clone();
                self.cells[row][column].update(cx, |input, _| {
                    input.set_vertical_neighbors(&previous, &next)
                });
            }
        }
        let columns = if self.voice_attack.is_some() { 4 } else { 3 };
        let fields = self
            .cells
            .iter()
            .flat_map(|row| row[..columns].iter())
            .cloned()
            .collect::<Vec<_>>();
        for (index, field) in fields.iter().enumerate() {
            field.update(cx, |input, _| {
                input.set_tab_neighbors(
                    &fields[(index + fields.len() - 1) % fields.len()],
                    &fields[(index + 1) % fields.len()],
                )
            });
        }
        self.initial_focus = positions
            .iter()
            .position(|p| *p == 0)
            .map(|i| self.cells[i][0].clone());
        self.positions = positions;
        self.selection = None;
        self.invalid = InvalidCells::default();
        cx.notify();
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        self.read_draft(cx);
        let mut events = Vec::new();
        for (row, offset) in self.positions.iter().enumerate() {
            let fields = &self.draft[offset];
            match CellEvent::from_fields(
                &self.system,
                BeatOffset::from_ticks(*offset).unwrap(),
                &fields[0],
                &fields[1],
                &fields[2],
            ) {
                Ok(Some(mut event)) => {
                    let attack = fields[3].trim();
                    event.attack_sharpness = if attack.is_empty() || attack == "default" {
                        None
                    } else {
                        match attack
                            .parse::<u8>()
                            .ok()
                            .and_then(|v| crate::voice::AttackSharpness::new(v).ok())
                        {
                            Some(value) => Some(value),
                            None => {
                                self.error = Some("attack must be 0–100 or default".into());
                                self.invalid = [(row, 3)].into_iter().collect();
                                data_grid::reveal_cell(&self.scroll, row, 3);
                                cx.notify();
                                return;
                            }
                        }
                    };
                    events.push(event);
                }
                Ok(None) => {
                    if !fields[3].trim().is_empty() && fields[3].trim() != "default" {
                        self.error = Some("an attack override needs a pitch".into());
                        self.invalid = [(row, 0), (row, 3)].into_iter().collect();
                        cx.notify();
                        return;
                    }
                }
                Err(e) => {
                    self.error = Some(format!("offset {}: {e}", score_cell::fraction(*offset)));
                    self.invalid = (0..3).map(|column| (row, column)).collect();
                    data_grid::reveal_cell(&self.scroll, row, 0);
                    cx.notify();
                    return;
                }
            }
        }
        let encoded = score_cell::encode(&events);
        if self.system.is_exact_key(&encoded) {
            self.error = Some("note details conflict with a named pitch".into());
            cx.notify();
            return;
        }
        // Opening and accepting an unchanged legacy note must not rewrite it.
        cx.emit(NoteDetailsMsg::Confirmed(
            if encoded == self.original_events {
                self.original.clone()
            } else {
                encoded
            },
        ));
    }
    pub fn save_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }
}
impl Render for NoteDetailsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(input) = self.initial_focus.take() {
            self.focus_subscriptions.clear();
            for (row, fields) in self.cells.iter().enumerate() {
                for (column, field) in fields.iter().enumerate() {
                    let handle = gpui::Focusable::focus_handle(field.read(cx), cx);
                    self.focus_subscriptions.push(cx.on_focus(
                        &handle,
                        window,
                        move |this, _, cx| {
                            data_grid::reveal_cell(&this.scroll, row, column);
                            cx.notify();
                        },
                    ));
                }
            }
            if let Some(row) = self.positions.iter().position(|p| *p == 0) {
                data_grid::reveal_cell(&self.scroll, row, 0);
            }
            input.read(cx).focus(window);
        }
        let labels = self
            .positions
            .iter()
            .map(|p| {
                if *p > 0 {
                    format!("+{}", score_cell::fraction(*p))
                } else {
                    score_cell::fraction(*p)
                }
            })
            .collect();
        let entity = cx.entity();
        let columns = if self.voice_attack.is_some() { 4 } else { 3 };
        let visible_cells = self
            .cells
            .iter()
            .map(|row| row[..columns].to_vec())
            .collect::<Vec<_>>();
        let grid = data_grid::editable_with_row_selection(
            "note-details-grid",
            ["pitch", "duration", "volume", "attack"][..columns]
                .iter()
                .map(|s| (*s).into())
                .collect(),
            &visible_cells,
            &self.invalid,
            labels,
            self.selection.and_then(|(a, b)| {
                crate::part::ScoreRowRange::new(a.min(b), a.max(b), self.cells.len())
            }),
            None,
            &self.scroll,
            move |row, header| {
                let click = entity.clone();
                let drag = entity.clone();
                header
                    .on_mouse_down(gpui::MouseButton::Left, move |event, _, cx| {
                        click.update(cx, |this, cx| {
                            this.select_row(row, event.modifiers.shift, cx)
                        });
                    })
                    .on_mouse_move(move |event, _, cx| {
                        if event.pressed_button == Some(gpui::MouseButton::Left) {
                            drag.update(cx, |this, cx| {
                                if let Some((_, head)) = this.selection {
                                    if head != row {
                                        this.select_row(row, true, cx);
                                        cx.notify();
                                    }
                                }
                            });
                        }
                    })
            },
        );
        let count = self
            .cells
            .iter()
            .filter(|row| !row[0].read(cx).value().trim().is_empty())
            .count();
        s::raised(
            div()
                .debug_selector(|| "note-detail-dialog".into())
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    if event.keystroke.key == "enter" && event.keystroke.modifiers.secondary() {
                        this.save(cx);
                        cx.stop_propagation();
                        return;
                    }
                    if event.keystroke.key == "escape" {
                        cx.emit(NoteDetailsMsg::Cancelled);
                        cx.stop_propagation();
                    }
                }))
                .child(title_bar(self.title.clone(), Some(self.close.clone())))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(s::CONTENT_PADDING)
                        .p(s::CONTENT_PADDING)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child("offsets in beats")
                                .child(self.step.clone()),
                        )
                        .child(
                            div()
                                .debug_selector(|| "note-detail-grid-region".into())
                                .flex()
                                .h(s::S9 + s::S7)
                                .child(grid),
                        )
                        .child(
                            div()
                                .debug_selector(|| "note-detail-help".into())
                                .text_color(s::TEXT_DEFAULT)
                                .child(if self.voice_attack.is_some() { "duration: beats or default · volume: 00–FF · attack: 0–100 or default" } else { "duration: beats or default · volume: 00–FF" }),
                        )
                        .when(self.voice_attack.is_some(), |body| body.child(
                            div().flex().flex_col().gap(s::S3)
                                .child(format!("attack · voice default {}% · select offset rows to edit together", self.voice_attack.unwrap().percent()))
                                .when(self.selection.is_some(), |panel| panel
                                    .child(self.attack.clone())
                                    .child(div().flex().child(self.inherit.clone())))
                        ))
                        .children(self.error.clone().map(error_message))
                        .child(
                            div()
                                .debug_selector(|| "note-detail-actions".into())
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(format!("{count} notes"))
                                .child(button::action_group([
                                    self.cancel.clone(),
                                    self.done.clone(),
                                ])),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;
    use std::{cell::RefCell, rc::Rc};
    #[gpui::test]
    fn attack_batch_edits_selected_notes_and_preserves_inheritance(cx: &mut TestAppContext) {
        let system = PitchSystem::western_twelve_tone();
        let mut events = score_cell::parse(&system, "C4").unwrap();
        let mut second = events[0].clone();
        second.offset = BeatOffset::from_ticks(24).unwrap();
        events.push(second);
        events[0].attack_sharpness = Some(crate::voice::AttackSharpness::new(25).unwrap());
        let original = score_cell::encode(&events);
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            let mut d = NoteDetailsDialog::new(system, "note details".into(), original, events, cx);
            d.set_voice_attack(Some(crate::voice::AttackSharpness::new(40).unwrap()), cx);
            d
        });
        let messages = Rc::new(RefCell::new(Vec::new()));
        let output = messages.clone();
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, msg: &NoteDetailsMsg, _| {
                if let NoteDetailsMsg::Confirmed(value) = msg {
                    output.borrow_mut().push(value.clone());
                }
            })
            .detach()
        });
        dialog.update(cx, |d, cx| {
            let first = d.positions.iter().position(|p| *p == 0).unwrap();
            let second = d.positions.iter().position(|p| *p == 24).unwrap();
            d.select_row(first, false, cx);
            d.apply_attack(Some(crate::voice::AttackSharpness::new(100).unwrap()), cx);
            assert_eq!(d.cells[first][3].read(cx).value(), "100");
            assert_eq!(d.cells[second][3].read(cx).value(), "");
            d.select_row(second, true, cx);
            d.apply_attack(Some(crate::voice::AttackSharpness::new(75).unwrap()), cx);
            d.save(cx);
            d.apply_attack(None, cx);
            d.save(cx);
        });
        let messages = messages.borrow();
        let system = PitchSystem::western_twelve_tone();
        let batch = score_cell::parse(&system, &messages[0]).unwrap();
        assert!(batch
            .iter()
            .all(|e| e.attack_sharpness.unwrap().percent() == 75));
        let inherited = score_cell::parse(&system, &messages[1]).unwrap();
        assert!(inherited.iter().all(|e| e.attack_sharpness.is_none()));
    }
    #[gpui::test]
    fn unchanged_note_is_preserved_and_invalid_detail_stays_open(cx: &mut TestAppContext) {
        let system = PitchSystem::western_twelve_tone();
        let events = score_cell::parse(&system, "C4@80").unwrap();
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            NoteDetailsDialog::new(system, "note details".into(), "C4@80".into(), events, cx)
        });
        let messages = Rc::new(RefCell::new(Vec::new()));
        let output = messages.clone();
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, msg: &NoteDetailsMsg, _| {
                if let NoteDetailsMsg::Confirmed(value) = msg {
                    output.borrow_mut().push(value.clone());
                }
            })
            .detach()
        });
        dialog.update(cx, |d, cx| d.save(cx));
        assert_eq!(&*messages.borrow(), &["C4@80"]);
        dialog.update(cx, |d, cx| {
            let row = d.positions.iter().position(|p| *p == 0).unwrap();
            d.cells[row][1].update(cx, |v, cx| v.sync_value("0", cx));
            d.save(cx);
            assert!(d.error.is_some());
            assert!(d.invalid.contains(row, 1));
        });
        assert_eq!(messages.borrow().len(), 1);
        let grid = cx.debug_bounds("note-detail-grid-region").unwrap();
        let help = cx.debug_bounds("note-detail-help").unwrap();
        let actions = cx.debug_bounds("note-detail-actions").unwrap();
        let bounds = cx.debug_bounds("note-detail-dialog").unwrap();
        assert!(help.top() >= grid.bottom() + s::CONTENT_PADDING);
        assert!(actions.top() >= help.bottom() + s::CONTENT_PADDING);
        assert!(actions.right() <= bounds.right());
    }
    #[gpui::test]
    fn changing_grid_step_keeps_fine_notes_and_cancel_emits_no_commit(cx: &mut TestAppContext) {
        let system = PitchSystem::western_twelve_tone();
        let events = vec![CellEvent::from_fields(
            &system,
            BeatOffset::from_ticks(12).unwrap(),
            "C4",
            "1/8",
            "80",
        )
        .unwrap()
        .unwrap()];
        let original = score_cell::encode(&events);
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            NoteDetailsDialog::new(system, "note details".into(), original, events, cx)
        });
        dialog.update(cx, |d, cx| {
            d.read_draft(cx);
            d.rebuild(24, cx);
            assert!(d.positions.contains(&12));
            assert_eq!(d.draft[&12][1], "1/8");
        });
    }
}
