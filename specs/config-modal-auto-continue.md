# Config Modal Auto-Continue Toggle

Add auto-continue toggle to the config modal so users can enable/disable the feature without editing the config file directly.

## User Behavior

The config modal includes a toggle for auto-continue, displayed alongside the other settings. Users can enable or disable auto-continue and save the change.

```
╭──────────────────────── Configuration ─────────────────────────╮
│                                                                │
│  Config file: ~/.config/ralph/config.toml                      │
│  Log directory: ~/.local/state/ralph/                          │
│                                                                │
│  ──────────────────────────────────────────────────────────    │
│                                                                │
│  Claude CLI path:                                              │
│  [~/.claude/local/claude____________________________]          │
│                                                                │
│  Prompt file:                                                  │
│  [./PROMPT.md_______________________________________]          │
│                                                                │
│  Specs directory:                                              │
│  [./specs___________________________________________]          │
│                                                                │
│  Log level:                                                    │
│  < info >                                                      │
│                                                                │
│  Auto-continue:                                                │
│  < On >                                                        │
│                                                                │
│                    [Save]     [Cancel]                         │
│                                                                │
╰────────────────────────────────────────────────────────────────╯
```

## Acceptance Criteria

- [x] Auto-continue field appears in config modal after Log level
- [x] Field uses same `< value >` cycling style as Log level (left/right arrows)
- [x] Options are `On` and `Off`
- [x] Field displays current value from `config.behavior.auto_continue`
- [x] Tab/Shift+Tab includes auto-continue field in navigation order
- [x] Save persists the auto-continue setting to config file
- [x] No validation needed (boolean field, always valid)

## Technical Constraints

- Add `AutoContinue` variant to `ConfigModalField` enum
- Reuse the cycling widget pattern from Log level field
- Map `true` → "On", `false` → "Off" for display
- Map "On" → `true`, "Off" → `false` when saving

## Error Cases

None — boolean toggle cannot have invalid state.

## Dependencies

- [config-modal](config-modal.md) (Done)
