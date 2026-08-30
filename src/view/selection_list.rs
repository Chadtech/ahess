use gpui::{div, prelude::*, px, CursorStyle, ElementId, Entity, SharedString};

use crate::{style as s, view::text_input::TextInput};

pub fn list(
    id: impl Into<ElementId>,
    empty_message: impl Into<SharedString>,
    rows: Vec<gpui::Div>,
) -> gpui::Div {
    let body = div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .bg(s::GREEN3);
    let body = if rows.is_empty() {
        body.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(s::TEXT_DEFAULT)
                .child(empty_message.into()),
        )
    } else {
        body.children(rows)
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .w(s::S9)
        .child(s::sunken(body).flex().flex_1().overflow_hidden())
}

pub fn searchable(
    id: impl Into<ElementId>,
    search: Entity<TextInput>,
    empty_message: impl Into<SharedString>,
    rows: Vec<gpui::Div>,
) -> gpui::Div {
    let body = div()
        .id(id)
        .flex()
        .flex_col()
        .h(s::S9)
        .min_h(px(0.0))
        .overflow_y_scroll()
        .bg(s::GREEN3);
    let body = if rows.is_empty() {
        body.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(s::TEXT_DEFAULT)
                .child(empty_message.into()),
        )
    } else {
        body.children(rows)
    };

    div()
        .flex()
        .flex_col()
        .w_full()
        .gap(s::S3)
        .child(search)
        .child(s::sunken(body).overflow_hidden())
}

pub fn row(index: usize, selected: bool, label: impl Into<SharedString>) -> gpui::Div {
    row_content(index, selected, label.into())
}

pub fn row_content(index: usize, selected: bool, content: impl IntoElement) -> gpui::Div {
    let background = if selected {
        s::GREEN4
    } else if index.is_multiple_of(2) {
        s::GREEN2
    } else {
        s::GREEN3
    };
    let text_color = if selected { s::GRAY6 } else { s::GRAY5 };

    div()
        .flex_none()
        .bg(background)
        .p(s::S4)
        .text_color(text_color)
        .cursor(CursorStyle::PointingHand)
        .child(content)
}
