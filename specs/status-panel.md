# Status Panel

Replaces the title bar with a visually striking, multi-line status panel that displays system state at a glance.

## Slice 1: Basic Status Panel

### User Behavior

The title bar is replaced with a bordered status panel. The panel uses ratatui's visual styling to create a polished, modern appearance. Users see the current app status (Running, Stopped, Error) with a colored indicator dot.

**Visual design:**
```
╭──────────────────────────────────────────────────────────╮
│ ● Running                                                │
╰──────────────────────────────────────────────────────────╯
```

- Rounded border (`BorderType::Rounded`) in cyan
- Status indicator: colored dot (●) followed by bold status text
  - Running: green dot, green text
  - Stopped: yellow dot, yellow text
  - Error: red dot, red text
- Panel height accommodates future rows (use `Constraint::Length(3)` for border + 1 content row)

### Acceptance Criteria

- [x] Title bar removed from layout
- [x] Status panel added with `BorderType::Rounded`
- [x] Border color is cyan (`Color::Cyan`)
- [x] Status displayed with colored dot indicator (●)
- [x] Status text is bold
- [x] Color coding: Running=green, Stopped=yellow, Error=red
- [x] Panel uses `Constraint::Length(3)` for height (borders + content)
- [x] Main pane height calculation updated to account for new panel size

### Technical Constraints

- Modify `draw_ui()` to replace the title bar constraint
- Use `Span` with `Style` for colored/bold text segments
- Use `Line::from(vec![...spans...])` for composing the status line
- Unicode dot character: `●` (U+25CF BLACK CIRCLE)

### Error Cases

- **Terminal too narrow**: Allow text to truncate naturally; ratatui handles this

## Slice 2: Session & Logging Info

(depends on logging feature)

### User Behavior

The status panel now shows the session ID and log directory, enabling users to quickly find their logs for debugging. If logging initialization failed, an error message is shown instead of the log path.

**Normal state:**
```
╭──────────────────────────────────────────────────────────╮
│ ● Running    Session: a1b2c3    Logs: ~/Library/Logs/ralph │
╰──────────────────────────────────────────────────────────╯
```

**Logging error state:**
```
╭──────────────────────────────────────────────────────────╮
│ ● Running    Session: a1b2c3    ⚠ Logging disabled        │
╰──────────────────────────────────────────────────────────╯
```

- Session ID displayed in bold after "Session:" label
- Log directory shown in dim color after "Logs:" label
- If logging failed: warning icon (⚠) in yellow, followed by error message in yellow
- Elements separated by adequate spacing for readability

### Acceptance Criteria

- [x] Session ID displayed with "Session:" label
- [x] Session ID text is bold
- [x] Log directory displayed with "Logs:" label (when logging succeeded)
- [x] Log directory text is dim (`Modifier::DIM`)
- [x] Warning icon (⚠) displayed when logging failed
- [x] Warning icon and error message in yellow
- [x] Elements have visual separation (spaces or divider characters)
- [x] Long paths truncate gracefully on narrow terminals

### Technical Constraints

- Read session ID from `App` state (added by logging feature)
- Read log directory path from `App` state (added by logging feature)
- Read logging error from `App.logging_error` (added by logging feature)
- Use `~` shorthand for home directory in display (expand for actual path, contract for display)
- Warning icon: `⚠` (U+26A0 WARNING SIGN)

### Error Cases

- **Logging not initialized yet**: Show "Session: ---" and "Logs: ---" as placeholders
- **Very long log path**: Truncate from the left with "..." prefix if needed

## Out of Scope

- Current spec detection (future spec)
- Error panel with detailed error list (future spec)
- Clickable elements or mouse interaction
- Animations or transitions
