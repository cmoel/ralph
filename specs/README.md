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
| [ui-overhaul](ui-overhaul.md) | Done | Redesigned layout with command panel, state-based styling, timer | — |
| [config-modal](config-modal.md) | Done | In-app config form with validation using ratatui native widgets | configuration |
| [remove-cli-args-field](remove-cli-args-field.md) | Done | Remove non-configurable CLI args field from config modal | config-modal |
| [auto-continue](auto-continue.md) | Done | Auto-continue claude until all specs complete | configuration |
| [specs-panel](specs-panel.md) | Done | Browse all specs with blocked specs highlighted | — |
| [config-modal-auto-continue](config-modal-auto-continue.md) | Done | Add auto-continue toggle to config modal | config-modal |
| [code-organization](code-organization.md) | Done | Split main.rs into logical files for readability | — |
| [dead-code-cleanup](dead-code-cleanup.md) | Done | Remove dead code and unnecessary allow annotations | — |
| [testing](testing.md) | Done | Unit tests for pure logic, refactor to separate I/O | code-organization |
| [release-process](release-process.md) | Done | GitHub Actions release workflow with pre-built binaries | testing |
| [project-readme](project-readme.md) | Done | Root README.md with install, config, and contributing docs | release-process |
| [iteration-control](iteration-control.md) | Done | Configurable iteration count replacing boolean auto-continue | auto-continue, config-modal-auto-continue |
| [output-panel-enhancements](output-panel-enhancements.md) | Done | Todo display, cumulative tokens, per-exchange tokens | formatted-output-display, status-panel |
| [output-panel-layout-refactor](output-panel-layout-refactor.md) | Done | Move iteration/tokens to bottom title, clean up command panel | output-panel-enhancements |
| [tasks-panel](tasks-panel.md) | Done | Dedicated panel for task list with collapse/expand and panel selection | output-panel-enhancements, configuration |
| [keep-awake](keep-awake.md) | Done | Prevent system sleep while claude is running | configuration |
| [formatted-streaming-output](formatted-streaming-output.md) | Done | Formatted output with tool call/result correlation | formatted-output-display |
| [project-init](project-init.md) | Done | Initialize project with Ralph scaffolding via `i` key | configuration |
| [simplified-command-panel](simplified-command-panel.md) | Done | Minimal command panel with color styling and Help modal | ui-overhaul |
| [task-tool-display](task-tool-display.md) | Done | Task tool results display extracted text instead of raw JSON | formatted-streaming-output |
| [project-config](project-config.md) | Ready | Project-specific config via `.ralph` file with merge precedence | configuration, config-modal, project-init |

## Notes

When picking work:
- Choose specs marked **Ready**
- Respect dependencies (don't start a spec until its dependencies are Done)
- If multiple specs are Ready with no dependencies, pick what interests you
