use gpui::{
    prelude::*, Context, CursorStyle, ElementId, EventEmitter, MouseButton, MouseDownEvent,
    MouseUpEvent, Pixels, SharedString, Window,
};

use crate::style as s;

pub fn action_group<Action>(actions: impl IntoIterator<Item = Action>) -> gpui::Div
where
    Action: IntoElement,
{
    gpui::div().flex().gap(s::S5).children(actions)
}

pub fn labeled_action_group<Action>(
    label: impl Into<SharedString>,
    actions: impl IntoIterator<Item = Action>,
) -> gpui::Div
where
    Action: IntoElement,
{
    gpui::div()
        .flex()
        .items_center()
        .gap(s::S3)
        .child(gpui::div().text_color(s::TEXT_HEADER).child(label.into()))
        .child(action_group(actions))
}

pub struct Button {
    id: ElementId,
    label: SharedString,
    trailing_label: Option<SharedString>,
    max_width: Option<Pixels>,
    size: Size,
    variant: ButtonVariant,
    disabled: bool,
    depressed: bool,
    pressing: bool,
    hovered: bool,
    hover_suppressed_until_exit: bool,
}

pub struct Clicked;

impl EventEmitter<Clicked> for Button {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ButtonVariant {
    #[default]
    Default,
    Primary,
    Danger,
}

enum Size {
    Text,
    Square,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            trailing_label: None,
            max_width: None,
            size: Size::Text,
            variant: ButtonVariant::Default,
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

    pub fn trailing_label(mut self, label: impl Into<SharedString>) -> Self {
        self.trailing_label = Some(label.into());
        self
    }

    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn square(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            trailing_label: None,
            max_width: None,
            size: Size::Square,
            variant: ButtonVariant::Default,
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

    pub fn set_variant(&mut self, variant: ButtonVariant, cx: &mut Context<Self>) {
        if self.variant == variant {
            return;
        }

        self.variant = variant;
        cx.notify();
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
        let (background, light_border, dark_border) = match self.variant {
            ButtonVariant::Default => (s::GRAY2, s::GRAY3, s::GRAY1),
            ButtonVariant::Primary => (s::YELLOW3, s::YELLOW5, s::YELLOW1),
            ButtonVariant::Danger => (s::RED1, s::RED2, s::RED1),
        };
        let background = if self.disabled { s::GRAY3 } else { background };
        let cursor = if self.disabled {
            CursorStyle::Arrow
        } else {
            CursorStyle::PointingHand
        };

        let label = gpui::div()
            .flex()
            .items_center()
            .min_w(s::S0)
            .max_w_full()
            .text_color(text_color)
            .child(
                gpui::div()
                    .min_w(s::S0)
                    .truncate()
                    .child(self.label.clone()),
            )
            .children(self.trailing_label.clone().map(|label| {
                gpui::div()
                    .flex_none()
                    .ml(s::S3)
                    .text_color(text_color)
                    .child(label)
            }));
        let button = gpui::div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .justify_center()
            .min_w(s::S0)
            .bg(background)
            .cursor(cursor)
            .child(label)
            .on_hover(cx.listener(Self::on_hover))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up_out));

        let button = match self.size {
            Size::Text => button.p(s::S3).px(s::S4),
            Size::Square => button.size(s::S6),
        };
        let button = button.when_some(self.max_width, |button, width| {
            button.max_w(width).overflow_hidden()
        });

        if self.disabled {
            s::raised(button)
        } else if self.depressed || self.pressing {
            s::sunken_with_border(button, light_border, dark_border)
        } else {
            s::raised_with_border(button, light_border, dark_border)
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{prelude::*, Context, Entity, TestAppContext, Window};

    use super::{action_group, Button, ButtonVariant};
    use crate::style as s;

    struct ActionGroupHost {
        first: Entity<Button>,
        second: Entity<Button>,
    }

    impl Render for ActionGroupHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            action_group([
                gpui::div()
                    .debug_selector(|| "first-action-control".to_string())
                    .child(self.first.clone()),
                gpui::div()
                    .debug_selector(|| "second-action-control".to_string())
                    .child(self.second.clone()),
            ])
        }
    }

    #[gpui::test]
    fn action_groups_separate_buttons_with_s5(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, cx| ActionGroupHost {
            first: cx.new(|_| Button::new("first-action", "first")),
            second: cx.new(|_| Button::new("second-action", "second")),
        });
        cx.run_until_parked();

        let first = cx.debug_bounds("first-action-control").unwrap();
        let second = cx.debug_bounds("second-action-control").unwrap();

        assert_eq!(second.left() - first.right(), s::S5);
    }

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

    #[gpui::test]
    fn button_variants_can_be_set_at_creation_and_runtime(cx: &mut TestAppContext) {
        let (button, cx) = cx.add_window_view(|_, _| {
            Button::new("variant-button", "play").variant(ButtonVariant::Primary)
        });

        assert_eq!(
            cx.update(|_, cx| button.read(cx).variant),
            ButtonVariant::Primary
        );

        button.update(cx, |button, cx| {
            button.set_label("stop", cx);
            button.set_variant(ButtonVariant::Danger, cx);
        });

        assert_eq!(
            cx.update(|_, cx| button.read(cx).variant),
            ButtonVariant::Danger
        );
    }
}
