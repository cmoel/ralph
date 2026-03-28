//! UI rendering functions.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::{App, AppStatus, DoltServerState};
use crate::modals::{
    draw_bead_picker, draw_config_modal, draw_help_modal, draw_init_modal, draw_kanban_board,
    draw_quit_modal, draw_specs_panel, draw_tool_allow_modal,
};
use crate::tool_panel::{SelectedPanel, ToolCallStatus};

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

/// Extract text content from Task tool results.
/// Task results are JSON arrays of objects with "text" fields: `[{"text":"..."},...]`
/// Returns the concatenated text from all objects, or None if parsing fails.
pub fn extract_text_from_task_result(content: &str) -> Option<String> {
    let items: serde_json::Value = serde_json::from_str(content).ok()?;
    let array = items.as_array()?;

    let texts: Vec<&str> = array
        .iter()
        .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
        .collect();

    if texts.is_empty() {
        return None;
    }

    Some(texts.join("\n"))
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

/// Draw the main UI.
pub fn draw_ui(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Wrap;

    let command_height = 3u16; // Fixed: border + 1 content + border
    let show_tools_column = !app.tool_panel.collapsed && !app.tool_panel.entries.is_empty();

    // Two-level layout: content area (flexible) + command bar (fixed)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),                 // Content area
            Constraint::Length(command_height), // Command panel
        ])
        .split(f.area());

    // Split content area: stream panel (left) + tools panel (right)
    let content_constraints = if show_tools_column {
        vec![Constraint::Percentage(70), Constraint::Percentage(30)]
    } else {
        vec![Constraint::Percentage(100)]
    };
    let content = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(content_constraints)
        .split(outer[0]);

    let stream_area = content[0];
    let tools_area = if show_tools_column {
        Some(content[1])
    } else {
        None
    };
    let command_area = outer[1];

    // Update pane dimensions for scroll calculations
    app.main_pane_height = stream_area.height.saturating_sub(2); // Account for borders
    let new_width = stream_area.width;
    if new_width != app.main_pane_width {
        app.main_pane_width = new_width;
        app.cached_visual_line_count = None;
    }

    // === Stream Panel ===
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
    let output_border_color = if app.tool_panel.selected_panel == SelectedPanel::Main {
        Color::White
    } else {
        app.status.status_color()
    };
    let mut output_block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.status.border_type())
        .border_style(Style::default().fg(output_border_color))
        .title(
            Line::from(if let Some(wt) = &app.worktree_name {
                format!(" {} | {} ", app.session_id, wt)
            } else {
                format!(" {} ", app.session_id)
            })
            .left_aligned(),
        );

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

    f.render_widget(output_panel, stream_area);

    // Stream scrollbar
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

        f.render_stateful_widget(scrollbar, stream_area, &mut scrollbar_state);
    }

    // === Tools Panel ===
    if let Some(tools_area) = tools_area {
        app.tool_panel.height = tools_area.height;
        draw_tool_panel(f, app, tools_area);
    } else {
        app.tool_panel.height = 0;
    }

    // === Command Panel ===
    let key_style = Style::default().fg(Color::Cyan);
    let label_style = Style::default().fg(Color::DarkGray);

    let start_stop_label = match app.status {
        AppStatus::Running => "Stop",
        _ => "Start",
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
    let status_color = app.status.status_color();

    // Build command spans: "s Start  q Quit  ? Help"
    let command_spans = vec![
        Span::styled("s", key_style),
        Span::styled(format!(" {}  ", start_stop_label), label_style),
        Span::styled("q", key_style),
        Span::styled(" Quit  ", label_style),
        Span::styled("?", key_style),
        Span::styled(" Help", label_style),
    ];

    // Dolt server indicator (beads mode only)
    let dolt_spans: Vec<Span> = if app.config.behavior.mode == "beads" {
        let dim = Style::default().fg(Color::DarkGray);
        match app.dolt.state {
            DoltServerState::On => vec![
                Span::styled("Dolt ", dim),
                Span::styled("● ", Style::default().fg(Color::Green)),
                Span::styled("│ ", dim),
            ],
            DoltServerState::Off => vec![
                Span::styled("Dolt ", dim),
                Span::styled("○ ", dim),
                Span::styled("│ ", dim),
            ],
            DoltServerState::Starting | DoltServerState::Stopping => vec![
                Span::styled("Dolt ", dim),
                Span::styled("… ", Style::default().fg(Color::Yellow)),
                Span::styled("│ ", dim),
            ],
            DoltServerState::Unknown => vec![
                Span::styled("Dolt ", dim),
                Span::styled("? ", dim),
                Span::styled("│ ", dim),
            ],
        }
    } else {
        vec![]
    };
    let dolt_len: usize = dolt_spans.iter().map(|s| s.content.len()).sum();

    let commands_len: usize = command_spans.iter().map(|s| s.content.len()).sum();
    let inner_width = command_area.width.saturating_sub(2) as usize;
    let status_len = status_dot.len() + status_text.len();

    let hint_span = app
        .hint
        .as_ref()
        .map(|(msg, _)| Span::styled(msg.as_str(), Style::default().fg(Color::Yellow)));
    let hint_len = hint_span.as_ref().map_or(0, |s| s.content.len());

    let total_fixed = commands_len + hint_len + dolt_len + status_len;
    let remaining = inner_width.saturating_sub(total_fixed);
    let left_pad = remaining / 2;
    let right_pad = remaining.saturating_sub(left_pad);

    let mut line_spans = command_spans;
    line_spans.push(Span::raw(" ".repeat(left_pad)));
    if let Some(span) = hint_span {
        line_spans.push(span);
    }
    line_spans.push(Span::raw(" ".repeat(right_pad)));
    line_spans.extend(dolt_spans);
    line_spans.push(Span::styled(status_dot, Style::default().fg(status_color)));
    line_spans.push(Span::styled(status_text, Style::default().fg(status_color)));

    let command_line = Line::from(line_spans);

    // Build config error warning for bottom title
    let config_error = app
        .config_reload_error
        .as_deref()
        .or(app.project_config_error.as_deref());

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.status.border_type())
        .border_style(Style::default().fg(app.status.status_color()));

    if let Some(error) = config_error {
        let warning_style = Style::default().fg(Color::Yellow);
        // Truncate error to fit in bottom border
        let max_len = command_area.width.saturating_sub(4) as usize;
        let truncated = if error.len() > max_len {
            format!("{}…", &error[..max_len.saturating_sub(1)])
        } else {
            error.to_string()
        };
        block = block.title_bottom(Line::styled(truncated, warning_style));
    }

    let command_panel = Paragraph::new(command_line).block(block);

    f.render_widget(command_panel, command_area);

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

    // Kanban board modal
    if app.show_kanban_board {
        draw_kanban_board(f, app);
    }

    // Init modal
    if app.show_init_modal {
        draw_init_modal(f, app);
    }

    // Help modal
    if app.show_help_modal {
        draw_help_modal(f, app);
    }

    // Tool allow modal
    if app.show_tool_allow_modal {
        draw_tool_allow_modal(f, app);
    }

    // Bead picker modal
    if app.show_bead_picker {
        draw_bead_picker(f, app);
    }

    // Quit confirmation modal
    if app.show_quit_modal {
        draw_quit_modal(f, app);
    }
}

