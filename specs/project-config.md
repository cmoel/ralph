# Project Config

Project-specific configuration via a `.ralph` file that overrides global settings.

## Slice 1: Config Loading and Merge

### User Behavior

When the user starts ralph in a directory containing a `.ralph` file, settings from that file override the global config. Settings not specified in `.ralph` fall through to the global config. Environment variables override both.

**Precedence (highest to lowest):** env vars > `.ralph` > global config

The user can drop a `.ralph` file like this into any project:

```toml
[paths]
prompt = "./custom-prompt.md"

[behavior]
iterations = 3
```

Only the specified fields are overridden — everything else uses the global config.

### Acceptance Criteria

- [x] A `PartialConfig` struct exists where every field is `Option<T>`
- [x] On startup, ralph checks for `.ralph` in the current working directory
- [x] If `.ralph` exists, it is parsed as `PartialConfig` and merged with the global config
- [x] Merge applies field-by-field: project value if `Some`, otherwise global value
- [x] Environment variable overrides are applied after the merge (on top of everything)
- [x] If `.ralph` does not exist, behavior is identical to today (global config only)
- [x] Hot-reload watches both files — changes to either `.ralph` or global config trigger a re-merge
- [x] The project config path (`.ralph`) is stored in app state for use by other slices
- [x] Config errors (parse failures for `.ralph`) are surfaced in the command panel as a yellow warning message, not just logged
- [x] Global config parse errors also display in the command panel (fixes existing gap where `config_reload_error` is stored but never rendered)

### Technical Constraints

- `PartialConfig` mirrors the `Config` struct but with `Option` wrappers on every leaf field
- `PartialConfig` uses `#[serde(default)]` so missing sections/fields deserialize as `None`
- The merge function signature: `fn merge_config(global: &Config, project: &PartialConfig) -> Config`
- After merging, `apply_env_overrides` is called on the result (same as today)
- `.ralph` path is resolved from `std::env::current_dir()` at startup — no parent directory searching
- The existing `reload_config` function needs to be extended (or a new function added) to handle re-merging both sources
- `LoadedConfig` should include an `Option<PathBuf>` for the project config path

### Error Cases

- `.ralph` is malformed TOML → show warning in command panel (e.g., "Invalid .ralph: expected value at line 3"), keep using global config only, log the error
- `.ralph` has unknown keys → ignore them silently (forward-compatible, matching existing behavior)
- `.ralph` is not readable (permissions) → show warning in command panel, use global config only
- `.ralph` disappears after startup → on next reload poll, revert to global-only config, clear any project config error
- Both `.ralph` and global config have errors → show both errors, use built-in defaults

## Slice 2: Init Creates `.ralph`

(depends on Slice 1)

### User Behavior

When the user presses `i` to initialize a project, ralph creates a `.ralph` file alongside the other scaffolding files. The file is nearly empty — just a comment header — so all settings inherit from the global config until the user explicitly overrides them.

### Acceptance Criteria

- [ ] Init file list includes `.ralph` as a new entry
- [ ] Created `.ralph` contains only a comment header: `# Project-specific Ralph config — edit with config modal (c)\n`
- [ ] Conflict detection works for `.ralph` (shows existing file with ✗ if it already exists)
- [ ] After init, ralph detects the new `.ralph` on the next reload poll

### Technical Constraints

- Add the `.ralph` template as a constant in `templates.rs`
- Follow the existing `InitFileEntry` pattern for conflict detection
- `.ralph` path is relative to cwd (same as other init files like `PROMPT.md`)

### Error Cases

- `.ralph` already exists → show as conflict in init modal (same as other files)
- Write fails → show error in init modal (same as other files)

## Slice 3: Config Modal Project/Global Tabs

(depends on Slice 1)

### User Behavior

When the user presses `c` to open the config modal:

- **If `.ralph` exists:** Modal shows two tabs — "Project" and "Global". Defaults to "Project" tab. The user can switch between tabs to edit either config file.
- **If `.ralph` does not exist:** Modal shows only the "Global" tab (same as today).

In the Project tab, fields that are not set in `.ralph` (inherited from global) are visually distinct from fields explicitly set at the project level — e.g., dimmed or shown as placeholder text. When the user edits an inherited field, it becomes an explicit project override.

### Acceptance Criteria

- [ ] Config modal has a tab bar at the top when `.ralph` exists
- [ ] Tab bar shows "Project" and "Global" labels
- [ ] Keyboard navigation: a key (e.g., `[` / `]` or left/right when tab bar is focused) switches tabs
- [ ] Tab bar visually indicates which tab is active
- [ ] "Project" tab is selected by default when `.ralph` exists
- [ ] "Global" tab is the only option when `.ralph` does not exist (no tab bar shown)
- [ ] Project tab: fields not set in `.ralph` display with a visual distinction (dimmed, placeholder, or similar)
- [ ] Project tab: editing an inherited field converts it to an explicit project override
- [ ] Save on Project tab writes only explicitly-set fields to `.ralph` (partial TOML)
- [ ] Save on Global tab writes to the global config file (full TOML, same as today)
- [ ] After save, config is re-merged and app state is updated
- [ ] Tab switching preserves unsaved edits within each tab during the modal session

### Technical Constraints

- Extend `ConfigModalState` with a `tab` field and per-tab form state
- The Project tab needs to track which fields are explicitly set vs inherited — this maps to `Option<T>` in `PartialConfig`
- When saving the Project tab, serialize only `Some` fields to TOML — fields left as `None` should not appear in the file
- The Global tab behavior is unchanged from today
- Tab rendering should be minimal — a simple `[Project] [Global]` bar above the form fields

### Error Cases

- Save to `.ralph` fails (permissions, disk full) → show error in modal (same as current global save error handling)
- `.ralph` is deleted while modal is open on Project tab → save creates the file (same as editing any file that's been removed)
- User switches tabs with unsaved changes → edits are preserved in memory for that tab (not lost)

## Out of Scope

- Nested `.ralph` lookup (searching parent directories)
- Creating `.ralph` from the config modal (use init for that)
- CLI flag to ignore `.ralph`
- Project config in formats other than TOML
