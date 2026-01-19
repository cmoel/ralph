# Code Organization

Split main.rs into logical files so people and AI agents can quickly understand where code lives.

## Problem

main.rs is 2,538 lines containing 8 different concerns. When someone asks "where's the spec parsing code?" or "how does the config modal work?", they have to search through one massive file.

## Goal

A developer or AI agent should be able to answer "where is X?" by looking at filenames alone.

## Target Structure

```
src/
├── main.rs         # Entry point + event loop
├── app.rs          # App struct + core methods
├── ui.rs           # All rendering (draw_* functions)
├── modals.rs       # ConfigModalState + SpecsPanelState
├── specs.rs        # Spec parsing + detection
├── config.rs       # (existing, unchanged)
├── events.rs       # (existing, unchanged)
└── logging.rs      # (existing, unchanged)
```

## Acceptance Criteria

- [ ] main.rs contains only: main(), run_app(), and the event loop
- [ ] app.rs contains: App struct, AppStatus enum, App impl
- [ ] ui.rs contains: all draw_* functions, format_* functions, centered_rect, truncate_str
- [ ] modals.rs contains: ConfigModalState, ConfigModalField, SpecsPanelState, SpecStatus, SpecEntry, input handlers, validators
- [ ] specs.rs contains: parse_specs_readme, detect_current_spec, check_specs_remaining, SpecsRemaining
- [ ] All existing tests pass
- [ ] `devbox run check` passes (clippy clean)
- [ ] No file exceeds 600 lines

## Technical Constraints

- Use `mod` declarations in main.rs to include other files
- Use `pub` and `pub(crate)` appropriately for visibility
- Move related constants with their functions (e.g., LOG_LEVELS with config modal)
- Keep `use` statements minimal — import what you need, not entire modules

## Out of Scope

- Changing any behavior
- Adding tests
- Nested module directories (keep it flat)
- Multi-crate workspace
- Performance optimization
