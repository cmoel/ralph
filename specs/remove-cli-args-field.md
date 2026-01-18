# Remove Claude CLI Args Field

Remove the "Claude CLI args" field from the config modal. These arguments are hardcoded requirements for Ralph to function correctly and should not be user-configurable.

## Rationale

The Claude CLI args (`--output-format=stream-json --verbose --print --include-partial-messages`) are not optional — Ralph depends on this exact format to parse the streaming output. Allowing users to modify these args would break Ralph.

## User Behavior

The config modal no longer shows or allows editing of Claude CLI args. The field is removed entirely.

**Before:**
```
│  Claude CLI path:                                              │
│  [~/.claude/local/claude____________________________]          │
│                                                                │
│  Claude CLI args:                                              │
│  [--output-format=stream-json --verbose --print...]            │
│                                                                │
│  Prompt file:                                                  │
```

**After:**
```
│  Claude CLI path:                                              │
│  [~/.claude/local/claude____________________________]          │
│                                                                │
│  Prompt file:                                                  │
```

## Acceptance Criteria

- [x] Remove Claude CLI args field from config modal UI
- [x] Remove Claude CLI args from `ConfigModalField` enum (if applicable)
- [x] Remove Claude CLI args from config modal state
- [x] Update focus order to skip the removed field
- [x] Hardcode the CLI args in the command spawning code (if not already)
- [x] Remove `claude.args` from config file schema (or keep but ignore)

## Technical Constraints

- The args string `--output-format=stream-json --verbose --print --include-partial-messages` must be hardcoded where the claude CLI is spawned
- If `claude.args` exists in user's config file, ignore it (don't error)

## Error Cases

- None

## Out of Scope

- Removing `claude.args` from existing user config files (they can keep it, we just ignore it)
