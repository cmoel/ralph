# Iteration Control

User can specify exactly how many iterations Ralph runs, with visual feedback showing progress.

## Slice 1: Core Iteration Control

### User Behavior

When the user presses `s` to start, Ralph reads the configured `iterations` value and begins a controlled loop:

- **Negative (-1)**: Infinite mode — continues until user presses `s` to stop or all specs complete
- **Zero (0)**: Stop state — Ralph never starts an iteration
- **Positive (N)**: Countdown mode — runs exactly N iterations, then stops

The UI displays iteration progress near the session ID:
```
Session: abc123 ── 2/5      # Countdown mode: iteration 2 of 5
Session: abc123 ── 3/∞      # Infinite mode: iteration 3
Session: abc123 ── ─        # Stopped / not running
```

### Acceptance Criteria

- [x] Config field `iterations` (i32) replaces `auto_continue` (bool) in `[behavior]` section
- [x] Default value is `-1` (infinite, preserving current behavior)
- [x] Runtime state tracks `current_iteration` and `total_iterations`
- [x] Pressing `s` reads config, sets `total_iterations`, sets `current_iteration = 1`, starts
- [x] Infinite mode (`total_iterations < 0`): always auto-continues when specs remain
- [x] Countdown mode: auto-continues only if `current_iteration < total_iterations`
- [x] Iteration counter increments on each auto-continue
- [x] UI displays `current/total` (e.g., `2/5`) near session ID with `──` separator
- [x] Infinite mode displays `∞` for total (e.g., `3/∞`)
- [x] Stopped state displays `─`
- [x] Config value of `0` prevents any iteration from starting

### Technical Constraints

- Change `BehaviorConfig.auto_continue: bool` → `BehaviorConfig.iterations: i32`
- Add runtime fields to `App`: `current_iteration: u32`, `total_iterations: i32`
- Update `handle_channel_disconnected()` to use countdown logic instead of boolean check
- Infinity symbol `∞` (U+221E) is safe cross-platform in terminal environments
- Config changes mid-loop do not affect the running loop (runtime state is independent)

### Error Cases

- Invalid config value (non-integer): Use default (-1), log warning
- User presses `s` when `iterations = 0`: No-op, remain in stopped state (or show brief message)

## Slice 2: Config Modal Update

(depends on Slice 1)

### User Behavior

The config modal shows an iterations field instead of the On/Off toggle. User can input any integer ≥ -1. The value `-1` displays as `∞` in the UI.

### Acceptance Criteria

- [x] Config modal replaces Auto-continue toggle with Iterations number input
- [x] Field accepts integers from -1 to any reasonable positive number
- [x] Value `-1` displays as `∞` in the modal
- [x] Left/right arrows or typing changes the value
- [x] Validation prevents values below -1

### Technical Constraints

- Update `ConfigModalField::AutoContinue` → `ConfigModalField::Iterations`
- Number input with increment/decrement via arrow keys
- Display transformation: `-1` ↔ `∞`

### Error Cases

- User enters invalid input (non-numeric): Reject keystroke or show validation error
- User tries to go below -1: Clamp to -1
