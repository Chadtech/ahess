use gpui::{div, prelude::*, SharedString};

use crate::style as s;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Status<T = ()> {
    #[default]
    Empty,
    Message(SharedString),
    Warning(SharedString),
    Error {
        message: SharedString,
        target: Option<T>,
    },
}

pub fn bar<T>(status: Status<T>) -> gpui::Div {
    let (background, text_color, message) = match status {
        Status::Empty => (s::GRAY2, s::TEXT_DEFAULT, None),
        Status::Message(message) => (s::GRAY2, s::TEXT_DEFAULT, Some(message)),
        Status::Warning(message) => (s::YELLOW2, s::YELLOW6, Some(message)),
        Status::Error { message, .. } => (s::RED1, s::WHITE, Some(message)),
    };

    div()
        .flex()
        .flex_none()
        .items_center()
        .w_full()
        .min_w(s::S0)
        .h(s::S6)
        .overflow_hidden()
        .border_t_2()
        .border_color(s::GRAY1)
        .bg(background)
        .px(s::S4)
        .text_color(text_color)
        .children(message.map(|message| div().w_full().min_w(s::S0).truncate().child(message)))
}
