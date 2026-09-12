use std::{ops::Range, time::Duration};

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgba, size, App, Bounds, ClipboardItem,
    Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Rgba, ShapedLine,
    SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    style as s,
    view::{context_menu, detail_marker},
};

enum CellInteraction {
    Ready,
    Menu,
    Copied {
        text: SharedString,
        _dismiss: gpui::Task<()>,
    },
}

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        FocusNext,
        FocusPrev,
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, None),
        KeyBinding::new("delete", Delete, None),
        KeyBinding::new("left", Left, None),
        KeyBinding::new("right", Right, None),
        KeyBinding::new("shift-left", SelectLeft, None),
        KeyBinding::new("shift-right", SelectRight, None),
        KeyBinding::new("secondary-a", SelectAll, None),
        KeyBinding::new("secondary-v", Paste, None),
        KeyBinding::new("secondary-c", Copy, None),
        KeyBinding::new("secondary-x", Cut, None),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrev, None),
        KeyBinding::new("home", Home, None),
        KeyBinding::new("end", End, None),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, None),
    ]);
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    background: Rgba,
    cell_interaction: Option<CellInteraction>,
    score_note_colors: Option<[Rgba; 3]>,
    details_summary: Option<fn(&str) -> Option<String>>,
    vertical_neighbors: Option<(gpui::WeakEntity<TextInput>, gpui::WeakEntity<TextInput>)>,
    tab_neighbors: Option<(gpui::WeakEntity<TextInput>, gpui::WeakEntity<TextInput>)>,
}

pub struct Changed;
pub struct DetailsRequested;
impl EventEmitter<DetailsRequested> for TextInput {}

impl EventEmitter<Changed> for TextInput {}

