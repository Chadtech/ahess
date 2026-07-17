use gpui::{div, point, prelude::*, ElementId, Entity, Pixels, ScrollHandle};

use crate::{style as s, view::text_input::TextInput};

const ROW_LABEL_WIDTH: Pixels = s::S6;
const CELL_WIDTH: Pixels = s::S8;

pub fn editable(
    id: impl Into<ElementId>,
    column_labels: Vec<String>,
    rows: &[Vec<Entity<TextInput>>],
    invalid_cells: &[(usize, usize)],
    playing_row: Option<usize>,
    scroll_handle: &ScrollHandle,
) -> gpui::Div {
    let content_width = ROW_LABEL_WIDTH + CELL_WIDTH * column_labels.len() as f32;
    let header = div()
        .flex()
        .flex_row()
        .child(header_cell(String::new(), ROW_LABEL_WIDTH))
        .children(
            column_labels
                .into_iter()
                .map(|label| header_cell(label, CELL_WIDTH)),
        );
    let body_rows = rows.iter().enumerate().map(|(row_index, row)| {
        div()
            .relative()
            .flex()
            .flex_row()
            .child(header_cell((row_index + 1).to_string(), ROW_LABEL_WIDTH))
            .children(
                row.iter()
                    .cloned()
                    .enumerate()
                    .map(|(column_index, input)| {
                        input_cell(input, invalid_cells.contains(&(row_index, column_index)))
                    }),
            )
            .children((playing_row == Some(row_index)).then(|| playback_row_border(row_index)))
    });
    let content = div()
        .flex()
        .flex_col()
        .w(content_width)
        .child(header)
        .children(body_rows);

    s::sunken(
        div()
            .id(id)
            .flex_1()
            .w_full()
            .min_w(s::S0)
            .min_h(s::S0)
            .overflow_scroll()
            .track_scroll(scroll_handle)
            .bg(s::GREEN3)
            .child(content),
    )
    .flex()
    .flex_1()
    .w_full()
    .min_w(s::S0)
    .min_h(s::S0)
    .overflow_hidden()
}

fn playback_row_border(row: usize) -> gpui::Div {
    div()
        .absolute()
        .inset_0()
        .border_2()
        .border_color(s::PLAYBACK_ROW_BORDER)
        .debug_selector(move || format!("score-playback-row-{row}"))
}

fn header_cell(label: String, width: Pixels) -> gpui::Div {
    s::raised(
        div()
            .flex()
            .items_center()
            .justify_center()
            .w(width)
            .h(row_height())
            .bg(s::GRAY2)
            .text_color(s::TEXT_HEADER)
            .child(label),
    )
}

pub fn reveal_cell(scroll_handle: &ScrollHandle, row: usize, column: usize) {
    let viewport = scroll_handle.bounds().size;
    if viewport.width <= s::S0 || viewport.height <= s::S0 {
        return;
    }

    let cell_left = ROW_LABEL_WIDTH + CELL_WIDTH * column as f32;
    let cell_right = cell_left + CELL_WIDTH;
    let cell_top = row_height() * (row + 1) as f32;
    let cell_bottom = cell_top + row_height();
    let mut offset = scroll_handle.offset();
    let visible_left = -offset.x;
    let visible_right = visible_left + viewport.width;
    let visible_top = -offset.y;
    let visible_bottom = visible_top + viewport.height;

    if cell_left < visible_left {
        offset.x = -cell_left;
    } else if cell_right > visible_right {
        offset.x = -(cell_right - viewport.width);
    }
    if cell_top < visible_top {
        offset.y = -cell_top;
    } else if cell_bottom > visible_bottom {
        offset.y = -(cell_bottom - viewport.height);
    }

    let max_offset = scroll_handle.max_offset();
    offset.x = clamp_offset(offset.x, max_offset.width);
    offset.y = clamp_offset(offset.y, max_offset.height);
    scroll_handle.set_offset(point(offset.x, offset.y));
}

fn input_cell(input: Entity<TextInput>, invalid: bool) -> gpui::Div {
    div()
        .relative()
        .w(CELL_WIDTH)
        .h(row_height())
        .bg(s::GREEN3)
        .child(s::sunken(input).size_full())
        .children(invalid.then(|| div().absolute().inset_0().border_2().border_color(s::RED2)))
}

fn row_height() -> Pixels {
    s::S6 + s::S2
}

fn clamp_offset(offset: Pixels, maximum: Pixels) -> Pixels {
    if offset > s::S0 {
        s::S0
    } else if offset < -maximum {
        -maximum
    } else {
        offset
    }
}
