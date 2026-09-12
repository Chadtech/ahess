//! A compact corner indicator for cells with an expanded editor.
use crate::style as s;
use gpui::{div, prelude::*};
pub fn corner() -> gpui::Div {
    div()
        .absolute()
        .top(s::S2)
        .right(s::S2)
        .w(s::S4)
        .h(s::S4)
        .child(
            gpui::canvas(
                |_, _, _| (),
                |bounds, _, window, _| {
                    let mut path = gpui::PathBuilder::fill();
                    path.move_to(bounds.origin);
                    path.line_to(gpui::point(bounds.right(), bounds.top()));
                    path.line_to(gpui::point(bounds.right(), bounds.bottom()));
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, s::YELLOW6);
                    }
                },
            )
            .size_full(),
        )
}
