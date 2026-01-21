# Simplified Command Panel

Streamlined command panel showing only essential keys with color styling, plus a Help modal for discoverability.

## Slice 1: Simplify Command Panel

### User Behavior

The command panel at the bottom of the screen shows three commands with colorful styling:

```
s Start  q Quit  ? Help
```

When running:
```
s Stop  q Quit  ? Help
```

Keys (`s`, `q`, `?`) are displayed in a bright accent color. Labels (`Start`, `Quit`, `Help`) are displayed in dim gray. No brackets or other punctuation — color creates the visual hierarchy.

The right side of the command panel continues to show the status indicator (dot + IDLE/timer/ERROR) as it does today.

### Acceptance Criteria

- [x] Command panel shows only three commands: `s`, `q`, `?`
- [x] Keys are styled in bright accent color (choose a vibrant color like Cyan, Yellow, or Magenta)
- [x] Labels are styled in dim gray (`Color::DarkGray`)
- [x] `s` label toggles between "Start" and "Stop" based on `AppStatus`
- [x] Status indicator (right side) remains unchanged
- [x] Removed commands from display: `c` (Config), `i` (Init), `l` (Specs)

### Technical Constraints

- Modify `draw_ui()` in `src/ui.rs` (around line 585-630)
- Use `Span::styled()` with different colors for keys vs labels
- Keep the spacing logic that right-aligns the status indicator

### Error Cases

None — this is purely a display change.

## Slice 2: Help Modal

(depends on Slice 1)

### User Behavior

Pressing `?` opens a centered Help modal displaying all available commands organized by category:

```
┌─ Help ─────────────────────────────────────┐
│                                            │
│  Control                                   │
│    s   Start / Stop                        │
│    q   Quit                                │
│                                            │
│  Panels                                    │
│    c   Config                              │
│    l   Specs                               │
│    i   Init project                        │
│    t   Toggle tasks panel                  │
│    Tab Switch panel focus                  │
│                                            │
│  Scroll (selected panel)                   │
│    j/k      Line up/down                   │
│    ↑/↓      Line up/down                   │
│    Ctrl+u/d Half-page up/down              │
│    Ctrl+b/f Full-page up/down              │
│                                            │
│                         ? or Esc to close  │
└────────────────────────────────────────────┘
```

The modal uses the same color styling as the command panel: keys in bright accent color, descriptions in dim gray, category headers in a distinct style.

Pressing `?` again or `Esc` closes the modal.

**Behavior change:** All commands now work regardless of application state. Previously `c` (Config) and `i` (Init) were blocked while running. Now they work anytime. The only state-dependent behavior is `s` which toggles between Start and Stop.

### Acceptance Criteria

- [x] `?` key opens Help modal from any state (not inside another modal)
- [x] `?` key closes Help modal when open (toggle behavior)
- [x] `Esc` closes Help modal
- [x] Modal is centered and compact (sized to fit content)
- [x] Commands organized into three categories: Control, Panels, Scroll
- [x] Keys styled in bright accent color (same as command panel)
- [x] Descriptions styled in dim gray
- [x] Category headers visually distinct (bold or different color)
- [x] Footer shows "? or Esc to close" right-aligned
- [x] `c` (Config) works while running
- [x] `i` (Init) works while running
- [x] `l` (Specs) already works in all states — no change needed

### Technical Constraints

- Add `show_help_modal: bool` to `App` struct in `src/app.rs`
- No separate state struct needed (modal has no interactive elements)
- Add `draw_help_modal()` function in `src/modal_ui.rs`
- Add `?` key handling in `src/main.rs` event loop
- Remove state checks that block `c` and `i` when running
- Follow existing modal patterns: use `Clear` widget, `centered_rect()` utility, `Block` with borders

### Error Cases

- If another modal is open (Config, Specs, Init), `?` should do nothing — user must close current modal first
