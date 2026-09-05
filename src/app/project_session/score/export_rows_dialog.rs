//! Controls for naming an export of selected score rows.

use super::editor::ExportRowsRequested;

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

use crate::part::ScoreRowRange;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportRowsConfirmed {
    pub part_name: crate::part::PartName,
    pub rows: ScoreRowRange,
    pub new_part_name: String,
}

pub enum ExportRowsDialogMsg {
    Confirmed(ExportRowsConfirmed),
    Cancelled,
}

pub struct ExportRowsDialog {
    request: ExportRowsRequested,
    name: Entity<TextInput>,
    close_button: Entity<Button>,
    cancel_button: Entity<Button>,
    export_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<ExportRowsDialogMsg> for ExportRowsDialog {}

impl ExportRowsDialog {
    pub fn new(request: ExportRowsRequested, cx: &mut Context<Self>) -> Self {
        let placeholder = format!("{} excerpt", request.part_name.as_str());
        let name = cx.new(|cx| TextInput::new("", placeholder, cx));
        let close_button = cx.new(|_| Button::x("close-export-score-rows"));
        let cancel_button = cx.new(|_| Button::new("cancel-export-score-rows", "cancel"));
        let export_button = cx.new(|_| Button::new("confirm-export-score-rows", "export as part"));

        cx.subscribe(&close_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&export_button, Self::on_export_clicked)
            .detach();

        Self {
            request,
            name,
            close_button,
            cancel_button,
            export_button,
            error: None,
        }
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ExportRowsDialogMsg::Cancelled);
    }

    fn on_export_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(ExportRowsDialogMsg::Confirmed(ExportRowsConfirmed {
            part_name: self.request.part_name.clone(),
            rows: self.request.rows,
            new_part_name: self.name.read(cx).value(),
        }));
    }

    pub fn export_failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    fn selection_description(&self) -> String {
        if self.request.rows.len() == 1 {
            format!(
                "export beat {} from {:?} into a new part",
                self.request.rows.first() + 1,
                self.request.part_name.as_str()
            )
        } else {
            format!(
                "export beats {}–{} from {:?} into a new part",
                self.request.rows.first() + 1,
                self.request.rows.last() + 1,
                self.request.part_name.as_str()
            )
        }
    }
}

impl Render for ExportRowsDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let actions =
            button::action_group([self.cancel_button.clone(), self.export_button.clone()])
                .justify_end();
        let content = div()
            .flex()
            .flex_col()
            .gap(s::CONTENT_PADDING)
            .p(s::CONTENT_PADDING)
            .child(
                div()
                    .text_color(s::TEXT_DEFAULT)
                    .child(self.selection_description()),
            )
            .child(field_group("new part name", self.name.clone()))
            .children(self.error.clone().map(error_message))
            .child(actions);

        s::raised(
            div()
                .flex()
                .flex_col()
                .w(s::S10)
                .bg(s::GRAY2)
                .child(title_bar(
                    "export selected rows",
                    Some(self.close_button.clone()),
                ))
                .child(content),
        )
    }
}
