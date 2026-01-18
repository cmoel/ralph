# Config Modal

Users can view and edit Ralph's configuration through an in-app form, preventing syntax errors and providing validation feedback.

## Overview

Press `c` to open a modal form that edits the config file directly. This is the blessed path for configuration — users cannot make TOML syntax errors, and validation catches issues before saving.

**Replaces:** The previous `$EDITOR` integration is removed. Users who prefer to edit the file directly in their editor can still do so, but errors will be surfaced in the error UI.

## Dependencies

**Library:** [rat-widget](https://github.com/thscharler/rat-salsa) (v3.x)
- Provides: text input, choice/dropdown, buttons, modal dialog, focus management
- Add to Cargo.toml: `rat-widget`, `rat-salsa` (for event handling)

**Note:** rat-widget has 44 GitHub stars. If the library becomes unmaintained, fallback options include tui-textarea + custom form logic, or deferring to external editor.

## Slice 1: Basic Modal with Read-Only Display

### User Behavior

Press `c` to open a modal showing current configuration values (read-only). Press `Esc` to close.

```
╭──────────────────────── Configuration ─────────────────────────╮
│                                                                │
│  Config file: ~/.config/ralph/config.toml                      │
│  Log directory: ~/.local/state/ralph/                          │
│                                                                │
│  ──────────────────────────────────────────────────────────    │
│                                                                │
│  Claude CLI path:    ~/.claude/local/claude                    │
│  Claude CLI args:    --output-format=stream-json --verbose...  │
│  Prompt file:        ./PROMPT.md                               │
│  Specs directory:    ./specs                                   │
│  Log level:          info                                      │
│                                                                │
│                           [Close]                              │
│                                                                │
╰────────────────────────────────────────────────────────────────╯
```

This slice establishes the modal infrastructure before adding editing.

### Acceptance Criteria

- [x] ~~Add `rat-widget` and `rat-salsa` dependencies to Cargo.toml~~ (Blocked: rat-salsa 3.1 requires crossterm 0.28, but ralph uses crossterm 0.29. Using ratatui native widgets instead.)
- [x] Press `c` opens modal (only when `AppStatus` is not `Running`)
- [x] Modal displays config file path (read-only)
- [x] Modal displays log directory path (read-only)
- [x] Modal displays all current config values (read-only)
- [x] `Esc` or `Close` button dismisses modal
- [x] Modal is centered and sized appropriately (using existing `centered_rect` helper)

### Technical Constraints

- Use ratatui's native `Block`, `Paragraph`, and `Clear` widgets for modal (fallback from rat-widget due to crossterm version incompatibility)
- Store `show_config_modal: bool` in `App` state
- Block other input while modal is open
- Read values from `app.config`

### Error Cases

- None for this slice

## Slice 2: Editable Form Fields

### User Behavior

The modal now contains editable form fields instead of read-only display.

```
╭──────────────────────── Configuration ─────────────────────────╮
│                                                                │
│  Config file: ~/.config/ralph/config.toml                      │
│  Log directory: ~/.local/state/ralph/                          │
│                                                                │
│  ──────────────────────────────────────────────────────────    │
│                                                                │
│  Claude CLI path:                                              │
│  [~/.claude/local/claude____________________________]          │
│                                                                │
│  Claude CLI args:                                              │
│  [--output-format=stream-json --verbose --print...]            │
│                                                                │
│  Prompt file:                                                  │
│  [./PROMPT.md_______________________________________]          │
│                                                                │
│  Specs directory:                                              │
│  [./specs___________________________________________]          │
│                                                                │
│  Log level:                                                    │
│  [info ▼]                                                      │
│                                                                │
│                    [Save]     [Cancel]                         │
│                                                                │
╰────────────────────────────────────────────────────────────────╯
```

**Navigation:**
- `Tab` / `Shift+Tab` — move between fields
- Arrow keys — navigate within field or dropdown options
- `Enter` on Save — save and close
- `Enter` on Cancel or `Esc` — close without saving

### Acceptance Criteria

- [x] Claude CLI path: text input field, pre-filled with current value
- [x] Claude CLI args: text input field, pre-filled with current value
- [x] Prompt file: text input field, pre-filled with current value
- [x] Specs directory: text input field, pre-filled with current value
- [x] Log level: dropdown with options: trace, debug, info, warn, error
- [x] Tab/Shift+Tab navigates between fields
- [x] Focus visually indicated on current field (cyan label, cursor block)
- [x] Save button saves to config file and closes modal
- [x] Cancel button closes modal without saving
- [x] Esc closes modal without saving

### Technical Constraints

- ~~Use `rat-widget::text_input::TextInput` for text fields~~ (Using custom text input with ratatui native widgets)
- ~~Use `rat-widget::choice::Choice` or similar for dropdown~~ (Using custom < value > cycling with left/right arrows)
- ~~Use `rat-widget::button::Button` for buttons~~ (Using styled Span with Enter key activation)
- ~~Use `rat-focus::FocusFlag` for focus management~~ (Using `ConfigModalField` enum with `focus` field)
- [x] Initialize fields with values from `app.config`
- [x] On Save, serialize to TOML and write to `app.config_path`

### Error Cases

- **Config file write fails**: Show error in modal, don't close
- **TOML serialization fails**: Should not happen with typed struct, but log error

## Slice 3: Validation with Inline Errors

### User Behavior

Fields are validated on blur (when focus leaves the field). Invalid fields show inline error messages. The Save button is disabled while any validation errors exist.

```
╭──────────────────────── Configuration ─────────────────────────╮
│                                                                │
│  Config file: ~/.config/ralph/config.toml                      │
│  Log directory: ~/.local/state/ralph/                          │
│                                                                │
│  ──────────────────────────────────────────────────────────    │
│                                                                │
│  Claude CLI path:                                              │
│  [/usr/bin/does-not-exist_______________________]              │
│  ⚠ File not found                                              │
│                                                                │
│  Claude CLI args:                                              │
│  [--output-format=stream-json --verbose --print...]            │
│                                                                │
│  Prompt file:                                                  │
│  [./PROMPT.md_______________________________________]          │
│                                                                │
│  Specs directory:                                              │
│  [./nonexistent_____________________________________]          │
│  ⚠ Directory not found                                         │
│                                                                │
│  Log level:                                                    │
│  [info ▼]                                                      │
│                                                                │
│                    [Save]     [Cancel]                         │
│                    (disabled)                                  │
│                                                                │
╰────────────────────────────────────────────────────────────────╯
```

**Validation rules:**
- **Claude CLI path**: File must exist AND be executable
- **Claude CLI args**: No validation (free-form)
- **Prompt file**: File must exist
- **Specs directory**: Directory must exist
- **Log level**: Must be one of the valid options (enforced by dropdown)

### Acceptance Criteria

- [x] Validation runs on blur (when Tab away from field)
- [x] Claude CLI path: validates file exists and is executable
- [x] Prompt file: validates file exists
- [x] Specs directory: validates directory exists
- [x] Invalid fields show warning icon (⚠) and error message below
- [x] Error message text is yellow/orange colored
- [x] Save button disabled when any validation errors exist
- [x] Save button visually indicates disabled state (dimmed)
- [x] Validation errors clear when field value changes

### Technical Constraints

- Store validation state per field: `HashMap<FieldName, Option<String>>` for errors
- Expand `~` before validating paths
- Use `std::fs::metadata()` for existence checks
- Use `std::os::unix::fs::PermissionsExt` for executable check on Unix
- Re-validate on each keystroke or on blur (blur preferred for performance)

### Error Cases

- **Path expansion fails**: Show "Invalid path" error
- **Permission check fails**: Show "Cannot access file" error

## Slice 4: Remove $EDITOR Integration

### User Behavior

The `$EDITOR` integration is removed. The `c` key now exclusively opens the config modal.

If users edit the config file externally and introduce errors, Ralph shows the error in the status panel (existing behavior from config auto-reload).

### Acceptance Criteria

- [ ] Remove `open_config_in_editor()` function
- [ ] Remove `$VISUAL` / `$EDITOR` / `vi` fallback logic
- [ ] Remove editor-related error handling (`NoEditor`, `SpawnFailed`, etc.)
- [ ] `c` key only opens config modal
- [ ] Footer hint remains `[c] Config`
- [ ] External config edits still trigger auto-reload (existing behavior)
- [ ] Invalid external edits show warning in status panel (existing behavior)

### Technical Constraints

- Remove from `src/main.rs`: `open_config_in_editor()`, `ConfigEditResult` enum
- Keep config file watching and auto-reload functionality

### Error Cases

- None (removing code)

## Dependencies

- Slice 1: None
- Slice 2: Slice 1
- Slice 3: Slice 2
- Slice 4: Slice 3 (remove old code after new form works)

## Out of Scope

- Config file syntax highlighting
- Config file backup before save
- Undo/redo within form fields (rat-widget may provide this)
- Multi-line text editing for args field
- Custom keybindings for form navigation
- Theming/styling customization for the modal
