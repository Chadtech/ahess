use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::{
    playback::BeatRange,
    style as s,
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
        field_group::field_group,
        text_input::TextInput,
    },
};

pub enum Msg {
    Applied(BeatRange),
    Closed,
}

pub struct LoopRangeDialog {
    arrangement_beat_count: u64,
    from_beat: Entity<TextInput>,
    to_beat: Entity<TextInput>,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    apply_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<Msg> for LoopRangeDialog {}

impl LoopRangeDialog {
    pub fn new(
        arrangement_beat_count: u64,
        range: Option<BeatRange>,
        cx: &mut Context<Self>,
    ) -> Self {
        let first = range.map(BeatRange::first).unwrap_or(1);
        let last = range
            .map(BeatRange::last)
            .unwrap_or(arrangement_beat_count.max(1));
        let from_beat = cx.new(|cx| TextInput::new(first.to_string(), "", cx));
        let to_beat = cx.new(|cx| TextInput::new(last.to_string(), "", cx));
        let close_button = cx.new(|_| Button::x("close-loop-range"));
        let cancel_button = cx.new(|_| Button::new("cancel-loop-range", "cancel"));
        let apply_button = cx.new(|_| {
            Button::new("apply-loop-range", "apply").disabled(arrangement_beat_count == 0)
        });

        cx.subscribe(&close_button, Self::on_close_clicked).detach();
        cx.subscribe(&cancel_button, Self::on_close_clicked)
            .detach();
        cx.subscribe(&apply_button, Self::on_apply_clicked).detach();

        Self {
            arrangement_beat_count,
            from_beat,
            to_beat,
            close_button,
            cancel_button,
            apply_button,
            error: None,
        }
    }

    fn on_close_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        cx.emit(Msg::Closed);
    }

    fn on_apply_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let range = parse_beat(&self.from_beat.read(cx).value(), "from beat")
            .and_then(|first| {
                parse_beat(&self.to_beat.read(cx).value(), "to beat").map(|last| (first, last))
            })
            .and_then(|(first, last)| {
                BeatRange::new(first, last, self.arrangement_beat_count)
                    .map_err(|error| error.to_string())
            });

        match range {
            Ok(range) => cx.emit(Msg::Applied(range)),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for LoopRangeDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let beat_label = if self.arrangement_beat_count == 1 {
            "beat"
        } else {
            "beats"
        };
        let mut form = div()
            .flex()
            .flex_col()
            .gap(s::CONTENT_PADDING)
            .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                "the arrangement contains {} {beat_label}; both ends of the loop are included",
                self.arrangement_beat_count
            )))
            .child(
                div()
                    .flex()
                    .gap(s::S4)
                    .debug_selector(|| "loop-range-fields".to_string())
                    .child(field_group("from beat", self.from_beat.clone()))
                    .child(field_group("to beat", self.to_beat.clone())),
            );
        if self.arrangement_beat_count == 0 {
            form = form.child(error_message(
                "add at least one part to the arrangement before setting a loop",
            ));
        } else if let Some(error) = &self.error {
            form = form.child(error_message(error.clone()));
        }

        let actions = div()
            .flex()
            .justify_end()
            .gap(s::S3)
            .debug_selector(|| "loop-range-actions".to_string())
            .child(self.cancel_button.clone())
            .child(self.apply_button.clone());

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .debug_selector(|| "loop-range-dialog".to_string())
                .child(title_bar("loop range", Some(self.close_button.clone())))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(s::CONTENT_PADDING)
                        .p(s::CONTENT_PADDING)
                        .child(form)
                        .child(actions),
                ),
        )
    }
}

fn parse_beat(value: &str, label: &str) -> Result<u64, String> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("{label} must be a whole number"))
}

#[cfg(test)]
mod tests {
    use gpui::{px, size, TestAppContext};

    use super::LoopRangeDialog;
    use crate::playback::BeatRange;

    #[gpui::test]
    fn loop_range_dialog_fits_its_fields_and_actions(cx: &mut TestAppContext) {
        let range = BeatRange::new(100, 120, 240).unwrap();
        let (_dialog, cx) = cx.add_window_view(|_, cx| LoopRangeDialog::new(240, Some(range), cx));
        cx.simulate_resize(size(px(640.0), px(480.0)));
        cx.run_until_parked();

        let dialog = cx.debug_bounds("loop-range-dialog").unwrap();
        let fields = cx.debug_bounds("loop-range-fields").unwrap();
        let actions = cx.debug_bounds("loop-range-actions").unwrap();

        assert_eq!(dialog.size.width, crate::style::S10);
        assert!(fields.size.width > px(0.0));
        assert!(fields.origin.x + fields.size.width <= dialog.origin.x + dialog.size.width);
        assert!(actions.origin.x + actions.size.width <= dialog.origin.x + dialog.size.width);
        assert!(actions.origin.y + actions.size.height <= dialog.origin.y + dialog.size.height);
    }
}
