# Output Panel Layout Refactor

Move iteration count and token display to the output panel's bottom title bar, clean up command panel to show only commands.

## User Behavior

The output panel has two title bars:
- **Top title:** Session ID (left-aligned), spec name (right-aligned)
- **Bottom title:** Iteration count (left-aligned), cumulative tokens (right-aligned)

The bottom title only appears when there's content to show. If no iterations have run and no tokens consumed, the bottom border is plain.

The command panel shows only keyboard shortcuts and status indicator — no tokens or iteration info.

### Visual Examples

**With content:**
```
╔═ 64oa27 ═══════════════════════════ spec-name ═╗
║ ... scrolling content ...                      ║
╚═ 1/5 ══════════════════════════ 12510 tokens ═╝

╔════════════════════════════════════════════════╗
║ [s] Start  [c] Config  [l] Specs  [q] Quit  ● IDLE
╚════════════════════════════════════════════════╝
```

**Empty state (no iterations, no tokens):**
```
╔═ 64oa27 ═══════════════════════════════════════╗
║ ... scrolling content ...                      ║
╚════════════════════════════════════════════════╝
```

**Partial state (iterations but no tokens yet):**
```
╔═ 64oa27 ═══════════════════════════ spec-name ═╗
║ ... scrolling content ...                      ║
╚═ 1/5 ══════════════════════════════════════════╝
```

## Acceptance Criteria

- [x] Top title shows session ID (left) and spec name (right) — no iteration count
- [x] Bottom title shows iteration count (left) when `current_iteration > 0`
- [x] Bottom title shows cumulative tokens (right) when `cumulative_tokens > 0`
- [x] Token format is raw number: `12510 tokens` (no thousands separator)
- [x] Bottom title content appears only when there's content (no empty spaces)
- [x] Command panel no longer displays tokens or iteration count
- [x] Remove `format_with_thousands` function from `ui.rs`
- [x] Iteration format unchanged: `1/5` or `2/∞`

## Technical Constraints

- Use ratatui's `.title_bottom()` for the bottom title bar
- Maintain existing iteration display logic (`current_iteration`, `total_iterations`)
- Keep `cumulative_tokens` field in `App` state
- Follow existing title styling patterns

## Error Cases

- **No spec detected:** Top title shows only session ID (left), nothing on right
- **Infinite iterations:** Display as `2/∞` (existing behavior)
- **Missing token data:** Don't display token portion of bottom title

## Out of Scope

- Changing the exchange display format in the scroll
- Token estimation or breakdown
- Changing iteration count format
