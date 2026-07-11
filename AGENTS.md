# Repository instructions

## UI work

- Read `docs/design-guide.md` before changing the interface.
- Use the tokens in `src/style.rs` and the shared components in `src/view`.
- Do not recreate a shared component or introduce a one-off color, text size, or
  content padding in screen code when the design system already expresses it.
- When adding a genuinely new reusable UI pattern, add it to `src/view` and
  document its rule in `docs/design-guide.md`.
