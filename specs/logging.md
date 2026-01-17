# Logging System

Developers can troubleshoot Ralph by reviewing structured logs in platform-standard locations.

## Slice 1: Infrastructure + Session Lifecycle

### User Behavior

When Ralph starts, it initializes logging to a platform-appropriate directory:
- **macOS**: `~/Library/Logs/ralph/`
- **Linux**: `~/.local/state/ralph/` (XDG_STATE_HOME)
- **Windows**: `%LocalAppData%\ralph\`

Logs are written with daily rotation (e.g., `ralph.2026-01-17.log`). Each log entry includes a session ID (6 hex characters) generated at startup, allowing developers to filter logs for a specific Ralph invocation.

If logging initialization fails, Ralph continues to function—the error is stored in `App` state and printed to stderr. The app remains usable without logging.

### Acceptance Criteria

- [ ] `directories` crate added to Cargo.toml
- [ ] Tracing subscriber initialized in `main()` with rolling daily file appender
- [ ] Non-blocking writes via `tracing_appender::non_blocking`
- [ ] `WorkerGuard` held for application lifetime (ensures flush on shutdown)
- [ ] Session ID (6 random hex chars) generated at startup
- [ ] Session ID included in all log entries via tracing span or fields
- [ ] `session_start` logged at INFO with session ID
- [ ] `session_end` logged at INFO with session ID and duration
- [ ] Log format: `TIMESTAMP LEVEL target session_id=X message key=value...`
- [ ] Initialization errors stored in `App` state (for future status panel)
- [ ] Initialization errors printed to stderr as fallback
- [ ] `RUST_LOG` environment variable controls log level filtering

### Technical Constraints

- Use existing dependencies: `tracing`, `tracing-appender`, `tracing-subscriber`
- Add `directories` crate for cross-platform path resolution
- Use `directories::ProjectDirs::from("dev", "cmoel", "ralph")` for path resolution
- Create log directory if it doesn't exist
- Default log level: INFO (configurable via `RUST_LOG`)

### Error Cases

- **Log directory creation fails** (permissions, disk full): Store error in `App.logging_error`, print to stderr, continue without file logging
- **Log file write fails**: `tracing_appender` handles this internally; non-blocking writer prevents app blocking
- **`ProjectDirs` returns None** (rare, unsupported platform): Fall back to stderr logging, store error in `App.logging_error`

## Slice 2: Comprehensive Event Instrumentation

(depends on Slice 1)

### User Behavior

Developers reviewing logs see detailed events for each command execution ("loop"). This enables debugging issues like "the command spawned but never produced output" or "the process exited with an unexpected code."

Raw JSON from the claude CLI is available at TRACE level for debugging protocol issues.

### Acceptance Criteria

- [ ] `loop_start` logged at INFO with loop number
- [ ] `loop_end` logged at INFO with loop number and exit status
- [ ] `command_spawned` logged at DEBUG with pid
- [ ] `process_killed` logged at INFO with pid
- [ ] Raw JSON lines logged at TRACE level (for protocol debugging)
- [ ] Existing `debug!()` and `warn!()` calls continue to work
- [ ] All log entries include session ID context

### Technical Constraints

- Use tracing spans or explicit fields to maintain session context
- Loop number should be a counter incremented each time `start_command()` is called
- TRACE-level JSON logging should be conditional on log level to avoid string formatting overhead

### Error Cases

- **Process exits with non-zero status**: Log at WARN with exit code
- **Process killed by signal**: Log at INFO with signal info if available
- **Channel disconnection before process exit**: Log at DEBUG

## Slice 3: Log Retention Cleanup

(depends on Slice 1)

### User Behavior

Old log files are automatically cleaned up on startup. Users don't need to manually manage log disk usage.

### Acceptance Criteria

- [ ] On startup, scan log directory for `ralph.*.log` files
- [ ] Delete files older than 7 days
- [ ] Log cleanup actions at DEBUG level (files deleted, count)
- [ ] Cleanup errors logged at WARN but don't prevent app startup

### Technical Constraints

- Run cleanup after logging is initialized (so cleanup actions are logged)
- Use file modification time for age calculation
- Handle concurrent Ralph instances gracefully (don't delete today's log)

### Error Cases

- **File deletion fails** (permissions, file locked): Log warning, continue with other files
- **Directory read fails**: Log warning, skip cleanup

## Out of Scope

- OTEL export (future enhancement if needed)
- User configuration of log levels via config file (future config spec)
- UI status panel for displaying logging errors (separate spec: status-panel)
- Log compression
- Size-based rotation
