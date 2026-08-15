use gpui::{prelude::*, CursorStyle, SharedString};

use crate::{style as s, view::selection_list};

pub fn menu(actions: Vec<gpui::Div>) -> gpui::Div {
    gpui::div()
        .flex()
        .flex_col()
        .absolute()
        .right_0()
        .top_0()
        .w(s::S9)
        .bg(s::GREEN3)
        .border_2()
        .border_color(s::GRAY1)
        .whitespace_nowrap()
        .occlude()
        .children(actions)
}

pub fn action(index: usize, label: impl Into<SharedString>) -> gpui::Div {
    selection_list::row(index, false, label)
        .hover(|style| style.bg(s::GREEN5).text_color(s::TEXT_HOVERED))
        .cursor(CursorStyle::PointingHand)
}
