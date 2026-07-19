use gpui::{prelude::*, px, SharedString};

use crate::{style as s, view::dialog::title_bar};

pub fn tile(title: impl Into<SharedString>, content: impl IntoElement) -> gpui::Div {
    s::raised(
        gpui::div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(s::S0)
            .min_h(px(0.0))
            .overflow_hidden()
            .bg(s::GRAY2)
            .child(title_bar(title, None))
            .child(content),
    )
    .flex()
    .flex_1()
    .min_w(s::S0)
    .min_h(s::S0)
    .overflow_hidden()
}
