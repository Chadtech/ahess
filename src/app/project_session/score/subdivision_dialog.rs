//! Subdivision editing controls and local input validation.

use crate::{
    style as s,
    view::{
        button::{self, Button},
        dialog::{error_message, title_bar},
        field_group::field_group,
        text_input::TextInput,
    },
};
use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

use crate::part::{MajorSubdivision, Part, SubdivisionPattern};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubdivisionDialogMsg {
    Confirmed {
        part_name: crate::part::PartName,
        subdivision_pattern: Option<SubdivisionPattern>,
        major_subdivision: Option<MajorSubdivision>,
    },
    Cancelled,
}

pub struct SubdivisionDialog {
    part_name: crate::part::PartName,
    subdivision_pattern: Entity<TextInput>,
    major_subdivision: Entity<TextInput>,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    save_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<SubdivisionDialogMsg> for SubdivisionDialog {}

impl SubdivisionDialog {
    pub fn new(part: &Part, cx: &mut Context<Self>) -> Self {
        let value = part
            .subdivision_pattern()
            .map(ToString::to_string)
            .unwrap_or_default();
        let subdivision_pattern = cx.new(|cx| TextInput::new(value, "4 or 4, 3, 3", cx));
        let major_value = part
            .major_subdivision()
            .map(|major| major.to_string())
            .unwrap_or_default();
        let major_subdivision = cx.new(|cx| TextInput::new(major_value, "12 or 16", cx));
        let close_button = cx.new(|_| Button::x("close-score-subdivision"));
        let cancel_button = cx.new(|_| Button::new("cancel-score-subdivision", "cancel"));
        let save_button = cx.new(|_| Button::new("save-score-subdivision", "save subdivisions"));

        cx.subscribe(&close_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_save_clicked).detach();

        Self {
            part_name: part.name.clone(),
            subdivision_pattern,
            major_subdivision,
            close_button,
            cancel_button,
            save_button,
            error: None,
        }
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(SubdivisionDialogMsg::Cancelled);
    }

    fn on_save_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        match (
            parse_optional_subdivision_pattern(&self.subdivision_pattern.read(cx).value()),
            parse_optional_major_subdivision(&self.major_subdivision.read(cx).value()),
        ) {
            (Ok(subdivision_pattern), Ok(major_subdivision)) => {
                cx.emit(SubdivisionDialogMsg::Confirmed {
                    part_name: self.part_name.clone(),
                    subdivision_pattern,
                    major_subdivision,
                })
            }
            (Err(error), _) | (_, Err(error)) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    pub fn save_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }
}

impl Render for SubdivisionDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let content =
            div()
                .flex()
                .flex_col()
                .gap(s::CONTENT_PADDING)
                .p(s::CONTENT_PADDING)
                .child(
                    div()
                        .text_color(s::TEXT_DEFAULT)
                        .child(format!("editing {:?}", self.part_name.as_str())),
                )
                .child(field_group(
                    "subdivision pattern (optional)",
                    self.subdivision_pattern.clone(),
                ))
                .child(field_group(
                    "major subdivision in beats (optional)",
                    self.major_subdivision.clone(),
                ))
                .child(div().text_color(s::TEXT_DEFAULT).child(
                    "major subdivisions restart the smaller beat groups, such as 4 within 16",
                ))
                .children(self.error.clone().map(error_message))
                .child(
                    button::action_group([self.cancel_button.clone(), self.save_button.clone()])
                        .justify_end(),
                );

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .child(title_bar(
                    "edit subdivisions",
                    Some(self.close_button.clone()),
                ))
                .child(content),
        )
    }
}

fn parse_optional_subdivision_pattern(value: &str) -> Result<Option<SubdivisionPattern>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    value
        .parse::<SubdivisionPattern>()
        .map(Some)
        .map_err(|error| error.to_string())
}

fn parse_optional_major_subdivision(value: &str) -> Result<Option<MajorSubdivision>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    value
        .parse::<MajorSubdivision>()
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{part::Part, view::button};
    use gpui::TestAppContext;
    #[test]
    fn subdivision_dialog_patterns_are_optional_positive_whole_number_lists() {
        assert!(parse_optional_subdivision_pattern("  ").unwrap().is_none());
        assert_eq!(
            parse_optional_subdivision_pattern(" 4, 3,3 ")
                .unwrap()
                .unwrap()
                .subdivisions()
                .collect::<Vec<_>>(),
            [4, 3, 3]
        );
        assert!(parse_optional_subdivision_pattern("4,,3").is_err());
        assert!(parse_optional_subdivision_pattern("4, 0").is_err());
        assert!(parse_optional_subdivision_pattern("4, 1.5").is_err());
        assert!(parse_optional_major_subdivision(" ").unwrap().is_none());
        assert_eq!(
            parse_optional_major_subdivision("16")
                .unwrap()
                .unwrap()
                .beats(),
            16
        );
        assert!(parse_optional_major_subdivision("0").is_err());
        assert!(parse_optional_major_subdivision("4.5").is_err());
    }

    #[gpui::test]
    fn subdivision_dialog_starts_with_the_current_pattern_and_keeps_invalid_input_open(
        cx: &mut TestAppContext,
    ) {
        let part = Part::new("intro", 10)
            .with_subdivision_pattern(Some("4, 3, 3".parse().unwrap()))
            .with_major_subdivision(Some("16".parse().unwrap()));
        let (dialog, cx) = cx.add_window_view(|_, cx| super::SubdivisionDialog::new(&part, cx));
        let (input, save_button) = cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            assert_eq!(dialog.subdivision_pattern.read(cx).value(), "4, 3, 3");
            assert_eq!(dialog.major_subdivision.read(cx).value(), "16");
            (
                dialog.subdivision_pattern.clone(),
                dialog.save_button.clone(),
            )
        });
        input.update(cx, |input, cx| input.sync_value("4,,3", cx));
        dialog.update(cx, |dialog, cx| {
            dialog.on_save_clicked(save_button, &button::Clicked, cx);
        });

        assert!(cx.update(|_, cx| dialog.read(cx).error.is_some()));
    }
}
