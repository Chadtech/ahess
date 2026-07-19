use gpui::{
    prelude::*, Context, ElementId, EventEmitter, FocusHandle, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, SharedString, Window,
};

use crate::{style as s, view::selection_list};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    primary: SharedString,
    secondary: SharedString,
    trailing: SharedString,
}

impl Row {
    pub fn new(
        primary: impl Into<SharedString>,
        secondary: impl Into<SharedString>,
        trailing: impl Into<SharedString>,
    ) -> Self {
        Self {
            primary: primary.into(),
            secondary: secondary.into(),
            trailing: trailing.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectedRange {
    first: usize,
    last: usize,
}

impl SelectedRange {
    pub fn new(first: usize, last: usize, row_count: usize) -> Option<Self> {
        (first <= last && last < row_count).then_some(Self { first, last })
    }

    pub fn first(self) -> usize {
        self.first
    }

    pub fn last(self) -> usize {
        self.last
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AnchoredSelection {
    anchor: usize,
    head: usize,
}

impl AnchoredSelection {
    fn new(anchor: usize, head: usize, row_count: usize) -> Option<Self> {
        (anchor < row_count && head < row_count).then_some(Self { anchor, head })
    }

    fn range(self) -> SelectedRange {
        SelectedRange {
            first: self.anchor.min(self.head),
            last: self.anchor.max(self.head),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Changed {
    pub range: SelectedRange,
}

pub struct RangeSelectionList {
    id: ElementId,
    empty_message: SharedString,
    rows: Vec<Row>,
    selection: Option<AnchoredSelection>,
    drag_anchor: Option<usize>,
    focus_handle: FocusHandle,
    viewport: Viewport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Viewport {
    Compact,
    Fill,
}

impl EventEmitter<Changed> for RangeSelectionList {}

impl RangeSelectionList {
    pub fn new(
        id: impl Into<ElementId>,
        empty_message: impl Into<SharedString>,
        rows: Vec<Row>,
        selected: Option<SelectedRange>,
        cx: &mut Context<Self>,
    ) -> Self {
        let selection = selected
            .and_then(|range| AnchoredSelection::new(range.first(), range.last(), rows.len()));
        Self {
            id: id.into(),
            empty_message: empty_message.into(),
            rows,
            selection,
            drag_anchor: None,
            focus_handle: cx.focus_handle().tab_stop(true),
            viewport: Viewport::Compact,
        }
    }

    pub fn fill_height(mut self) -> Self {
        self.viewport = Viewport::Fill;
        self
    }

    pub fn selected_range(&self) -> Option<SelectedRange> {
        self.selection.map(AnchoredSelection::range)
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() - 1;
        self.set_selection(AnchoredSelection::new(0, last, self.rows.len()), cx);
    }

    fn on_row_mouse_down(
        &mut self,
        index: usize,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window);
        let anchor = if event.modifiers.shift {
            self.selection.map_or(index, |selection| selection.anchor)
        } else {
            index
        };
        self.drag_anchor = Some(anchor);
        self.set_selection(AnchoredSelection::new(anchor, index, self.rows.len()), cx);
    }

    fn on_row_mouse_move(
        &mut self,
        index: usize,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.dragging() {
            return;
        }
        let Some(anchor) = self.drag_anchor else {
            return;
        };
        self.set_selection(AnchoredSelection::new(anchor, index, self.rows.len()), cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag_anchor = None;
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let offset = match event.keystroke.key.as_str() {
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        if self.rows.is_empty() {
            return;
        }

        let current = self
            .selection
            .unwrap_or(AnchoredSelection { anchor: 0, head: 0 });
        let Some(head) = current.head.checked_add_signed(offset) else {
            return;
        };
        if head >= self.rows.len() {
            return;
        }
        let anchor = if event.keystroke.modifiers.shift {
            current.anchor
        } else {
            head
        };
        self.set_selection(AnchoredSelection::new(anchor, head, self.rows.len()), cx);
        cx.stop_propagation();
    }

    fn set_selection(&mut self, selection: Option<AnchoredSelection>, cx: &mut Context<Self>) {
        if self.selection == selection {
            return;
        }
        let previous_range = self.selected_range();
        self.selection = selection;
        let selected_range = self.selected_range();
        cx.notify();
        if selected_range != previous_range {
            if let Some(range) = selected_range {
                cx.emit(Changed { range });
            }
        }
    }
}

impl Render for RangeSelectionList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let list_debug_id = self.id.to_string();
        let selected = self.selected_range();
        let rows = self
            .rows
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, row)| {
                let is_selected =
                    selected.is_some_and(|range| (range.first()..=range.last()).contains(&index));
                let content = gpui::div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(s::S4)
                    .w_full()
                    .child(gpui::div().flex_1().child(row.primary))
                    .child(gpui::div().w(s::S8).child(row.secondary))
                    .child(gpui::div().w(s::S8).child(row.trailing));
                selection_list::row_content(index, is_selected, content)
                    .debug_selector({
                        let id = self.id.clone();
                        move || format!("{id}-row-{index}")
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |list, event, window, cx| {
                            list.on_row_mouse_down(index, event, window, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(move |list, event, window, cx| {
                        list.on_row_mouse_move(index, event, window, cx);
                    }))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            })
            .collect();

        let list = selection_list::list(self.id.clone(), self.empty_message.clone(), rows)
            .w_full()
            .debug_selector(move || list_debug_id.clone())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up));
        match self.viewport {
            Viewport::Compact => list.flex_none().flex_basis(s::S8).h(s::S8).max_h(s::S8),
            Viewport::Fill => list.flex_1().min_h(s::S0),
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        point, prelude::*, px, Context, Entity, Modifiers, MouseButton, ScrollDelta,
        ScrollWheelEvent, TestAppContext, Window,
    };

    use super::{RangeSelectionList, Row, SelectedRange};

    struct Host {
        list: Entity<RangeSelectionList>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            gpui::div().size_full().child(self.list.clone())
        }
    }

    fn rows() -> Vec<Row> {
        (1..=4)
            .map(|index| Row::new(format!("{index}. part"), "8 beats", "beats 1–8"))
            .collect()
    }

    #[gpui::test]
    fn click_and_drag_selects_a_contiguous_range(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| Host {
            list: cx.new(|cx| RangeSelectionList::new("range", "empty", rows(), None, cx)),
        });
        let list = cx.update(|_, cx| host.read(cx).list.clone());
        let second = cx.debug_bounds("range-row-1").unwrap();
        let third = cx.debug_bounds("range-row-2").unwrap();

        cx.simulate_mouse_down(second.center(), MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            third.center(),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(third.center(), MouseButton::Left, Modifiers::default());

        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected_range()),
            SelectedRange::new(1, 2, 4)
        );
    }

    #[gpui::test]
    fn shift_click_extends_from_the_original_anchor(cx: &mut TestAppContext) {
        let (list, cx) =
            cx.add_window_view(|_, cx| RangeSelectionList::new("range", "empty", rows(), None, cx));
        let second = cx.debug_bounds("range-row-1").unwrap();
        let third = cx.debug_bounds("range-row-2").unwrap();
        cx.simulate_click(second.center(), Modifiers::default());
        cx.simulate_click(
            third.center(),
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );

        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected_range()),
            SelectedRange::new(1, 2, 4)
        );
    }

    #[gpui::test]
    fn shift_arrow_extends_the_focused_selection(cx: &mut TestAppContext) {
        let (list, cx) =
            cx.add_window_view(|_, cx| RangeSelectionList::new("range", "empty", rows(), None, cx));
        let second = cx.debug_bounds("range-row-1").unwrap();
        cx.simulate_click(second.center(), Modifiers::default());

        cx.simulate_keystrokes("shift-down");

        assert_eq!(
            cx.update(|_, cx| list.read(cx).selected_range()),
            SelectedRange::new(1, 2, 4)
        );
    }

    #[gpui::test]
    fn long_range_lists_scroll_inside_their_fixed_height(cx: &mut TestAppContext) {
        let rows = (1..=24)
            .map(|index| Row::new(format!("{index}. part"), "8 beats", "beats 1–8"))
            .collect();
        let (_, cx) =
            cx.add_window_view(|_, cx| RangeSelectionList::new("range", "empty", rows, None, cx));
        let list = cx.debug_bounds("range").unwrap();
        let last_before = cx.debug_bounds("range-row-23").unwrap();

        cx.simulate_event(ScrollWheelEvent {
            position: list.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            ..Default::default()
        });

        let last_after = cx.debug_bounds("range-row-23").unwrap();
        assert_eq!(list.size.height, crate::style::S8);
        assert!(last_after.origin.y < last_before.origin.y);
    }
}
