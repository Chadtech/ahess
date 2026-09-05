use crate::project::parts::{combined_subdivision_pattern, variant_part_name};

use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, EventEmitter, MouseButton, MouseDownEvent,
    Window,
};

use crate::{
    part::{MajorSubdivision, Part, PartName, SubdivisionPattern},
    style as s,
    view::{
        action_menu::{self, ActionMenu},
        button::{self, Button},
        dialog::{destructive_dialog, error_message},
        field_group::field_group,
        range_selection_list::{self, RangeSelectionList, Row, SelectedRange},
        selection_list,
        text_input::TextInput,
        workspace,
    },
};

/// Commands passed to the project owner after local form processing.
/// Project-wide validation and persistence remain the owner's responsibility.
pub enum Request {
    Add {
        name: String,
        length: u32,
        subdivision_pattern: Option<SubdivisionPattern>,
        major_subdivision: Option<MajorSubdivision>,
    },
    Duplicate {
        source: PartName,
        name: String,
    },
    Update {
        source: PartName,
        name: String,
        subdivision_pattern: Option<SubdivisionPattern>,
        major_subdivision: Option<MajorSubdivision>,
    },
    ConfirmDelete {
        name: PartName,
    },
    Combine {
        sources: Vec<PartName>,
        name: String,
    },
    AppendVariants {
        sources: Vec<PartName>,
        suffix: String,
    },
    ChangeSequence {
        sequence: Vec<PartName>,
        selected_range: Option<SelectedRange>,
    },
}

pub(super) enum Overlay {
    ConfirmDelete(Entity<DeleteDialog>),
}

impl Overlay {
    pub(super) fn element(&self) -> AnyElement {
        match self {
            Self::ConfirmDelete(dialog) => dialog.clone().into_any_element(),
        }
    }
}

pub(super) enum DeleteDialogMsg {
    Cancelled,
    Confirmed { name: PartName },
}

pub(super) struct DeleteDialog {
    name: PartName,
    cancel_button: Entity<Button>,
    confirm_button: Entity<Button>,
    error: Option<String>,
}

impl EventEmitter<DeleteDialogMsg> for DeleteDialog {}

impl DeleteDialog {
    pub(super) fn new(name: PartName, cx: &mut Context<Self>) -> Self {
        let cancel_button = cx.new(|_| Button::new("cancel-delete-part", "keep part"));
        let confirm_button = cx.new(|_| Button::new("confirm-delete-part", "delete part"));
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&confirm_button, Self::on_confirm_clicked)
            .detach();
        Self {
            name,
            cancel_button,
            confirm_button,
            error: None,
        }
    }

    pub(super) fn failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DeleteDialogMsg::Cancelled);
    }

    fn on_confirm_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DeleteDialogMsg::Confirmed {
            name: self.name.clone(),
        });
    }
}

impl Render for DeleteDialog {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let actions = div()
            .flex()
            .flex_col()
            .gap(s::S3)
            .children(self.error.clone().map(error_message))
            .child(
                button::action_group([self.cancel_button.clone(), self.confirm_button.clone()])
                    .justify_end(),
            );
        destructive_dialog(
            "delete part",
            None,
            format!(
                "delete {:?}? its csv file will be moved to the deleted folder.",
                self.name.as_str()
            ),
            actions,
        )
    }
}

struct ListView {
    add_new_button: Entity<Button>,
    combine_button: Entity<Button>,
    edit_button: Entity<Button>,
    duplicate_button: Entity<Button>,
    delete_button: Entity<Button>,
    add_to_arrangement_button: Entity<Button>,
    move_earlier_button: Entity<Button>,
    move_later_button: Entity<Button>,
    arrangement_action_menu: Entity<ActionMenu>,
    arrangement_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArrangementAction {
    Repeat,
    AppendVariants,
    Remove,
}

impl ArrangementAction {
    const ALL: [Self; 3] = [Self::Repeat, Self::AppendVariants, Self::Remove];

    fn index(self) -> usize {
        self as usize
    }

    fn label(self) -> &'static str {
        match self {
            Self::Repeat => "repeat selected range",
            Self::AppendVariants => "append as variants",
            Self::Remove => "remove selected range",
        }
    }
}

enum View {
    List(Box<ListView>),
    Add {
        name: Entity<TextInput>,
        length: Entity<TextInput>,
        subdivision_pattern: Entity<TextInput>,
        major_subdivision: Entity<TextInput>,
        cancel_button: Entity<Button>,
        add_button: Entity<Button>,
        form_error: Option<String>,
    },
    Duplicate {
        source: PartName,
        name: Entity<TextInput>,
        cancel_button: Entity<Button>,
        duplicate_button: Entity<Button>,
        form_error: Option<String>,
    },
    Edit {
        source: PartName,
        name: Entity<TextInput>,
        subdivision_pattern: Entity<TextInput>,
        major_subdivision: Entity<TextInput>,
        cancel_button: Entity<Button>,
        save_button: Entity<Button>,
        form_error: Option<String>,
    },
    Combine {
        available_part: Option<PartName>,
        sources: Vec<PartName>,
        selected_source: Option<usize>,
        name: Entity<TextInput>,
        add_source_button: Entity<Button>,
        move_source_earlier_button: Entity<Button>,
        move_source_later_button: Entity<Button>,
        remove_source_button: Entity<Button>,
        cancel_button: Entity<Button>,
        combine_button: Entity<Button>,
        form_error: Option<String>,
    },
    AppendVariants {
        sources: Vec<PartName>,
        suffix: Entity<TextInput>,
        cancel_button: Entity<Button>,
        append_button: Entity<Button>,
        form_error: Option<String>,
    },
}

pub struct PartsWorkspace {
    parts: Vec<Part>,
    sequence: Vec<PartName>,
    selected_part: Option<PartName>,
    arrangement_range: Entity<RangeSelectionList>,
    view: View,
}

impl EventEmitter<Request> for PartsWorkspace {}

impl PartsWorkspace {
    pub fn new(parts: Vec<Part>, sequence: Vec<PartName>, cx: &mut Context<Self>) -> Self {
        let selected_part = parts.first().map(|part| part.name.clone());
        let selected_range = SelectedRange::new(0, 0, sequence.len());
        let arrangement_range = cx.new(|cx| {
            RangeSelectionList::new(
                "arrangement-list",
                "no arranged parts yet",
                arrangement_rows(&sequence),
                selected_range,
                cx,
            )
            .fill_height()
        });
        cx.subscribe(&arrangement_range, Self::on_arrangement_range_changed)
            .detach();

        let workspace = Self {
            parts,
            sequence,
            selected_part,
            arrangement_range,
            view: Self::list_view(cx),
        };
        workspace.sync_button_states(cx);
        workspace
    }

    pub fn has_draft(&self) -> bool {
        match &self.view {
            View::List(_) => false,
            View::Add { .. }
            | View::Duplicate { .. }
            | View::Edit { .. }
            | View::Combine { .. }
            | View::AppendVariants { .. } => true,
        }
    }

    pub fn begin_editing_part(&mut self, name: &PartName, cx: &mut Context<Self>) -> bool {
        if let View::Edit { source, .. } = &self.view {
            if source.eq_ignore_ascii_case(name) {
                self.selected_part = Some(source.clone());
                return true;
            }
        }
        if self.has_draft() {
            return false;
        }

        let Some(part) = find_part(&self.parts, name).cloned() else {
            return false;
        };
        self.selected_part = Some(part.name.clone());
        self.view = Self::edit_view(part, cx);
        cx.notify();
        true
    }

    #[cfg(test)]
    pub(crate) fn start_add_for_test(&mut self, cx: &mut Context<Self>) {
        self.view = Self::add_view(cx);
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn editing_part(&self) -> Option<&PartName> {
        match &self.view {
            View::Edit { source, .. } => Some(source),
            _ => None,
        }
    }

    fn list_view(cx: &mut Context<Self>) -> View {
        let add_new_button = cx.new(|_| Button::new("add-new-part", "add new part"));
        let combine_button = cx.new(|_| Button::new("combine-parts", "combine"));
        let edit_button = cx.new(|_| Button::new("edit-part", "edit"));
        let duplicate_button = cx.new(|_| Button::new("duplicate-part", "duplicate"));
        let delete_button = cx.new(|_| Button::new("delete-part", "delete"));
        let add_to_arrangement_button = cx.new(|_| Button::new("add-to-arrangement", "add ->"));
        let move_earlier_button =
            cx.new(|_| Button::square("move-arrangement-earlier", "↑").disabled(true));
        let move_later_button =
            cx.new(|_| Button::square("move-arrangement-later", "↓").disabled(true));
        let arrangement_action_menu = cx.new(|cx| {
            ActionMenu::new_upward(
                "arrangement-action-menu",
                "actions",
                ArrangementAction::ALL.map(ArrangementAction::label),
                cx,
            )
        });

        cx.subscribe(&add_new_button, Self::on_add_new_clicked)
            .detach();
        cx.subscribe(&combine_button, Self::on_combine_clicked)
            .detach();
        cx.subscribe(&edit_button, Self::on_edit_clicked).detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_clicked)
            .detach();
        cx.subscribe(&delete_button, Self::on_delete_clicked)
            .detach();
        cx.subscribe(
            &add_to_arrangement_button,
            Self::on_add_to_arrangement_clicked,
        )
        .detach();
        cx.subscribe(&move_earlier_button, Self::on_move_earlier_clicked)
            .detach();
        cx.subscribe(&move_later_button, Self::on_move_later_clicked)
            .detach();
        cx.subscribe(
            &arrangement_action_menu,
            Self::on_arrangement_action_selected,
        )
        .detach();

        View::List(Box::new(ListView {
            add_new_button,
            combine_button,
            edit_button,
            duplicate_button,
            delete_button,
            add_to_arrangement_button,
            move_earlier_button,
            move_later_button,
            arrangement_action_menu,
            arrangement_error: None,
        }))
    }

