use gpui::{div, prelude::*, Entity, SharedString};

use crate::{style as s, view::text_input::TextInput};

pub fn field_group(label: impl Into<SharedString>, input: Entity<TextInput>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(s::S3)
        .flex_1()
        .child(div().text_color(s::FIELD_LABEL_TEXT).child(label.into()))
        .child(s::sunken(input).overflow_hidden())
}
