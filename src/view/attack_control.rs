//! A bounded attack-sharpness slider shared by voice and note editors.
use crate::{style as s, voice::AttackSharpness};
use gpui::{
    canvas, div, prelude::*, Bounds, Context, EventEmitter, FocusHandle, Focusable, MouseButton,
    Pixels, Window,
};

pub struct Changed(pub AttackSharpness);
pub struct AttackControl {
    value: AttackSharpness,
    bounds: Option<Bounds<Pixels>>,
    focus: FocusHandle,
}
impl EventEmitter<Changed> for AttackControl {}
impl Focusable for AttackControl {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}
impl AttackControl {
    pub fn new(value: AttackSharpness, cx: &mut Context<Self>) -> Self {
        Self {
            value,
            bounds: None,
            focus: cx.focus_handle(),
        }
    }
    pub fn value(&self) -> AttackSharpness {
        self.value
    }
    pub fn sync(&mut self, value: AttackSharpness, cx: &mut Context<Self>) {
        self.value = value;
        cx.notify();
    }
    fn change(&mut self, percent: u8, cx: &mut Context<Self>) {
        self.value = AttackSharpness::new(percent.min(100)).unwrap();
        cx.emit(Changed(self.value));
        cx.notify();
    }
    fn point(&mut self, x: Pixels, cx: &mut Context<Self>) {
        if let Some(bounds) = self.bounds {
            if bounds.size.width > s::S5 {
                self.change(
                    (((x - bounds.left() - s::S4) / (bounds.size.width - s::S5)).clamp(0.0, 1.0)
                        * 100.0)
                        .round() as u8,
                    cx,
                );
            }
        }
    }
}
impl Render for AttackControl {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let recorder = canvas(
            move |bounds, _, cx| {
                entity.update(cx, |this, _| this.bounds = Some(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        div()
            .flex()
            .flex_col()
            .gap(s::S3)
            .w(s::S9)
            .max_w_full()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child("natural")
                    .child(format!("{}%", self.value.percent()))
                    .child("immediate"),
            )
            .child(
                div()
                    .relative()
                    .h(s::S6)
                    .w_full()
                    .id("attack-sharpness-slider")
                    .debug_selector(|| "attack-sharpness-slider".into())
                    .track_focus(&self.focus)
                    .cursor_pointer()
                    .hover(|style| style.bg(s::GRAY3))
                    .when(self.focus.is_focused(window), |style| {
                        style.border(s::BORDER_WIDTH).border_color(s::TEXT_DEFAULT)
                    })
                    .child(
                        div()
                            .absolute()
                            .left(s::S4)
                            .right(s::S4)
                            .top_0()
                            .bottom_0()
                            .child(
                                s::sunken(
                                    div().h(s::S4).w_full().bg(s::GRAY1).child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(
                                                f32::from(self.value.percent()) / 100.0,
                                            ))
                                            .bg(s::GREEN5),
                                    ),
                                )
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(s::S4 + s::S3),
                            )
                            .child(
                                s::raised(
                                    div()
                                        .w(s::S5)
                                        .h(s::S6)
                                        .bg(s::GRAY4)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(div().w(s::BORDER_WIDTH).h(s::S5).bg(s::GRAY1)),
                                )
                                .absolute()
                                .top_0()
                                .left(gpui::relative(f32::from(self.value.percent()) / 100.0))
                                .ml(-s::S4)
                                .debug_selector(|| "attack-sharpness-handle".into()),
                            ),
                    )
                    .child(recorder)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                            this.focus.focus(window);
                            this.point(event.position.x, cx);
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(MouseButton::Left) {
                            this.point(event.position.x, cx);
                        }
                    }))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                        let value = this.value.percent();
                        let next = match event.keystroke.key.as_str() {
                            "left" | "down" => value.saturating_sub(1),
                            "right" | "up" => value.saturating_add(1).min(100),
                            "home" => 0,
                            "end" => 100,
                            _ => return,
                        };
                        this.change(next, cx);
                        cx.stop_propagation();
                    })),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    fn slider_mouse_and_keyboard_stay_bounded(cx: &mut gpui::TestAppContext) {
        let (control, cx) = cx.add_window_view(|_, cx| AttackControl::new(Default::default(), cx));
        cx.run_until_parked();
        let bounds = cx.debug_bounds("attack-sharpness-slider").unwrap();
        assert_eq!(bounds.size.height, s::S6);
        let handle = cx.debug_bounds("attack-sharpness-handle").unwrap();
        assert_eq!(handle.size.height, s::S6);
        assert_eq!(handle.left(), bounds.left());
        cx.simulate_mouse_down(
            bounds.center(),
            MouseButton::Left,
            gpui::Modifiers::default(),
        );
        assert!((45..=55).contains(&control.read_with(cx, |c, _| c.value().percent())));
        cx.simulate_keystrokes("end");
        assert_eq!(control.read_with(cx, |c, _| c.value().percent()), 100);
        cx.run_until_parked();
        let handle = cx.debug_bounds("attack-sharpness-handle").unwrap();
        assert!(handle.right() <= bounds.right());
        assert!(handle.right() >= bounds.right() - s::S3);
        cx.simulate_keystrokes("right");
        assert_eq!(control.read_with(cx, |c, _| c.value().percent()), 100);
        cx.simulate_keystrokes("home");
        assert_eq!(control.read_with(cx, |c, _| c.value().percent()), 0);
    }
}
