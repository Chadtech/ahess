use gpui::{
    prelude::*, Context, CursorStyle, ElementId, EventEmitter, MouseButton, MouseDownEvent,
    MouseUpEvent, SharedString, Window,
};

use crate::style as s;

pub struct Button {
    id: ElementId,
    label: SharedString,
    size: Size,
    depressed: bool,
    pressing: bool,
    hovered: bool,
    hover_suppressed_until_exit: bool,
}

pub struct Clicked;

impl EventEmitter<Clicked> for Button {}

enum Size {
    Text,
    #[allow(dead_code)]
    Square,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            size: Size::Text,
            depressed: false,
            pressing: false,
            hovered: false,
            hover_suppressed_until_exit: false,
        }
    }

    pub fn depressed(mut self, depressed: bool) -> Self {
        self.depressed = depressed;
        self
    }

    #[allow(dead_code)]
    pub fn x(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "X".into(),
            size: Size::Square,
            depressed: false,
            pressing: false,
            hovered: false,
            hover_suppressed_until_exit: false,
        }
    }

    pub fn set_depressed(&mut self, depressed: bool, cx: &mut Context<Self>) {
        if self.depressed == depressed {
            return;
        }

        self.depressed = depressed;
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
        if self.pressing == pressing {
            return;
        }

        self.pressing = pressing;
        cx.notify();
    }

    fn on_hover(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
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
        self.set_pressing(true, cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
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
        let text_color = if self.hovered {
            s::TEXT_HOVERED
        } else {
            s::BUTTON_TEXT
        };

        let button = gpui::div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .justify_center()
            .bg(s::GRAY2)
            .cursor(CursorStyle::PointingHand)
            .child(gpui::div().text_color(text_color).child(self.label.clone()))
            .on_hover(cx.listener(Self::on_hover))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up_out));

        let button = match self.size {
            Size::Text => button.p(s::S3).px(s::S4),
            Size::Square => button.size(s::S6),
        };

        if self.depressed || self.pressing {
            s::sunken(button)
        } else {
            s::raised(button)
        }
    }
}
