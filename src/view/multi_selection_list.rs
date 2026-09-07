//! Independent row selection with the same surfaces as ordinary selection lists.
use crate::{style as s, view::selection_list};
use gpui::{
    prelude::*, Context, EventEmitter, FocusHandle, KeyDownEvent, MouseButton, SharedString, Window,
};
use std::collections::BTreeSet;

pub struct Changed;
struct Row {
    label: SharedString,
    focus: FocusHandle,
}
pub struct MultiSelectionList {
    rows: Vec<Row>,
    selected: BTreeSet<usize>,
}
impl EventEmitter<Changed> for MultiSelectionList {}
impl MultiSelectionList {
    pub fn new(
        labels: Vec<String>,
        selected: impl IntoIterator<Item = usize>,
        cx: &mut Context<Self>,
    ) -> Self {
        let selected = selected
            .into_iter()
            .filter(|index| *index < labels.len())
            .collect();
        Self {
            rows: labels
                .into_iter()
                .map(|label| Row {
                    label: label.into(),
                    focus: cx.focus_handle().tab_stop(true),
                })
                .collect(),
            selected,
        }
    }
    pub fn sync_rows(
        &mut self,
        labels: Vec<String>,
        selected: impl IntoIterator<Item = usize>,
        cx: &mut Context<Self>,
    ) {
        let mut previous = std::mem::take(&mut self.rows);
        self.selected = selected
            .into_iter()
            .filter(|index| *index < labels.len())
            .collect();
        self.rows = labels
            .into_iter()
            .map(|label| {
                if let Some(index) = previous.iter().position(|row| row.label.as_ref() == label) {
                    previous.remove(index)
                } else {
                    Row {
                        label: label.into(),
                        focus: cx.focus_handle().tab_stop(true),
                    }
                }
            })
            .collect();
        cx.notify();
    }
    pub fn selected(&self) -> impl Iterator<Item = usize> + '_ {
        self.selected.iter().copied()
    }
    fn toggle(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.selected.remove(&index) {
            self.selected.insert(index);
        }
        cx.emit(Changed);
        cx.notify();
    }
}
impl Render for MultiSelectionList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                gpui::div().child(
                    selection_list::row(index, self.selected.contains(&index), row.label.clone())
                        .id(SharedString::from(format!("multi-selection-row-{index}")))
                        .debug_selector(move || format!("multi-selection-row-{index}"))
                        .track_focus(&row.focus)
                        .focus(|style| style.border(s::BORDER_WIDTH).border_color(s::GRAY5))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.rows[index].focus.focus(window);
                                this.toggle(index, cx);
                            }),
                        )
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                            match event.keystroke.key.as_str() {
                                "space" | "enter" => {
                                    this.toggle(index, cx);
                                }
                                "up" if index > 0 => this.rows[index - 1].focus.focus(window),
                                "down" if index + 1 < this.rows.len() => {
                                    this.rows[index + 1].focus.focus(window)
                                }
                                _ => return,
                            }
                            cx.stop_propagation();
                        })),
                )
            })
            .collect();
        selection_list::list("multi-selection", "no items", rows).w_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext};
    #[gpui::test]
    fn independent_selection_supports_mouse_and_keyboard(cx: &mut TestAppContext) {
        let (list, cx) = cx.add_window_view(|_, cx| {
            MultiSelectionList::new(vec!["a".into(), "b".into(), "c".into()], [0, 0, 99], cx)
        });
        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected().collect::<Vec<_>>()),
            [0]
        );
        let third = cx.debug_bounds("multi-selection-row-2").unwrap();
        cx.simulate_click(third.center(), Modifiers::default());
        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected().collect::<Vec<_>>()),
            [0, 2]
        );
        cx.simulate_keystrokes("space");
        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected().collect::<Vec<_>>()),
            [0]
        );
        cx.simulate_keystrokes("up enter");
        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected().collect::<Vec<_>>()),
            [0, 1]
        );
    }
}
