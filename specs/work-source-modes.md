# Work Source Modes

Ralph currently assumes work comes from spec files (`specs/README.md` + `specs/*.md`). Add a "mode" concept so users can choose between **specs mode** (current behavior, made explicit) and **beads mode** (work items from the `bd` CLI).

## Slice 1: Extract work source trait and make specs mode explicit

Make the existing spec-file behavior explicit behind a `WorkSource` trait, and add a `mode` config field that defaults to `"specs"`.

### User Behavior

No visible change — ralph works exactly as before. The config modal shows a new "mode" field set to `specs`. The `.ralph` project config can set `mode = "beads"` (errors gracefully until Slice 2 implements it).

### Acceptance Criteria

- [x] New `WorkSource` trait in `src/work_source.rs` with three methods:
  ```rust
  pub trait WorkSource {
      /// Check if there's remaining work (for auto-continue decisions).
      fn check_remaining(&self) -> WorkRemaining;
      /// Detect the currently active work item name (for status bar display).
      fn detect_current(&self) -> Option<String>;
      /// List all work items with status (for the specs/work panel).
      fn list_items(&self) -> Result<Vec<WorkItem>, String>;
  }
  ```
- [x] `WorkRemaining` enum mirrors existing `SpecsRemaining`: `Yes`, `No`, `Missing`, `ReadError(String)`
- [x] `WorkItem` struct: `{ name: String, status: WorkItemStatus, timestamp: Option<SystemTime> }` — generalizes `SpecEntry`
- [x] `WorkItemStatus` enum: `{ Blocked, Ready, InProgress, Done }` — same values as `SpecStatus`, keeps colors and labels
- [x] `SpecsWorkSource` struct implements `WorkSource` by delegating to existing `specs.rs` functions. The `specs.rs` functions stay unchanged — `SpecsWorkSource` is a thin wrapper.
- [x] Config: add `mode: String` to `BehaviorConfig` (default `"specs"`), add `mode: Option<String>` to `PartialBehaviorConfig`. Env override: `RALPH_MODE`.
- [x] `App` holds a `Box<dyn WorkSource>` constructed at startup based on config mode. `poll_spec()` and `handle_channel_disconnected()` call trait methods instead of `specs.rs` functions directly.
- [x] `modals.rs` specs panel uses `WorkSource::list_items()` instead of `parse_specs_readme()` directly
- [x] If `mode = "beads"`, log a warning and fall back to specs mode (beads not yet implemented)

### Technical Constraints

- `WorkSource` does NOT need to be `Send + Sync` — it's only used on the main thread
- Keep `specs.rs` unchanged — `SpecsWorkSource` wraps it, doesn't replace it
- The trait methods are **synchronous** (matching current polling model — `bd` calls in Slice 2 will also be synchronous subprocess calls)
- Update `is_partial_behavior_empty` to include the new `mode` field
- Config modal should display mode as read-only text (not editable in modal — edit `.ralph` or env var)

### Error Cases

- Unknown mode string in config → log warning, fall back to `"specs"`, show error in config reload status
- Mode changes on config hot-reload → reconstruct the `WorkSource` (or log that restart is needed)

---

## Slice 2: Implement beads mode

Add `BeadsWorkSource` that shells out to `bd` to discover and track work.

### User Behavior

With `mode = "beads"` in `.ralph`, ralph loops pull work from `bd` instead of spec files. The status bar shows the current bead ID + title. The specs panel (renamed "Work" panel) shows all beads with status. Auto-continue works: ralph loops until `bd ready` returns empty.

The PROMPT.md for beads-mode projects tells Claude to use `bd` commands instead of `specs/README.md`.

### Acceptance Criteria

- [x] `BeadsWorkSource` struct implements `WorkSource`:
  - `check_remaining()`: runs `bd ready --json`, returns `Yes` if non-empty array, `No` if empty, `ReadError` if command fails
  - `detect_current()`: runs `bd list --json --status in_progress`, returns first item's `"{id} {title}"` or `None`
  - `list_items()`: runs `bd list --json`, maps each bead to `WorkItem` with status mapping: `"open"` → `Ready`, `"in_progress"` → `InProgress`, `"closed"` → `Done`, `"deferred"` → `Blocked`
- [x] `bd` JSON output schema (fields used): `{ "id": String, "title": String, "status": String, "priority": Number, "updated_at": String }`
- [x] `BeadsWorkSource` has a `bd_path: String` field (default `"bd"`) for the CLI binary, configurable via `BehaviorConfig`
- [x] Shell-out uses `std::process::Command` with `--json` flag, captures stdout, parses with `serde_json`
- [x] Command timeout: if `bd` hangs for >5 seconds, kill and return `ReadError("bd command timed out")`
- [x] Status bar shows bead ID + truncated title (e.g., `PackPack-8tg Create and host pr...`)
- [x] "ALL SPECS COMPLETE" message changes to "ALL WORK COMPLETE" (or mode-appropriate: "ALL BEADS COMPLETE" / "ALL SPECS COMPLETE")
- [x] Specs panel title updates: "Specs" in specs mode, "Beads" in beads mode

### Technical Constraints

- `bd` commands run synchronously in the polling methods. This is fine because polling is throttled to every 2 seconds and `bd` commands typically complete in <100ms.
- Parse only the fields you need from `bd` JSON — use `serde_json::Value` or a minimal struct with `#[serde(default)]` to be resilient to schema changes.
- The `bd` binary must be on `$PATH` or configured. If not found, `check_remaining()` returns `ReadError("bd not found")` and ralph shows an error status (not a crash).
- No changes to how Claude is invoked (still `cat PROMPT.md | claude ...`). The PROMPT.md is a user concern — ralph doesn't switch prompts per mode. But update the beads-mode init template (if init modal is used) to generate a beads-aware PROMPT.md.

### Error Cases

- `bd` not installed / not on PATH → `ReadError("bd: command not found")`, ralph shows error, doesn't crash
- `bd` returns non-zero exit → `ReadError` with stderr content
- `bd` returns invalid JSON → `ReadError("failed to parse bd output")`
- `bd` hangs → timeout after 5s, kill process, `ReadError("bd command timed out")`
- Empty beads database → `check_remaining()` returns `No` (empty `bd ready` result), ralph shows "ALL BEADS COMPLETE"

---

## Out of Scope

- Bidirectional sync (ralph doesn't close beads or update bead status — Claude does that via `bd` commands in its session)
- A `bd`-aware PROMPT.md template in `templates.rs` (can be added later; users write their own PROMPT.md)
- Specs panel editing or interactive bead management from ralph's TUI
- Mixed mode (some specs + some beads)

## Dependencies

- None — this is self-contained within ralph
