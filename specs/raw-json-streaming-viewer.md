# Raw JSON Streaming Viewer

User can watch streaming JSON output from the `claude` CLI in a scrollable TUI.

## User Behavior

1. User launches `ralph`
2. Sees idle TUI: title bar with "RALPH" and "STOPPED" status, empty main pane, footer with keybindings
3. Presses `s` to start
4. Command runs: `cat PROMPT.md | claude --output-format=stream-json --verbose --print`
5. Raw JSON lines stream into main pane, auto-scrolling to follow
6. User can scroll up to review previous output (pauses auto-scroll)
7. Scrolling back to bottom resumes auto-scroll
8. When command finishes, status shows "STOPPED", output remains visible
9. User can press `s` again to start new run (prints divider, appends output)
10. User presses `q` to quit

## Acceptance Criteria

### Layout
- [ ] Title bar shows "RALPH" on left, status on right
- [ ] Status displays "STOPPED" when idle, "RUNNING" when command is active
- [ ] Main pane fills available space between title bar and footer
- [ ] Footer shows keybindings: `[s] Start  [q] Quit`

### Command Execution
- [ ] Pressing `s` when idle runs `cat PROMPT.md | claude --output-format=stream-json --verbose --print`
- [ ] App state transitions from idle to running when command starts
- [ ] App state transitions from running to idle when command exits
- [ ] Each JSON line from stdout appears as a new line in main pane
- [ ] Starting a new run prints a visual divider before appending new output

### Scrolling
- [ ] Output auto-scrolls to follow new content while running
- [ ] User can scroll up with `k`, mouse wheel up
- [ ] User can scroll down with `j`, mouse wheel down
- [ ] `ctrl-u` scrolls up half page
- [ ] `ctrl-d` scrolls down half page
- [ ] `ctrl-b` scrolls up full page
- [ ] `ctrl-f` scrolls down full page
- [ ] Scrolling up pauses auto-scroll
- [ ] Scrolling to bottom resumes auto-scroll

### Quit
- [ ] Pressing `q` exits the application
- [ ] If command is running when `q` is pressed, terminate the command before exiting

## Technical Constraints

### Architecture
- Async event loop with tokio
- Ratatui 0.30 for TUI rendering
- Crossterm for terminal manipulation and events
- Enable mouse capture for scroll support

### Scrolling Implementation
Auto-follow is not built into Ratatui. Implement manually:
- Track `scroll_offset` and `is_auto_following` in app state
- When new content arrives and `is_auto_following` is true, update scroll to bottom
- When user scrolls up, set `is_auto_following` to false
- When user scrolls to bottom, set `is_auto_following` to true

### Command Execution
- Spawn command as async child process
- Read stdout line by line, append each line to content buffer
- Capture stderr and display inline or in error state

### Widgets
- Use `Paragraph` widget with `.scroll((offset, 0))` for main pane
- Use `Block` for title bar and footer areas
- Calculate visible lines based on pane height for page scrolling

## Error Cases

### PROMPT.md does not exist
- Display error message in main pane: "Error: PROMPT.md not found"
- Status shows "ERROR"
- Only `q` to quit is available (ignore `s`)

### Command fails to start or exits with error
- Display error output in main pane
- Return to idle state
- User can attempt to start again with `s`

### User presses `s` while command is running
- Show popup dialog: "Command already running"
- User dismisses with Enter
- Use Ratatui popup pattern: Clear background, render centered Block

### Command produces no output
- Main pane remains empty (or shows only divider if not first run)
- Return to idle when command exits
