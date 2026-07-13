use gpui::{div, prelude::*, px, Entity, SharedString};

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

pub struct ListDetailArgs<Title, List, Details> {
    pub title: Title,
    pub close_button: Entity<Button>,
    pub list: List,
    pub details: Details,
    pub add_button: Entity<Button>,
}

pub fn list_detail_dialog<Title, List, Details>(
    args: ListDetailArgs<Title, List, Details>,
) -> gpui::Div
where
    Title: Into<SharedString>,
    List: IntoElement,
    Details: IntoElement,
{
    let ListDetailArgs {
        title,
        close_button,
        list,
        details,
        add_button,
    } = args;

    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S11)
            .h(s::S10)
            .bg(s::GRAY2)
            .child(title_bar(title, Some(close_button)))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .gap(s::CONTENT_PADDING)
                    .p(s::CONTENT_PADDING)
                    .child(list)
                    .child(details),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .p(s::CONTENT_PADDING)
                    .pt(s::S0)
                    .child(add_button),
            ),
    )
}

pub fn management_form_dialog(
    title: impl Into<SharedString>,
    close_button: Entity<Button>,
    form: impl IntoElement,
    actions: impl IntoElement,
) -> gpui::Div {
    s::raised(
        div()
            .flex()
            .flex_col()
            .w(s::S11)
            .h(s::S10)
            .bg(s::GRAY2)
            .child(title_bar(title, Some(close_button)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .justify_between()
                    .gap(s::CONTENT_PADDING)
                    .p(s::CONTENT_PADDING)
                    .child(form)
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
