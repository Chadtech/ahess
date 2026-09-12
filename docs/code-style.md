# Rust code style

## Imports

- Keep crate-root paths in `use` declarations instead of repeating them in
  types and expressions throughout a file.
- Import types directly when their names are clear, such as `VoiceId` and
  `AttackSharpness`.
- Import modules for related functions and events where the qualifier provides
  useful context, such as `score_cell::parse`, `text_input::DetailsRequested`,
  and `detail_marker::corner`.
- Use an alias or a short module qualifier when needed to distinguish names.

## Pattern matching

- Use explicit `match` expressions instead of the `matches!` macro.
- Spell out enum variants when practical so adding a variant requires reviewing
  the decision.

## Scope and naming

- Name parameters for what they currently represent. For example, `columns`
  describes a selected set of score columns; introduce a general filter type
  when additional filtering requirements make that abstraction useful.