    fn duplicate_view(source: PartName, cx: &mut Context<Self>) -> View {
        let placeholder = format!("{} copy", source.as_str());
        let name = cx.new(|cx| TextInput::new("", placeholder, cx));
        let cancel_button = cx.new(|_| Button::new("cancel-duplicate-part", "cancel"));
        let duplicate_button = cx.new(|_| Button::new("confirm-duplicate-part", "duplicate part"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&duplicate_button, Self::on_duplicate_confirmed)
            .detach();

        View::Duplicate {
            source,
            name,
            cancel_button,
            duplicate_button,
            form_error: None,
        }
    }

    fn edit_view(part: Part, cx: &mut Context<Self>) -> View {
        let source = part.name.clone();
        let name = cx.new(|cx| TextInput::new(source.as_str().to_owned(), "part name", cx));
        let subdivision_pattern = part
            .subdivision_pattern()
            .map(ToString::to_string)
            .unwrap_or_default();
        let subdivision_pattern =
            cx.new(|cx| TextInput::new(subdivision_pattern, "4 or 4, 3, 3", cx));
        let major_subdivision = part
            .major_subdivision()
            .map(|major| major.to_string())
            .unwrap_or_default();
        let major_subdivision = cx.new(|cx| TextInput::new(major_subdivision, "12 or 16", cx));
        let cancel_button = cx.new(|_| Button::new("cancel-edit-part", "cancel"));
        let save_button = cx.new(|_| Button::new("confirm-edit-part", "save part"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&save_button, Self::on_edit_confirmed).detach();

        View::Edit {
            source,
            name,
            subdivision_pattern,
            major_subdivision,
            cancel_button,
            save_button,
            form_error: None,
        }
    }

    fn add_view(cx: &mut Context<Self>) -> View {
        let name = cx.new(|cx| TextInput::new("", "intro", cx));
        let length = cx.new(|cx| TextInput::new("16", "16", cx));
        let subdivision_pattern = cx.new(|cx| TextInput::new("", "4 or 4, 3, 3", cx));
        let major_subdivision = cx.new(|cx| TextInput::new("", "12 or 16", cx));
        let cancel_button = cx.new(|_| Button::new("cancel-add-part", "cancel"));
        let add_button = cx.new(|_| Button::new("confirm-add-part", "add part"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&add_button, Self::on_add_clicked).detach();

        View::Add {
            name,
            length,
            subdivision_pattern,
            major_subdivision,
            cancel_button,
            add_button,
            form_error: None,
        }
    }

    fn combine_view(parts: &[Part], cx: &mut Context<Self>) -> View {
        let available_part = parts.first().map(|part| part.name.clone());
        let name = cx.new(|cx| TextInput::new("", "combined part", cx));
        let add_source_button = cx.new(|_| {
            Button::new("add-combination-source", "add →").disabled(available_part.is_none())
        });
        let move_source_earlier_button =
            cx.new(|_| Button::square("move-combination-source-earlier", "↑").disabled(true));
        let move_source_later_button =
            cx.new(|_| Button::square("move-combination-source-later", "↓").disabled(true));
        let remove_source_button =
            cx.new(|_| Button::new("remove-combination-source", "remove").disabled(true));
        let cancel_button = cx.new(|_| Button::new("cancel-combine-parts", "cancel"));
        let combine_button =
            cx.new(|_| Button::new("confirm-combine-parts", "combine").disabled(true));

        cx.subscribe(&add_source_button, Self::on_add_source_clicked)
            .detach();
        cx.subscribe(
            &move_source_earlier_button,
            Self::on_move_source_earlier_clicked,
        )
        .detach();
        cx.subscribe(
            &move_source_later_button,
            Self::on_move_source_later_clicked,
        )
        .detach();
        cx.subscribe(&remove_source_button, Self::on_remove_source_clicked)
            .detach();
        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&combine_button, Self::on_combine_confirmed)
            .detach();

        View::Combine {
            available_part,
            sources: Vec::new(),
            selected_source: None,
            name,
            add_source_button,
            move_source_earlier_button,
            move_source_later_button,
            remove_source_button,
            cancel_button,
            combine_button,
            form_error: None,
        }
    }

    fn append_variants_view(
        parts: &[Part],
        sources: Vec<PartName>,
        cx: &mut Context<Self>,
    ) -> View {
        let suffix = next_variant_suffix(parts, &sources);
        let suffix = cx.new(|cx| TextInput::new(suffix, "v1", cx));
        let cancel_button = cx.new(|_| Button::new("cancel-append-variants", "cancel"));
        let append_button = cx.new(|_| Button::new("confirm-append-variants", "append variants"));

        cx.subscribe(&cancel_button, Self::on_cancel_clicked)
            .detach();
        cx.subscribe(&append_button, Self::on_append_variants_confirmed)
            .detach();

        View::AppendVariants {
            sources,
            suffix,
            cancel_button,
            append_button,
            form_error: None,
        }
    }

    fn on_add_new_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::add_view(cx);
        cx.notify();
    }

    fn on_combine_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::combine_view(&self.parts, cx);
        cx.notify();
    }

    fn on_duplicate_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.selected_part.clone() else {
            return;
        };

