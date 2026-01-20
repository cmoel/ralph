# Keep Awake

Prevent the system from sleeping while claude is running.

## Slice 1: Core Wake Lock

### User Behavior

When the user starts a claude command, Ralph acquires a wake lock that prevents the system from idle sleeping. The display may still sleep. When the command ends (success, error, or manually stopped), the wake lock is released.

If Ralph cannot acquire the wake lock, a bright, bold warning appears in the output panel:

```
⚠ Warning: Could not acquire wake lock - system may sleep during execution
```

The command continues running regardless of wake lock success.

### Acceptance Criteria

- [ ] Add `keepawake` crate dependency
- [ ] Create `src/wake_lock.rs` module with `WakeLock` struct
- [ ] `WakeLock::new()` returns `Result<Self, Error>` wrapping `keepawake::Builder`
- [ ] Configure keepawake with `idle(true)` only (display can sleep)
- [ ] Set reason to "Running claude CLI" and app_name to "ralph"
- [ ] Acquire wake lock in `start_command()` before spawning process
- [ ] Store `Option<WakeLock>` in `App` struct
- [ ] Release wake lock when process ends (drop the struct)
- [ ] On wake lock failure: log warning via tracing
- [ ] On wake lock failure: display styled warning line in output panel
- [ ] Warning uses bright/bold styling to stand out
- [ ] Wake lock works on macOS, Linux, and Windows

### Technical Constraints

- Follow existing module pattern (see `src/logging.rs`, `src/config.rs`)
- Use `keepawake::Builder::default().idle(true).reason(...).app_name(...).create()`
- Warning styling should use ratatui's `Style` with bold + bright yellow/orange
- The `WakeLock` struct should be a thin wrapper that handles the error conversion

### Error Cases

- Wake lock acquisition fails → log warning, show output warning, continue command
- Wake lock already held (shouldn't happen) → log and continue
- Platform doesn't support wake locks → same as acquisition failure

## Slice 2: Configuration

(depends on Slice 1)

### User Behavior

Users can disable the keep-awake feature via the config modal. A new "Keep Awake" toggle appears in the settings. When disabled, Ralph does not attempt to acquire a wake lock.

The warning message only appears if keep-awake is enabled AND acquisition fails.

### Acceptance Criteria

- [ ] Add `keep_awake: bool` field to `Config` struct (default: `true`)
- [ ] Add toggle to config modal UI
- [ ] Skip wake lock acquisition when `keep_awake` is `false`
- [ ] Config change takes effect on next command start (not mid-command)
- [ ] Document setting in config file comments

### Technical Constraints

- Follow existing config field pattern (see `auto_continue`, `iterations`)
- Toggle should match existing config modal styling
- Place toggle in a logical location within the modal

### Error Cases

- Config file has invalid `keep_awake` value → use default (`true`)