/// Draw the tools panel (right column).
fn draw_tool_panel(f: &mut Frame, app: &App, area: Rect) {
    let is_selected = app.tool_panel.selected_panel == SelectedPanel::Tools;
    let border_color = if is_selected {
        Color::White
    } else {
        app.status.status_color()
    };
    let border_type = app.status.border_type();

    let entry_count = app.tool_panel.entries.len();

    // Collapsed state: single line with count
    if entry_count == 0 || app.tool_panel.collapsed {
        let title = if entry_count == 0 {
            " Tools ".to_string()
        } else {
            format!(" Tools [{}] ", entry_count)
        };
        let collapsed = Paragraph::new("").block(
            Block::default()
                .borders(Borders::TOP)
                .border_type(border_type)
                .border_style(Style::default().fg(border_color))
                .title(title),
        );
        f.render_widget(collapsed, area);
        return;
    }

    // Build tool call lines
    let dim = Style::default().fg(Color::DarkGray);
    let cyan_bold = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let selected_idx = if is_selected {
        app.tool_panel.selected
    } else {
        None
    };

    let lines: Vec<Line> = app
        .tool_panel
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_item_selected = selected_idx == Some(i);
            let (icon, icon_style) = match entry.status {
                ToolCallStatus::Pending => ("▶", Style::default().fg(Color::Yellow)),
                ToolCallStatus::Success => ("✓", Style::default().fg(Color::Green)),
                ToolCallStatus::Error => ("✗", Style::default().fg(Color::Red)),
            };

            let name_style = if entry.status == ToolCallStatus::Error {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                cyan_bold
            };

            let mut line = if entry.summary.is_empty() {
                Line::from(vec![
                    Span::styled(format!(" {} ", icon), icon_style),
                    Span::styled(entry.tool_name.clone(), name_style),
                ])
            } else {
                Line::from(vec![
                    Span::styled(format!(" {} ", icon), icon_style),
                    Span::styled(entry.tool_name.clone(), name_style),
                    Span::styled(format!("({})", entry.summary), dim),
                ])
            };

            if is_item_selected {
                line = line.style(Style::default().bg(Color::DarkGray));
            }

            line
        })
        .collect();

    let title = format!(" Tools [{}] ", entry_count);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let inner_height = area.height.saturating_sub(2);

    // Auto-scroll: if not focused, always show the latest entries
    let scroll_offset = if !is_selected {
        entry_count.saturating_sub(inner_height as usize) as u16
    } else {
        app.tool_panel.scroll_offset
    };

    let panel = Paragraph::new(lines)
        .block(block)
        .scroll((scroll_offset, 0));

    f.render_widget(panel, area);

    // Scrollbar if content exceeds panel
    if entry_count as u16 > inner_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(entry_count)
            .position(scroll_offset as usize)
            .viewport_content_length(inner_height as usize);

        f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Extract the key argument summary from tool arguments JSON.