        self.view = Self::duplicate_view(source, cx);
        cx.notify();
    }

    fn on_edit_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let Some(source) = self.selected_part.clone() else {
            return;
        };
        let Some(part) = find_part(&self.parts, &source).cloned() else {
            return;
        };

        self.view = Self::edit_view(part, cx);
        cx.notify();
    }

    fn on_cancel_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        self.suppress_add_new_hover(cx);
        cx.notify();
    }

    fn on_add_clicked(&mut self, _: Entity<Button>, _: &button::Clicked, cx: &mut Context<Self>) {
        let View::Add {
            name,
            length,
            subdivision_pattern,
            major_subdivision,
            ..
        } = &self.view
        else {
            return;
        };
        let name = name.read(cx).value();
        let length = length.read(cx).value();
        let subdivision_pattern = subdivision_pattern.read(cx).value();
        let major_subdivision = major_subdivision.read(cx).value();
        match (
            parse_part_length(&length),
            parse_subdivision_pattern(&subdivision_pattern),
            parse_major_subdivision(&major_subdivision),
        ) {
            (Ok(length), Ok(subdivision_pattern), Ok(major_subdivision)) => cx.emit(Request::Add {
                name,
                length,
                subdivision_pattern,
                major_subdivision,
            }),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                if let View::Add { form_error, .. } = &mut self.view {
                    *form_error = Some(error);
                    cx.notify();
                }
            }
        }
    }

    fn on_edit_confirmed(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Edit {
            source,
            name,
            subdivision_pattern,
            major_subdivision,
            ..
        } = &self.view
        else {
            return;
        };
        let subdivision_pattern = subdivision_pattern.read(cx).value();
        let major_subdivision = major_subdivision.read(cx).value();
        match (
            parse_subdivision_pattern(&subdivision_pattern),
            parse_major_subdivision(&major_subdivision),
        ) {
            (Ok(subdivision_pattern), Ok(major_subdivision)) => cx.emit(Request::Update {
                source: source.clone(),
                name: name.read(cx).value(),
                subdivision_pattern,
                major_subdivision,
            }),
            (Err(error), _) | (_, Err(error)) => {
                if let View::Edit { form_error, .. } = &mut self.view {
                    *form_error = Some(error);
                    cx.notify();
                }
            }
        }
    }

    fn on_duplicate_confirmed(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Duplicate { source, name, .. } = &self.view else {
            return;
        };
        cx.emit(Request::Duplicate {
            source: source.clone(),
            name: name.read(cx).value(),
        });
    }

    fn on_combine_confirmed(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Combine { sources, name, .. } = &self.view else {
            return;
        };
        if sources.len() < 2 {
            return;
        }
        cx.emit(Request::Combine {
            sources: sources.clone(),
            name: name.read(cx).value(),
        });
    }

    fn on_append_variants_confirmed(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::AppendVariants {
            sources, suffix, ..
        } = &self.view
        else {
            return;
        };
        let suffix = suffix.read(cx).value();
        if suffix.trim().is_empty() {
            if let View::AppendVariants { form_error, .. } = &mut self.view {
                *form_error = Some("variant suffix cannot be empty".to_string());
                cx.notify();
            }
            return;
        }
        cx.emit(Request::AppendVariants {
            sources: sources.clone(),
            suffix,
        });
    }

    fn on_add_source_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Combine {
            available_part,
            sources,
            selected_source,
            form_error,
            ..
        } = &mut self.view
        else {
            return;
        };
        let Some(source) = available_part.clone() else {
            return;
        };
        sources.push(source);
        *selected_source = Some(sources.len() - 1);
        *form_error = None;
        self.sync_combine_button_states(cx);
        cx.notify();
    }

    fn on_move_source_earlier_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Combine {
            sources,
            selected_source,
            form_error,
            ..
        } = &mut self.view
        else {
            return;
        };
        let Some(index) = selected_source.filter(|index| *index > 0 && *index < sources.len())
        else {
            return;
        };
        sources.swap(index, index - 1);
        *selected_source = Some(index - 1);
        *form_error = None;
        self.sync_combine_button_states(cx);
        cx.notify();
    }

    fn on_move_source_later_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Combine {
            sources,
            selected_source,
            form_error,
            ..
        } = &mut self.view
        else {
            return;
        };
        let Some(index) = selected_source.filter(|index| *index + 1 < sources.len()) else {
            return;
        };
        sources.swap(index, index + 1);
        *selected_source = Some(index + 1);
        *form_error = None;
        self.sync_combine_button_states(cx);
        cx.notify();
    }

    fn on_remove_source_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let View::Combine {
            sources,
            selected_source,
            form_error,
            ..
        } = &mut self.view
        else {
            return;
        };
        let Some(index) = selected_source.filter(|index| *index < sources.len()) else {
            return;
        };
        sources.remove(index);
        *selected_source =
            (!sources.is_empty()).then(|| index.min(sources.len().saturating_sub(1)));
        *form_error = None;
        self.sync_combine_button_states(cx);
        cx.notify();
    }

    fn on_delete_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(name) = self.selected_part.clone() else {
            return;
        };

        cx.emit(Request::ConfirmDelete { name });
    }

    fn on_add_to_arrangement_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(part_name) = self.selected_part.clone() else {
            return;
        };
        let (sequence, selected_range) = sequence_with_inserted_part(
            &self.sequence,
            part_name,
            self.selected_arrangement_range(cx),
        );
        cx.emit(Request::ChangeSequence {
            sequence,
            selected_range: Some(selected_range),
        });
    }

    fn on_move_earlier_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_range) = self.selected_arrangement_range(cx) else {
            return;
        };
        let Some((sequence, selected_range)) =
            sequence_with_moved_range(&self.sequence, selected_range, -1)
        else {
            return;
        };
        cx.emit(Request::ChangeSequence {
            sequence,
            selected_range: Some(selected_range),
        });
    }

    fn on_move_later_clicked(
        &mut self,
        _: Entity<Button>,
        _: &button::Clicked,
        cx: &mut Context<Self>,
    ) {
        let Some(selected_range) = self.selected_arrangement_range(cx) else {
            return;
        };
        let Some((sequence, selected_range)) =
            sequence_with_moved_range(&self.sequence, selected_range, 1)
        else {
            return;
        };
        cx.emit(Request::ChangeSequence {
            sequence,
            selected_range: Some(selected_range),
        });
    }

    fn on_arrangement_action_selected(
        &mut self,
        _: Entity<ActionMenu>,
        selected: &action_menu::Selected,
        cx: &mut Context<Self>,
    ) {
        match ArrangementAction::ALL.get(selected.index).copied() {
            Some(ArrangementAction::Repeat) => self.repeat_selected_range(cx),
            Some(ArrangementAction::AppendVariants) => self.begin_appending_variants(cx),
            Some(ArrangementAction::Remove) => self.remove_selected_range(cx),
            None => {}
        }
    }

    fn repeat_selected_range(&self, cx: &mut Context<Self>) {
        let Some(selected_range) = self.selected_arrangement_range(cx) else {
            return;
        };
        let Some((sequence, selected_range)) =
            sequence_with_repeated_range(&self.sequence, selected_range)
        else {
            return;
        };
        cx.emit(Request::ChangeSequence {
            sequence,
            selected_range: Some(selected_range),
        });
    }

    fn begin_appending_variants(&mut self, cx: &mut Context<Self>) {
        let Some(selected_range) = self.selected_arrangement_range(cx) else {
            return;
        };
        let Some(sources) = selected_sequence(&self.sequence, selected_range) else {
            return;
        };
        self.view = Self::append_variants_view(&self.parts, sources.to_vec(), cx);
        cx.notify();
    }

    fn remove_selected_range(&self, cx: &mut Context<Self>) {
        let Some(selected_range) = self.selected_arrangement_range(cx) else {
            return;
        };
        let Some((sequence, selected_range)) =
            sequence_with_removed_range(&self.sequence, selected_range)
        else {
            return;
        };
        cx.emit(Request::ChangeSequence {
            sequence,
            selected_range,
        });
    }

    fn on_arrangement_range_changed(
        &mut self,
        _: Entity<RangeSelectionList>,
        _: &range_selection_list::Changed,
        cx: &mut Context<Self>,
    ) {
        if let View::List(view) = &mut self.view {
            view.arrangement_error = None;
        }
        self.sync_button_states(cx);
        cx.notify();
    }

    fn selected_arrangement_range(&self, cx: &Context<Self>) -> Option<SelectedRange> {
        self.arrangement_range.read(cx).selected_range()
    }

    fn select_available_part(&mut self, name: &PartName, cx: &mut Context<Self>) {
        if find_part(&self.parts, name).is_none() {
            return;
        }
        let View::Combine {
            available_part,
            form_error,
            ..
        } = &mut self.view
        else {
            return;
        };
        if available_part.as_ref() == Some(name) {
            return;
        }
        *available_part = Some(name.clone());
        *form_error = None;
        self.sync_combine_button_states(cx);
        cx.notify();
    }

    fn select_combination_source(&mut self, index: usize, cx: &mut Context<Self>) {
        let View::Combine {
            sources,
            selected_source,
            form_error,
            ..
        } = &mut self.view
        else {
            return;
        };
        if index >= sources.len() || *selected_source == Some(index) {
            return;
        }
        *selected_source = Some(index);
        *form_error = None;
        self.sync_combine_button_states(cx);
        cx.notify();
    }

    fn select_part(&mut self, name: &PartName, cx: &mut Context<Self>) {
        let Some(part) = find_part(&self.parts, name) else {
            return;
        };
        if self.selected_part.as_ref() == Some(&part.name) {
            return;
        }

        self.selected_part = Some(part.name.clone());
        if let View::List(view) = &mut self.view {
            view.arrangement_error = None;
        }
        self.sync_button_states(cx);
        cx.notify();
    }

    fn sync_button_states(&self, cx: &mut Context<Self>) {
        let View::List(view) = &self.view else {
            return;
        };
        let selected_range = self.selected_arrangement_range(cx);
        let can_move_earlier = selected_range.is_some_and(|range| range.first() > 0);
        let can_move_later =
            selected_range.is_some_and(|range| range.last() + 1 < self.sequence.len());
        let can_delete = self.selected_part.as_ref().is_some_and(|selected| {
            !self
                .sequence
                .iter()
                .any(|name| name.eq_ignore_ascii_case(selected))
        });
        view.delete_button.update(cx, |button, cx| {
            button.set_disabled(!can_delete, cx);
        });
        view.move_earlier_button.update(cx, |button, cx| {
            button.set_disabled(!can_move_earlier, cx);
        });
        view.move_later_button.update(cx, |button, cx| {
            button.set_disabled(!can_move_later, cx);
        });
        view.arrangement_action_menu.update(cx, |menu, cx| {
            for action in ArrangementAction::ALL {
                menu.set_disabled(action.index(), selected_range.is_none(), cx);
            }
        });
    }

    fn sync_combine_button_states(&self, cx: &mut Context<Self>) {
        let View::Combine {
            available_part,
            sources,
            selected_source,
            add_source_button,
            move_source_earlier_button,
            move_source_later_button,
            remove_source_button,
            combine_button,
            ..
        } = &self.view
        else {
            return;
        };
        let selected_source = selected_source.filter(|index| *index < sources.len());
        add_source_button.update(cx, |button, cx| {
            button.set_disabled(available_part.is_none(), cx);
        });
        move_source_earlier_button.update(cx, |button, cx| {
            button.set_disabled(selected_source.is_none_or(|index| index == 0), cx);
        });
        move_source_later_button.update(cx, |button, cx| {
            button.set_disabled(
                selected_source.is_none_or(|index| index + 1 >= sources.len()),
                cx,
            );
        });
        remove_source_button.update(cx, |button, cx| {
            button.set_disabled(selected_source.is_none(), cx);
        });
        combine_button.update(cx, |button, cx| {
            button.set_disabled(sources.len() < 2, cx);
        });
    }

    fn suppress_add_new_hover(&self, cx: &mut Context<Self>) {
        if let View::List(view) = &self.view {
            view.add_new_button.update(cx, |button, cx| {
                button.suppress_hover_until_pointer_exit(cx);
            });
        }
    }

    pub fn part_added(&mut self, parts: Vec<Part>, added: PartName, cx: &mut Context<Self>) {
        self.parts = parts;
        self.selected_part = Some(added);
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        self.suppress_add_new_hover(cx);
        cx.notify();
    }

    pub fn add_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let View::Add { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn duplicate_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let View::Duplicate { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn part_updated(
        &mut self,
        parts: Vec<Part>,
        sequence: Vec<PartName>,
        updated: PartName,
        cx: &mut Context<Self>,
    ) {
        self.parts = parts;
        self.sequence = sequence;
        self.selected_part = Some(updated);
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        cx.notify();
    }

    pub fn update_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let View::Edit { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn part_deleted(&mut self, parts: Vec<Part>, deleted: &PartName, cx: &mut Context<Self>) {
        let deleted_index = self
            .parts
            .iter()
            .position(|part| part.name.eq_ignore_ascii_case(deleted));
        self.parts = parts;
        self.selected_part = deleted_index
            .and_then(|index| {
                self.parts
                    .get(index.min(self.parts.len().saturating_sub(1)))
            })
            .map(|part| part.name.clone());
        self.view = Self::list_view(cx);
        self.sync_button_states(cx);
        cx.notify();
    }

    pub fn sequence_changed(
        &mut self,
        sequence: Vec<PartName>,
        selected_range: Option<SelectedRange>,
        cx: &mut Context<Self>,
    ) {
        self.sequence = sequence;
        let rows = arrangement_rows(&self.sequence);
        self.arrangement_range.update(cx, |range, cx| {
            range.sync_rows(rows, cx);
            range.select_range(selected_range, cx);
        });
        self.sync_button_states(cx);
        if let View::List(view) = &mut self.view {
            view.arrangement_error = None;
        }
        cx.notify();
    }

    pub fn combine_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let View::Combine { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn variants_appended(
        &mut self,
        parts: Vec<Part>,
        sequence: Vec<PartName>,
        first_variant: PartName,
        selected_range: SelectedRange,
        cx: &mut Context<Self>,
    ) {
        self.parts = parts;
        self.sequence = sequence;
        self.selected_part = Some(first_variant);
        self.view = Self::list_view(cx);
        let rows = arrangement_rows(&self.sequence);
        self.arrangement_range.update(cx, |range, cx| {
            range.sync_rows(rows, cx);
            range.select_range(Some(selected_range), cx);
        });
        self.sync_button_states(cx);
        cx.notify();
    }

    pub fn append_variants_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let View::AppendVariants { form_error, .. } = &mut self.view {
            *form_error = Some(error);
            cx.notify();
        }
    }

    pub fn sequence_change_failed(&mut self, error: String, cx: &mut Context<Self>) {
        if let View::List(view) = &mut self.view {
            view.arrangement_error = Some(error);
            cx.notify();
        }
    }

    pub fn sync_project(
        &mut self,
        parts: Vec<Part>,
        sequence: Vec<PartName>,
        cx: &mut Context<Self>,
    ) {
        self.parts = parts;
        self.sequence = sequence;
        if self
            .selected_part
            .as_ref()
            .is_none_or(|selected| find_part(&self.parts, selected).is_none())
        {
            self.selected_part = self.parts.first().map(|part| part.name.clone());
        }
        let rows = arrangement_rows(&self.sequence);
        self.arrangement_range.update(cx, |range, cx| {
            range.sync_rows(rows, cx);
        });
        self.sync_button_states(cx);
        cx.notify();
    }
}

impl Render for PartsWorkspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match &self.view {
            View::List(view) => workspace::list_detail(workspace::ListDetailArgs {
                list: workspace::column_with_actions(
                    part_list(&self.parts, self.selected_part.as_ref(), cx),
                    button::action_group([
                        view.add_new_button.clone(),
                        view.combine_button.clone(),
                    ])
                    .debug_selector(|| "part-list-actions".to_string()),
                ),
                details: part_details(
                    self.selected_part
                        .as_ref()
                        .and_then(|name| find_part(&self.parts, name)),
                    PartDetailsButtons {
                        edit: view.edit_button.clone(),
                        duplicate: view.duplicate_button.clone(),
                        delete: view.delete_button.clone(),
                        add_to_arrangement: view.add_to_arrangement_button.clone(),
                    },
                    &self.sequence,
                ),
                auxiliary: Some(
                    arrangement_panel(
                        &self.parts,
                        &self.sequence,
                        self.arrangement_range.clone(),
                        view.move_earlier_button.clone(),
                        view.move_later_button.clone(),
                        view.arrangement_action_menu.clone(),
                        view.arrangement_error.clone(),
                    )
                    .into_any_element(),
                ),
                footer: None,
            }),
            View::Add {
                name,
                length,
                subdivision_pattern,
                major_subdivision,
                cancel_button,
                add_button,
                form_error,
            } => {
                let form = div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(field_group("part name", name.clone()))
                    .child(field_group("length in beats", length.clone()))
                    .child(field_group(
                        "subdivision pattern (optional)",
                        subdivision_pattern.clone(),
                    ))
                    .child(field_group(
                        "major subdivision in beats (optional)",
                        major_subdivision.clone(),
                    ));
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                workspace::management_form(
                    form,
                    button::action_group([cancel_button.clone(), add_button.clone()]).justify_end(),
                )
            }
            View::Duplicate {
                source,
                name,
                cancel_button,
                duplicate_button,
                form_error,
            } => {
                let form = div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_color(s::TEXT_DEFAULT)
                            .child(format!("copying {:?}", source.as_str())),
                    )
                    .child(field_group("new part name", name.clone()));
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                workspace::management_form(
                    form,
                    button::action_group([cancel_button.clone(), duplicate_button.clone()])
                        .justify_end(),
                )
            }
            View::Edit {
                source,
                name,
                subdivision_pattern,
                major_subdivision,
                cancel_button,
                save_button,
                form_error,
            } => {
                let form = div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_color(s::TEXT_DEFAULT)
                            .child(format!("editing {:?}", source.as_str())),
                    )
                    .child(field_group("part name", name.clone()))
                    .child(field_group(
                        "subdivision pattern (optional)",
                        subdivision_pattern.clone(),
                    ))
                    .child(field_group(
                        "major subdivision in beats (optional)",
                        major_subdivision.clone(),
                    ));
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                workspace::management_form(
                    form,
                    button::action_group([cancel_button.clone(), save_button.clone()])
                        .justify_end(),
                )
            }
            View::Combine {
                available_part,
                sources,
                selected_source,
                name,
                add_source_button,
                move_source_earlier_button,
                move_source_later_button,
                remove_source_button,
                cancel_button,
                combine_button,
                form_error,
            } => {
                let available = combine_available_parts(&self.parts, available_part.as_ref(), cx);
                let selected = combination_sources(&self.parts, sources, *selected_source, cx);
                let source_actions = div()
                    .flex()
                    .justify_end()
                    .debug_selector(|| "combine-available-actions".to_string())
                    .child(add_source_button.clone());
                let movement_actions = button::labeled_action_group(
                    "move",
                    [
                        move_source_earlier_button.clone(),
                        move_source_later_button.clone(),
                    ],
                );
                let selected_actions = div()
                    .flex()
                    .justify_between()
                    .gap(s::S5)
                    .debug_selector(|| "combine-selected-actions".to_string())
                    .child(movement_actions)
                    .child(remove_source_button.clone());
                let columns = div()
                    .flex()
                    .flex_1()
                    .min_h(s::S0)
                    .gap(s::CONTENT_PADDING)
                    .debug_selector(|| "combine-columns".to_string())
                    .child(
                        workspace::column_with_actions(available, source_actions)
                            .flex_1()
                            .w(s::S0)
                            .min_w(s::S0)
                            .debug_selector(|| "combine-available-column".to_string()),
                    )
                    .child(
                        workspace::column_with_actions(selected, selected_actions)
                            .flex_1()
                            .w(s::S0)
                            .min_w(s::S0)
                            .debug_selector(|| "combine-selected-column".to_string()),
                    );
                let form = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(s::S0)
                    .gap(s::CONTENT_PADDING)
                    .debug_selector(|| "combine-form".to_string())
                    .child(field_group("new part name", name.clone()).flex_none())
                    .child(columns);
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                workspace::management_form(
                    form,
                    button::action_group([cancel_button.clone(), combine_button.clone()])
                        .justify_end()
                        .debug_selector(|| "combine-workspace-actions".to_string()),
                )
            }
            View::AppendVariants {
                sources,
                suffix,
                cancel_button,
                append_button,
                form_error,
            } => {
                let occurrence_count = sources.len();
                let distinct_count = distinct_part_names(sources).len();
                let occurrence_label = if occurrence_count == 1 {
                    "occurrence"
                } else {
                    "occurrences"
                };
                let part_label = if distinct_count == 1 { "part" } else { "parts" };
                let selected_names = sources
                    .iter()
                    .map(PartName::as_str)
                    .collect::<Vec<_>>()
                    .join(", ");
                let form = div()
                    .flex()
                    .flex_col()
                    .gap(s::CONTENT_PADDING)
                    .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                        "append {occurrence_count} selected {occurrence_label} as variants of {distinct_count} distinct {part_label}"
                    )))
                    .child(
                        div()
                            .text_color(s::TEXT_DEFAULT)
                            .child("each distinct part is copied once; repeated occurrences use the same variant"),
                    )
                    .child(
                        div()
                            .text_color(s::TEXT_DEFAULT)
                            .child(format!("selected range: {selected_names}")),
                    )
                    .child(field_group("variant suffix", suffix.clone()));
                let form = if let Some(error) = form_error {
                    form.child(error_message(error.clone()))
                } else {
                    form
                };

                workspace::management_form(
                    form,
                    button::action_group([cancel_button.clone(), append_button.clone()])
                        .justify_end(),
                )
            }
        }
    }
}

