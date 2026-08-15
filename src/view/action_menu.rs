use gpui::{
    canvas, deferred, font, prelude::*, Bounds, ClickEvent, Context, CursorStyle, ElementId,
    Entity, EventEmitter, MouseButton, MouseUpEvent, Pixels, SharedString, TextRun, Window,
};

use crate::{
    style as s,
    view::{
        button::{self, Button},
        selection_list,
    },
};

#[derive(Clone)]
struct Action {
    label: SharedString,
    disabled: bool,
}

pub struct ActionMenu {
    id: ElementId,
    actions: Vec<Action>,
    expanded: bool,
    direction: MenuDirection,
    trigger: Entity<Button>,
    trigger_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MenuDirection {
    Down,
    Up,
}

pub struct Selected {
    pub index: usize,
}

impl EventEmitter<Selected> for ActionMenu {}

impl ActionMenu {
    pub fn new(
        id: impl Into<ElementId>,
        trigger_label: impl Into<SharedString>,
        actions: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_direction(id, trigger_label, actions, MenuDirection::Down, cx)
    }

    pub fn new_upward(
        id: impl Into<ElementId>,
        trigger_label: impl Into<SharedString>,
        actions: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new_with_direction(id, trigger_label, actions, MenuDirection::Up, cx)
    }

    fn new_with_direction(
        id: impl Into<ElementId>,
        trigger_label: impl Into<SharedString>,
        actions: impl IntoIterator<Item = impl Into<SharedString>>,
        direction: MenuDirection,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = id.into();
        let actions = actions
            .into_iter()
            .map(|label| Action {
                label: label.into(),
                disabled: false,
            })
            .collect::<Vec<_>>();
        assert!(
            !actions.is_empty(),
            "an action menu requires at least one action"
        );

        let trigger = cx.new({
            let trigger_id = (id.clone(), "trigger");
            let label = trigger_label.into();
            let direction_label = match direction {
                MenuDirection::Down => "▾",
                MenuDirection::Up => "▴",
            };
            move |_| Button::new(trigger_id, label).trailing_label(direction_label)
        });
        cx.subscribe(&trigger, Self::on_trigger_clicked).detach();

        Self {
            id,
            actions,
            expanded: false,
            direction,
            trigger,
            trigger_bounds: None,
        }
    }

    pub fn set_disabled(&mut self, index: usize, disabled: bool, cx: &mut Context<Self>) {
        let action = self
            .actions
            .get_mut(index)
            .expect("the action index must identify a menu item");
        if action.disabled == disabled {
            return;
        }

        action.disabled = disabled;
        cx.notify();
    }

    pub fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.actions.get(index).is_none_or(|action| action.disabled) {
            return;
        }

        self.set_expanded(false, cx);
        cx.emit(Selected { index });
        cx.notify();
    }

