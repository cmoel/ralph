# Tasks Panel

Dedicated panel displaying the current task list, updating in place rather than appending to the main output.

## Slice 1: Tasks Panel Foundation

### User Behavior

When Claude calls `TodoWrite`, tasks appear in a dedicated Tasks panel between the Output panel and Command panel. The main output no longer shows task blocks—it remains focused on Claude's text output and tool summaries.

The Tasks panel:
- Has a dynamic max height based on screen size (agent chooses reasonable proportion; main output remains the hero)
- Scrolls vertically when tasks exceed max height
- Displays tasks with existing status indicators: `▶` (in_progress), `○` (pending), `✓` (completed)
- Shows in-progress tasks using `activeForm` text; pending/completed use `content`
- Collapses to a single title line when no tasks exist
- Clears and collapses when the session enters `Stopped` state

Task changes are logged for history preservation (since they no longer appear in main output).

### Acceptance Criteria

- [x] Three-panel vertical layout: Output (flexible) → Tasks (dynamic, max height) → Command (fixed 3 lines)
- [x] Tasks stored in `App` state (e.g., `Vec<TodoItem>`) instead of appending to `output_lines`
- [x] `TodoWrite` events update the stored tasks (replace, not append)
- [x] Tasks panel renders current tasks with status indicators
- [x] Panel scrolls when tasks exceed max height
- [x] Panel collapses to single title line (`━━━ Tasks ━━━`) when no tasks
- [x] Panel clears and collapses when session stops
- [x] Task changes logged at info level (task count, status summary)

### Technical Constraints

- Follow existing panel patterns from `ui.rs` (Block, Paragraph, Layout)
- Use existing `TodoItem` and `TodoStatus` types from `ui.rs`
- Reuse scrolling logic pattern from main output panel
- Max height calculation should leave majority of space for main output (e.g., 20-30% max for tasks)

### Error Cases

- Empty `todos` array in `TodoWrite`: Clear tasks, collapse panel
- Malformed JSON: Log error, preserve existing tasks (don't clear on parse failure)
- Very small screen: Ensure minimum usable height for all three panels

---

## Slice 2: Panel Selection System

(depends on Slice 1)

### User Behavior

Users can switch focus between the Main (Output) panel and Tasks panel using `Tab`. The selected panel has a visual indicator (e.g., brighter border or different border style). Scroll controls (`j`/`k`, arrow keys, `Ctrl+u`/`Ctrl+d`, `Ctrl+b`/`Ctrl+f`, mouse wheel) operate on the selected panel.

On app launch, the Main panel is selected by default. Selection state is not persisted.

### Acceptance Criteria

- [x] `Tab` key toggles focus between Main and Tasks panels
- [x] Visual indicator shows which panel is selected (e.g., brighter border color or different style)
- [x] `j`/`k`, arrow keys scroll the selected panel
- [x] `Ctrl+u`/`Ctrl+d` (half page), `Ctrl+b`/`Ctrl+f` (full page) work on selected panel
- [x] Mouse wheel scrolls the selected panel
- [x] Main panel selected by default on launch
- [x] Selection state stored in `App` (e.g., `selected_panel: Panel` enum)

### Technical Constraints

- [x] Add `Panel` enum (e.g., `Main`, `Tasks`) to represent selectable panels
- [x] Each panel needs its own `scroll_offset` and potentially `is_auto_following` state
- [x] Refactor scroll handling to operate on the selected panel's state

### Error Cases

- [x] Tab when Tasks panel is collapsed: Still toggles selection (panel can be selected while collapsed)
- [x] Scrolling Tasks panel when no tasks: No-op (nothing to scroll)

---

## Slice 3: Manual Collapse & Count Display

(depends on Slice 1)

### User Behavior

Users can manually collapse or expand the Tasks panel with the `t` key. When collapsed and tasks exist, the title line shows the completion count: `━━━ Tasks [3/7] ━━━` where 3 is completed and 7 is total. In-progress tasks are not counted as "done."

Manual collapse state is remembered until tasks are cleared (session stops).

### Acceptance Criteria

- [x] `t` key toggles Tasks panel between collapsed and expanded
- [x] Collapsed state with tasks shows: `━━━ Tasks [3/7] ━━━`
- [x] Count format: `[completed/total]` where in-progress counts toward total but not completed
- [x] Manual collapse state tracked in `App` (e.g., `tasks_panel_collapsed: bool`)
- [x] Collapse state resets to auto-managed when tasks clear

### Technical Constraints

- [x] `t` key only affects Tasks panel (not a panel selection toggle)
- [x] Count calculation: `completed = tasks.filter(status == Completed).count()`, `total = tasks.len()`

### Error Cases

- [x] `t` when no tasks: No-op (panel already collapsed, nothing to toggle)
- [x] All tasks completed then new tasks arrive: Respect current collapse state

---

## Slice 4: Auto-expand Configuration

(depends on Slice 3)

### User Behavior

The `auto_expand_tasks_panel` configuration setting (default: `true`) controls whether the Tasks panel automatically expands when tasks arrive. When `true`, receiving tasks expands the panel. When `false`, the panel stays collapsed and shows the count; users must press `t` to expand.

The setting appears in the config modal as a toggle.

### Acceptance Criteria

- [x] `auto_expand_tasks_panel` added to TOML config (default: `true`)
- [x] Setting respected when `TodoWrite` event received with non-empty tasks
- [x] Config modal includes toggle for this setting
- [x] Manual `t` toggle works regardless of this setting

### Technical Constraints

- [x] Add field to `Config` struct in config handling code
- [x] Follow existing config modal patterns for boolean toggles
- [x] Default to `true` for backwards-compatible behavior (tasks visible by default)

### Error Cases

- [x] Config file missing this field: Default to `true`
- [x] Setting changed while tasks visible: No immediate effect (applies to next task arrival)

---

## Out of Scope

- Persisting panel selection state across sessions
- Filtering or searching tasks
- Task history view (beyond logging)
- Multiple task lists or task grouping