fn combine_available_parts(
    parts: &[Part],
    selected_part: Option<&PartName>,
    cx: &mut Context<PartsWorkspace>,
) -> gpui::Div {
    let rows = parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let part_name = part.name.clone();
            let beat_label = if part.length == 1 { "beat" } else { "beats" };
            selection_list::row(
                index,
                selected_part == Some(&part.name),
                format!("{} · {} {beat_label}", part.name.as_str(), part.length),
            )
            .debug_selector(move || format!("combine-available-part-{index}"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
                    dialog.select_available_part(&part_name, cx);
                }),
            )
        })
        .collect::<Vec<_>>();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(s::S0)
        .child(
            div()
                .pb(s::S4)
                .text_color(s::TEXT_HEADER)
                .child("available parts"),
        )
        .child(
            selection_list::list("combine-available-parts-scroll", "no parts available", rows)
                .w_full()
                .debug_selector(|| "combine-available-list".to_string()),
        )
}

fn combination_sources(
    parts: &[Part],
    sources: &[PartName],
    selected_source: Option<usize>,
    cx: &mut Context<PartsWorkspace>,
) -> gpui::Div {
    let rows = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let beat_count = find_part(parts, source).map_or(0, |part| part.length);
            let beat_label = if beat_count == 1 { "beat" } else { "beats" };
            selection_list::row(
                index,
                selected_source == Some(index),
                format!(
                    "{}. {} · {beat_count} {beat_label}",
                    index + 1,
                    source.as_str()
                ),
            )
            .debug_selector(move || format!("combination-source-{index}"))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
                    dialog.select_combination_source(index, cx);
                }),
            )
        })
        .collect::<Vec<_>>();
    let source_parts = sources
        .iter()
        .filter_map(|source| find_part(parts, source))
        .cloned()
        .collect::<Vec<_>>();
    let total_beats = source_parts
        .iter()
        .map(|part| u64::from(part.length))
        .sum::<u64>();
    let source_label = if sources.len() == 1 { "part" } else { "parts" };
    let beat_label = if total_beats == 1 { "beat" } else { "beats" };
    let subdivision = combined_subdivision_pattern(&source_parts).map_or_else(
        || {
            if source_parts
                .iter()
                .all(|part| part.subdivision_pattern().is_none())
            {
                "subdivision pattern: none".to_string()
            } else {
                "subdivision pattern: none because the sources differ".to_string()
            }
        },
        |pattern| format!("subdivision pattern: {pattern}"),
    );

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(s::S0)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(s::S4)
                .pb(s::S3)
                .child(div().text_color(s::TEXT_HEADER).child("combined part"))
                .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                    "{} {source_label}, {total_beats} {beat_label}",
                    sources.len()
                ))),
        )
        .child(
            div()
                .pb(s::S4)
                .text_color(s::TEXT_DEFAULT)
                .child(subdivision),
        )
        .child(
            selection_list::list(
                "combination-sources-scroll",
                "add parts from the left",
                rows,
            )
            .w_full()
            .debug_selector(|| "combine-selected-list".to_string()),
        )
}

