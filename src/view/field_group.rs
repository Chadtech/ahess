use gpui::{div, prelude::*, Entity, SharedString};

use crate::{style as s, view::text_input::TextInput};

pub fn field_group(label: impl Into<SharedString>, input: Entity<TextInput>) -> gpui::Div {
    control_group(label, s::sunken(input).overflow_hidden())
}

pub fn control_group(label: impl Into<SharedString>, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(s::S3)
        .flex_1()
        .child(div().text_color(s::FIELD_LABEL_TEXT).child(label.into()))
        .child(control)
}

pub fn compact_control_group(
    label: impl Into<SharedString>,
    control: impl IntoElement,
) -> gpui::Div {
    control_group(label, control)
        .flex_none()
        .w(s::S9)
        .max_w_full()
}
