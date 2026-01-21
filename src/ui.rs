//! UI rendering functions.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::{App, AppStatus, SelectedPanel};
use crate::modal_ui::{draw_config_modal, draw_init_modal, draw_specs_panel};

/// Maximum length for truncated tool input display.
pub const TOOL_INPUT_MAX_LEN: usize = 60;

/// Maximum length for Bash command display (spec says 50 chars).
const BASH_COMMAND_MAX_LEN: usize = 50;

/// Icon for tool calls.
const TOOL_ICON: &str = "⏺";

/// Icon for successful results.
const SUCCESS_ICON: &str = "✅";

/// Icon for error results.
const ERROR_ICON: &str = "❌";

/// Icon for warnings (no result received).
const WARNING_ICON: &str = "⚠";

/// Formats a duration as M:SS (under 1 hour) or H:MM:SS (1+ hours).
pub fn format_elapsed(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

/// Truncates a string to the given maximum length, appending "..." if truncated.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    // Replace newlines with spaces for single-line display
    let single_line: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();

    if single_line.len() <= max_len {
        single_line
    } else {
        format!("{}...", &single_line[..max_len.saturating_sub(3)])
    }
}

/// Formats a tool invocation for display (plain text version for tests).
///
/// Returns a formatted string like `⏺ Bash(git status)` for known tools,
/// or `⏺ ToolName` for unknown tools.
#[cfg(test)]
pub fn format_tool_summary(tool_name: &str, input_json: &str) -> String {
    // Try to parse the accumulated JSON
    let input: serde_json::Value = match serde_json::from_str(input_json) {
        Ok(v) => v,
        Err(_) => return format!("{} {}", TOOL_ICON, tool_name),
    };

    // Extract key argument based on tool type
    let key_arg = match tool_name {
        "Bash" => extract_bash_arg(&input),
        "Read" => extract_file_path(&input),
        "Edit" => extract_file_path(&input),
        "Write" => extract_file_path(&input),
        "Grep" => extract_pattern(&input),
        "Glob" => extract_pattern(&input),
        _ => None,
    };

    match key_arg {
        Some(arg) => format!("{} {}({})", TOOL_ICON, tool_name, arg),
        None => format!("{} {}", TOOL_ICON, tool_name),
    }
}

/// Extract command argument for Bash tool (truncated to 50 chars).
fn extract_bash_arg(input: &serde_json::Value) -> Option<String> {
    input
        .get("command")
        .and_then(|v| v.as_str())
        .map(|cmd| truncate_str(cmd, BASH_COMMAND_MAX_LEN))
}

/// Extract file_path argument.
fn extract_file_path(input: &serde_json::Value) -> Option<String> {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
}

/// Extract pattern argument.
fn extract_pattern(input: &serde_json::Value) -> Option<String> {
    input
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
}

/// Maximum number of preview lines to show for tool results.
const RESULT_PREVIEW_LINES: usize = 3;

/// Formats a tool invocation as a styled line.
///
/// Returns a styled `Line` with cyan icon and bold cyan tool name.
pub fn format_tool_summary_styled(tool_name: &str, input_json: &str) -> Line<'static> {
    let cyan = Style::default().fg(Color::Cyan);
    let cyan_bold = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    // Try to parse the accumulated JSON
    let input: serde_json::Value = match serde_json::from_str(input_json) {
        Ok(v) => v,
        Err(_) => {
            return Line::from(vec![
                Span::styled(format!("{} ", TOOL_ICON), cyan),
                Span::styled(tool_name.to_string(), cyan_bold),
            ]);
        }
    };

    // Extract key argument based on tool type
    let key_arg = match tool_name {
        "Bash" => extract_bash_arg(&input),
        "Read" => extract_file_path(&input),
        "Edit" => extract_file_path(&input),
        "Write" => extract_file_path(&input),
        "Grep" => extract_pattern(&input),
        "Glob" => extract_pattern(&input),
        _ => None,
    };

    match key_arg {
        Some(arg) => Line::from(vec![
            Span::styled(format!("{} ", TOOL_ICON), cyan),
            Span::styled(tool_name.to_string(), cyan_bold),
            Span::raw(format!("({})", arg)),
        ]),
        None => Line::from(vec![
            Span::styled(format!("{} ", TOOL_ICON), cyan),
            Span::styled(tool_name.to_string(), cyan_bold),
        ]),
    }
}

