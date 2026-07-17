# Repository instructions

## UI work

- Read `docs/design-guide.md` before changing the interface.
- Use the tokens in `src/style.rs` and the shared components in `src/view`.
- Do not recreate a shared component or introduce a one-off color, text size, or
  content padding in screen code when the design system already expresses it.
- When adding a genuinely new reusable UI pattern, add it to `src/view` and
  document its rule in `docs/design-guide.md`.

## State modeling

- Make impossible states unrepresentable whenever practical.
- Model mutually exclusive states with enums rather than combinations of
  booleans.
- Use `Option<T>` when a value is meaningful only under a particular condition;
  do not store a separate boolean alongside conditionally meaningful state.
- Prefer domain-specific types and validated constructors over primitive values
  that permit invalid data.
- Derive UI visibility and enabled states from the underlying domain state
  instead of maintaining duplicated flags.
- Validate untrusted data at system boundaries, then use validated types
  internally.
- When changing stateful code, consider whether the type system can eliminate
  invalid cases before adding runtime checks.
