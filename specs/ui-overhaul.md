# UI Overhaul

Redesign Ralph's layout for better information hierarchy and visual feedback.

## Overview

**Current layout:**
```
╭─ Status Panel (cramped) ────────────────────────────────╮
│ ● RUNNING  Session: d5cc63  Spec: -  Logs: ~/Library... │
╰─────────────────────────────────────────────────────────╯
╭─ Output ────────────────────────────────────────────────╮
│ ...                                                     │
╰─────────────────────────────────────────────────────────╯
[s] Start  [c] Config  [q] Quit
```

**New layout:**
```
╭─ d5cc63 ──────────────────────────────── configuration ─╮
│ I'll start by reading the specs README...               │▲
│ [Tool: Read] /Users/cmoel/Desktop/ralph/specs/README.md ││
│ ...                                                     │▼
╰─────────────────────────────────────────────────────────╯
╭─────────────────────────────────────────────────────────╮
│ [s] Start  [c] Config  [q] Quit              ● 2:34    │
╰─────────────────────────────────────────────────────────╯
```

**Key changes:**
- Session ID becomes output panel's left title
- Current spec becomes output panel's right title (when known)
- Status moves to command panel (right-aligned)
- Elapsed timer replaces static "RUNNING" text
- Border color/style changes based on state
- Scrollbar added to output panel
- Old status panel removed entirely

## Slice 1: Layout Restructure

### User Behavior

The UI is reorganized into two panels:
1. **Output panel** — Streaming content with session ID as title
2. **Command panel** — Keyboard shortcuts (left) and status (right)

Session ID is generated at app start and displayed immediately.

**Stopped state:**
```
╭─ a1b2c3 ────────────────────────────────────────────────╮
│                                                         │
│                                                         │
╰─────────────────────────────────────────────────────────╯
╭─────────────────────────────────────────────────────────╮
│ [s] Start  [c] Config  [q] Quit            ● STOPPED   │
╰─────────────────────────────────────────────────────────╯
```

**Running state (before spec detected):**
```
╭─ a1b2c3 ────────────────────────────────────────────────╮
│ I'll start by reading the specs README...               │
╰─────────────────────────────────────────────────────────╯
╭─────────────────────────────────────────────────────────╮
│ [s] Start  [c] Config  [q] Quit            ● RUNNING   │
╰─────────────────────────────────────────────────────────╯
```

### Acceptance Criteria

- [x] Remove old status panel (title bar replacement)
- [x] Generate session ID (6 hex chars) at app start
- [x] Output panel uses session ID as left title via `Block::title()`
- [x] Command panel with rounded border at bottom
- [x] Keyboard shortcuts left-aligned in command panel
- [x] Status indicator right-aligned in command panel
- [x] Status shows colored dot + text: `● STOPPED` / `● RUNNING` / `● ERROR`
- [x] Layout uses two vertical chunks: output (flexible) + command (fixed height 3)

### Technical Constraints

- Use `Block::title()` with `Title::from()` for panel titles
- Use `Line::from(vec![...])` with spacing spans for command panel layout
- Session ID generated via `rand` crate (6 hex chars)
- Store session ID in `App` at construction time

### Error Cases

- None for this slice

## Slice 2: Current Spec in Title

(depends on current-spec-detection feature)

### User Behavior

When Ralph detects the current spec being worked on, it appears as the right title of the output panel.

**With spec detected:**
```
╭─ a1b2c3 ──────────────────────────────── configuration ─╮
│ ...                                                     │
╰─────────────────────────────────────────────────────────╯
```

**No spec yet:**
```
╭─ a1b2c3 ────────────────────────────────────────────────╮
│ ...                                                     │
╰─────────────────────────────────────────────────────────╯
```

### Acceptance Criteria

- [x] Right title shows current spec name when available
- [x] Right title hidden when no spec detected
- [x] Use `Line::from().right_aligned()` for right alignment (ratatui 0.30 API)

### Technical Constraints

- Read `app.current_spec` (set by current-spec-detection feature)
- Only add right title if `current_spec.is_some()`

### Error Cases

- None

## Slice 3: State-Based Border Styling

### User Behavior

The output panel's border changes based on app state, providing immediate visual feedback.

| State | Border Color | Border Type |
|-------|--------------|-------------|
| Stopped | Cyan | Rounded |
| Running | Green | Double |
| Error | Red | Double |

