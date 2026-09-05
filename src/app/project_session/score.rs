//! Score workspace types. Ownership stays with the existing document, editor, and dialogs.

mod document;
mod editor;
mod export_rows_dialog;
mod row_edit_confirmation;
mod subdivision_dialog;

pub use document::{DocumentEvent, ScoreDocument};
pub(super) use document::{SaveState, ScoreCellEdit};
#[cfg(test)]
pub(super) use editor::ScoreAction;
pub use editor::{
    EditPartRequested, EditSubdivisionRequested, ExportRowsRequested, PartLoopRequested,
    PartSelected, RowEditRequested, ScoreEditor,
};
pub use export_rows_dialog::{ExportRowsConfirmed, ExportRowsDialog, ExportRowsDialogMsg};
pub use row_edit_confirmation::{RowEditConfirmation, RowEditConfirmationMsg};
pub use subdivision_dialog::{SubdivisionDialog, SubdivisionDialogMsg};

use gpui::{prelude::*, AnyElement, Entity};

pub(super) enum Overlay {
    ExportRows(Entity<ExportRowsDialog>),
    RowEdit(Entity<RowEditConfirmation>),
    Subdivision(Entity<SubdivisionDialog>),
}

impl Overlay {
    pub(super) fn element(&self) -> AnyElement {
        match self {
            Self::ExportRows(dialog) => dialog.clone().into_any_element(),
            Self::RowEdit(dialog) => dialog.clone().into_any_element(),
            Self::Subdivision(dialog) => dialog.clone().into_any_element(),
        }
    }
}
