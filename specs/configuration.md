# Configuration

Users can customize Ralph's behavior through a TOML config file.

## Slice 1: Config Infrastructure

### User Behavior

Ralph loads configuration from a platform-appropriate location on startup. If no config file exists, Ralph creates one with default values. Users can customize:

- Claude CLI path (default: `~/.claude/local/claude`)
- PROMPT.md path (default: `./PROMPT.md`)
- Specs directory (default: `./specs`)
- Log level (default: `info`)

**Config file locations:**
- macOS: `~/Library/Application Support/dev.cmoel.ralph/config.toml`
- Linux: `~/.config/ralph/config.toml`
- Windows: `%APPDATA%\cmoel\ralph\config.toml`

**Example config.toml:**
```toml
[claude]
path = "~/.claude/local/claude"
args = "--output-format=stream-json --verbose --print --include-partial-messages"

[paths]
prompt = "./PROMPT.md"
specs = "./specs"

[logging]
level = "info"  # trace, debug, info, warn, error
```

Environment variables override config file values:
- `RALPH_CLAUDE_PATH` → `claude.path`
- `RALPH_PROMPT_PATH` → `paths.prompt`
- `RALPH_SPECS_DIR` → `paths.specs`
- `RALPH_LOG` → `logging.level`

### Acceptance Criteria

- [x] Add dependencies: `toml`, `serde` (with derive), `directories`
- [x] Define `Config` struct with serde Serialize/Deserialize
- [x] All fields have sensible defaults via `Default` trait
- [x] Load config from platform-appropriate location using `directories` crate
- [x] Create default config file on first run if none exists
- [x] Parse environment variables and override config values
- [x] Invalid config file → log warning, use defaults
- [x] Missing config file → create default, log info
- [x] Store loaded config in `App` state
- [x] Store config load status in `App` (success, created, or error message)
- [x] Use `config.claude.path` when spawning claude CLI
- [x] Use `config.paths.prompt` when checking for PROMPT.md
- [x] Use `config.paths.specs` for specs directory path

### Technical Constraints

- Use `directories::ProjectDirs::from("dev", "cmoel", "ralph")`
- Expand `~` in paths to actual home directory
- Config parsing errors should not crash the app
- Store both the config values AND the config file path in `App`

### Error Cases

- **Config file doesn't exist**: Create default, log at INFO
- **Config file malformed TOML**: Log warning with line/column, use defaults
- **Unknown keys in config**: Ignore (forward compatibility)
- **Permission denied reading config**: Log warning, use defaults
- **Permission denied creating default**: Log warning, continue without file

## Slice 2: Edit Config in $EDITOR

### User Behavior

Users press `c` to edit the config file in their preferred editor. Ralph suspends the TUI, opens the editor, and waits for it to close.

**Keyboard shortcut:** `c` — opens config file in `$EDITOR`

Footer updates to show: `[s] Start  [c] Config  [q] Quit`

### Acceptance Criteria

- [x] Press `c` opens config file in `$EDITOR` (or `$VISUAL`, fallback to `vi`)
- [x] Ralph suspends TUI (disable raw mode, leave alternate screen)
- [x] Spawn editor process and wait for exit
- [x] Restore TUI after editor closes
- [x] Footer shows `[c] Config` hint
- [x] `c` key only works when `AppStatus` is `Stopped` (not while running)

### Technical Constraints

- Check `$VISUAL` first, then `$EDITOR`, fallback to `vi`
- Use `std::process::Command` with `.status()` to wait for editor
- Must properly suspend/restore terminal state (crossterm)
- If config file doesn't exist, create default before opening editor

### Error Cases

- **No $EDITOR set and vi not found**: Show error in status panel
- **Editor exits with error**: Log warning, continue normally
- **Editor spawn fails**: Show error in status panel

## Slice 3: Auto-Reload and Status Panel Integration

(depends on status-panel feature)

### User Behavior

The status panel shows the config file path. When the config file changes (for any reason), Ralph automatically reloads it and briefly shows "Config reloaded" indicator.

If the config is invalid, the status panel shows a warning.

**Status panel with config info:**
```
╭──────────────────────────────────────────────────────────────────────╮
│ ● Stopped    Session: a1b2c3    Config: ~/.config/ralph/config.toml  │
╰──────────────────────────────────────────────────────────────────────╯
```

**After config reload:**
```
╭──────────────────────────────────────────────────────────────────────────╮
│ ● Stopped    Session: a1b2c3    Config: ~/.config/ralph/  ✓ Reloaded    │
╰──────────────────────────────────────────────────────────────────────────╯
```

**With invalid config:**
```
╭──────────────────────────────────────────────────────────────────────────╮
│ ● Stopped    Session: a1b2c3    ⚠ Invalid config, using defaults        │
╰──────────────────────────────────────────────────────────────────────────╯
```

### Acceptance Criteria

- [x] Show config directory path in status panel (abbreviated with `~`)
- [x] Poll config file mtime periodically (every 2 seconds)
- [x] Reload config when mtime changes
- [x] Show "✓ Reloaded" indicator briefly after reload (fade after 3 seconds)
- [x] Show warning icon and message if config is invalid
- [x] Reload failures logged at WARN, don't crash

### Technical Constraints

- Same polling approach as current-spec-detection: no locks, no caching
- Store last known mtime in `App` state
- Store "reloaded at" timestamp to control indicator fade
- Show directory only (not full path) to save space

### Error Cases

- **File read fails during reload**: Log warning, keep previous config
- **Parse fails during reload**: Log warning, show status panel warning, keep previous config
- **Mtime check fails**: Log at DEBUG, skip this poll cycle

## Slice 4: Log Level Configuration

(depends on logging feature)

### User Behavior

The log level from config is applied to the logging system. Users can change verbosity by editing the config.

### Acceptance Criteria

- [x] Read `logging.level` from config
- [x] Apply to tracing subscriber during initialization
- [x] `RALPH_LOG` env var overrides config value
- [x] Valid levels: trace, debug, info, warn, error
- [x] Invalid level → default to info, log warning
- [x] On config reload, log level change takes effect

### Technical Constraints

- Use `tracing_subscriber::reload` handle for dynamic level changes
- Parse level string to `tracing::Level`

### Error Cases

- **Invalid log level string**: Use "info", log warning

## Dependencies

- Slice 1: None (can start immediately)
- Slice 2: Slice 1
- Slice 3: Slice 1, status-panel feature
- Slice 4: Slice 1, logging feature

## Out of Scope

- In-app config editor UI (edit values directly in Ralph)
- Config file validation beyond TOML parsing
- Config schema versioning/migration
- Per-project config files (only user-level config)
