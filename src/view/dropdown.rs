use gpui::{
    canvas, deferred, prelude::*, Bounds, ClickEvent, Context, ElementId, Entity, EventEmitter,
    MouseButton, MouseUpEvent, Pixels, SharedString, Window,
};

use crate::{
    style as s,
    view::{
        button::{self, Button},
        selection_list,
    },
};

pub struct Dropdown {
    id: ElementId,
    options: Vec<SharedString>,
    selected_index: usize,
    expanded: bool,
    trigger: Entity<Button>,
    trigger_bounds: Option<Bounds<Pixels>>,
}

pub struct Selected {
    pub index: usize,
}

impl EventEmitter<Selected> for Dropdown {}

impl Dropdown {
    pub fn new(
        id: impl Into<ElementId>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        selected_index: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let id = id.into();
        let options = options.into_iter().map(Into::into).collect::<Vec<_>>();
        assert!(
            !options.is_empty(),
            "a dropdown requires at least one option"
        );
        assert!(
            selected_index < options.len(),
            "the selected dropdown index must identify an option"
        );

        let trigger = cx.new({
            let trigger_id = (id.clone(), "trigger");
            let label = trigger_label(&options[selected_index]);
            move |_| Button::new(trigger_id, label)
        });
        cx.subscribe(&trigger, Self::on_trigger_clicked).detach();

        Self {
            id,
            options,
            selected_index,
            expanded: false,
            trigger,
            trigger_bounds: None,
        }
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn set_options(
        &mut self,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        selected_index: usize,
        cx: &mut Context<Self>,
    ) {
        let options = options.into_iter().map(Into::into).collect::<Vec<_>>();
        assert!(
            !options.is_empty(),
            "a dropdown requires at least one option"
        );
        assert!(
            selected_index < options.len(),
            "the selected dropdown index must identify an option"
        );

        self.options = options;
        self.selected_index = selected_index;
        self.set_expanded(false, cx);
        self.sync_trigger(cx);
        cx.notify();
    }

    pub fn set_selected_index(&mut self, index: usize, cx: &mut Context<Self>) {
        assert!(
            index < self.options.len(),
            "the selected dropdown index must identify an option"
        );
        if self.selected_index == index {
            return;
        }

        self.selected_index = index;
        self.sync_trigger(cx);
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

    fn on_option_clicked(
        &mut self,
        index: usize,
        _: &ClickEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.selected_index != index;
        self.selected_index = index;
        self.set_expanded(false, cx);

        if changed {
            self.sync_trigger(cx);
            cx.emit(Selected { index });
            cx.notify();
        }
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

    fn sync_trigger(&self, cx: &mut Context<Self>) {
        let label = trigger_label(&self.options[self.selected_index]);
        self.trigger.update(cx, |button, cx| {
            button.set_label(label, cx);
        });
    }

    fn menu(&self, cx: &mut Context<Self>) -> gpui::Div {
        let rows = self
            .options
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let option_id = (self.id.clone(), format!("option-{index}"));
                let debug_id = format!("{}-option-{index}", self.id);
                selection_list::row(index, index == self.selected_index, label.clone())
                    .id(option_id)
                    .hover(|style| style.bg(s::GREEN5).text_color(s::TEXT_HOVERED))
                    .debug_selector(move || debug_id.clone())
                    .on_click(cx.listener(move |dropdown, event, window, cx| {
                        dropdown.on_option_clicked(index, event, window, cx);
                    }))
            })
            .collect::<Vec<_>>();

        gpui::div()
            .flex()
            .flex_col()
            .bg(s::GREEN3)
            .border_2()
            .border_color(s::GRAY1)
            .children(rows)
            .absolute()
            .top(trigger_height())
            .left_0()
            .right_0()
            .overflow_hidden()
            .occlude()
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_menu_mouse_up_out))
    }
}

impl Render for Dropdown {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let debug_id = format!("{}-trigger", self.id);
        let dropdown = cx.entity();
        let bounds_recorder = canvas(
            move |bounds, _, cx| {
                dropdown.update(cx, |dropdown, _| {
                    dropdown.trigger_bounds = Some(bounds);
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
                    .then(|| deferred(self.menu(cx)).with_priority(1)),
            )
    }
}

fn trigger_label(option: &SharedString) -> SharedString {
    format!("{option} ▾").into()
}

fn trigger_height() -> gpui::Pixels {
    s::TEXT_LINE_HEIGHT + s::S3 * 2.0
}

#[cfg(test)]
mod tests {
    use gpui::{point, px, Modifiers, TestAppContext};

    use super::Dropdown;

    #[gpui::test]
    fn selecting_an_option_updates_the_trigger_and_closes_the_menu(cx: &mut TestAppContext) {
        let (dropdown, cx) = cx.add_window_view(|_, cx| {
            Dropdown::new("test-dropdown", ["one", "two", "three"], 0, cx)
        });
        cx.run_until_parked();

        let trigger = cx.debug_bounds("test-dropdown-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        let option = cx.debug_bounds("test-dropdown-option-2").unwrap();

        cx.simulate_click(option.center(), Modifiers::default());

        let (selected_index, expanded) = cx.update(|_, cx| {
            (
                dropdown.read(cx).selected_index(),
                dropdown.read(cx).expanded,
            )
        });
        assert_eq!(selected_index, 2);
        assert!(!expanded);
    }

    #[gpui::test]
    fn clicking_outside_closes_the_menu(cx: &mut TestAppContext) {
        let (dropdown, cx) =
            cx.add_window_view(|_, cx| Dropdown::new("outside-dropdown", ["one", "two"], 0, cx));
        let trigger = cx.debug_bounds("outside-dropdown-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        assert!(cx.update(|_, cx| dropdown.read(cx).expanded));

        cx.simulate_click(
            point(trigger.center().x, trigger.bottom() + px(200.0)),
            Modifiers::default(),
        );

        assert!(!cx.update(|_, cx| dropdown.read(cx).expanded));
    }

    #[gpui::test]
    fn clicking_the_trigger_again_closes_the_menu(cx: &mut TestAppContext) {
        let (dropdown, cx) =
            cx.add_window_view(|_, cx| Dropdown::new("toggle-dropdown", ["one", "two"], 0, cx));
        let trigger = cx.debug_bounds("toggle-dropdown-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        assert!(cx.update(|_, cx| dropdown.read(cx).expanded));

        cx.simulate_click(trigger.center(), Modifiers::default());

        assert!(!cx.update(|_, cx| dropdown.read(cx).expanded));
    }

    #[gpui::test]
    fn replacing_options_updates_the_trigger_and_selection(cx: &mut TestAppContext) {
        let (dropdown, cx) =
            cx.add_window_view(|_, cx| Dropdown::new("replace-dropdown", ["one", "two"], 0, cx));

        dropdown.update(cx, |dropdown, cx| {
            dropdown.set_options(["alpha", "beta", "gamma"], 2, cx);
        });
        cx.run_until_parked();

        assert_eq!(cx.update(|_, cx| dropdown.read(cx).selected_index()), 2);
        let trigger = cx.debug_bounds("replace-dropdown-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        assert!(cx.debug_bounds("replace-dropdown-option-2").is_some());
    }
}
