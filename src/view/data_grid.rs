use std::{collections::HashSet, iter::FromIterator, ops::Range, sync::Arc};

use gpui::{
    div, point, prelude::*, uniform_list, CursorStyle, ElementId, Entity, ListSizingBehavior,
    Pixels, ScrollHandle, ScrollStrategy, UniformListScrollHandle,
};

use crate::{part::ScoreRowRange, style as s, view::text_input::TextInput};

const ROW_LABEL_WIDTH: Pixels = s::S7;
const STANDARD_CELL_WIDTH: Pixels = s::S8;
const COMPACT_CELL_WIDTH: Pixels = s::S7;

#[derive(Clone, Copy, Debug, Default)]
enum ColumnDensity {
    #[default]
    Standard,
    Compact,
}

impl ColumnDensity {
    fn width(self) -> Pixels {
        match self {
            Self::Standard => STANDARD_CELL_WIDTH,
            Self::Compact => COMPACT_CELL_WIDTH,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct InvalidCells(Arc<HashSet<(usize, usize)>>);

impl InvalidCells {
    pub fn contains(&self, row: usize, column: usize) -> bool {
        self.0.contains(&(row, column))
    }
}

impl FromIterator<(usize, usize)> for InvalidCells {
    fn from_iter<T: IntoIterator<Item = (usize, usize)>>(cells: T) -> Self {
        Self(Arc::new(cells.into_iter().collect()))
    }
}

#[derive(Clone, Debug)]
pub struct DataGridScrollHandle {
    horizontal: ScrollHandle,
    vertical: UniformListScrollHandle,
    column_density: ColumnDensity,
}

impl DataGridScrollHandle {
    pub fn new() -> Self {
        Self {
            horizontal: ScrollHandle::new(),
            vertical: UniformListScrollHandle::new(),
            column_density: ColumnDensity::Standard,
        }
    }

    pub fn compact() -> Self {
        Self {
            horizontal: ScrollHandle::new(),
            vertical: UniformListScrollHandle::new(),
            column_density: ColumnDensity::Compact,
        }
    }

    fn cell_width(&self) -> Pixels {
        self.column_density.width()
    }
}

impl Default for DataGridScrollHandle {
    fn default() -> Self {
        Self::new()
    }
}

pub fn editable(
    id: impl Into<ElementId>,
    column_labels: Vec<String>,
    rows: &[Vec<Entity<TextInput>>],
    invalid_cells: &InvalidCells,
    playing_row: Option<usize>,
    scroll_handle: &DataGridScrollHandle,
) -> gpui::Div {
    editable_grid(
        id,
        column_labels,
        rows,
        invalid_cells,
        None,
        None,
        playing_row,
        scroll_handle,
        false,
        |_, header| header,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn editable_with_row_selection<F>(
    id: impl Into<ElementId>,
    column_labels: Vec<String>,
    rows: &[Vec<Entity<TextInput>>],
    invalid_cells: &InvalidCells,
    row_labels: Vec<String>,
    selected_rows: Option<ScoreRowRange>,
    playing_row: Option<usize>,
    scroll_handle: &DataGridScrollHandle,
    decorate_row_header: F,
) -> gpui::Div
where
    F: Fn(usize, gpui::Div) -> gpui::Div + 'static,
{
    editable_grid(
        id,
        column_labels,
        rows,
        invalid_cells,
        Some(row_labels),
        selected_rows,
        playing_row,
        scroll_handle,
        true,
        decorate_row_header,
    )
}

#[allow(clippy::too_many_arguments)]
fn editable_grid<F>(
    id: impl Into<ElementId>,
    column_labels: Vec<String>,
    rows: &[Vec<Entity<TextInput>>],
    invalid_cells: &InvalidCells,
    row_labels: Option<Vec<String>>,
    selected_rows: Option<ScoreRowRange>,
    playing_row: Option<usize>,
    scroll_handle: &DataGridScrollHandle,
    row_selection_enabled: bool,
    decorate_row_header: F,
) -> gpui::Div
where
    F: Fn(usize, gpui::Div) -> gpui::Div + 'static,
{
    let id = id.into();
    let cell_width = scroll_handle.cell_width();
    let content_width = ROW_LABEL_WIDTH + cell_width * column_labels.len() as f32;
    let row_count = rows.len();
    let rows = rows.to_vec();
    let invalid_cells = invalid_cells.clone();
    let row_labels = row_labels.unwrap_or_else(|| {
        (1..=row_count)
            .map(|row_number| row_number.to_string())
            .collect()
    });
    let content = uniform_list((id.clone(), "rows"), row_count + 1, move |range, _, _| {
        render_rows(
            range,
            &column_labels,
            &rows,
            &invalid_cells,
            &row_labels,
            selected_rows,
            playing_row,
            row_selection_enabled,
            cell_width,
            &decorate_row_header,
        )
    })
    .flex_none()
    .w(content_width)
    .with_sizing_behavior(ListSizingBehavior::Infer)
    .track_scroll(scroll_handle.vertical.clone());

    let mut viewport = div()
        .id(id)
        .flex()
        .flex_1()
        .w_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
        .overflow_x_scroll()
        .track_scroll(&scroll_handle.horizontal)
        .bg(s::GREEN3)
        .child(content);
    viewport.style().restrict_scroll_to_axis = Some(true);

    s::sunken(viewport)
        .flex()
        .flex_1()
        .w_full()
        .min_w(s::S0)
        .min_h(s::S0)
        .overflow_hidden()
}

#[allow(clippy::too_many_arguments)]
fn render_rows<F>(
    range: Range<usize>,
    column_labels: &[String],
    rows: &[Vec<Entity<TextInput>>],
    invalid_cells: &InvalidCells,
    row_labels: &[String],
    selected_rows: Option<ScoreRowRange>,
    playing_row: Option<usize>,
    row_selection_enabled: bool,
    cell_width: Pixels,
    decorate_row_header: &F,
) -> Vec<gpui::Div>
where
    F: Fn(usize, gpui::Div) -> gpui::Div,
{
    range
        .map(|item_index| {
            let Some(row_index) = item_index.checked_sub(1) else {
                return header_row(column_labels, cell_width);
            };
            let row = &rows[row_index];
            let selected = selected_rows.is_some_and(|rows| rows.contains(row_index));
            let row_header = row_header_cell(
                row_labels
                    .get(row_index)
                    .cloned()
                    .unwrap_or_else(|| (row_index + 1).to_string()),
                row_index,
                selected,
                row_selection_enabled,
            );
            div()
                .relative()
                .flex()
                .flex_row()
                .child(decorate_row_header(row_index, row_header))
                .children(
                    row.iter()
                        .cloned()
                        .enumerate()
                        .map(|(column_index, input)| {
                            input_cell(
                                input,
                                invalid_cells.contains(row_index, column_index),
                                cell_width,
                            )
                        }),
                )
                .children((playing_row == Some(row_index)).then(|| playback_row_border(row_index)))
        })
        .collect()
}

fn header_row(column_labels: &[String], cell_width: Pixels) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .child(header_cell(String::new(), ROW_LABEL_WIDTH))
        .children(
            column_labels
                .iter()
                .cloned()
                .map(|label| header_cell(label, cell_width)),
        )
}

fn row_header_cell(label: String, row: usize, selected: bool, selectable: bool) -> gpui::Div {
    let text_color = if selected {
        s::TEXT_HOVERED
    } else {
        s::TEXT_HEADER
    };
    let content = div()
        .flex()
        .items_center()
        .justify_center()
        .w(ROW_LABEL_WIDTH)
        .h(row_height())
        .bg(s::GRAY2)
        .text_color(text_color)
        .cursor(if selectable {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .when(selectable && !selected, |header| {
            header.hover(|style| style.text_color(s::TEXT_HOVERED))
        })
        .child(label);
    let header = if selected {
        s::sunken(content).debug_selector(move || format!("score-selected-row-header-{row}"))
    } else {
        s::raised(content)
    };

    div().relative().child(header)
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
            .child(div().min_w(s::S0).max_w_full().truncate().child(label)),
    )
}

pub fn reveal_cell(scroll_handle: &DataGridScrollHandle, row: usize, column: usize) {
    scroll_handle
        .vertical
        .scroll_to_item(row + 1, ScrollStrategy::Center);

    let viewport_width = scroll_handle.horizontal.bounds().size.width;
    if viewport_width <= s::S0 {
        return;
    }

    let cell_width = scroll_handle.cell_width();
    let cell_left = ROW_LABEL_WIDTH + cell_width * column as f32;
    let cell_right = cell_left + cell_width;
    let mut offset = scroll_handle.horizontal.offset();
    let visible_left = -offset.x;
    let visible_right = visible_left + viewport_width;

    if cell_left < visible_left {
        offset.x = -cell_left;
    } else if cell_right > visible_right {
        offset.x = -(cell_right - viewport_width);
    }

    let max_offset = scroll_handle.horizontal.max_offset();
    offset.x = clamp_offset(offset.x, max_offset.width);
    scroll_handle
        .horizontal
        .set_offset(point(offset.x, offset.y));
}

fn input_cell(input: Entity<TextInput>, invalid: bool, cell_width: Pixels) -> gpui::Div {
    div()
        .relative()
        .w(cell_width)
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

#[cfg(test)]
mod tests {
    use gpui::{
        div, point, prelude::*, px, size, Context, Entity, ScrollDelta, ScrollWheelEvent,
        TestAppContext, Window,
    };

    use super::{editable, DataGridScrollHandle, InvalidCells};
    use crate::{style as s, view::text_input::TextInput};

    struct GridHost {
        rows: Vec<Vec<Entity<TextInput>>>,
        scroll_handle: DataGridScrollHandle,
    }

    impl Render for GridHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let column_labels = (1..=8).map(|column| format!("column {column}")).collect();
            div().flex().size_full().child(
                editable(
                    "grid",
                    column_labels,
                    &self.rows,
                    &InvalidCells::default(),
                    None,
                    &self.scroll_handle,
                )
                .debug_selector(|| "test-grid".to_string()),
            )
        }
    }

    #[test]
    fn compact_columns_use_the_six_character_width_token() {
        assert_eq!(DataGridScrollHandle::compact().cell_width(), s::S7);
        assert_eq!(DataGridScrollHandle::new().cell_width(), s::S8);
    }

    #[gpui::test]
    fn vertical_overscroll_does_not_move_the_grid_horizontally(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, cx| {
            let rows = (0..40)
                .map(|_| {
                    (0..8)
                        .map(|_| cx.new(|cx| TextInput::new("", "", cx)))
                        .collect()
                })
                .collect();
            GridHost {
                rows,
                scroll_handle: DataGridScrollHandle::new(),
            }
        });
        cx.simulate_resize(size(px(320.0), px(240.0)));
        cx.run_until_parked();

        let grid = cx.debug_bounds("test-grid").unwrap();
        let scroll_handle = cx.update(|_, cx| host.read(cx).scroll_handle.clone());
        assert!(scroll_handle.vertical.is_scrollable());
        assert!(scroll_handle.horizontal.max_offset().width > px(0.0));

        for _ in 0..3 {
            cx.simulate_event(ScrollWheelEvent {
                position: grid.center(),
                delta: ScrollDelta::Pixels(point(px(0.0), px(-10_000.0))),
                ..Default::default()
            });
            cx.run_until_parked();
        }

        assert_eq!(scroll_handle.horizontal.offset().x, px(0.0));

        cx.simulate_event(ScrollWheelEvent {
            position: grid.center(),
            delta: ScrollDelta::Pixels(point(px(-500.0), px(0.0))),
            ..Default::default()
        });
        cx.run_until_parked();

        assert!(scroll_handle.horizontal.offset().x < px(0.0));
    }
}