pub fn extract_tool_summary(tool_name: &str, input_json: &str) -> String {
    let input: serde_json::Value = match serde_json::from_str(input_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    let key_arg = match tool_name {
        "Bash" => extract_bash_arg(&input),
        "Read" => extract_file_path(&input),
        "Edit" => extract_file_path(&input),
        "Write" => extract_file_path(&input),
        "Grep" => extract_pattern(&input),
        "Glob" => extract_pattern(&input),
        _ => None,
    };

    key_arg.unwrap_or_default()
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

    // extract_text_from_task_result tests

    #[test]
    fn test_extract_text_from_task_result_single_item() {
        let json = r#"[{"text":"Hello world"}]"#;
        let result = extract_text_from_task_result(json);
        assert_eq!(result, Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_from_task_result_multiple_items() {
        let json = r#"[{"text":"First"},{"text":"Second"}]"#;
        let result = extract_text_from_task_result(json);
        assert_eq!(result, Some("First\nSecond".to_string()));
    }

    #[test]
    fn test_extract_text_from_task_result_empty_array() {
        let json = r#"[]"#;
        let result = extract_text_from_task_result(json);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_text_from_task_result_invalid_json() {
        let json = "not valid json";
        let result = extract_text_from_task_result(json);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_text_from_task_result_not_array() {
        let json = r#"{"text":"Hello"}"#;
        let result = extract_text_from_task_result(json);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_text_from_task_result_missing_text_field() {
        let json = r#"[{"other":"value"}]"#;
        let result = extract_text_from_task_result(json);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_text_from_task_result_skips_items_without_text() {
        let json = r#"[{"text":"First"},{"other":"skip"},{"text":"Third"}]"#;
        let result = extract_text_from_task_result(json);
        assert_eq!(result, Some("First\nThird".to_string()));
    }
}
