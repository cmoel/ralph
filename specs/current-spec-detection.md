# Current Spec Detection

Ralph displays the currently active spec in the status panel by polling `specs/README.md`.

## Prerequisites

Before implementing, update `PROMPT.md` to ensure agents mark specs "In Progress" immediately after selection:

In the "1. Discover" section, after "Select ONE spec...", add:

```markdown
**Immediately after selecting a spec:**
1. Mark its status as **In Progress** in `specs/README.md`
2. Commit this change before doing any implementation work

Only one spec should be In Progress at a time.
```

## User Behavior

While Ralph is running, the status panel shows which spec the agent is working on. This helps users understand what the agent is doing at a glance.

**Status panel with active spec:**
```
╭──────────────────────────────────────────────────────────────────╮
│ ● Running    Session: a1b2c3    Spec: logging    Logs: ~/...     │
╰──────────────────────────────────────────────────────────────────╯
```

**No spec selected yet:**
```
╭──────────────────────────────────────────────────────────────────╮
│ ● Running    Session: a1b2c3    Spec: —    Logs: ~/...           │
╰──────────────────────────────────────────────────────────────────╯
```

When `AppStatus` transitions to `Stopped`, the spec field clears.

## Acceptance Criteria

- [x] PROMPT.md updated with "In Progress" instructions (prerequisite)
- [x] Poll `specs/README.md` every 2 seconds while `AppStatus` is `Running`
- [x] Polling stops when `AppStatus` is `Stopped`
- [x] Parse markdown table to find row with "In Progress" status
- [x] Extract spec name from the matching row
- [x] Store current spec name in `App` state
- [x] Display spec name in status panel with "Spec:" label
- [x] Show "—" when no spec is in progress
- [x] Clear spec display when transitioning to `Stopped`
- [x] Parse failures handled gracefully (show "—", no crash)

## Technical Constraints

**Keep it simple — no async complexity:**

- Poll in the main event loop using elapsed time check, not a background task
- Synchronous file read (file is tiny, ~1KB)
- No retries on failure
- No locks or coordination
- No caching — re-read and re-parse each poll cycle
- Any error at any step → show "—" and continue

**Polling implementation:**

```rust
// In run_app(), track last poll time
let mut last_spec_poll = Instant::now();

// In the event loop, check elapsed time
if app.status == AppStatus::Running && last_spec_poll.elapsed() >= Duration::from_secs(2) {
    app.current_spec = detect_current_spec();
    last_spec_poll = Instant::now();
}
```

**README parsing:**

- Read `specs/README.md` as string
- Find lines matching table row pattern: `| [spec-name](...)  | In Progress | ... |`
- Extract spec name from first matching row
- Regex or simple string parsing both acceptable

**State management:**

- Add `current_spec: Option<String>` to `App` struct
- Set to `None` when `AppStatus` transitions to `Stopped`
- Set to `None` on any parse/read failure

## Error Cases

- **File doesn't exist**: Show "—", log at DEBUG
- **File read fails** (permissions, being written): Show "—", log at DEBUG
- **Parse fails** (malformed table): Show "—", log at DEBUG
- **No "In Progress" row**: Show "—" (not an error, just no active spec)
- **Multiple "In Progress" rows**: Take first one, log at WARN

## Dependencies

- Status panel feature (for display location)
- Logging feature (for debug/warn logging of failures)

## Out of Scope

- Slice-level tracking (only spec-level)
- Real-time file watching (polling is sufficient)
- Caching or incremental parsing
- Retry logic
