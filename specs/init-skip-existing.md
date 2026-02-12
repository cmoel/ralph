# Init Skip Existing

Init creates missing files and skips existing ones instead of blocking on conflicts.

## Slice 1: Skip Existing Files

### User Behavior

When the user presses `i` to initialize, the modal shows all files with their status. Files that already exist show as "Exists (skipped)" rather than blocking initialization. Pressing Initialize creates only the missing files and reports what it did.

If all files already exist, the Initialize button is disabled with a message like "Nothing to create."

### Acceptance Criteria

- [x] Initialize button is shown regardless of whether some files already exist
- [x] Existing files display as skipped (not as errors/conflicts)
- [x] Pressing Initialize creates only the missing files (existing files are untouched)
- [x] After initialization, a summary is shown: e.g., "Created 2 files, skipped 3 existing"
- [x] If all files already exist, the Initialize button is disabled with "Nothing to create" messaging
- [x] Conflict warning panel is removed

### Technical Constraints

- `create_files()` already skips files with `Conflict` status — no change needed there
- Remove the `has_conflicts()` guard on the Initialize button in `handle_init_modal_input`
- Update `draw_init_modal` in `modal_ui.rs` to remove the conflict warning panel and always show the Initialize button
- Change file status rendering: `Conflict` → show a neutral skip indicator instead of red ✗
- `InitFileStatus::Conflict` could be renamed to `Exists` for clarity
- If all files have `Exists` status, disable the Initialize button (no files to create)

### Error Cases

- Write failure on a specific file → show error in modal, files already created are kept (same as today)
- All files exist → Initialize button disabled, user sees "Nothing to create"
