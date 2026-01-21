# Task Tool Display

Task tool results display extracted text instead of raw JSON.

## User Behavior

When Claude spawns a subagent via the Task tool, the user sees a summarized result showing what the subagent reported—not the raw JSON structure. This gives users a glanceable view of subagent activity.

**Before:** `[{"text":"Perfect! Now I have all the information needed...`

**After:**
```
✅ (1 lines, 4053 chars)
  Perfect! Now I have all the information needed. Let me provide...
  ## Summary of Modal UI Structure in ralph
  ### 1. How Existing Modals Are Structured
  (42 more lines)
```

## Acceptance Criteria

- [x] Task tool results parse the `[{"text":"..."}]` JSON format
- [x] Text from all objects in the array is concatenated
- [x] Extracted text displays using existing `format_tool_result_styled` (icon, line/char count, 3-line preview)
- [x] Malformed JSON falls back to displaying raw content

## Technical Constraints

- Modify content extraction in `main.rs` around line 593-597
- Add a helper function to extract text from Task results (similar pattern to `parse_todos_from_json` in `ui.rs`)
- Keep the change minimal—no new display logic needed, reuse existing formatting

## Error Cases

- **Malformed JSON:** Fall back to displaying the raw content string (current behavior)
- **Missing `text` field:** Skip that object, continue extracting from others
- **Empty array:** Display as empty result (existing behavior handles this)
