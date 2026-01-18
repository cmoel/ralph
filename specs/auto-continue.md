# Auto-Continue

Ralph automatically continues running claude until all specs are complete.

## Slice 1: Stop Functionality

### User Behavior

When claude is running, the user sees `[s] Stop` instead of `[s] Start` in the command panel. Pressing `s` while running kills the claude process and transitions to Stopped (IDLE) state.

### Acceptance Criteria

- [ ] Command panel shows `[s] Stop` when status is Running
- [ ] Command panel shows `[s] Start` when status is Stopped or Error
- [ ] Pressing `s` while Running kills the child process
- [ ] After killing, status transitions to Stopped
- [ ] Killing a process is logged

### Technical Constraints

- Use the existing child process handle in the App struct
- Killed processes exit with non-zero status (signal termination)

### Error Cases

- If kill fails, log the error and transition to Stopped anyway

## Slice 2: Auto-Continue Until Complete

(depends on Slice 1)

### User Behavior

When enabled (default), Ralph automatically restarts claude after it completes successfully, continuing until all specs are Done or Blocked. A bold, standout message appears in the output when auto-continuing.

### Acceptance Criteria

- [ ] Config option `[behavior] auto_continue = true` exists (enabled by default)
- [ ] On exit code 0: check `specs/README.md` for remaining work
- [ ] If README has specs with Ready or In Progress status → auto-continue
- [ ] If README has no Ready or In Progress specs → transition to Stopped
- [ ] If README cannot be read → transition to Error state
- [ ] On exit code non-zero → transition to Error state
- [ ] Auto-continue displays bold/standout message in output area
- [ ] Auto-continue is logged
- [ ] Manual stop (Slice 1) does not trigger auto-continue (killed processes exit non-zero)

### Technical Constraints

- Add `[behavior]` section to Config struct in `src/config.rs`
- Extend spec detection to check for Ready and In Progress statuses
- State transition logic lives in `poll_output()` at `main.rs:1172`
- Bold message should be visually distinct from claude output (consider color, prefix, or decoration)

### Error Cases

- README missing → Error state with message
- README unreadable (permissions) → Error state with message
- README malformed (no table found) → treat as "no specs remain" (conservative)