**Visual examples:**

Stopped (cyan, rounded):
```
╭─ a1b2c3 ────────────────────────────────────────────────╮
│                                                         │
╰─────────────────────────────────────────────────────────╯
```

Running (green, double):
```
╔═ a1b2c3 ════════════════════════════════════════════════╗
║ I'll start by reading...                                ║
╚═════════════════════════════════════════════════════════╝
```

Error (red, double):
```
╔═ a1b2c3 ════════════════════════════════════════════════╗
║ Error: spawn failed                                     ║
╚═════════════════════════════════════════════════════════╝
```

### Acceptance Criteria

- [x] Border color: Cyan (stopped), Green (running), Red (error)
- [x] Border type: Rounded (stopped), Double (running/error)
- [x] Status dot in command panel matches border color
- [x] Status text color matches state

### Technical Constraints

- Use `BorderType::Rounded` and `BorderType::Double`
- Use `border_style(Style::default().fg(color))` for color
- Match logic for both output panel and status indicator

### Error Cases

- None

## Slice 4: Elapsed Timer

### User Behavior

When running, the status shows elapsed time instead of static "RUNNING" text.

**Format:** `● 0:05` → `● 1:23` → `● 12:34` → `● 1:23:45`

- Under 1 hour: `M:SS` (e.g., `2:34`)
- Over 1 hour: `H:MM:SS` (e.g., `1:23:45`)

**Stopped:** `● STOPPED`
**Error:** `● ERROR`

### Acceptance Criteria

- [x] Track start time when transitioning to Running
- [x] Display elapsed time in command panel status area
- [x] Format: `M:SS` under 1 hour, `H:MM:SS` over 1 hour
- [x] Timer updates every render cycle
- [x] Timer freezes on Error state (shows last elapsed time)
- [x] Timer clears on Stopped state

### Technical Constraints

- Store `run_start_time: Option<Instant>` in `App`
- Set on transition to Running, clear on transition to Stopped
- Calculate elapsed in `draw_ui()` for live updates
- Use `Instant::elapsed()` for duration

### Error Cases

- None

## Slice 5: Scrollbar

### User Behavior

A scrollbar appears on the right edge of the output panel, showing current scroll position within the content.

```
╭─ a1b2c3 ────────────────────────────────────────────────╮
│ Line 1                                                  │▲
│ Line 2                                                  │█
│ Line 3                                                  │█
│ Line 4                                                  │
│ Line 5                                                  │▼
╰─────────────────────────────────────────────────────────╯
```

### Acceptance Criteria

- [x] Scrollbar on right edge of output panel
- [x] Shows `▲` at top, `▼` at bottom
- [x] Thumb position reflects scroll position
- [x] Thumb size reflects viewport vs content ratio
- [x] Scrollbar only visible when content exceeds viewport

### Technical Constraints

- Use `Scrollbar::new(ScrollbarOrientation::VerticalRight)`
- Use `ScrollbarState` with `content_length`, `position`, `viewport_content_length`
- Render with `render_stateful_widget()`
- Calculate content length from `app.output_lines.len()` + wrapped lines

### Error Cases

- **No content**: Hide scrollbar or show full-height thumb

## Slice 6: Error State Pulsing

### User Behavior

When in error state, the output panel border pulses between red and dark red to draw attention.

Pulse rate: ~2Hz (alternates every ~15 frames at 30fps)

### Acceptance Criteria

- [x] Error state border pulses red ↔ dark red
- [x] Pulsing uses `frame.count()` for timing
- [x] Only pulses in Error state, solid color otherwise
- [x] Status dot in command panel also pulses

### Technical Constraints

- Use `f.count()` to get frame number
- Calculate pulse: `(f.count() / 15) % 2 == 0`
- Alternate between `Color::Red` and `Color::DarkRed`

### Error Cases

- None

## Dependencies

- Slice 1: None (can start immediately)
- Slice 2: current-spec-detection feature
- Slice 3: Slice 1
- Slice 4: Slice 1
- Slice 5: Slice 1
- Slice 6: Slice 3

## Out of Scope

- Logs/config path display (deferred to config modal)
- "Config reloaded" indicator (moves to future config modal spec)
- Session ID in logs (handled by logging spec)
- Animated transitions between states
