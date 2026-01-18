# Specs

Single source of truth for specification status.

## Status Key

- **Ready** — Shaped and ready for implementation
- **In Progress** — An agent is actively working on this
- **Done** — All slices complete
- **Blocked** — Cannot proceed (see spec for details)

## Specs

| Spec | Status | Summary | Depends On |
|------|--------|---------|------------|
| [raw-json-streaming-viewer](raw-json-streaming-viewer.md) | Done | TUI displays streaming JSON from claude CLI | — |
| [formatted-output-display](formatted-output-display.md) | Done | Formatted text, tool use, and usage summary | raw-json-streaming-viewer |
| [logging](logging.md) | Done | Structured file logging with rotation and retention | — |
| [status-panel](status-panel.md) | Done | Visual status panel replacing title bar | — |
| [current-spec-detection](current-spec-detection.md) | Done | Display active spec by polling README | status-panel, logging |
| [configuration](configuration.md) | Done | TOML config file with auto-reload | — |
| [ui-overhaul](ui-overhaul.md) | Ready | Redesigned layout with command panel, state-based styling, timer | — (slice 2: current-spec-detection) |

## Notes

When picking work:
- Choose specs marked **Ready**
- Respect dependencies (don't start a spec until its dependencies are Done)
- If multiple specs are Ready with no dependencies, pick what interests you
