use gpui::{div, prelude::*, Entity, SharedString};

use crate::{style as s, view::button::Button};

pub fn title_bar(
    title: impl Into<SharedString>,
    close_button: Option<Entity<Button>>,
) -> gpui::Div {
    let bar = div()
        .flex()
        .items_center()
        .justify_between()
        .bg(s::GRAY5)
        .text_color(s::DIALOG_TITLE_TEXT)
        .p(s::S3)
        .px(s::S4)
        .child(title.into());

    if let Some(close_button) = close_button {
        bar.child(close_button)
    } else {
        bar
    }
}

pub fn error_message(message: impl Into<SharedString>) -> gpui::Div {
    danger_panel(div().child(message.into()))
}

pub fn destructive_confirmation(
    message: impl Into<SharedString>,
    actions: impl IntoElement,
) -> gpui::Div {
    danger_panel(
        div()
            .flex()
            .flex_col()
            .gap(s::S3)
            .child(div().child(message.into()))
            .child(actions),
    )
}

pub fn destructive_dialog(
    title: impl Into<SharedString>,
    close_button: Option<Entity<Button>>,
    message: impl Into<SharedString>,
    actions: impl IntoElement,
) -> gpui::Div {
    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S10)
            .bg(s::RED1)
            .text_color(s::WHITE)
            .child(title_bar(title, close_button))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(s::S3)
                    .p(s::CONTENT_PADDING)
                    .child(div().child(message.into()))
                    .child(actions),
            ),
    )
}

fn danger_panel(content: impl IntoElement) -> gpui::Div {
    s::sunken(
        div()
            .bg(s::RED1)
            .text_color(s::WHITE)
            .p(s::S4)
            .child(content),
    )
    .overflow_hidden()
}

pub fn modal_overlay(dialog: impl IntoElement) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(s::MODAL_BACKDROP)
        .occlude()
        .child(dialog)
}
