# Formatted Output Display

User sees readable, formatted output from the claude CLI instead of raw JSON.

## Dependencies

- [raw-json-streaming-viewer](raw-json-streaming-viewer.md) must be complete

## Slice 1: Text Content Display

### User Behavior

Instead of seeing raw JSON lines, user sees assistant text responses as readable text. The stream-json events are parsed, and text content is extracted and displayed.

Example transformation:
```
Raw JSON:
{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

Displayed:
Hello
```

### Acceptance Criteria

- [x] Parse NDJSON lines into typed Rust structs
- [x] Handle event types: `message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`
- [x] Handle Claude CLI wrapper events: `system` (init), `assistant`, `result`
- [x] Extract and display `text_delta` content as plain text
- [x] Accumulate text deltas into continuous output (no line break per delta)
- [x] Handle `ping` events silently (no display)
- [x] Unknown event types are logged but don't crash the app

### Technical Constraints

- Define Rust enums with `#[serde(tag = "type")]` for event discrimination
- Use `serde_json` for parsing
- Buffer partial lines (NDJSON requires complete lines)
- Content blocks have an `index` - track which block is being updated

### Error Cases

- Malformed JSON line → log error, skip line, continue processing
- Unknown event type → log warning, skip, continue

---

## Slice 2: Tool Use Display

### User Behavior

User sees when claude invokes tools. Tool calls are displayed with the tool name and a summary of the input.

Example display:
```
[Tool: Bash] git status
[Tool: Read] /path/to/file.rs
[Tool: Edit] /path/to/file.rs (lines 10-15)
```

### Acceptance Criteria

- [x] Parse `tool_use` content blocks
- [x] Display tool name when `content_block_start` has type `tool_use`
- [x] Accumulate `input_json_delta` events to build complete input
- [x] Parse tool input JSON at `content_block_stop`
- [x] Display concise summary of tool input (not full JSON)
- [x] Handle common tools: Bash (show command), Read (show path), Edit (show path), Write (show path), Grep (show pattern), Glob (show pattern)
- [x] Fallback for unknown tools: show tool name + truncated input

### Technical Constraints

- Tool input arrives as partial JSON strings via `input_json_delta`
- Must accumulate all deltas before parsing
- Parse complete JSON only after `content_block_stop`
- Track tool_use blocks by index separately from text blocks

### Error Cases

- Incomplete tool input JSON → display "[Tool: X] (input parsing failed)"
- Tool input too long → truncate display with "..."

---

## Slice 3: Usage Summary

### User Behavior

When the command finishes, user sees a summary line with cost and token usage.

Example display:
```
───────────────────────────────────
Cost: $0.05 | Tokens: 7,371 in / 9 out | Duration: 2.3s
```

### Acceptance Criteria

- [x] Parse `result` event from Claude CLI wrapper
- [x] Extract `total_cost_usd`, `duration_ms`, and `usage` fields
- [x] Display formatted summary after command completes
- [x] Format cost with 2 decimal places and $ prefix
- [x] Format token counts with thousands separators
- [x] Format duration in seconds with 1 decimal place
- [x] Visual separator before summary (horizontal line)

### Technical Constraints

- `result` event structure:
  ```json
  {
    "type": "result",
    "total_cost_usd": 0.053,
    "duration_ms": 2255,
    "usage": {
      "input_tokens": 7371,
      "output_tokens": 9
    }
  }
  ```
- Summary appears after all content, before returning to idle

### Error Cases

- Missing `result` event (known CLI bug) → no summary displayed, return to idle normally
- Missing fields in result → display available info, omit missing
