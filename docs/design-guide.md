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

- use `CONTENT_PADDING` (`S5`) for padding around workspace and dialog content
  and actions.
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

## file imports

- use `view::file_import::file_import` for a project-owned file selected from
  outside the workspace.
- keep the current selection visible in a sunken field with choose or replace
  and remove actions beside it.
- give the sunken field and its action group `S4` padding and spacing so the
  field remains visually distinct from the raised buttons inside it.
- use the shared button component for file actions and disable remove when no
  file is selected.
- show concise identifying metadata in the field; report validation failures
  through the containing dialog or status area.

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
- the menu is at least as wide as its trigger, expands to keep concise option
  labels on one line, and scrolls vertically when its contents exceed `S9`.
- use the dropdown's capped-trigger variant in dense horizontal toolbars. It
  truncates a long selected label while keeping the arrow visible and the full
  option labels available in the menu.
- menu rows use the selection-list colors: `GREEN4` identifies the selected
  option, while hover uses the brighter `GREEN5` background and
  `TEXT_HOVERED` text.
- keep the option text concise; use a dialog or full selection list when the
  choices require supporting detail.

## action menus

- use `view::action_menu::ActionMenu` when several commands share one compact
  toolbar location and no command represents a persistent selection.
- label the trigger `actions` when its scope is already clear from the
  containing editor. Use short verb-first labels for menu items.
- keep unavailable commands visible and disabled so the menu remains stable
  and users can discover selection-dependent actions.
- action menus open toward the inside of their containing toolbar and use the
  same border, hover, height limit, and one-line labels as dropdown menus.

## data grids

- use `view::data_grid::editable` for an editable row-and-column table.
- the component owns scrolling, row numbering, column headers, cell dimensions,
  and raised/sunken table chrome.
- use the data grid's custom row-label input when the domain gives rows a
  structured position such as a beat subdivision; keep row labels concise and
  derived from domain state.
- give each independently scrollable grid view its own `DataGridScrollHandle`;
  grid rows have a uniform height and the component renders only the visible
  rows.
- use compact data-grid columns for short values of up to six monospaced
  characters; column headers truncate within that width.
- when actions operate on whole rows, select rows through the numbered row
  headers. Use click for one row and drag or shift-click for one contiguous
  range. Clicking the sole selected row header again clears the selection.
  Keep cell clicks independent from row selection. Row headers remain neutral
  gray controls: hover keeps the raised treatment and brightens only the
  numeral, while selection uses the shared sunken treatment with a bright
  numeral. Keep the score cells themselves visually unchanged.
- keep row actions in the editor header alongside other grid-level controls so
  they do not consume a separate row of vertical space. Keep compact actions
  on one line and cap or truncate flexible selectors before wrapping controls.
  When the command set outgrows a compact toolbar, put the complete set in a
  shared action menu rather than adding another toolbar row. Keep every menu
  item visible and disable actions that do not apply without a row selection.

## ordered input lists

- use `view::ordered_input_list::editable` for a one-dimensional sequence of
  domain values. Do not present a single value column as a data grid.
- the component owns its header, compact input alignment, error borders, and
  vertical scrolling without adding table headers or cell chrome.
- use `TEXT_HEADER` for the list header and align it with the inputs.
- when sequence position already identifies each value and the header makes
  their meaning clear, do not add redundant labels to individual inputs.

## selection lists

- use `view::selection_list::list` and `view::selection_list::row` for selectable
  resource lists.
- the component owns the sunken container, empty state, alternating row colors,
  and selected-row colors.
- use `view::range_selection_list::RangeSelectionList` when an ordered list
  selects one contiguous range rather than independent rows.
- a range selection starts with a single clicked row and extends by dragging,
  shift-clicking, or using shift with the up/down keys. Keep the entire
  selected span visible with the ordinary selection-list colors.
- the component uses an `S8`-high viewport by default and scrolls vertically
  when its rows exceed that space. Use its fill-height mode only inside a
  parent with an explicit bounded height.
- keep identifying content at the start of each range row. Put concise metadata
  in aligned columns to the right when it helps users confirm the boundaries.

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
- make an actionable error status explicit in its message and give the entire
  bar a pointing cursor. Use the action to reveal the affected input or retry
  the failed operation.
- when an editor continuously autosaves, keep routine save state in the status
  bar and offer manual retry only after a save failure instead of keeping a
  permanent save button in the editor.
- when an error belongs to a specific control or data cell, pair the status
  message with a local error highlight and make navigation available when
  helpful.

## project workspaces

- use one persistent workspace section for each primary project activity.
  Switching sections replaces the main content instead of opening a modal over
  another section.
- keep the workspace selector in the single-row project bar. Use fixed,
  concise labels and the shared depressed-button treatment to identify the
  selected section. Compose its buttons with `view::workspace::selector`.
- group workspace selectors, workspace controls, transport controls, and
  project-level actions by purpose. Use `S3` within a group and `S5` between
  groups.
- keep inactive workspace models alive so selection, scrolling, forms,
  validation feedback, and other unfinished state survive navigation.
- switching workspace sections must not save, apply, reset, or discard
  unfinished work. Block section switching only while a truly modal overlay is
  active.
- own a modal overlay in the narrowest workspace section that can invoke it.
  Keep its overlay type with that workspace module, and reserve project-level
  overlays for actions that apply across workspace sections.
- when closing a project would discard unfinished workspace forms or settings,
  use a project-level confirmation overlay rather than silently resetting those
  sections.
- use `view::workspace::tile` for a full-page project workspace with one raised
  gray surface on the green application background. Do not add a title bar or
  close button: the selected workspace control already identifies the content.
- use `view::workspace::list_detail` for resource management sections and
  `view::workspace::management_form` for their add, edit, duplicate, or combine
  forms.
- use `view::workspace::column_with_actions` to place controls that act on one
  column in a single toolbar directly beneath that column. Keep controls for
  the same operation adjacent and use a larger spacing token to distinguish
  separate control groups.
- a list/detail workspace may use an auxiliary third column for an ordered
  workflow that directly consumes the selected resource. Keep all three
  columns visible together and give them fixed equal widths so changing one
  column does not resize the others.

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

- use `S10` for standard dialog widths. Use `S11` for wider dialogs and pair it
  with an `S10` height when a bounded list/detail layout requires it.
- when independent settings sections would make a standard dialog excessively
  tall, use two equal columns in an `S11`-wide dialog. Keep general settings on
  the left, specialized settings on the right, and shared feedback and actions
  beneath both columns.
- when a workflow combines compact controls with a long range-selection list,
  use the wide dialog size with controls in a fixed left column and let the
  list fill the bounded right column. Keep actions beneath the controls.
- use `view::dialog::title_bar` for dialog title bars.
- use `view::dialog::error_message` for errors within dialog content.
- use `view::dialog::destructive_confirmation` for irreversible confirmation
  prompts. It groups the warning text and actions in a dark-red, sunken panel.
- when an irreversible confirmation is the only content in a standalone dialog,
  use `view::dialog::destructive_dialog`. Its dark-red body directly contains
  the warning and actions without an additional sunken panel.
- use `view::dialog::modal_overlay` for a modal dialog displayed over an active
  screen. Modal overlays block interaction with the screen beneath them.