fn part_list(
    parts: &[Part],
    selected_part: Option<&PartName>,
    cx: &mut Context<PartsWorkspace>,
) -> gpui::Div {
    let rows = parts
        .iter()
        .enumerate()
        .map(|(index, part)| part_list_row(index, part, selected_part == Some(&part.name), cx))
        .collect::<Vec<_>>();

    selection_list::list("parts-list-scroll", "no parts yet", rows)
        .w_full()
        .debug_selector(|| "parts-list-column".to_string())
}

fn part_list_row(
    index: usize,
    part: &Part,
    selected: bool,
    cx: &mut Context<PartsWorkspace>,
) -> gpui::Div {
    let part_name = part.name.clone();
    selection_list::row(index, selected, part.name.as_str().to_owned())
        .debug_selector(move || format!("part-list-row-{index}"))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |dialog, _: &MouseDownEvent, _: &mut Window, cx| {
                dialog.select_part(&part_name, cx);
            }),
        )
}

struct PartDetailsButtons {
    edit: Entity<Button>,
    duplicate: Entity<Button>,
    delete: Entity<Button>,
    add_to_arrangement: Entity<Button>,
}

fn part_details(
    part: Option<&Part>,
    buttons: PartDetailsButtons,
    sequence: &[PartName],
) -> gpui::Div {
    let PartDetailsButtons {
        edit,
        duplicate,
        delete,
        add_to_arrangement,
    } = buttons;
    let details = match part {
        Some(part) => {
            let occurrence_count = sequence
                .iter()
                .filter(|name| name.eq_ignore_ascii_case(&part.name))
                .count();
            let actions = div()
                .flex()
                .items_start()
                .debug_selector(|| "part-details-actions".to_string())
                .child(button::action_group([
                    div()
                        .debug_selector(|| "edit-part-control".to_string())
                        .child(edit),
                    div()
                        .debug_selector(|| "duplicate-part-control".to_string())
                        .child(duplicate),
                    div()
                        .debug_selector(|| "delete-part-control".to_string())
                        .child(delete),
                    div()
                        .debug_selector(|| "add-to-arrangement-control".to_string())
                        .child(add_to_arrangement),
                ]));
            let beat_label = if part.length == 1 { "beat" } else { "beats" };

            div()
                .flex()
                .flex_col()
                .flex_1()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .text_color(s::TEXT_DEFAULT)
                                .child(part.name.as_str().to_owned()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_color(s::TEXT_HEADER).child("length"))
                                .child(
                                    div()
                                        .text_color(s::TEXT_DEFAULT)
                                        .child(format!("{} {beat_label}", part.length)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_color(s::TEXT_HEADER).child("major subdivision"))
                                .child(
                                    div().text_color(s::TEXT_DEFAULT).child(
                                        part.major_subdivision()
                                            .map(|major| format!("{} beats", major.beats()))
                                            .unwrap_or_else(|| "none".to_string()),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_color(s::TEXT_HEADER)
                                        .child("subdivision pattern"),
                                )
                                .child(
                                    div().text_color(s::TEXT_DEFAULT).child(
                                        part.subdivision_pattern()
                                            .map(ToString::to_string)
                                            .unwrap_or_else(|| "none".to_string()),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(div().text_color(s::TEXT_HEADER).child("arrangement"))
                                .child(div().text_color(s::TEXT_DEFAULT).child(
                                    match occurrence_count {
                                        0 => "not used".to_string(),
                                        1 => "used once".to_string(),
                                        count => format!("used {count} times"),
                                    },
                                )),
                        ),
                )
                .child(actions)
        }
        None => div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(s::TEXT_DEFAULT)
            .child("add a part to get started"),
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .w(s::S0)
        .min_w(s::S0)
        .min_h(px(0.0))
        .debug_selector(|| "part-details-column".to_string())
        .child(details)
}

fn arrangement_panel(
    parts: &[Part],
    sequence: &[PartName],
    arrangement_range: Entity<RangeSelectionList>,
    move_earlier_button: Entity<Button>,
    move_later_button: Entity<Button>,
    arrangement_action_menu: Entity<ActionMenu>,
    arrangement_error: Option<String>,
) -> gpui::Div {
    let total_beats = sequence
        .iter()
        .filter_map(|name| find_part(parts, name))
        .map(|part| part.length)
        .sum::<u32>();
    let part_label = if sequence.len() == 1 { "part" } else { "parts" };
    let beat_label = if total_beats == 1 { "beat" } else { "beats" };

    let movement_actions =
        button::labeled_action_group("move", [move_earlier_button, move_later_button])
            .debug_selector(|| "arrangement-movement-actions".to_string());
    let occurrence_actions = div().debug_selector(|| "arrangement-occurrence-actions".to_string());
    let action_row = div()
        .flex()
        .items_start()
        .gap(s::S5)
        .debug_selector(|| "arrangement-actions".to_string())
        .child(movement_actions)
        .child(occurrence_actions.child(arrangement_action_menu));
    let actions = div().flex().flex_col().gap_3().pt(s::S5).child(action_row);
    let actions = if let Some(error) = arrangement_error {
        actions.child(error_message(error))
    } else {
        actions
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.0))
        .w(s::S9)
        .debug_selector(|| "arrangement-column".to_string())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .pb(s::S4)
                .child(div().text_color(s::TEXT_HEADER).child("arrangement"))
                .child(div().text_color(s::TEXT_DEFAULT).child(format!(
                    "{} {part_label}, {total_beats} {beat_label}",
                    sequence.len()
                ))),
        )
        .child(arrangement_range)
        .child(actions)
}

fn arrangement_rows(sequence: &[PartName]) -> Vec<Row> {
    sequence
        .iter()
        .enumerate()
        .map(|(index, part_name)| {
            Row::new(format!("{}. {}", index + 1, part_name.as_str()), "", "")
        })
        .collect()
}

fn sequence_with_inserted_part(
    sequence: &[PartName],
    part_name: PartName,
    selected_range: Option<SelectedRange>,
) -> (Vec<PartName>, SelectedRange) {
    let insertion_index = selected_range
        .filter(|range| range.last() < sequence.len())
        .map_or(sequence.len(), |range| range.last() + 1);
    let mut updated = sequence.to_vec();
    updated.insert(insertion_index, part_name);
    let selected = SelectedRange::new(insertion_index, insertion_index, updated.len())
        .expect("the inserted part must be selectable");
    (updated, selected)
}

fn sequence_with_moved_range(
    sequence: &[PartName],
    selected: SelectedRange,
    offset: isize,
) -> Option<(Vec<PartName>, SelectedRange)> {
    if selected.last() >= sequence.len() {
        return None;
    }
    let mut updated = sequence.to_vec();
    match offset {
        -1 if selected.first() > 0 => {
            updated[(selected.first() - 1)..=selected.last()].rotate_left(1);
            let moved =
                SelectedRange::new(selected.first() - 1, selected.last() - 1, updated.len())?;
            Some((updated, moved))
        }
        1 if selected.last() + 1 < updated.len() => {
            updated[selected.first()..=(selected.last() + 1)].rotate_right(1);
            let moved =
                SelectedRange::new(selected.first() + 1, selected.last() + 1, updated.len())?;
            Some((updated, moved))
        }
        _ => None,
    }
}

fn sequence_with_repeated_range(
    sequence: &[PartName],
    selected: SelectedRange,
) -> Option<(Vec<PartName>, SelectedRange)> {
    let repeated = selected_sequence(sequence, selected)?.to_vec();
    let repeated_first = selected.last() + 1;
    let mut updated = sequence.to_vec();
    updated.splice(repeated_first..repeated_first, repeated);
    let repeated_last = repeated_first + selected.last() - selected.first();
    let selected = SelectedRange::new(repeated_first, repeated_last, updated.len())?;
    Some((updated, selected))
}

fn sequence_with_removed_range(
    sequence: &[PartName],
    selected: SelectedRange,
) -> Option<(Vec<PartName>, Option<SelectedRange>)> {
    selected_sequence(sequence, selected)?;
    let mut updated = sequence.to_vec();
    updated.drain(selected.first()..=selected.last());
    let selected_range = (!updated.is_empty()).then(|| {
        let index = selected.first().min(updated.len() - 1);
        SelectedRange::new(index, index, updated.len()).expect("the remaining part is selectable")
    });
    Some((updated, selected_range))
}

fn selected_sequence(sequence: &[PartName], selected: SelectedRange) -> Option<&[PartName]> {
    sequence.get(selected.first()..=selected.last())
}

fn distinct_part_names(sources: &[PartName]) -> Vec<&PartName> {
    let mut distinct = Vec::<&PartName>::new();
    for source in sources {
        if distinct
            .iter()
            .all(|existing| !existing.eq_ignore_ascii_case(source))
        {
            distinct.push(source);
        }
    }
    distinct
}

fn next_variant_suffix(parts: &[Part], sources: &[PartName]) -> String {
    for number in 1_u32.. {
        let suffix = format!("v{number}");
        let available = distinct_part_names(sources).into_iter().all(|source| {
            let candidate = variant_part_name(source, &suffix);
            find_part(parts, &candidate).is_none()
        });
        if available {
            return suffix;
        }
    }
    "variant".to_string()
}

fn find_part<'a>(parts: &'a [Part], name: &PartName) -> Option<&'a Part> {
    parts
        .iter()
        .find(|part| part.name.eq_ignore_ascii_case(name))
}