/// Formats a tool result as styled lines.
///
/// Returns a vector of styled `Line`s with colored icons and dim metadata.
pub fn format_tool_result_styled(
    _tool_name: &str,
    content: &str,
    is_error: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let (icon, icon_style) = if is_error {
        (ERROR_ICON, Style::default().fg(Color::Red))
    } else {
        (SUCCESS_ICON, Style::default().fg(Color::Green))
    };
    let dim = Style::default().fg(Color::DarkGray);

    if content.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon), icon_style),
            Span::styled("(empty)".to_string(), dim),
        ]));
        return lines;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let line_count = content_lines.len();
    let char_count = content.len();

    // Build summary line with icon
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", icon), icon_style),
        Span::styled(format!("({} lines, {} chars)", line_count, char_count), dim),
    ]));

    // Add preview lines (indented) in default color
    let preview_count = line_count.min(RESULT_PREVIEW_LINES);
    for line in content_lines.iter().take(preview_count) {
        lines.push(Line::raw(format!("  {}", line)));
    }

    // Add truncation indicator if needed
    let remaining = line_count.saturating_sub(RESULT_PREVIEW_LINES);
    if remaining > 0 {
        lines.push(Line::from(Span::styled(
            format!("  ({} more lines)", remaining),
            dim,
        )));
    }

    lines
}

/// Returns a styled warning line for tool calls with no result.
pub fn format_no_result_warning_styled() -> Line<'static> {
    let yellow = Style::default().fg(Color::Yellow);
    Line::from(vec![Span::styled(
        format!("{} no result received", WARNING_ICON),
        yellow,
    )])
}

/// Returns a styled assistant header line.
pub fn format_assistant_header_styled() -> Line<'static> {
    let green = Style::default().fg(Color::Green);
    let green_bold = Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(format!("{} ", TOOL_ICON), green),
        Span::styled("Assistant".to_string(), green_bold),
    ])
}

/// Formats a tool result for display (plain text version for tests).
///
/// Returns a vector of lines:
/// - First line: `✅ (N lines, M chars)` or `❌` followed by content for errors
/// - Following lines: indented preview of first N lines
/// - Final line: `(N more lines)` if truncated
///
/// Note: The `tool_name` parameter is kept for potential future use but not currently displayed
/// since the tool call is shown above the result.
#[cfg(test)]
pub fn format_tool_result(_tool_name: &str, content: &str, is_error: bool) -> Vec<String> {
    let mut lines = Vec::new();

    if content.is_empty() {
        let icon = if is_error { ERROR_ICON } else { SUCCESS_ICON };
        lines.push(format!("{} (empty)", icon));
        return lines;
    }

    let content_lines: Vec<&str> = content.lines().collect();
    let line_count = content_lines.len();
    let char_count = content.len();

    // Build summary line with icon
    let icon = if is_error { ERROR_ICON } else { SUCCESS_ICON };
    let summary = format!("{} ({} lines, {} chars)", icon, line_count, char_count);
    lines.push(summary);

    // Add preview lines (indented)
    let preview_count = line_count.min(RESULT_PREVIEW_LINES);
    for line in content_lines.iter().take(preview_count) {
        lines.push(format!("  {}", line));
    }

    // Add truncation indicator if needed
    let remaining = line_count.saturating_sub(RESULT_PREVIEW_LINES);
    if remaining > 0 {
        lines.push(format!("  ({} more lines)", remaining));
    }

    lines
}

/// Formats a malformed tool result (when parsing fails).
#[allow(dead_code)]
pub fn format_malformed_result(_tool_name: &str, raw_content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("{} error parsing result", ERROR_ICON));

    // Show first 100 chars of raw content
    let truncated = if raw_content.len() > 100 {
        format!("{}...", &raw_content[..100])
    } else {
        raw_content.to_string()
    };
    lines.push(format!("  {}", truncated));

    lines
}

/// Returns the warning message for tool calls with no result (plain text version for tests).
#[cfg(test)]
pub fn format_no_result_warning() -> String {
    format!("{} no result received", WARNING_ICON)
}

/// Represents a single todo item parsed from TodoWrite input.
#[derive(Debug, Clone)]
pub struct TodoItem {
    pub content: String,
    pub active_form: String,
    pub status: TodoStatus,
}

