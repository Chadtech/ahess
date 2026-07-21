use gpui::{div, prelude::*, Entity, SharedString};

use crate::{style as s, view::button::Button};

pub fn file_import(
    selection: impl Into<SharedString>,
    choose_button: Entity<Button>,
    remove_button: Entity<Button>,
) -> gpui::Div {
    s::sunken(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(s::S4)
            .bg(s::GRAY2)
            .p(s::S4)
            .child(
                div()
                    .flex_1()
                    .min_w(s::S0)
                    .truncate()
                    .child(selection.into()),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(s::S4)
                    .child(choose_button)
                    .child(remove_button),
            ),
    )
    .overflow_hidden()
}
