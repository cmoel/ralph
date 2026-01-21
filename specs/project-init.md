# Project Init

Initialize a project with Ralph scaffolding via the `i` key.

## Slice 1: Init Modal with File Status

### User Behavior

When the user presses `i` (and Ralph is not running), a modal opens showing which files will be created. Each file shows its status:

- `✓` Will create (file doesn't exist)
- `✗` Already exists (conflict)

The modal reads paths from the current config (including environment variable overrides):
- `{prompt_file}` from config
- `{specs_dir}/README.md`
- `{specs_dir}/TEMPLATE.md`
- `.claude/commands/ralph-spec.md`

User can press Escape or click Cancel to dismiss.

### Acceptance Criteria

- [x] `i` key opens init modal (only when not running)
- [x] Modal displays list of 4 files with their full paths
- [x] Each file shows ✓ or ✗ based on existence check
- [x] Escape or Cancel closes modal
- [x] Add `i` to command panel shortcuts hint

### Technical Constraints

- Follow existing modal pattern: `show_init_modal: bool` + `init_modal_state: Option<InitModalState>`
- Add keyboard handler in main event loop
- Use `app.config.prompt_file` and `app.config.specs_path()` for paths
- Check file existence with `std::path::Path::exists()`

### Error Cases

- specs_dir path is a file (not directory) → treat as conflict for all specs/* files
- Config paths are empty → show validation error, disable Initialize

---

## Slice 2: File Creation

Depends on Slice 1.

### User Behavior

When no conflicts exist, the modal shows an "Initialize" button. Pressing Enter or selecting the button creates all files and shows a success message.

Files created:

| File | Content |
|------|---------|
| `{prompt_file}` | Generic agent workflow prompt |
| `{specs_dir}/README.md` | Empty specs status table |
| `{specs_dir}/TEMPLATE.md` | Spec authoring guidelines |
| `.claude/commands/ralph-spec.md` | Spec shaping interview command |

After success, modal closes automatically.

### Acceptance Criteria

- [x] "Initialize" button visible when no conflicts
- [x] Enter key triggers Initialize when button focused
- [x] Creates parent directories if needed (e.g., `.claude/commands/`)
- [x] Creates all 4 files with correct content
- [x] Shows success message briefly before closing
- [x] Files use config paths (not hardcoded)

### Technical Constraints

- Store file templates in new `templates.rs` module as `const &str`
- Use `std::fs::create_dir_all()` for parent directories
- Use `std::fs::write()` for file creation
- PROMPT.md template: generic workflow (Discover → Understand → Search → Implement → Validate → Commit → Exit) with placeholder validation commands
- specs/README.md template: status key + empty table
- specs/TEMPLATE.md template: copy from current Ralph repo
- ralph-spec.md template: copy from current Ralph repo

### Error Cases

- Permission denied on write → show error message in modal, don't close
- Disk full → show error message in modal
- Path creation fails → show specific error with path

---

## Slice 3: Conflict Messaging

Depends on Slice 1.

### User Behavior

When one or more files already exist, the modal shows a warning panel instead of the Initialize button:

```
Cannot initialize — these files already exist:
  ✗ PROMPT.md
  ✗ specs/README.md

Rename them or update your config (press `c` to open config).
```

User can only Cancel/Escape to dismiss.

### Acceptance Criteria

- [x] Warning panel shown when any file has conflict status
- [x] Lists only the conflicting files
- [x] Suggests renaming or updating config
- [x] Mentions `c` key to open config
- [x] No Initialize button when conflicts exist
- [x] Non-conflicting files still show with ✓

### Technical Constraints

- Reuse existing warning styling (yellow/orange)
- Keep modal same size regardless of conflict state

### Error Cases

- All files conflict → show all in warning list
- Mix of conflicts and non-conflicts → show both sections clearly

---

## Out of Scope

- Editing file paths in the init modal (use config instead)
- Automatic conflict resolution (renaming existing files)
- Detecting partially initialized projects and offering to "complete" setup
- Custom template content (users edit files after creation)

---

## File Templates

### PROMPT.md (Generic)

```markdown
# Agent Workflow

Complete ONE vertical slice per session. A vertical slice delivers observable value to the end user.

## 1. Discover

Read `specs/README.md` to understand project state.

Select ONE spec marked **Ready** to work on.

**Immediately after selecting:**
1. Mark its status as **In Progress** in `specs/README.md`
2. Commit this change before doing any implementation work

## 2. Understand

Read the selected spec. Identify:
- What user-facing behavior this delivers
- Key implementation requirements
- Dependencies on other code

## 3. Search

Before writing code, search the codebase for:
- Existing implementations you can extend
- Patterns to follow
- Code your changes might affect

## 4. Implement

Build the vertical slice. As you work:
- Mark completed items in the spec with `[x]`
- Keep `specs/README.md` accurate

**If blocked:** Document in BOTH the spec AND `specs/README.md`:
- What failed
- Why it's blocking
- Options to resolve

## 5. Validate

Before committing, run your project's validation:

```bash
# Run your tests
# Run your linter
# Run your type checker
```

Do not commit until validation passes.

## 6. Commit

When the slice is complete:
1. Mark the spec complete in `specs/README.md`
2. Commit with a clear message describing the user-facing change

## 7. Exit

After committing ONE vertical slice, exit immediately. Do not start another task.
```

### specs/README.md

```markdown
# Specs

Single source of truth for specification status.

## Status Key

- **Ready** — Shaped and ready for implementation
- **In Progress** — Currently being worked on
- **Done** — Complete
- **Blocked** — Cannot proceed (see spec for details)

## Specs

| Spec | Status | Summary | Depends On |
|------|--------|---------|------------|

## Notes

When picking work:
- Choose specs marked **Ready**
- Respect dependencies (don't start until dependencies are Done)
```

### specs/TEMPLATE.md

Same content as current Ralph `specs/TEMPLATE.md`.

### .claude/commands/ralph-spec.md

Same content as current Ralph `.claude/commands/spec.md` (the spec shaping interview).