/// Status of a todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Unknown,
}

/// Parse TodoWrite JSON input into a vector of TodoItems.
/// Returns Ok(Vec<TodoItem>) on success, or Err(String) on parse failure.
pub fn parse_todos_from_json(input_json: &str) -> Result<Vec<TodoItem>, String> {
    let input: serde_json::Value =
        serde_json::from_str(input_json).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    let todos = input
        .get("todos")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing or invalid todos array".to_string())?;

    Ok(todos.iter().map(parse_todo_item).collect())
}

/// Parse a single todo item from JSON.
fn parse_todo_item(todo: &serde_json::Value) -> TodoItem {
    let content = todo
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let active_form = todo
        .get("activeForm")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| content.clone());

    let status = match todo.get("status").and_then(|v| v.as_str()) {
        Some("pending") => TodoStatus::Pending,
        Some("in_progress") => TodoStatus::InProgress,
        Some("completed") => TodoStatus::Completed,
        _ => TodoStatus::Unknown,
    };

    // If content is empty but activeForm exists, use activeForm for content
    let content = if content.is_empty() && !active_form.is_empty() {
        active_form.clone()
    } else {
        content
    };

    TodoItem {
        content,
        active_form,
        status,
    }
}

/// Exchange type for categorizing what triggered this exchange.
#[derive(Debug)]
pub enum ExchangeType {
    InitialPrompt,
    AfterTool(String),
    Continuation,
}

impl std::fmt::Display for ExchangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeType::InitialPrompt => write!(f, "initial prompt"),
            ExchangeType::AfterTool(name) => write!(f, "after {}", name),
            ExchangeType::Continuation => write!(f, "continuation"),
        }
    }
}

/// Formats the usage summary from a Result event with exchange information.
///
/// Returns a multi-line string: a separator line, exchange info, and the summary.
/// Example:
/// ```text
/// ───────────────────────────────────
/// Exchange 1 (initial prompt): 7,371 in / 892 out
/// Cost: $0.05 | Duration: 2.3s
/// ───────────────────────────────────
/// ```
pub fn format_usage_summary(
    result: &crate::events::ResultEvent,
    exchange_num: u32,
    exchange_type: ExchangeType,
) -> String {
    let separator = "─".repeat(35);

    // Format exchange header with tokens
    let tokens_str = if let Some(usage) = &result.usage {
        let input = usage
            .input_tokens
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string());
        let output = usage
            .output_tokens
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string());
        format!("{} in / {} out", input, output)
    } else {
        "— in / — out".to_string()
    };

    let exchange_line = format!(
        "Exchange {} ({}): {}",
        exchange_num, exchange_type, tokens_str
    );

    // Format additional metrics
    let mut parts = Vec::new();

    if let Some(cost) = result.total_cost_usd {
        parts.push(format!("Cost: ${:.2}", cost));
    }

    if let Some(duration_ms) = result.duration_ms {
        let seconds = duration_ms as f64 / 1000.0;
        parts.push(format!("Duration: {:.1}s", seconds));
    }

    // Build the summary
    if parts.is_empty() {
        format!("{}\n{}\n{}", separator, exchange_line, separator)
    } else {
        let metrics = parts.join(" | ");
        format!(
            "{}\n{}\n{}\n{}",
            separator, exchange_line, metrics, separator
        )
    }
}

/// Calculate a centered rectangle within the given area.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

/// Calculate the height for the tasks panel based on task count, collapsed state, and screen size.
/// Returns the height including borders.
fn calculate_tasks_panel_height(
    task_count: usize,
    is_collapsed: bool,
    available_height: u16,
) -> u16 {
    // Collapsed (either no tasks or manually collapsed): single title line
    if task_count == 0 || is_collapsed {
        return 1;
    }

    // Max ~25% of available space, minimum of 3 lines (border + 1 task + border)
    let max_height = (available_height as usize * 25 / 100).max(3);

    // Height needed: borders (2) + tasks
    let needed_height = task_count + 2;

    // Clamp between min and max
    needed_height.clamp(3, max_height) as u16
}

