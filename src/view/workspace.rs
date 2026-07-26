use gpui::{div, prelude::*, px, AnyElement, Entity};

use crate::{style as s, view::button::Button};

pub struct ListDetailArgs<List, Details> {
    pub list: List,
    pub details: Details,
    pub auxiliary: Option<AnyElement>,
    pub footer: Option<AnyElement>,
}

pub fn selector(buttons: impl IntoIterator<Item = Entity<Button>>) -> gpui::Div {
    div().flex().gap(s::S3).children(buttons)
}

pub fn list_detail<List, Details>(args: ListDetailArgs<List, Details>) -> gpui::Div
where
    List: IntoElement,
    Details: IntoElement,
{
    let ListDetailArgs {
        list,
        details,
        auxiliary,
        footer,
    } = args;
    let content = match auxiliary {
        Some(auxiliary) => div()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .child(equal_width_column(list))
            .child(equal_width_column(details))
            .child(equal_width_column(auxiliary)),
        None => div()
            .flex()
            .flex_1()
            .min_h(px(0.0))
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .child(list)
            .child(details),
    };

    let body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .child(content);
    let body = if let Some(footer) = footer {
        body.child(
            div()
                .flex()
                .justify_end()
                .p(s::CONTENT_PADDING)
                .pt(s::S0)
                .child(footer),
        )
    } else {
        body
    };

    tile(body)
}

pub fn management_form(form: impl IntoElement, actions: impl IntoElement) -> gpui::Div {
    tile(
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .justify_between()
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .child(form)
            .child(actions),
    )
}

pub fn column_with_actions(content: impl IntoElement, actions: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .child(content)
        .child(div().flex().pt(s::S4).child(actions))
}

pub fn tile(content: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .size_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
        .bg(s::GREEN2)
        .p(s::CONTENT_PADDING)
        .child(
            s::raised(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .min_w(s::S0)
                    .min_h(s::S0)
                    .overflow_hidden()
                    .bg(s::GRAY2)
                    .child(content),
            )
            .flex()
            .flex_1()
            .min_w(s::S0)
            .min_h(s::S0)
            .overflow_hidden(),
        )
}

fn equal_width_column(child: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_1()
        .w(s::S0)
        .min_w(s::S0)
        .min_h(px(0.0))
        .child(child)
}