fn parse_part_length(value: &str) -> Result<u32, String> {
    let length = value
        .trim()
        .parse::<u32>()
        .map_err(|_| "part length must be a whole number".to_string())?;
    if length == 0 {
        return Err("part length must be at least one beat".to_string());
    }

    Ok(length)
}

fn parse_subdivision_pattern(value: &str) -> Result<Option<SubdivisionPattern>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    value
        .parse::<SubdivisionPattern>()
        .map(Some)
        .map_err(|error| error.to_string())
}

fn parse_major_subdivision(value: &str) -> Result<Option<MajorSubdivision>, String> {
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
    use std::{cell::RefCell, rc::Rc};

    use gpui::{point, px, size, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext};

    use super::{
        combined_subdivision_pattern, next_variant_suffix, parse_part_length,
        parse_subdivision_pattern, sequence_with_inserted_part, sequence_with_moved_range,
        sequence_with_removed_range, sequence_with_repeated_range, ArrangementAction, DeleteDialog,
        PartsWorkspace, Request, View,
    };
    use crate::{
        part::{Part, PartName, SubdivisionPattern},
        style as s,
        view::{button, range_selection_list::SelectedRange},
    };

    #[gpui::test]
    fn add_click_validates_current_inputs_before_requesting_a_project_change(
        cx: &mut TestAppContext,
    ) {
        let (workspace, cx) =
            cx.add_window_view(|_, cx| PartsWorkspace::new(Vec::new(), Vec::new(), cx));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let received = requests.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&workspace, move |_, request: &Request, _| {
                let Request::Add {
                    name,
                    length,
                    subdivision_pattern,
                    major_subdivision,
                } = request
                else {
                    panic!("add should only request an addition");
                };
                received.borrow_mut().push((
                    name.clone(),
                    *length,
                    subdivision_pattern.clone(),
                    *major_subdivision,
                ));
            })
        });
        let open_add = cx.update(|_, cx| {
            let View::List(view) = &workspace.read(cx).view else {
                panic!("expected list");
            };
            view.add_new_button.clone()
        });
        open_add.update(cx, |_, cx| cx.emit(button::Clicked));
        let (name, length, pattern, major, add, cancel) = cx.update(|_, cx| {
            let View::Add {
                name,
                length,
                subdivision_pattern,
                major_subdivision,
                add_button,
                cancel_button,
                ..
            } = &workspace.read(cx).view
            else {
                panic!("click should open add form");
            };
            (
                name.clone(),
                length.clone(),
                subdivision_pattern.clone(),
                major_subdivision.clone(),
                add_button.clone(),
                cancel_button.clone(),
            )
        });
        name.update(cx, |input, cx| input.sync_value("bridge", cx));
        for (length_value, pattern_value, major_value) in
            [("0", "", ""), ("8", "4,0", ""), ("8", "4", "0")]
        {
            length.update(cx, |input, cx| input.sync_value(length_value, cx));
            pattern.update(cx, |input, cx| input.sync_value(pattern_value, cx));
            major.update(cx, |input, cx| input.sync_value(major_value, cx));
            add.update(cx, |_, cx| cx.emit(button::Clicked));
            cx.update(|_, cx| {
                let View::Add { form_error, .. } = &workspace.read(cx).view else {
                    panic!("invalid input should keep the form open");
                };
                assert!(form_error.is_some());
            });
            assert!(requests.borrow().is_empty());
        }
        major.update(cx, |input, cx| input.sync_value("", cx));
        add.update(cx, |_, cx| cx.emit(button::Clicked));
        let requests = requests.borrow();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0, "bridge");
        assert_eq!(requests[0].1, 8);
        assert_eq!(requests[0].2, Some(SubdivisionPattern::new([4]).unwrap()));
        assert_eq!(requests[0].3, None);
        drop(requests);
        cancel.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.update(|_, cx| assert!(matches!(workspace.read(cx).view, View::List(_))));
    }

    #[test]
    fn part_length_is_a_positive_whole_number() {
        assert_eq!(parse_part_length(" 16 ").unwrap(), 16);
        assert!(parse_part_length("").is_err());
        assert!(parse_part_length("0").is_err());
        assert!(parse_part_length("1.5").is_err());
    }

    #[test]
    fn subdivision_patterns_are_optional_positive_comma_separated_whole_numbers() {
        assert!(parse_subdivision_pattern("  ").unwrap().is_none());
        assert_eq!(
            parse_subdivision_pattern(" 4, 3,3 ")
                .unwrap()
                .unwrap()
                .subdivisions()
                .collect::<Vec<_>>(),
            [4, 3, 3]
        );
        assert!(parse_subdivision_pattern("4,,3").is_err());
        assert!(parse_subdivision_pattern("4, 0").is_err());
        assert!(parse_subdivision_pattern("4, 1.5").is_err());
    }

    #[test]
    fn combined_parts_keep_only_a_common_subdivision_pattern() {
        let common = SubdivisionPattern::new([4]).unwrap();
        let matching = vec![
            Part::new("intro", 8).with_subdivision_pattern(Some(common.clone())),
            Part::new("verse", 16).with_subdivision_pattern(Some(common.clone())),
        ];
        assert_eq!(combined_subdivision_pattern(&matching), Some(common));

        let mixed = vec![
            Part::new("intro", 8)
                .with_subdivision_pattern(Some(SubdivisionPattern::new([4]).unwrap())),
            Part::new("bridge", 7)
                .with_subdivision_pattern(Some(SubdivisionPattern::new([3, 4]).unwrap())),
        ];
        assert!(combined_subdivision_pattern(&mixed).is_none());
    }

    #[test]
    fn selected_parts_are_inserted_after_the_selected_range() {
        let sequence = names(["part-a", "part-b", "part-b"]);
        let selected_range = SelectedRange::new(0, 1, sequence.len());

        let (updated, selected) =
            sequence_with_inserted_part(&sequence, "bridge".into(), selected_range);

        assert_eq!(
            name_strings(&updated),
            ["part-a", "part-b", "bridge", "part-b"]
        );
        assert_eq!(selected, SelectedRange::new(2, 2, 4).unwrap());

        let (updated, selected) = sequence_with_inserted_part(&sequence, "bridge".into(), None);
        assert_eq!(
            name_strings(&updated),
            ["part-a", "part-b", "part-b", "bridge"]
        );
        assert_eq!(selected, SelectedRange::new(3, 3, 4).unwrap());
    }

    #[test]
    fn arrangement_ranges_can_move_repeat_and_be_removed() {
        let sequence = names(["part-a", "part-b", "bridge", "outro"]);
        let selected = SelectedRange::new(1, 2, sequence.len()).unwrap();

        let (moved, moved_selection) = sequence_with_moved_range(&sequence, selected, -1).unwrap();
        assert_eq!(
            name_strings(&moved),
            ["part-b", "bridge", "part-a", "outro"]
        );
        assert_eq!(moved_selection, SelectedRange::new(0, 1, 4).unwrap());
        let (moved, moved_selection) = sequence_with_moved_range(&sequence, selected, 1).unwrap();
        assert_eq!(
            name_strings(&moved),
            ["part-a", "outro", "part-b", "bridge"]
        );
        assert_eq!(moved_selection, SelectedRange::new(2, 3, 4).unwrap());
        assert!(
            sequence_with_moved_range(&sequence, SelectedRange::new(0, 1, 4).unwrap(), -1)
                .is_none()
        );
        assert!(
            sequence_with_moved_range(&sequence, SelectedRange::new(2, 3, 4).unwrap(), 1).is_none()
        );

        let (repeated, repeated_selection) =
            sequence_with_repeated_range(&sequence, selected).unwrap();
        assert_eq!(
            name_strings(&repeated),
            ["part-a", "part-b", "bridge", "part-b", "bridge", "outro"]
        );
        assert_eq!(repeated_selection, SelectedRange::new(3, 4, 6).unwrap());

        let (removed, remaining_selection) =
            sequence_with_removed_range(&sequence, selected).unwrap();
        assert_eq!(name_strings(&removed), ["part-a", "outro"]);
        assert_eq!(remaining_selection, SelectedRange::new(1, 1, 2));
    }

    #[test]
    fn variant_suffix_advances_past_existing_variant_names() {
        let parts = vec![
            Part::new("d1", 4),
            Part::new("d2", 4),
            Part::new("d1 v1", 4),
            Part::new("d2 v1", 4),
        ];
        assert_eq!(
            next_variant_suffix(&parts, &names(["d1", "d2", "d2"])),
            "v2"
        );
    }

    #[gpui::test]
    fn parts_workspace_renders_part_list_details_and_arrangement_columns(cx: &mut TestAppContext) {
        let parts = vec![
            Part::new("part-a", 16),
            Part::new("part-b", 8),
            Part::new("bridge", 12),
        ];
        let sequence = names(["part-a", "part-b", "part-b"]);
        let (dialog, cx) = cx.add_window_view(|_, cx| PartsWorkspace::new(parts, sequence, cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let part_list = cx.debug_bounds("parts-list-column").unwrap();
        let details = cx.debug_bounds("part-details-column").unwrap();
        let arrangement = cx.debug_bounds("arrangement-column").unwrap();
        let arrangement_list = cx.debug_bounds("arrangement-list").unwrap();
        let part_list_actions = cx.debug_bounds("part-list-actions").unwrap();
        let part_details_actions = cx.debug_bounds("part-details-actions").unwrap();
        let arrangement_actions = cx.debug_bounds("arrangement-actions").unwrap();
        let movement_actions = cx.debug_bounds("arrangement-movement-actions").unwrap();
        let occurrence_actions = cx.debug_bounds("arrangement-occurrence-actions").unwrap();

        assert!(part_list.size.width > px(0.0));
        assert!(details.size.width > px(0.0));
        assert!(arrangement.size.width > px(0.0));
        assert_eq!(arrangement_list.size.width, arrangement.size.width);
        assert!(part_list.origin.x < details.origin.x);
        assert!(details.origin.x < arrangement.origin.x);
        assert_eq!(part_list_actions.origin.x, part_list.origin.x);
        assert!(part_list.origin.y + part_list.size.height < part_list_actions.origin.y);
        assert_eq!(part_details_actions.origin.y, part_list_actions.origin.y);
        assert_eq!(
            part_list_actions.origin.y + part_list_actions.size.height,
            part_details_actions.origin.y + part_details_actions.size.height
        );
        assert_eq!(
            part_list_actions.origin.y + part_list_actions.size.height,
            arrangement_actions.origin.y + arrangement_actions.size.height
        );
        assert_eq!(movement_actions.origin.y, occurrence_actions.origin.y);
        assert!(movement_actions.origin.x < occurrence_actions.origin.x);
        assert!(
            occurrence_actions.origin.x + occurrence_actions.size.width
                <= arrangement.origin.x + arrangement.size.width,
            "movement {:?}, occurrence {:?}, arrangement {:?}",
            movement_actions,
            occurrence_actions,
            arrangement
        );
        assert!(
            (part_list.size.width / details.size.width - 1.0).abs() < 0.01,
            "part list {:?}, details {:?}, arrangement {:?}",
            part_list.size.width,
            details.size.width,
            arrangement.size.width
        );
        assert!(
            (details.size.width / arrangement.size.width - 1.0).abs() < 0.01,
            "part list {:?}, details {:?}, arrangement {:?}",
            part_list.size.width,
            details.size.width,
            arrangement.size.width
        );
        assert!(cx.debug_bounds("arrangement-list-row-2").is_some());
        let action_trigger = cx.debug_bounds("arrangement-action-menu-trigger").unwrap();
        cx.simulate_click(action_trigger.center(), Default::default());
        let action_menu = cx.debug_bounds("arrangement-action-menu-menu").unwrap();
        assert!(
            action_menu.origin.y + action_menu.size.height <= action_trigger.origin.y,
            "arrangement action menu should open upward: trigger {action_trigger:?}, menu {action_menu:?}"
        );
        assert!(
            action_menu.origin.y >= arrangement.origin.y,
            "arrangement action menu should stay inside the workspace: arrangement {arrangement:?}, menu {action_menu:?}"
        );
        cx.simulate_click(action_trigger.center(), Default::default());
        let add_to_arrangement = cx.debug_bounds("add-to-arrangement-control").unwrap();
        let edit_part = cx.debug_bounds("edit-part-control").unwrap();
        let duplicate_part = cx.debug_bounds("duplicate-part-control").unwrap();
        let delete_part_control = cx.debug_bounds("delete-part-control").unwrap();
        assert_eq!(edit_part.origin.y, duplicate_part.origin.y);
        assert_eq!(duplicate_part.origin.y, delete_part_control.origin.y);
        assert_eq!(delete_part_control.origin.y, add_to_arrangement.origin.y);
        assert!(delete_part_control.origin.x < add_to_arrangement.origin.x);
        assert!(
            add_to_arrangement.size.width < details.size.width
                && duplicate_part.size.width < details.size.width
        );
        assert!(
            add_to_arrangement.origin.x + add_to_arrangement.size.width
                <= details.origin.x + details.size.width,
            "part actions should stay inside details: actions {part_details_actions:?}, \
             add to arrangement {add_to_arrangement:?}, details {details:?}"
        );

        dialog.update(cx, |dialog, cx| {
            dialog.select_part(&PartName::new("bridge"), cx);
        });
        cx.run_until_parked();

        assert_eq!(
            cx.debug_bounds("parts-list-column").unwrap().size.width,
            part_list.size.width
        );
        assert_eq!(
            cx.debug_bounds("part-details-column").unwrap().size.width,
            details.size.width
        );
        assert_eq!(
            cx.debug_bounds("arrangement-column").unwrap().size.width,
            arrangement.size.width
        );

        let (delete_part, move_earlier, move_later) = cx.update(|_, cx| {
            let View::List(view) = &dialog.read(cx).view else {
                panic!("parts workspace should show its list view");
            };
            (
                view.delete_button.clone(),
                view.move_earlier_button.clone(),
                view.move_later_button.clone(),
            )
        });
        assert!(cx.debug_bounds("delete-part-control").is_some());
        assert!(cx.debug_bounds("delete-part-control").unwrap().size.width < details.size.width);
        assert!(!cx.update(|_, cx| delete_part.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| move_earlier.read(cx).is_disabled()));
        assert!(!cx.update(|_, cx| move_later.read(cx).is_disabled()));

        let last_occurrence = cx.debug_bounds("arrangement-list-row-2").unwrap();
        cx.simulate_click(last_occurrence.center(), Default::default());

        assert!(!cx.update(|_, cx| move_earlier.read(cx).is_disabled()));
        assert!(cx.update(|_, cx| move_later.read(cx).is_disabled()));

        dialog.update(cx, |dialog, cx| {
            dialog.select_part(&PartName::new("part-b"), cx);
        });
        cx.run_until_parked();

        assert!(cx.debug_bounds("delete-part-control").is_some());
        assert!(cx.debug_bounds("delete-part-control").unwrap().size.width < details.size.width);
        assert!(cx.update(|_, cx| delete_part.read(cx).is_disabled()));

        dialog.update(cx, |dialog, cx| {
            dialog.sequence_changed(Vec::new(), None, cx);
        });
        cx.run_until_parked();

        assert!(!cx.update(|_, cx| delete_part.read(cx).is_disabled()));
    }

    #[gpui::test]
    fn selected_arrangement_range_opens_append_variants_form(cx: &mut TestAppContext) {
        let parts = vec![Part::new("d1", 4), Part::new("d2", 4), Part::new("d3", 4)];
        let sequence = names(["d1", "d2", "d2", "d3"]);
        let (dialog, cx) = cx.add_window_view(|_, cx| PartsWorkspace::new(parts, sequence, cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let last = cx.debug_bounds("arrangement-list-row-3").unwrap();
        cx.simulate_click(
            last.center(),
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        );
        let action_menu = cx.update(|_, cx| {
            let View::List(view) = &dialog.read(cx).view else {
                panic!("parts workspace should show its list view");
            };
            view.arrangement_action_menu.clone()
        });
        action_menu.update(cx, |menu, cx| {
            menu.activate(ArrangementAction::AppendVariants.index(), cx);
        });

        cx.update(|_, cx| {
            let View::AppendVariants {
                sources, suffix, ..
            } = &dialog.read(cx).view
            else {
                panic!("append as variants should open its form");
            };
            assert_eq!(name_strings(sources), ["d1", "d2", "d2", "d3"]);
            assert_eq!(suffix.read(cx).value(), "v1");
        });
    }

    #[gpui::test]
    fn part_and_arrangement_lists_scroll_independently(cx: &mut TestAppContext) {
        let parts = (0..24)
            .map(|index| Part::new(format!("part-{index}"), 16))
            .collect::<Vec<_>>();
        let sequence = parts.iter().map(|part| part.name.clone()).collect();
        let (_, cx) = cx.add_window_view(|_, cx| PartsWorkspace::new(parts, sequence, cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let parts_list = cx.debug_bounds("parts-list-column").unwrap();
        let arrangement_list = cx.debug_bounds("arrangement-list").unwrap();
        let last_part_before = cx.debug_bounds("part-list-row-23").unwrap();
        let last_occurrence_before = cx.debug_bounds("arrangement-list-row-23").unwrap();

        cx.simulate_event(ScrollWheelEvent {
            position: parts_list.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            ..Default::default()
        });

        let last_part_after = cx.debug_bounds("part-list-row-23").unwrap();
        assert!(last_part_after.origin.y < last_part_before.origin.y);
        assert_eq!(
            cx.debug_bounds("arrangement-list-row-23").unwrap(),
            last_occurrence_before
        );

        cx.simulate_event(ScrollWheelEvent {
            position: arrangement_list.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            ..Default::default()
        });

        assert!(
            cx.debug_bounds("arrangement-list-row-23").unwrap().origin.y
                < last_occurrence_before.origin.y
        );
    }

    #[gpui::test]
    fn duplicate_part_action_opens_a_new_name_form(cx: &mut TestAppContext) {
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            PartsWorkspace::new(vec![Part::new("intro", 16)], Vec::new(), cx)
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let duplicate = cx.debug_bounds("duplicate-part-control").unwrap();
        cx.simulate_click(duplicate.center(), Default::default());

        cx.update(|_, cx| {
            let View::Duplicate { source, name, .. } = &dialog.read(cx).view else {
                panic!("duplicate action should open its form");
            };
            assert_eq!(source.as_str(), "intro");
            assert_eq!(name.read(cx).value(), "");
        });
    }

    #[gpui::test]
    fn combine_opens_an_empty_ordered_source_list(cx: &mut TestAppContext) {
        let parts = vec![Part::new("intro", 8), Part::new("verse", 16)];
        let sequence = names(["intro", "verse"]);
        let (dialog, cx) = cx.add_window_view(|_, cx| PartsWorkspace::new(parts, sequence, cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let combine = cx.update(|_, cx| {
            let View::List(view) = &dialog.read(cx).view else {
                panic!("parts workspace should show its list view");
            };
            view.combine_button.clone()
        });
        combine.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.run_until_parked();

        let name = cx.update(|_, cx| {
            let View::Combine {
                available_part,
                sources,
                selected_source,
                name,
                combine_button,
                ..
            } = &dialog.read(cx).view
            else {
                panic!("combine action should open its form");
            };
            assert_eq!(available_part.as_ref().map(PartName::as_str), Some("intro"));
            assert!(sources.is_empty());
            assert_eq!(*selected_source, None);
            assert_eq!(name.read(cx).value(), "");
            assert!(combine_button.read(cx).is_disabled());
            name.clone()
        });
        name.update(cx, |name, cx| name.sync_value("party", cx));
        cx.run_until_parked();

        let columns = cx.debug_bounds("combine-columns").unwrap();
        let available = cx.debug_bounds("combine-available-column").unwrap();
        let selected = cx.debug_bounds("combine-selected-column").unwrap();
        for selector in [
            "combine-form",
            "combine-available-list",
            "combine-available-actions",
            "combine-selected-list",
            "combine-selected-actions",
            "combine-workspace-actions",
        ] {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(
                bounds.size.width > px(0.0) && bounds.size.height > px(0.0),
                "{selector} should remain visible after editing the name: {bounds:?}"
            );
        }
        assert!(
            columns.size.height > px(0.0),
            "combine columns should be visible: {columns:?}"
        );
        assert!(
            columns.size.height >= s::S9,
            "the name field should leave a usable composer viewport: {columns:?}"
        );
        assert!(
            available.size.height > px(0.0),
            "available-parts column should be visible: {available:?}"
        );
        assert!(
            selected.size.height > px(0.0),
            "selected-parts column should be visible: {selected:?}"
        );
        assert_eq!(available.size.width, selected.size.width);
        assert!(available.origin.x < selected.origin.x);
        assert!(available.origin.x >= columns.origin.x);
        assert!(selected.origin.x + selected.size.width <= columns.origin.x + columns.size.width);
        assert!(cx.debug_bounds("combine-available-part-0").is_some());
        assert!(cx.debug_bounds("combination-source-0").is_none());
    }

    #[gpui::test]
    fn combination_sources_can_be_added_reordered_and_removed(cx: &mut TestAppContext) {
        let parts = vec![Part::new("intro", 8), Part::new("verse", 16)];
        let (dialog, cx) = cx.add_window_view(|_, cx| PartsWorkspace::new(parts, Vec::new(), cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let open_combine = cx.update(|_, cx| {
            let View::List(view) = &dialog.read(cx).view else {
                panic!("parts workspace should show its list view");
            };
            view.combine_button.clone()
        });
        open_combine.update(cx, |_, cx| cx.emit(button::Clicked));

        let add_source = cx.update(|_, cx| {
            let View::Combine {
                add_source_button, ..
            } = &dialog.read(cx).view
            else {
                panic!("combine action should open its form");
            };
            add_source_button.clone()
        });
        add_source.update(cx, |_, cx| cx.emit(button::Clicked));
        dialog.update(cx, |dialog, cx| {
            dialog.select_available_part(&PartName::new("verse"), cx);
        });
        add_source.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.run_until_parked();

        let (move_earlier, remove_source) = cx.update(|_, cx| {
            let View::Combine {
                sources,
                selected_source,
                move_source_earlier_button,
                remove_source_button,
                combine_button,
                ..
            } = &dialog.read(cx).view
            else {
                panic!("combine action should stay on its form");
            };
            assert_eq!(name_strings(sources), ["intro", "verse"]);
            assert_eq!(*selected_source, Some(1));
            assert!(!combine_button.read(cx).is_disabled());
            (
                move_source_earlier_button.clone(),
                remove_source_button.clone(),
            )
        });

        move_earlier.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let View::Combine {
                sources,
                selected_source,
                ..
            } = &dialog.read(cx).view
            else {
                panic!("combine action should stay on its form");
            };
            assert_eq!(name_strings(sources), ["verse", "intro"]);
            assert_eq!(*selected_source, Some(0));
        });

        remove_source.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let View::Combine {
                sources,
                selected_source,
                combine_button,
                ..
            } = &dialog.read(cx).view
            else {
                panic!("combine action should stay on its form");
            };
            assert_eq!(name_strings(sources), ["intro"]);
            assert_eq!(*selected_source, Some(0));
            assert!(combine_button.read(cx).is_disabled());
        });
    }

    #[gpui::test]
    fn long_combination_lists_stay_inside_the_workspace(cx: &mut TestAppContext) {
        let parts = (0..24)
            .map(|index| Part::new(format!("part-{index}"), 16))
            .collect::<Vec<_>>();
        let (dialog, cx) = cx.add_window_view(|_, cx| PartsWorkspace::new(parts, Vec::new(), cx));
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let open_combine = cx.update(|_, cx| {
            let View::List(view) = &dialog.read(cx).view else {
                panic!("parts workspace should show its list view");
            };
            view.combine_button.clone()
        });
        open_combine.update(cx, |_, cx| cx.emit(button::Clicked));
        cx.run_until_parked();

        let available_list = cx.debug_bounds("combine-available-list").unwrap();
        let actions = cx.debug_bounds("combine-workspace-actions").unwrap();
        for selector in [
            "combine-form",
            "combine-columns",
            "combine-available-column",
            "combine-available-list",
            "combine-selected-column",
            "combine-selected-list",
            "combine-workspace-actions",
        ] {
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(
                bounds.origin.y + bounds.size.height <= px(700.0),
                "{selector} should stay inside the workspace: {bounds:?}"
            );
        }

        let last_part_before = cx.debug_bounds("combine-available-part-23").unwrap();
        cx.simulate_event(ScrollWheelEvent {
            position: available_list.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-500.0))),
            ..Default::default()
        });
        let last_part_after = cx.debug_bounds("combine-available-part-23").unwrap();
        assert!(last_part_after.origin.y < last_part_before.origin.y);
        assert_eq!(
            cx.debug_bounds("combine-available-list").unwrap(),
            available_list
        );
        assert_eq!(
            cx.debug_bounds("combine-workspace-actions").unwrap(),
            actions
        );
    }

    #[gpui::test]
    fn edit_part_action_opens_a_form_with_the_current_configuration(cx: &mut TestAppContext) {
        let pattern = "4, 3, 3".parse().unwrap();
        let (dialog, cx) = cx.add_window_view(|_, cx| {
            PartsWorkspace::new(
                vec![Part::new("intro", 16).with_subdivision_pattern(Some(pattern))],
                Vec::new(),
                cx,
            )
        });
        cx.simulate_resize(size(px(1_200.0), px(700.0)));
        cx.run_until_parked();

        let edit = cx.debug_bounds("edit-part-control").unwrap();
        cx.simulate_click(edit.center(), Default::default());

        cx.update(|_, cx| {
            let View::Edit {
                source,
                name,
                subdivision_pattern,
                ..
            } = &dialog.read(cx).view
            else {
                panic!("edit action should open its form");
            };
            assert_eq!(source.as_str(), "intro");
            assert_eq!(name.read(cx).value(), "intro");
            assert_eq!(subdivision_pattern.read(cx).value(), "4, 3, 3");
        });
    }

    #[gpui::test]
    fn delete_dialog_owns_confirmation_controls_and_failure(cx: &mut TestAppContext) {
        let (dialog, cx) =
            cx.add_window_view(|_, cx| DeleteDialog::new(PartName::from("intro"), cx));
        cx.simulate_resize(size(px(800.0), px(700.0)));
        cx.run_until_parked();

        cx.update(|_, cx| {
            let dialog = dialog.read(cx);
            assert_eq!(dialog.name.as_str(), "intro");
            assert_ne!(dialog.cancel_button, dialog.confirm_button);
            assert!(dialog.error.is_none());
        });

        dialog.update(cx, |dialog, cx| {
            dialog.failed("couldn't delete part".to_string(), cx);
        });
        cx.run_until_parked();

        assert_eq!(
            cx.update(|_, cx| dialog.read(cx).error.clone()),
            Some("couldn't delete part".to_string())
        );
    }

    fn names<const N: usize>(names: [&str; N]) -> Vec<PartName> {
        names.into_iter().map(PartName::from).collect()
    }

    fn name_strings(names: &[PartName]) -> Vec<&str> {
        names.iter().map(PartName::as_str).collect()
    }
}
