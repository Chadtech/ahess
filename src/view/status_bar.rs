use gpui::{div, prelude::*, ClipboardItem, CursorStyle, MouseButton, SharedString};

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
    let (background, text_color, message, error_to_copy) = match status {
        Status::Empty => (s::GRAY2, s::TEXT_DEFAULT, None, None),
        Status::Message(message) => (s::GRAY2, s::TEXT_DEFAULT, Some(message), None),
        Status::Warning(message) => (s::YELLOW2, s::YELLOW6, Some(message), None),
        Status::Error { message, .. } => (s::RED1, s::WHITE, Some(message.clone()), Some(message)),
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
        .gap(s::S4)
        .children(message.map(|message| div().flex_1().min_w(s::S0).truncate().child(message)))
        .children(error_to_copy.map(copy_error_button))
}

fn copy_error_button(message: SharedString) -> gpui::Div {
    s::raised(
        div()
            .id("copy-status-error")
            .debug_selector(|| "copy-status-error".to_string())
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(s::S5)
            .px(s::S4)
            .bg(s::GRAY2)
            .text_color(s::BUTTON_TEXT)
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.text_color(s::TEXT_HOVERED))
            .active(|style| style.bg(s::GRAY1))
            .child("copy")
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                cx.write_to_clipboard(ClipboardItem::new_string(message.to_string()));
            }),
    )
}

#[cfg(test)]
mod tests {
    use gpui::{prelude::*, Context, MouseButton, MouseDownEvent, TestAppContext, Window};

    use super::{bar, Status};

    struct StatusBarHost {
        status: Status,
        background_clicked: bool,
    }

    impl Render for StatusBarHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            bar(self.status.clone()).on_mouse_down(
                MouseButton::Left,
                cx.listener(|host, _: &MouseDownEvent, _, _| {
                    host.background_clicked = true;
                }),
            )
        }
    }

    #[gpui::test]
    fn error_statuses_can_be_copied_to_the_clipboard(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(|_, _| StatusBarHost {
            status: Status::Error {
                message: "couldn't save the score".into(),
                target: None,
            },
            background_clicked: false,
        });
        cx.run_until_parked();

        let copy_button = cx.debug_bounds("copy-status-error").unwrap();
        cx.simulate_click(copy_button.center(), Default::default());

        let copied = cx
            .update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
            .unwrap();
        assert_eq!(copied, "couldn't save the score");
        assert!(!cx.update(|_, cx| host.read(cx).background_clicked));
    }

    #[gpui::test]
    fn non_error_statuses_do_not_show_the_copy_button(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| StatusBarHost {
            status: Status::Message("score changes saved".into()),
            background_clicked: false,
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("copy-status-error").is_none());
    }
}
