# Config Modal Mode Selector

User can change the work source mode (specs/beads) from the config modal instead of editing config files manually.

## Slice 1: Editable mode dropdown with change warning

### User Behavior

The mode field in the config modal changes from read-only text to an interactive dropdown. When focused, it shows `< specs >` or `< beads >` and the user cycles between options with arrow keys (matching the LogLevel pattern). When the selected mode differs from the currently active mode, an inline warning appears: "Changing mode will reset your work panel". On save, the new mode is written to the config file and the work source is reconstructed.

### Acceptance Criteria

- [x] Add `Mode` variant to `ConfigModalField` enum, positioned after `KeepAwake` and before `SaveButton`
- [x] Add `mode_index: usize` to `TabFormState`, indexing into a `MODE_OPTIONS` constant (`["specs", "beads"]`)
- [x] `TabFormState::from_config()` and `from_partial_config()` initialize `mode_index` from config value
- [x] Field navigation (`next()`/`prev()`) includes `Mode` in the tab order
- [x] Arrow keys cycle through mode options when `Mode` is focused (same pattern as LogLevel)
- [x] Rendering follows LogLevel style: `< specs >` when focused (cyan), plain text otherwise
- [x] On project tab, mode respects inherited vs explicit field tracking (dark gray when inherited)
- [x] Inline warning text appears below the mode field when selected value differs from `app.config.behavior.mode`
- [x] `to_config()` and `to_partial_config()` include the selected mode value
- [x] Remove the old read-only mode rendering and "(set via .ralph or RALPH_MODE)" hint
- [x] No validation needed — options are hardcoded

### Technical Constraints

- Follow the LogLevel dropdown pattern exactly — `mode_prev()` / `mode_next()` methods with wrapping
- The warning is rendering-only — it doesn't block save
- `mark_explicit()` must be called when mode changes on the project tab
- Keep `RALPH_MODE` env var override working (applied after config load, independent of modal)

### Error Cases

- No new error cases — mode values are constrained to valid options by the dropdown
- Unknown mode values in existing config files still fall back to `"specs"` with a warning log (existing behavior, unchanged)

## Out of Scope

- Making `bd_path` editable in the modal
- Confirmation dialog or save-blocking behavior for mode changes
- Any changes to how the work source is reconstructed on config reload (already works)

## Dependencies

- [work-source-modes](work-source-modes.md) (Done)
- [config-modal](config-modal.md) (Done)