    fn on_trigger_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.set_expanded(!self.expanded, cx);
    }

    fn on_action_clicked(
        &mut self,
        index: usize,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate(index, cx);
    }

    fn on_menu_mouse_up_out(
        &mut self,
        event: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .trigger_bounds
            .is_some_and(|bounds| bounds.contains(&event.position))
        {
            return;
        }

        self.set_expanded(false, cx);
    }

    fn set_expanded(&mut self, expanded: bool, cx: &mut Context<Self>) {
        if self.expanded == expanded {
            return;
        }

        self.expanded = expanded;
        self.trigger.update(cx, |button, cx| {
            button.set_depressed(expanded, cx);
        });
        cx.notify();
    }

    fn menu(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu_debug_id = format!("{}-menu", self.id);
        let menu_width = self.menu_width(window);
        let rows = self
            .actions
            .iter()
            .enumerate()
            .map(|(index, action)| {
                let action_id = (self.id.clone(), format!("action-{index}"));
                let debug_id = format!("{}-action-{index}", self.id);
                let disabled = action.disabled;
                selection_list::row(index, false, action.label.clone())
                    .id(action_id)
                    .when(disabled, |row| {
                        row.bg(s::GRAY3)
                            .text_color(s::TEXT_HEADER)
                            .cursor(CursorStyle::Arrow)
                    })
                    .when(!disabled, |row| {
                        row.hover(|style| style.bg(s::GREEN5).text_color(s::TEXT_HOVERED))
                            .on_click(cx.listener(move |menu, event, window, cx| {
                                menu.on_action_clicked(index, event, window, cx);
                            }))
                    })
                    .debug_selector(move || debug_id.clone())
            })
            .collect::<Vec<_>>();

        let menu = gpui::div()
            .id((self.id.clone(), "menu"))
            .flex()
            .flex_col()
            .bg(s::GREEN3)
            .border_2()
            .border_color(s::GRAY1)
            .children(rows)
            .absolute()
            .right_0()
            .w(menu_width)
            .min_w_full()
            .max_h(s::S9)
            .whitespace_nowrap()
            .overflow_y_scroll()
            .occlude()
            .debug_selector(move || menu_debug_id.clone())
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_menu_mouse_up_out));
        match self.direction {
            MenuDirection::Down => menu.top(trigger_height()),
            MenuDirection::Up => menu.bottom(trigger_height()),
        }
    }

    fn menu_width(&self, window: &Window) -> Pixels {
        let widest_label = self.actions.iter().fold(s::S0, |widest, action| {
            let run = TextRun {
                len: action.label.len(),
                font: font(s::FONT),
                color: s::TEXT_DEFAULT.into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let width = window
                .text_system()
                .layout_line(&action.label, s::TEXT_SIZE, &[run], None)
                .width;
            if width > widest {
                width
            } else {
                widest
            }
        });

        widest_label + s::S4 * 2.0 + s::S2 * 2.0
    }

    #[cfg(test)]
    pub(crate) fn is_disabled(&self, index: usize) -> bool {
        self.actions[index].disabled
    }
}

impl Render for ActionMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let debug_id = format!("{}-trigger", self.id);
        let menu = cx.entity();
        let bounds_recorder = canvas(
            move |bounds, _, cx| {
                menu.update(cx, |menu, _| {
                    menu.trigger_bounds = Some(bounds);
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();

        gpui::div()
            .relative()
            .h(trigger_height())
            .text_size(s::TEXT_SIZE)
            .line_height(s::TEXT_LINE_HEIGHT)
            .child(
                gpui::div()
                    .relative()
                    .debug_selector(move || debug_id.clone())
                    .child(self.trigger.clone())
                    .child(bounds_recorder),
            )
            .children(
                self.expanded
                    .then(|| deferred(self.menu(window, cx)).with_priority(1)),
            )
    }
}

fn trigger_height() -> Pixels {
    s::TEXT_LINE_HEIGHT + s::S3 * 2.0
}

#[cfg(test)]
mod tests {
    use gpui::{Modifiers, TestAppContext};

    use super::ActionMenu;

    #[gpui::test]
    fn disabled_actions_stay_visible_and_do_not_close_the_menu(cx: &mut TestAppContext) {
        let (menu, cx) = cx.add_window_view(|_, cx| {
            ActionMenu::new("test-actions", "actions", ["always", "sometimes"], cx)
        });
        menu.update(cx, |menu, cx| menu.set_disabled(1, true, cx));

        let trigger = cx.debug_bounds("test-actions-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        let disabled = cx.debug_bounds("test-actions-action-1").unwrap();
        cx.simulate_click(disabled.center(), Modifiers::default());

        assert!(cx.update(|_, cx| menu.read(cx).expanded));
        assert!(cx.debug_bounds("test-actions-action-0").is_some());
        assert!(cx.debug_bounds("test-actions-action-1").is_some());
    }

    #[gpui::test]
    fn selecting_an_enabled_action_closes_the_menu(cx: &mut TestAppContext) {
        let (menu, cx) = cx.add_window_view(|_, cx| {
            ActionMenu::new("enabled-actions", "actions", ["one", "two"], cx)
        });

        let trigger = cx.debug_bounds("enabled-actions-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        let action = cx.debug_bounds("enabled-actions-action-1").unwrap();
        cx.simulate_click(action.center(), Modifiers::default());

        assert!(!cx.update(|_, cx| menu.read(cx).expanded));
    }
}