/// Draw the main UI.
pub fn draw_ui(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Wrap;

    // Increment frame counter for animations
    app.frame_count = app.frame_count.wrapping_add(1);

    // Calculate tasks panel height based on task count and collapsed state
    let total_height = f.area().height;
    let command_height = 3u16; // Fixed: border + 1 content + border
    let available_for_panels = total_height.saturating_sub(command_height);
    let tasks_panel_height = calculate_tasks_panel_height(
        app.tasks.len(),
        app.tasks_panel_collapsed,
        available_for_panels,
    );

    // Three-panel layout: output (flexible) + tasks (dynamic) + command (fixed)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),                     // Output panel (flexible, minimum 5 lines)
            Constraint::Length(tasks_panel_height), // Tasks panel (dynamic)
            Constraint::Length(command_height),     // Command panel (fixed 3 lines)
        ])
        .split(f.area());

    // Update pane dimensions for scroll calculations
    app.main_pane_height = chunks[0].height.saturating_sub(2); // Account for borders
    app.main_pane_width = chunks[0].width;
    app.tasks_pane_height = chunks[1].height.saturating_sub(2); // Account for borders

    // === Output Panel ===
    let mut content: Vec<Line> = app.output_lines.to_vec();
    if !app.current_line.is_empty() {
        content.push(Line::raw(&app.current_line));
    }

    // Build iteration progress display for bottom title
    let iteration_display = if app.current_iteration == 0 {
        None
    } else if app.total_iterations < 0 {
        Some(format!("{}/∞", app.current_iteration))
    } else {
        Some(format!(
            "{}/{}",
            app.current_iteration, app.total_iterations
        ))
    };

    // Build tokens display for bottom title
    let tokens_display = if app.cumulative_tokens > 0 {
        Some(format!("{} tokens", app.cumulative_tokens))
    } else {
        None
    };

    // Top title: session ID (left), spec name (right)
    // Selected panel gets brighter border
    let output_border_color = if app.selected_panel == SelectedPanel::Main {
        Color::White
    } else {
        app.status.pulsing_color(app.frame_count)
    };
    let mut output_block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.status.border_type())
        .border_style(Style::default().fg(output_border_color))
        .title(Line::from(format!(" {} ", app.session_id)).left_aligned());

    if let Some(spec) = &app.current_spec {
        output_block = output_block.title(Line::from(format!(" {} ", spec)).right_aligned());
    }

    // Bottom title: iteration count (left), cumulative tokens (right)
    if let Some(iter) = &iteration_display {
        output_block = output_block.title_bottom(Line::from(format!(" {} ", iter)).left_aligned());
    }
    if let Some(tokens) = &tokens_display {
        output_block =
            output_block.title_bottom(Line::from(format!(" {} ", tokens)).right_aligned());
    }

    let output_panel = Paragraph::new(content)
        .block(output_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset, 0));

    f.render_widget(output_panel, chunks[0]);

    // Output scrollbar
    let visual_lines = app.visual_line_count();
    if visual_lines > app.main_pane_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(visual_lines as usize)
            .position(app.scroll_offset as usize)
            .viewport_content_length(app.main_pane_height as usize);

        f.render_stateful_widget(scrollbar, chunks[0], &mut scrollbar_state);
    }

    // === Tasks Panel ===
    draw_tasks_panel(f, app, chunks[1]);

    // === Command Panel ===
    let shortcuts = match app.status {
        AppStatus::Error => "[i] Init  [l] Specs  [q] Quit",
        AppStatus::Stopped => "[s] Start  [c] Config  [i] Init  [l] Specs  [q] Quit",
        AppStatus::Running => "[s] Stop  [l] Specs  [q] Quit",
    };

    let status_dot = "● ";
    let status_text = match app.status {
        AppStatus::Stopped => "IDLE".to_string(),
        AppStatus::Running => {
            if let Some(start_time) = app.run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "RUNNING".to_string()
            }
        }
        AppStatus::Error => {
            if let Some(start_time) = app.run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "ERROR".to_string()
            }
        }
    };
    let status_color = app.status.pulsing_color(app.frame_count);

    let inner_width = chunks[2].width.saturating_sub(2) as usize;
    let status_len = status_dot.len() + status_text.len();
    let shortcuts_len = shortcuts.len();
    let spacing = inner_width.saturating_sub(shortcuts_len + status_len);

    let command_line = Line::from(vec![
        Span::styled(shortcuts, Style::default().fg(Color::DarkGray)),
        Span::raw(" ".repeat(spacing)),
        Span::styled(status_dot, Style::default().fg(status_color)),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]);

    let command_panel = Paragraph::new(command_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(app.status.border_type())
            .border_style(Style::default().fg(app.status.pulsing_color(app.frame_count))),
    );

    f.render_widget(command_panel, chunks[2]);

    // Popup dialog if needed
    if app.show_already_running_popup {
        let popup_area = centered_rect(40, 5, f.area());
        f.render_widget(Clear, popup_area);
        let popup = Paragraph::new("Command already running")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Notice")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default());
        f.render_widget(popup, popup_area);
    }

    // Config modal
    if app.show_config_modal {
        draw_config_modal(f, app);
    }

    // Specs panel modal
    if app.show_specs_panel {
        draw_specs_panel(f, app);
    }

    // Init modal
    if app.show_init_modal {
        draw_init_modal(f, app);
    }
}

