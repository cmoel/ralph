# Formatted Streaming Output

Formatted, easy-to-read streaming output with tool call/result correlation.

## Slice 1: Tool Result Display

### User Behavior

When Claude uses a tool, users see the result displayed after the tool call. Currently tool results are invisible (skipped). After this slice, users see what tools actually return.

Example output:
```
[Tool: Read] src/main.rs
[Result: Read] (423 lines, 12847 chars)
  fn main() {
      let config = Config::load();
      let app = App::new(config);
  (420 more lines)
```

### Acceptance Criteria

- [ ] Parse `User` events instead of skipping them
- [ ] Extract tool result content and metadata (tool name, content)
- [ ] Display result summary: `[Result: ToolName] (N lines, M chars)`
- [ ] Show first 3 lines of content as preview
- [ ] Show "(N more lines)" when truncated
- [ ] Handle malformed results: show `[Result: ToolName] error parsing result` + truncated raw content
- [ ] Handle orphan results (unknown ID): display with `[Result: unknown]` and log warning

### Technical Constraints

- Modify `ClaudeEvent::User` handling in `main.rs` (currently skipped at line 534)
- Add result formatting function in `ui.rs` alongside `format_tool_summary`
- Keep TodoWrite handling unchanged (updates tasks panel, not output)

### Error Cases

- Malformed JSON in result content → show error indicator + first 100 chars raw
- Empty result content → show `(empty)`
- Result for TodoWrite → skip display (already handled by tasks panel)

---

## Slice 2: Correlation

Depends on Slice 1.

### User Behavior

Tool calls and their results display together as a unit, making cause-and-effect clear.

Example output:
```
[Tool: Read] src/main.rs
  [Result] (423 lines, 12847 chars)
    fn main() {
        let config = Config::load();
    (421 more lines)
```

When the process exits with pending tool calls:
```
[Tool: Bash] cargo build
  ⚠ no result received
```

### Acceptance Criteria

- [ ] Buffer tool calls by ID when `ContentBlockStop` fires
- [ ] When result arrives, find matching call and display together
- [ ] Indent result under its tool call
- [ ] On process exit (`Result` event), flush pending calls with "⚠ no result received"
- [ ] Tool calls without ID display immediately (no buffering)

### Technical Constraints

- Add `pending_tool_calls: HashMap<String, PendingToolCall>` to `App` state
- `PendingToolCall` stores: tool name, formatted args, timestamp
- Clear pending calls map on `MessageStart` (new assistant turn)

### Error Cases

- Tool call has no ID → display immediately without waiting for result
- Result arrives before call (out of order) → buffer result, match when call arrives
- Multiple pending calls → each matches independently by ID

---

## Slice 3: Icon-based Format

Depends on Slice 2.

### User Behavior

Tool calls and results use icons for cleaner, more scannable output.

Example output:
```
⏺ Read(src/main.rs)
  ✅ (423 lines, 12847 chars)
    fn main() {
        let config = Config::load();
    (421 more lines)

⏺ Bash(cargo build)
  ❌ error: could not compile
    error[E0382]: borrow of moved value
    (15 more lines)
```

### Acceptance Criteria

- [ ] Tool calls display as: `⏺ ToolName(key_arg)`
- [ ] Successful results display as: `✅ (N lines, M chars)`
- [ ] Error results display as: `❌` followed by content
- [ ] No result displays as: `⚠ no result received`
- [ ] Key argument extraction per tool:
  - Bash: `command` (truncated to 50 chars)
  - Read: `file_path`
  - Edit: `file_path`
  - Write: `file_path`
  - Grep: `pattern`
  - Glob: `pattern`

### Technical Constraints

- Update `format_tool_summary` to return icon-based format
- Add `format_tool_result` for result formatting
- Reuse existing key argument extractors from `ui.rs`

### Error Cases

- Unknown tool type → show `⏺ ToolName` without key arg
- Missing key argument → show `⏺ ToolName` without parens

---

## Slice 4: Color Coding

Depends on Slice 3.

### User Behavior

Different message types have distinct colors for faster visual scanning.

| Element | Color |
|---------|-------|
| Tool call icon (⏺) | Cyan |
| Tool name | Cyan bold |
| Success icon (✅) | Green |
| Error icon (❌) | Red |
| Warning icon (⚠) | Yellow |
| Result metadata | Dim/gray |
| Preview content | Default |
| Assistant text | Green |

### Acceptance Criteria

- [ ] Tool calls render in cyan
- [ ] Success results render with green icon
- [ ] Error results render with red icon
- [ ] Warnings render with yellow icon
- [ ] Result metadata (line count, char count) renders dim
- [ ] Preview content renders in default color
- [ ] Colors work with Ratatui's `Style` system

### Technical Constraints

- Use `ratatui::style::{Color, Style, Modifier}`
- Return `Vec<Span>` or `Line` from formatters instead of `String`
- Update `app.add_line()` to accept styled content or keep plain text with inline ANSI

### Error Cases

- Terminal doesn't support colors → Ratatui handles gracefully

---

## Slice 5: Assistant Text Formatting

Depends on Slice 4.

### User Behavior

Assistant text messages display with consistent formatting that matches tool output style.

Example output:
```
⏺ Assistant
  I'll read the configuration file to understand the current setup.

⏺ Read(src/config.rs)
  ✅ (45 lines, 1203 chars)
    pub struct Config {
    ...
```

### Acceptance Criteria

- [ ] Assistant text blocks display with `⏺ Assistant` header
- [ ] Text content indented under header
- [ ] Long text truncated with "(N more lines)" for consistency
- [ ] Multiple text blocks in same message grouped appropriately

### Technical Constraints

- Modify `ContentBlockDelta` handling for text blocks
- Buffer text until `ContentBlockStop` to format as unit
- Or: stream text but prepend header on first chunk

### Error Cases

- Empty assistant text → skip display
- Very long unbroken line → wrap or truncate at reasonable width

---

## Out of Scope

- Additional tools beyond Bash, Read, Edit, Write, Grep, Glob (can add later)
- Time-based "waiting..." indicators for long-running tools
- Syntax highlighting for code in previews
- Collapsible/expandable result content
