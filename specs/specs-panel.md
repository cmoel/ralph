# Specs Panel

Read-only panel to browse all specs, with blocked specs prominently highlighted.

## Slice 1: Specs List Panel

### User Behavior

User presses `l` to open the Specs panel. A modal overlay appears showing all specs from `specs/README.md`, sorted by importance: Blocked specs first (they need attention), then Ready, then In Progress, then Done. Within each status group, specs are sorted newest first (by file creation timestamp).

When blocked specs exist, a bold warning banner appears at the top demanding attention.

User navigates with up/down arrows. The list scrolls when selection moves off-screen. Press `Esc` to close.

### Acceptance Criteria

- [x] `l` opens the Specs panel (available in all states except when another modal is open)
- [x] `Esc` closes the panel
- [x] Panel parses `specs/README.md` for all specs and their statuses
- [x] Specs sorted by status: Blocked → Ready → In Progress → Done
- [x] Within each status, sorted by file creation timestamp (newest first)
- [x] Up/down arrows navigate the list
- [x] List scrolls when selection moves beyond visible area
- [x] Color-coded statuses:
  - Blocked: Red
  - Ready: Cyan
  - In Progress: Green
  - Done: Dim gray
- [x] Selected spec highlighted (inverted colors or similar)
- [x] When blocked specs exist, bold warning banner appears:
  ```
  ██████████████████████████████████████████████████████████████
  ██  ⚠ N BLOCKED SPECS - ACTION REQUIRED                    ██
  ██████████████████████████████████████████████████████████████
  ```
- [x] Banner uses high-contrast colors (red background, white/yellow text)

### Technical Constraints

- Follow the existing modal pattern from config modal (centered overlay, `Clear` widget)
- Use `fs::metadata().created()` for file creation timestamp
- Panel size similar to config modal (~70×24 or adjust as needed)
- Parse README table format: `| [spec-name](spec-name.md) | Status | ... |`

### Error Cases

- README missing or unreadable → show error message in panel body
- No specs found in README → show "No specs found" message
- File creation time unavailable → fall back to modification time, or sort to end

## Slice 2: Spec Preview Pane

(depends on Slice 1)

### User Behavior

The panel splits into two sections: spec list on top, preview on bottom. When a spec is selected, the preview pane shows the head of that spec's markdown file. The preview is static (not scrollable) - it shows what fits in the available space.

### Acceptance Criteria

- [x] Panel layout splits: list (top ~40%), preview (bottom ~60%)
- [x] Preview shows head of selected spec file (first N lines that fit)
- [x] Preview updates when selection changes
- [x] Preview displays raw markdown (no rendering needed)
- [x] Horizontal separator between list and preview

### Technical Constraints

- Only read the head of the spec file (not entire file)
- Preview is truncated, not scrollable
- Use monospace display for markdown content

### Error Cases

- Spec file missing → show "File not found" in preview pane
- Spec file unreadable → show error message in preview pane
