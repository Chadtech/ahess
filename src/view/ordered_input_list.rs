use gpui::{div, prelude::*, ElementId, Entity, ScrollHandle, SharedString};

use crate::{style as s, view::text_input::TextInput};

pub fn editable(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    inputs: &[Entity<TextInput>],
    invalid_items: &[usize],
    scroll_handle: &ScrollHandle,
) -> gpui::Div {
    let rows = inputs.iter().cloned().enumerate().map(|(index, input)| {
        div()
            .flex()
            .flex_none()
            .w_full()
            .child(
                input_field(input, invalid_items.contains(&index))
                    .debug_selector(move || format!("ordered-input-field-{index}")),
            )
            .debug_selector(move || format!("ordered-input-item-{index}"))
    });

    let label = div()
        .flex_none()
        .w_full()
        .text_color(s::TEXT_HEADER)
        .child(label.into())
        .debug_selector(|| "ordered-input-label".to_string());

    let body = div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .min_h(s::S0)
        .w_full()
        .gap(s::S4)
        .overflow_y_scroll()
        .track_scroll(scroll_handle)
        .children(rows);

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(s::S0)
        .w(s::S9)
        .max_w_full()
        .gap(s::S3)
        .child(label)
        .child(body)
}

fn input_field(input: Entity<TextInput>, invalid: bool) -> gpui::Div {
    div()
        .relative()
        .w_full()
        .min_w(s::S0)
        .child(s::sunken(input).overflow_hidden())
        .children(invalid.then(|| div().absolute().inset_0().border_2().border_color(s::RED2)))
}
