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
| [formatted-output-display](formatted-output-display.md) | In Progress | Formatted text, tool use, and usage summary (Slice 1 done) | raw-json-streaming-viewer |

## Notes

When picking work:
- Choose specs marked **Ready**
- Respect dependencies (don't start a spec until its dependencies are Done)
- If multiple specs are Ready with no dependencies, pick what interests you
