# ahess design guide

The design system is implemented in `src/style.rs` and the shared components in
`src/view`. Screens should use those tokens and components instead of recreating
their styles locally.

## text

- do not capitalize english words by default in the interface.
- keep product or project names in their chosen casing.
- use `TEXT_SIZE` for all interface text.
- do not use bold text.
- use `TEXT_DEFAULT` for ordinary content text.
- use `TEXT_HEADER` for headers. Header text is slightly darker than content
  text, not larger, brighter, or bolder.
- use `FIELD_LABEL_TEXT` for field-group labels and `BUTTON_TEXT` for ordinary
  button labels.
- use `DIALOG_TITLE_TEXT` for text in a dialog title bar. It is the same dark
  gray as the panel surrounding the title bar.

## spacing

- use `CONTENT_PADDING` (`S5`) for padding around dialog content and actions.
- use the spacing scale in `src/style.rs`; do not introduce an arbitrary value
  when an existing spacing token expresses the intended spacing.

## fields

- use `view::field_group::field_group` for text fields.
- arrange the label above the input.
- style field labels with `FIELD_LABEL_TEXT`.

## dialogs

- use `view::dialog::title_bar` for dialog title bars.
- use `view::dialog::error_message` for errors within dialog content.
- use `view::dialog::destructive_confirmation` for irreversible confirmation
  prompts. It groups the warning text and actions in a dark-red, sunken panel.
- use `view::dialog::modal_overlay` for a modal dialog displayed over an active
  screen. Modal overlays block interaction with the screen beneath them.
