use gpui::{
    prelude::*, Context, CursorStyle, ElementId, EventEmitter, MouseButton, MouseDownEvent,
    MouseUpEvent, SharedString, Window,
};

use crate::style as s;

pub struct Button {
    id: ElementId,
    label: SharedString,
    size: Size,
    disabled: bool,
    depressed: bool,
    pressing: bool,
    hovered: bool,
    hover_suppressed_until_exit: bool,
}

pub struct Clicked;

impl EventEmitter<Clicked> for Button {}

enum Size {
    Text,
    Square,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            size: Size::Text,
            disabled: false,
            depressed: false,
            pressing: false,
            hovered: false,
            hover_suppressed_until_exit: false,
        }
    }

    pub fn depressed(mut self, depressed: bool) -> Self {
        self.depressed = depressed && !self.disabled;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        if disabled {
            self.depressed = false;
            self.pressing = false;
            self.hovered = false;
            self.hover_suppressed_until_exit = false;
        }
        self
    }

    pub fn square(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            size: Size::Square,
            disabled: false,
            depressed: false,
            pressing: false,
            hovered: false,
            hover_suppressed_until_exit: false,
        }
    }

    pub fn x(id: impl Into<ElementId>) -> Self {
        Self::square(id, "X")
    }

    pub fn set_depressed(&mut self, depressed: bool, cx: &mut Context<Self>) {
        if self.disabled && depressed {
            return;
        }
        if self.depressed == depressed {
            return;
        }

        self.depressed = depressed;
        cx.notify();
    }

    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        let state_changed = self.disabled != disabled;
        let interaction_changed = disabled && (self.depressed || self.pressing || self.hovered);
        if !state_changed && !interaction_changed {
            return;
        }

        self.disabled = disabled;
        if disabled {
            self.depressed = false;
            self.pressing = false;
            self.hovered = false;
            self.hover_suppressed_until_exit = false;
        }
        cx.notify();
    }

    #[cfg(test)]
    pub(crate) fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn set_label(&mut self, label: impl Into<SharedString>, cx: &mut Context<Self>) {
        let label = label.into();
        if self.label == label {
            return;
        }

        self.label = label;
        cx.notify();
    }

    pub fn suppress_hover_until_pointer_exit(&mut self, cx: &mut Context<Self>) {
        let changed = self.hovered || !self.hover_suppressed_until_exit;
        self.hovered = false;
        self.hover_suppressed_until_exit = true;

        if changed {
            cx.notify();
        }
    }

    fn set_pressing(&mut self, pressing: bool, cx: &mut Context<Self>) {
        if self.disabled && pressing {
            return;
        }
        if self.pressing == pressing {
            return;
        }

        self.pressing = pressing;
        cx.notify();
    }

    fn on_hover(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if *hovered && self.hover_suppressed_until_exit {
            return;
        }

        let hover_suppression_changed = !*hovered && self.hover_suppressed_until_exit;
        let hovered_changed = self.hovered != *hovered;
        self.hover_suppressed_until_exit &= *hovered;
        self.hovered = *hovered;

        if hover_suppression_changed || hovered_changed {
            cx.notify();
        }
    }

    fn on_mouse_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.set_pressing(true, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            self.set_pressing(false, cx);
            return;
        }
        let was_pressing = self.pressing;
        self.set_pressing(false, cx);

        if was_pressing {
            cx.emit(Clicked);
        }
    }

    fn on_mouse_up_out(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_pressing(false, cx);
    }
}

impl Render for Button {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text_color = if self.hovered && !self.disabled {
            s::TEXT_HOVERED
        } else {
            s::BUTTON_TEXT
        };
        let background = if self.disabled { s::GRAY3 } else { s::GRAY2 };
        let cursor = if self.disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        };

        let button = gpui::div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .justify_center()
            .bg(background)
            .cursor(cursor)
            .child(gpui::div().text_color(text_color).child(self.label.clone()))
            .on_hover(cx.listener(Self::on_hover))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up_out));

        let button = match self.size {
            Size::Text => button.p(s::S3).px(s::S4),
            Size::Square => button.size(s::S6),
        };

        if !self.disabled && (self.depressed || self.pressing) {
            s::sunken(button)
        } else {
            s::raised(button)
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::Button;

    #[gpui::test]
    fn disabled_buttons_ignore_hover_press_and_depressed_states(cx: &mut TestAppContext) {
        let (button, cx) =
            cx.add_window_view(|_, _| Button::new("disabled-button", "disabled").disabled(true));
        cx.update(|window, cx| {
            button.update(cx, |button, cx| {
                button.on_hover(&true, window, cx);
                button.set_pressing(true, cx);
                button.set_depressed(true, cx);
            });
        });

        let state = cx.update(|_, cx| {
            let button = button.read(cx);
            (
                button.disabled,
                button.hovered,
                button.pressing,
                button.depressed,
            )
        });
        assert_eq!(state, (true, false, false, false));
    }

    #[gpui::test]
    fn buttons_can_be_reenabled(cx: &mut TestAppContext) {
        let (button, cx) =
            cx.add_window_view(|_, _| Button::new("reenabled-button", "enabled").disabled(true));

        button.update(cx, |button, cx| {
            button.set_disabled(false, cx);
            button.set_pressing(true, cx);
        });

        assert!(cx.update(|_, cx| button.read(cx).pressing));
    }
}
