# ahess design guide

This guide records the reusable visual language, interaction principles, and
component contracts for ahess. The design system is implemented in
`src/style.rs` and the shared components in `src/view`; screens should compose
those tokens and components instead of recreating their styles locally.

Keep feature behavior and the requirements of individual screens out of this
guide. Add a rule here when it should apply consistently to multiple features
or when it defines how a shared component is used.

## text

- do not capitalize english words by default in the interface.
- keep proper names in their chosen casing.
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
- use `view::field_group::control_group` for labeled non-text controls such as
  dropdowns.
- use `view::field_group::compact_control_group` for short controls such as
  dropdowns; it caps them at `S9` instead of stretching them across a wide
  editor or dialog.
- arrange the label above the input.
- style field labels with `FIELD_LABEL_TEXT`.

## buttons

- use the shared `view::button::Button` disabled state for unavailable actions
  that must keep a stable position. Disabled buttons use the lighter `GRAY3`
  background, do not react to hover or presses, and cannot become depressed.
- use shared square buttons for compact, familiar symbols. When multiple square
  buttons express directions within a toolbar, give the group a nearby text
  label rather than repeating text in every button.

## dropdowns

- use `view::dropdown::Dropdown` for a compact choice among a short list of
  mutually exclusive options.
- the trigger displays the selected option and the menu uses a solid `GRAY1`
  border.
- menu rows use the selection-list colors: `GREEN4` identifies the selected
  option, while hover uses the brighter `GREEN5` background and
  `TEXT_HOVERED` text.
- keep the option text concise; use a dialog or full selection list when the
  choices require supporting detail.

## data grids

- use `view::data_grid::editable` for an editable row-and-column table.
- the component owns scrolling, row numbering, column headers, cell dimensions,
  and raised/sunken table chrome.
- give each independently scrollable grid view its own `ScrollHandle`.

## selection lists

- use `view::selection_list::list` and `view::selection_list::row` for selectable
  resource lists.
- the component owns the sunken container, empty state, alternating row colors,
  and selected-row colors.

## status bars

- use `view::status_bar::bar` for a fixed-height status area at the bottom of a
  workspace.
- keep the bar present in its neutral, blank state so status changes never
  reflow the workspace.
- place workspace status bars outside content padding so they span the full
  width and meet the bottom and side edges of the window.
- separate the bar from workspace content with a top border only; do not frame
  its left, right, or bottom edges.
- use the neutral message state for routine, non-actionable feedback; it keeps
  the ordinary status-bar colors.
- use the warning state for conditions that need attention but do not prevent
  continued work. Use the error state for invalid input and failed operations.
- when an error belongs to a specific control or data cell, pair the status
  message with a local error highlight and make navigation available when
  helpful.

## workspace resource editors

- use a full-page workspace for reusable resources that need creation,
  selection, detailed editing, and deletion outside any one project.
- contain the editor in `view::workspace_tile::tile`, a single raised gray tile
  with a title bar. Sunken lists and grids belong inside this tile rather than
  directly on the green application background.
- keep the resource selection list in the left column and the selected details
  or edit form in the larger right column.
- when a resource combines scalar settings with an editable collection, keep
  the scalar fields in the left side of the detail area and give the collection
  its own full-height column on the right.
- keep resource-level delete, cancel, and save actions together in the scalar
  settings column footer, aligned to its right edge. Use a larger gap before
  separating the destructive action from the non-destructive group. Collection
  row actions stay directly beneath their collection and align to its right
  edge. Parallel column footers share the same bottom edge.
- keep navigation in the application frame and routine feedback in the fixed
  workspace status bar so editor content does not reflow.
- show built-in resources as read-only and offer duplication when they are
  useful starting points for user-owned resources.

## dialogs

- use `S10` for standard dialog widths. Use `S11` by `S10` for the wider
  list/detail management dialogs.
- use `view::dialog::title_bar` for dialog title bars.
- use `view::dialog::error_message` for errors within dialog content.
- use `view::dialog::destructive_confirmation` for irreversible confirmation
  prompts. It groups the warning text and actions in a dark-red, sunken panel.
- use `view::dialog::list_detail_dialog` and
  `view::dialog::management_form_dialog` for management dialogs that pair a
  selectable resource list and detail view with an add form.
- use `view::dialog::column_with_actions` to place controls that act on one
  column in a single horizontal toolbar directly beneath that column's content.
  Keep controls for the same operation adjacent and use a larger spacing token
  to distinguish separate control groups within the toolbar.
- a list/detail management dialog may use its auxiliary third column for an
  ordered workflow that directly consumes the selected resource. Keep the
  resource list, details, and workflow visible together instead of opening a
  second management dialog. When present, all three columns have fixed equal
  widths; changing content in one column must not resize the others.
- use `view::dialog::modal_overlay` for a modal dialog displayed over an active
  screen. Modal overlays block interaction with the screen beneath them.