/// Draw the tasks panel.
fn draw_tasks_panel(f: &mut Frame, app: &App, area: Rect) {
    let is_selected = app.selected_panel == SelectedPanel::Tasks;
    // Selected panel gets brighter border (white), unselected uses status color
    let border_color = if is_selected {
        Color::White
    } else {
        app.status.pulsing_color(app.frame_count)
    };
    let border_type = app.status.border_type();

    // Show collapsed state if no tasks OR manually collapsed
    if app.tasks.is_empty() || app.tasks_panel_collapsed {
        // Build title: "Tasks" or "Tasks [completed/total]" if there are tasks
        let title = if app.tasks.is_empty() {
            "Tasks".to_string()
        } else {
            let completed = app.completed_task_count();
            let total = app.tasks.len();
            format!("Tasks [{}/{}]", completed, total)
        };

        // Collapsed state: single line with title
        // Calculate padding to center the title
        let title_len = title.len() + 2; // +2 for spaces around title
        let left_dashes = 3;
        let right_dashes =
            area.width
                .saturating_sub(left_dashes as u16 + 1 + title_len as u16) as usize;

        let collapsed_line = Line::from(vec![
            Span::styled("━━━ ", Style::default().fg(border_color)),
            Span::styled(title, Style::default().fg(Color::DarkGray)),
            Span::styled(
                " ".to_string() + &"━".repeat(right_dashes),
                Style::default().fg(border_color),
            ),
        ]);
        let collapsed = Paragraph::new(collapsed_line);
        f.render_widget(collapsed, area);
        return;
    }

    // Build task lines with status indicators
    let task_lines: Vec<Line> = app
        .tasks
        .iter()
        .skip(app.tasks_scroll_offset as usize)
        .take(app.tasks_pane_height as usize)
        .map(|task| {
            let (prefix, color, text) = match task.status {
                TodoStatus::InProgress => ("▶", Color::Cyan, &task.active_form),
                TodoStatus::Pending => ("○", Color::DarkGray, &task.content),
                TodoStatus::Completed => ("✓", Color::Green, &task.content),
                TodoStatus::Unknown => ("?", Color::Yellow, &task.content),
            };
            Line::from(vec![
                Span::styled(format!("{} ", prefix), Style::default().fg(color)),
                Span::styled(text.as_str(), Style::default().fg(Color::White)),
            ])
        })
        .collect();

    let tasks_block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .title(Line::from(" Tasks ").left_aligned());

    let tasks_panel = Paragraph::new(task_lines).block(tasks_block);

    f.render_widget(tasks_panel, area);

    // Tasks scrollbar
    let task_count = app.tasks.len() as u16;
    if task_count > app.tasks_pane_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(task_count as usize)
            .position(app.tasks_scroll_offset as usize)
            .viewport_content_length(app.tasks_pane_height as usize);

        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // format_elapsed tests

    #[test]
    fn test_format_elapsed_zero() {
        assert_eq!(format_elapsed(Duration::from_secs(0)), "0:00");
    }

    #[test]
    fn test_format_elapsed_seconds_only() {
        assert_eq!(format_elapsed(Duration::from_secs(5)), "0:05");
        assert_eq!(format_elapsed(Duration::from_secs(45)), "0:45");
        assert_eq!(format_elapsed(Duration::from_secs(59)), "0:59");
    }

    #[test]
    fn test_format_elapsed_minutes_and_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(60)), "1:00");
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1:05");
        assert_eq!(format_elapsed(Duration::from_secs(125)), "2:05");
        assert_eq!(format_elapsed(Duration::from_secs(3599)), "59:59");
    }

    #[test]
    fn test_format_elapsed_hours() {
        assert_eq!(format_elapsed(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(format_elapsed(Duration::from_secs(3661)), "1:01:01");
        assert_eq!(format_elapsed(Duration::from_secs(7325)), "2:02:05");
        assert_eq!(format_elapsed(Duration::from_secs(36000)), "10:00:00");
    }

    // truncate_str tests

    #[test]
    fn test_truncate_str_short_string() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_exact_length() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long_string() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
        assert_eq!(truncate_str("hello world", 10), "hello w...");
    }

    #[test]
    fn test_truncate_str_with_newlines() {
        assert_eq!(truncate_str("hello\nworld", 20), "hello world");
        assert_eq!(truncate_str("a\nb\nc", 10), "a b c");
    }

    #[test]
    fn test_truncate_str_newlines_then_truncate() {
        assert_eq!(truncate_str("hello\nworld", 8), "hello...");
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 10), "");
    }

    #[test]
    fn test_truncate_str_small_max_len() {
        // max_len < 3 should still work via saturating_sub
        assert_eq!(truncate_str("hello", 2), "...");
        assert_eq!(truncate_str("hello", 3), "...");
        assert_eq!(truncate_str("hello", 4), "h...");
    }

    // format_tool_result tests

    #[test]
    fn test_format_tool_result_empty() {
        let result = format_tool_result("Read", "", false);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "✅ (empty)");
    }

    #[test]
    fn test_format_tool_result_single_line() {
        let result = format_tool_result("Read", "hello world", false);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "✅ (1 lines, 11 chars)");
        assert_eq!(result[1], "  hello world");
    }

    #[test]
    fn test_format_tool_result_three_lines() {
        let content = "line1\nline2\nline3";
        let result = format_tool_result("Bash", content, false);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "✅ (3 lines, 17 chars)");
        assert_eq!(result[1], "  line1");
        assert_eq!(result[2], "  line2");
        assert_eq!(result[3], "  line3");
    }

    #[test]
    fn test_format_tool_result_truncated() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let result = format_tool_result("Read", content, false);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], "✅ (5 lines, 29 chars)");
        assert_eq!(result[1], "  line1");
        assert_eq!(result[2], "  line2");
        assert_eq!(result[3], "  line3");
        assert_eq!(result[4], "  (2 more lines)");
    }

    #[test]
    fn test_format_tool_result_error() {
        let content = "error: could not compile";
        let result = format_tool_result("Bash", content, true);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "❌ (1 lines, 24 chars)");
        assert_eq!(result[1], "  error: could not compile");
    }

    // format_tool_summary tests

    #[test]
    fn test_format_tool_summary_bash() {
        let result = format_tool_summary("Bash", r#"{"command": "git status"}"#);
        assert_eq!(result, "⏺ Bash(git status)");
    }

    #[test]
    fn test_format_tool_summary_read() {
        let result = format_tool_summary("Read", r#"{"file_path": "/path/to/file.rs"}"#);
        assert_eq!(result, "⏺ Read(/path/to/file.rs)");
    }

    #[test]
    fn test_format_tool_summary_grep() {
        let result = format_tool_summary("Grep", r#"{"pattern": "fn main"}"#);
        assert_eq!(result, "⏺ Grep(fn main)");
    }

    #[test]
    fn test_format_tool_summary_unknown_tool() {
        let result = format_tool_summary("CustomTool", r#"{"custom": "arg"}"#);
        assert_eq!(result, "⏺ CustomTool");
    }

    #[test]
    fn test_format_tool_summary_invalid_json() {
        let result = format_tool_summary("Read", "not valid json");
        assert_eq!(result, "⏺ Read");
    }

    #[test]
    fn test_format_tool_summary_missing_arg() {
        let result = format_tool_summary("Read", r#"{"other_field": "value"}"#);
        assert_eq!(result, "⏺ Read");
    }

    #[test]
    fn test_format_no_result_warning() {
        let result = format_no_result_warning();
        assert_eq!(result, "⚠ no result received");
    }
}
