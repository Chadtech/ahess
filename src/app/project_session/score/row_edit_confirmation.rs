//! Confirmation controls for a score-row edit.

use super::editor::RowEditRequested;
use crate::{
    part::PartRowEdit,
    view::{
        button::{self, Button},
        dialog::destructive_dialog,
    },
};
use gpui::{div, prelude::*, Context, Entity, EventEmitter, Window};

pub enum RowEditConfirmationMsg {
    Confirmed(RowEditRequested),
    Cancelled,
}

pub struct RowEditConfirmation {
    request: RowEditRequested,
    cancel_button: Entity<Button>,
    confirm_button: Entity<Button>,
}

impl EventEmitter<RowEditConfirmationMsg> for RowEditConfirmation {}

impl RowEditConfirmation {
    pub fn new(request: RowEditRequested, cx: &mut Context<Self>) -> Self {
        let source_editor = request.source_editor;
        let cancel_button =
            cx.new(move |_| Button::new(("cancel-row-edit", source_editor), "keep rows"));
        let confirm_label = match request.edit {
            PartRowEdit::Clear(_) => "clear rows",
            PartRowEdit::Delete(_) => "delete rows",
            PartRowEdit::InsertBefore(_) | PartRowEdit::InsertAfter(_) => "continue",
        };
        let confirm_button =
            cx.new(move |_| Button::new(("confirm-row-edit", source_editor), confirm_label));
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&confirm_button, Self::on_confirm_clicked)
            .detach();

        Self {
            request,
            cancel_button,
            confirm_button,
        }
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(RowEditConfirmationMsg::Cancelled);
    }

    fn on_confirm_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(RowEditConfirmationMsg::Confirmed(self.request.clone()));
    }

    fn message(&self) -> String {
        let cell_label = if self.request.populated_cell_count == 1 {
            "score value"
        } else {
            "score values"
        };
        let (verb, rows, consequence) = match self.request.edit {
            PartRowEdit::Clear(rows) => ("clear", rows, "the part length will stay the same"),
            PartRowEdit::Delete(rows) => (
                "delete",
                rows,
                "later beats will shift earlier and the part will become shorter",
            ),
            PartRowEdit::InsertBefore(_) | PartRowEdit::InsertAfter(_) => {
                return "continue with this row change?".to_string();
            }
        };
        let beat_label = if rows.len() == 1 {
            format!("beat {}", rows.first() + 1)
        } else {
            format!("beats {}–{}", rows.first() + 1, rows.last() + 1)
        };
        format!(
            "{verb} {beat_label}? {} {cell_label} will be removed; {consequence}.",
            self.request.populated_cell_count
        )
    }
}

impl Render for RowEditConfirmation {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let actions = button::action_group([
            div()
                .debug_selector(|| "cancel-row-edit-control".to_string())
                .child(self.cancel_button.clone()),
            div()
                .debug_selector(|| "confirm-row-edit-control".to_string())
                .child(self.confirm_button.clone()),
        ])
        .justify_end();
        destructive_dialog("confirm row change", None, self.message(), actions)
    }
}
