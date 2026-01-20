# Output Panel Enhancements

Glanceable output that shows what Ralph is doing NOW, what's NEXT, and how much context has been consumed.

## Slice 1: Todo Display in Scroll

### User Behavior

When Claude calls the TodoWrite tool, Ralph displays a formatted task block in the output panel showing all tasks with their current status. Each TodoWrite call adds a new block, providing a history of task progression.

Example display:
```
━━━ Tasks ━━━━━━━━━━━━━━━━━━━━━
▶ Fixing authentication bug
○ Add validation to user input
✓ Run the test suite
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Acceptance Criteria

- [ ] Parse TodoWrite tool input JSON to extract todos array
- [ ] Display in_progress tasks with `▶` prefix using `activeForm` text
- [ ] Display pending tasks with `○` prefix using `content` text
- [ ] Display completed tasks with `✓` prefix using `content` text
- [ ] Each TodoWrite call adds a new block to the scroll (not replacing previous)
- [ ] Block has visual separator (━━━ Tasks ━━━) to stand out from other output

### Technical Constraints

- Follow existing `format_tool_summary()` pattern in `ui.rs`
- TodoWrite input structure: `{"todos": [{"content": "...", "status": "...", "activeForm": "..."}, ...]}`
- Status values: `pending`, `in_progress`, `completed`

### Error Cases

- **Malformed JSON**: Show `[Tool: TodoWrite] (parse error)` in scroll, log error with tracing
- **Empty todos array**: Show empty block with just the separator, log warning
- **Missing fields on individual todo**: Show with `?` prefix (unknown status), use `content` if `activeForm` missing and vice versa

## Slice 2: Cumulative Tokens in Status Panel

### User Behavior

The status panel displays cumulative token usage for the session, updated after each exchange. Format matches Claude Code: `40977 tokens`.

### Acceptance Criteria

- [ ] Track cumulative input + output tokens across all exchanges in the session
- [ ] Display total in status panel as `{total} tokens`
- [ ] Update after each `result` event is received
- [ ] Persists across the session (resets only on new session)

### Technical Constraints

- Add `cumulative_tokens: u64` field to `App` state
- Sum `input_tokens + output_tokens` from each `ResultEvent`
- Display in status panel alongside existing elements

### Error Cases

- **Missing usage in result event**: Display "—" instead of token count, do not update cumulative total

## Slice 3: Per-Exchange Tokens in Scroll

### User Behavior

Each exchange shows its token count inline in the scroll, categorized by exchange type. This helps users understand which operations consume the most context.

Example:
```
───────────────────────────────────
Exchange 1 (initial prompt): 7,371 in / 892 out
Cost: $0.05 | Duration: 2.3s
───────────────────────────────────
```

### Acceptance Criteria

- [ ] Display exchange number (incrementing counter)
- [ ] Categorize exchange type based on preceding events:
  - "initial prompt" for first exchange
  - "after {tool_name}" when following a tool use
  - "continuation" for other cases
- [ ] Show input and output tokens separately: `{in} in / {out} out`
- [ ] Include in existing usage summary block

### Technical Constraints

- Add `exchange_count: u32` field to `App` state
- Track `last_tool_used: Option<String>` to determine exchange type
- Modify `format_usage_summary()` to include exchange info

### Error Cases

- **Missing token counts**: Show "— in / — out"
- **No previous tool**: Default to "continuation" type

## Out of Scope

- Token breakdown by message type (system, CLAUDE.md, prompt, etc.) — would require estimation
- Context window percentage display — context size may vary
- Compaction detection — insufficient hooks from Claude CLI
- Persisting token usage to SQLite or spec README — future spec