impl TextInput {
    pub fn new(
        content: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            content: content.into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            background: s::GREEN3,
            cell_interaction: None,
            score_note_colors: None,
            details_summary: None,
            vertical_neighbors: None,
            tab_neighbors: None,
        }
    }

    pub fn set_tab_neighbors(&mut self, previous: &Entity<TextInput>, next: &Entity<TextInput>) {
        self.tab_neighbors = Some((previous.downgrade(), next.downgrade()));
    }
    pub fn set_vertical_neighbors(
        &mut self,
        previous: &Entity<TextInput>,
        next: &Entity<TextInput>,
    ) {
        self.vertical_neighbors = Some((previous.downgrade(), next.downgrade()));
    }
    pub fn with_details(mut self, summary: fn(&str) -> Option<String>) -> Self {
        self.details_summary = Some(summary);
        self
    }
    fn detail_summary(&self) -> Option<String> {
        self.details_summary.and_then(|f| f(&self.content))
    }

    pub fn with_cell_clipboard(mut self) -> Self {
        self.cell_interaction = Some(CellInteraction::Ready);
        self
    }

    fn copy_cell(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(self.content.to_string()));
        let text = if self.content.is_empty() {
            "copied empty cell".into()
        } else {
            format!(
                "copied {}",
                self.detail_summary()
                    .unwrap_or_else(|| self.content.to_string())
            )
            .into()
        };
        let dismiss = cx.spawn(async move |input, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1200))
                .await;
            let _ = input.update(cx, |input, cx| {
                input.cell_interaction = Some(CellInteraction::Ready);
                cx.notify();
            });
        });
        self.cell_interaction = Some(CellInteraction::Copied {
            text,
            _dismiss: dismiss,
        });
        self.is_selecting = false;
        cx.notify();
    }

    fn paste_cell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cell_interaction = Some(CellInteraction::Ready);
        self.is_selecting = false;
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let range = self.range_to_utf16(&(0..self.content.len()));
            self.replace_text_in_range(Some(range), &text.replace('\n', " "), window, cx);
        }
        cx.notify();
    }

    fn close_cell_menu(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cell_interaction, Some(CellInteraction::Menu)) {
            self.cell_interaction = Some(CellInteraction::Ready);
            cx.notify();
        }
    }

    pub fn with_background(mut self, background: Rgba) -> Self {
        self.background = background;
        self
    }

    /// Color six-character note pairs or five-character pitch@volume notation.
    /// Keep the value as one continuous editable input.
    pub fn with_score_note_colors(mut self, colors: [Rgba; 3]) -> Self {
        self.score_note_colors = Some(colors);
        self
    }

    pub(crate) fn set_background(&mut self, background: Rgba, cx: &mut Context<Self>) {
        if self.background == background {
            return;
        }
        self.background = background;
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn background(&self) -> Rgba {
        self.background
    }

    pub fn value(&self) -> String {
        self.content.to_string()
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    pub(crate) fn sync_value(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        let value = value.into();
        if self.content == value {
            return;
        }

        self.content = value;
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.selected_range.end), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.selected_range.end), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window);
        if self.cell_interaction.is_some() && event.modifiers.secondary() {
            if event.modifiers.shift {
                self.paste_cell(window, cx);
            } else {
                self.copy_cell(cx);
            }
            cx.stop_propagation();
            return;
        }
        self.close_cell_menu(cx);
        if self.details_summary.is_some()
            && (event.click_count == 2 || self.detail_summary().is_some())
        {
            cx.emit(DetailsRequested);
            cx.stop_propagation();
            return;
        }
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() && self.cell_interaction.is_some() {
            self.copy_cell(cx);
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(next) = self
            .tab_neighbors
            .as_ref()
            .and_then(|(_, next)| next.upgrade())
        {
            next.read(cx).focus(window);
        } else {
            window.focus_next();
        }
    }

    fn focus_prev(&mut self, _: &FocusPrev, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous) = self
            .tab_neighbors
            .as_ref()
            .and_then(|(previous, _)| previous.upgrade())
        {
            previous.read(cx).focus(window);
        } else {
            window.focus_prev();
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };

        if position.y < bounds.top() {
            return 0;
        }

        if position.y > bounds.bottom() {
            return self.content.len();
        }

        line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };

        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }

        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }

            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }

            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.detail_summary().is_some() && range_utf16.is_none() {
            if new_text.is_empty() {
                self.content = "".into();
                self.selected_range = 0..0;
                cx.emit(Changed);
                cx.notify();
            } else {
                cx.emit(DetailsRequested);
            }
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        cx.emit(Changed);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.detail_summary().is_some() {
            cx.emit(DetailsRequested);
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..range.start + new_text.len());
        }

        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;

        cx.emit(Changed);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);

        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;

        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();

        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let is_placeholder = content.is_empty();
        let display_text = if is_placeholder {
            input.placeholder.clone()
        } else {
            content
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: if is_placeholder {
                s::GRAY4.into()
            } else {
                style.color
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        if let Some(colors) = input.score_note_colors {
            let boundaries = match display_text.as_bytes() {
                [_, _, _, _, _, _] if display_text.is_ascii() => Some([2, 4, 6]),
                [_, _, b'@', _, _] if display_text.is_ascii() => Some([2, 3, 5]),
                _ => None,
            };
            if let Some(boundaries) = boundaries.filter(|_| !is_placeholder) {
                let mut offset = 0;
                runs = runs
                    .into_iter()
                    .flat_map(|run| {
                        let end = offset + run.len;
                        let mut pairs = Vec::new();
                        while offset < end {
                            let pair = boundaries.partition_point(|boundary| *boundary <= offset);
                            let next = end.min(boundaries[pair]);
                            pairs.push(TextRun {
                                len: next - offset,
                                color: colors[pair].into(),
                                ..run.clone()
                            });
                            offset = next;
                        }
                        pairs
                    })
                    .collect();
            }
        }

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top() + px(4.0)),
                        size(px(2.0), bounds.bottom() - bounds.top() - px(8.0)),
                    ),
                    s::RED2,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top() + px(4.0),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom() - px(4.0),
                        ),
                    ),
                    rgba(0x0abab540),
                )),
                None,
            )
        };

        PrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        if focus_handle.is_focused(window) {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection)
            }
        }

        let line = prepaint.line.take().unwrap();
        line.paint(bounds.origin, window.line_height(), window, cx)
            .unwrap();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .when(self.cell_interaction.is_some(), |input| {
                input.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|input, _, window, cx| {
                        input.focus(window);
                        input.is_selecting = false;
                        input.cell_interaction = Some(CellInteraction::Menu);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
            })
            .on_key_down(
                cx.listener(|input, event: &gpui::KeyDownEvent, window, cx| {
                    if let Some((previous, next)) = &input.vertical_neighbors {
                        let neighbor = match event.keystroke.key.as_str() {
                            "up" => Some(previous),
                            "down" => Some(next),
                            "enter"
                                if !event.keystroke.modifiers.alt
                                    && !event.keystroke.modifiers.secondary() =>
                            {
                                Some(next)
                            }
                            _ => None,
                        };
                        if let Some(neighbor) = neighbor.and_then(|n| n.upgrade()) {
                            neighbor.read(cx).focus(window);
                            cx.stop_propagation();
                            return;
                        }
                    }
                    if event.keystroke.key == "escape" {
                        input.close_cell_menu(cx);
                    }
                    if event.keystroke.key == "enter"
                        && event.keystroke.modifiers.alt
                        && input.details_summary.is_some()
                    {
                        cx.emit(DetailsRequested);
                        cx.stop_propagation();
                    }
                }),
            )
            .children(match &self.cell_interaction {
                Some(CellInteraction::Menu) => {
                    let modifier = if cfg!(target_os = "macos") {
                        "⌘"
                    } else {
                        "ctrl"
                    };
                    Some(
                        gpui::deferred(
                            context_menu::menu({
                                let mut actions = vec![
                                    context_menu::action(
                                        0,
                                        format!("copy cell   {modifier}-click"),
                                    )
                                    .debug_selector(|| "copy-cell".into())
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|input, _, _, cx| {
                                            input.copy_cell(cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
                                    context_menu::action(
                                        1,
                                        format!("paste cell   {modifier}-shift-click"),
                                    )
                                    .debug_selector(|| "paste-cell".into())
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|input, _, window, cx| {
                                            input.paste_cell(window, cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
                                ];
                                if self.details_summary.is_some() {
                                    actions.push(
                                        context_menu::action(2, "note details   double-click")
                                            .debug_selector(|| "note-details".into())
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(|input, _, _, cx| {
                                                    input.close_cell_menu(cx);
                                                    cx.emit(DetailsRequested);
                                                    cx.stop_propagation();
                                                }),
                                            ),
                                    );
                                }
                                actions
                            })
                            .left_0()
                            .right_auto()
                            .w_auto()
                            .on_mouse_down_out(
                                cx.listener(|input, _, _, cx| input.close_cell_menu(cx)),
                            ),
                        )
                        .with_priority(1)
                        .into_any_element(),
                    )
                }
                Some(CellInteraction::Copied { text, .. }) => Some(
                    gpui::deferred(
                        div()
                            .absolute()
                            .left_0()
                            .top_full()
                            .px(s::S3)
                            .bg(s::GRAY2)
                            .text_color(s::TEXT_DEFAULT)
                            .whitespace_nowrap()
                            .child(text.clone()),
                    )
                    .with_priority(1)
                    .into_any_element(),
                ),
                _ => None,
            })
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_prev))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .h(px(34.0))
            .w_full()
            .flex()
            .items_center()
            .px(s::S3)
            .line_height(s::TEXT_LINE_HEIGHT)
            .text_size(s::TEXT_SIZE)
            .text_color(s::TEXT_DEFAULT)
            .bg(self.background)
            .border(s::BORDER_WIDTH)
            .border_color(s::GREEN3)
            .when(self.detail_summary().is_some(), |input| {
                input.child(detail_marker::corner())
            })
            .child(if let Some(summary) = self.detail_summary() {
                div()
                    .relative()
                    .w_full()
                    .truncate()
                    .text_color(s::SCORE_PITCH_TEXT)
                    .child(summary)
                    .into_any_element()
            } else {
                TextElement { input: cx.entity() }.into_any_element()
            })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod clipboard_tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn cell_gestures_copy_all_and_replace_all(cx: &mut TestAppContext) {
        let (input, cx) =
            cx.add_window_view(|_, cx| TextInput::new("310880", "", cx).with_cell_clipboard());
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.selected_range = 1..5;
                input.on_mouse_down(
                    &MouseDownEvent {
                        modifiers: gpui::Modifiers::secondary_key(),
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "310880");
                assert!(!input.is_selecting);
                input.sync_value("6001ff", cx);
                input.marked_range = Some(1..3);
                input.on_mouse_down(
                    &MouseDownEvent {
                        modifiers: gpui::Modifiers {
                            shift: true,
                            ..gpui::Modifiers::secondary_key()
                        },
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                assert_eq!(input.value(), "310880");
                assert_eq!(input.selected_range, 6..6);
                assert!(input.marked_range.is_none());
            });
        });
    }

    #[gpui::test]
    fn context_menu_dispatches_whole_cell_commands(cx: &mut TestAppContext) {
        let (input, cx) =
            cx.add_window_view(|_, cx| TextInput::new("310880", "", cx).with_cell_clipboard());
        cx.run_until_parked();
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Right,
            position: point(px(20.0), px(15.0)),
            ..Default::default()
        });
        cx.run_until_parked();
        let copy = cx.debug_bounds("copy-cell").unwrap();
        cx.simulate_click(copy.center(), gpui::Modifiers::default());
        cx.update(|_, cx| {
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "310880");
            assert!(matches!(
                input.read(cx).cell_interaction,
                Some(CellInteraction::Copied { .. })
            ));
            input.update(cx, |input, cx| input.sync_value("6001ff", cx));
        });
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Right,
            position: point(px(20.0), px(15.0)),
            ..Default::default()
        });
        cx.run_until_parked();
        let paste = cx.debug_bounds("paste-cell").unwrap();
        cx.simulate_click(paste.center(), gpui::Modifiers::default());
        cx.update(|_, cx| {
            assert_eq!(input.read(cx).value(), "310880");
            assert!(matches!(
                input.read(cx).cell_interaction,
                Some(CellInteraction::Ready)
            ));
        });
    }

    #[gpui::test]
    fn keyboard_copy_respects_selection_and_plain_fields(cx: &mut TestAppContext) {
        let (input, cx) = cx.add_window_view(|_, cx| TextInput::new("310880", "", cx));
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into()));
                input.copy(&Copy, window, cx);
                assert_eq!(
                    cx.read_from_clipboard().unwrap().text().unwrap(),
                    "unchanged"
                );
                input.cell_interaction = Some(CellInteraction::Ready);
                input.copy(&Copy, window, cx);
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "310880");
                input.selected_range = 2..4;
                input.copy(&Copy, window, cx);
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "08");
            });
        });
    }
}
