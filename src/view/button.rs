use gpui::{prelude::*, ClickEvent, Context, CursorStyle, ElementId, SharedString, Window};

use crate::style as s;

pub struct Button {
    id: ElementId,
    label: SharedString,
    size: Size,
    depressed: bool,
}

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
        }
    }

    #[allow(dead_code)]
    pub fn x(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: "X".into(),
            size: Size::Square,
            depressed: false,
        }
    }

    pub fn set_depressed(&mut self, depressed: bool, cx: &mut Context<Self>) {
        if self.depressed == depressed {
            return;
        }

        self.depressed = depressed;
        cx.notify();
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.set_depressed(true, cx);
    }
}

impl Render for Button {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let button = gpui::div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .justify_center()
            .bg(s::GRAY2)
            .text_color(s::GRAY6)
            .cursor(CursorStyle::PointingHand)
            .child(self.label.clone())
            .on_click(cx.listener(Self::on_click));

        let button = match self.size {
            Size::Text => button.p(s::S3).px(s::S4),
            Size::Square => button.size(s::S6),
        };

        if self.depressed {
            s::sunken(button)
        } else {
            s::raised(button)
        }
    }
}
