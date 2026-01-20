//! UI rendering functions.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use crate::app::{App, AppStatus};
use crate::modal_ui::{draw_config_modal, draw_specs_panel};

/// Maximum length for truncated tool input display.
pub const TOOL_INPUT_MAX_LEN: usize = 60;

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

/// Formats a tool invocation for display.
///
/// Returns a formatted string like `[Tool: Bash] git status` for known tools,
/// or a truncated JSON representation for unknown tools.
pub fn format_tool_summary(tool_name: &str, input_json: &str) -> String {
    let prefix = format!("[Tool: {}]", tool_name);

    // Try to parse the accumulated JSON
    let input: serde_json::Value = match serde_json::from_str(input_json) {
        Ok(v) => v,
        Err(_) => return format!("{} (input parsing failed)", prefix),
    };

    // Format based on tool type
    let summary = match tool_name {
        "Bash" => format_bash_tool(&input),
        "Read" => format_read_tool(&input),
        "Edit" => format_edit_tool(&input),
        "Write" => format_write_tool(&input),
        "Grep" => format_grep_tool(&input),
        "Glob" => format_glob_tool(&input),
        _ => format_unknown_tool(&input),
    };

    format!("{} {}", prefix, summary)
}

fn format_bash_tool(input: &serde_json::Value) -> String {
    input
        .get("command")
        .and_then(|v| v.as_str())
        .map(|cmd| truncate_str(cmd, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no command)".to_string())
}

fn format_read_tool(input: &serde_json::Value) -> String {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no path)".to_string())
}

fn format_edit_tool(input: &serde_json::Value) -> String {
    let path = input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("(no path)");

    // Try to show context about what's being edited
    if let Some(old_str) = input.get("old_string").and_then(|v| v.as_str()) {
        let preview = truncate_str(old_str, 30);
        format!("{} \"{}\"", truncate_str(path, 40), preview)
    } else {
        truncate_str(path, TOOL_INPUT_MAX_LEN)
    }
}

fn format_write_tool(input: &serde_json::Value) -> String {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no path)".to_string())
}

fn format_grep_tool(input: &serde_json::Value) -> String {
    input
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no pattern)".to_string())
}

fn format_glob_tool(input: &serde_json::Value) -> String {
    input
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no pattern)".to_string())
}

fn format_unknown_tool(input: &serde_json::Value) -> String {
    let json_str = input.to_string();
    truncate_str(&json_str, TOOL_INPUT_MAX_LEN)
}

/// Formats a number with thousands separators (e.g., 7371 -> "7,371").
pub fn format_with_thousands(n: u64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

/// Formats the usage summary from a Result event.
///
/// Returns a two-line string: a separator line followed by the summary.
/// Example: "───────────────────────────────────\nCost: $0.05 | Tokens: 7,371 in / 9 out | Duration: 2.3s"
pub fn format_usage_summary(result: &crate::events::ResultEvent) -> String {
    let mut parts = Vec::new();

    // Format cost
    if let Some(cost) = result.total_cost_usd {
        parts.push(format!("Cost: ${:.2}", cost));
    }

    // Format tokens
    if let Some(usage) = &result.usage {
        let input = usage
            .input_tokens
            .map(format_with_thousands)
            .unwrap_or_else(|| "?".to_string());
        let output = usage
            .output_tokens
            .map(format_with_thousands)
            .unwrap_or_else(|| "?".to_string());
        parts.push(format!("Tokens: {} in / {} out", input, output));
    }

    // Format duration
    if let Some(duration_ms) = result.duration_ms {
        let seconds = duration_ms as f64 / 1000.0;
        parts.push(format!("Duration: {:.1}s", seconds));
    }

    // Build the summary line
    let separator = "─".repeat(35);
    let summary = parts.join(" | ");
    format!("{}\n{}", separator, summary)
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

    // Increment frame counter for animations
    app.frame_count = app.frame_count.wrapping_add(1);

    // Two-panel layout: output (flexible) + command (fixed height 3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // Output panel (flexible)
            Constraint::Length(3), // Command panel (border + 1 content row + border)
        ])
        .split(f.area());

    // Update main pane dimensions for scroll calculations
    app.main_pane_height = chunks[0].height.saturating_sub(2); // Account for borders
    app.main_pane_width = chunks[0].width;

    // Output panel with session ID as title
    let mut content: Vec<Line> = app.output_lines.iter().map(Line::raw).collect();
    if !app.current_line.is_empty() {
        content.push(Line::raw(&app.current_line));
    }

    // Build iteration progress display
    let iteration_display = if app.current_iteration == 0 {
        // Not running or stopped
        "─".to_string()
    } else if app.total_iterations < 0 {
        // Infinite mode
        format!("{}/∞", app.current_iteration)
    } else {
        // Countdown mode
        format!("{}/{}", app.current_iteration, app.total_iterations)
    };

    let mut output_block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.status.border_type())
        .border_style(Style::default().fg(app.status.pulsing_color(app.frame_count)))
        .title(Line::from(format!(" {} ── {} ", app.session_id, iteration_display)).left_aligned());

    if let Some(spec) = &app.current_spec {
        output_block = output_block.title(Line::from(format!(" {} ", spec)).right_aligned());
    }

    let output_panel = Paragraph::new(content)
        .block(output_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset, 0));

    f.render_widget(output_panel, chunks[0]);

    // Scrollbar - only visible when content exceeds viewport
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

    // Command panel with keyboard shortcuts (left) and status indicator (right)
    let shortcuts = match app.status {
        AppStatus::Error => "[l] Specs  [q] Quit",
        AppStatus::Stopped => "[s] Start  [c] Config  [l] Specs  [q] Quit",
        AppStatus::Running => "[s] Stop  [l] Specs  [q] Quit",
    };

    // Status indicator: colored dot + text (elapsed time when running)
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
            // Show frozen elapsed time if available, otherwise just ERROR
            if let Some(start_time) = app.run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "ERROR".to_string()
            }
        }
    };
    let status_color = app.status.pulsing_color(app.frame_count);

    // Calculate spacing to right-align the status indicator
    // Total width minus borders (2), shortcuts length, status indicator length
    let inner_width = chunks[1].width.saturating_sub(2) as usize;
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

    f.render_widget(command_panel, chunks[1]);

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

    // format_with_thousands tests

    #[test]
    fn test_format_with_thousands_zero() {
        assert_eq!(format_with_thousands(0), "0");
    }

    #[test]
    fn test_format_with_thousands_small() {
        assert_eq!(format_with_thousands(1), "1");
        assert_eq!(format_with_thousands(12), "12");
        assert_eq!(format_with_thousands(123), "123");
        assert_eq!(format_with_thousands(999), "999");
    }

    #[test]
    fn test_format_with_thousands_thousands() {
        assert_eq!(format_with_thousands(1000), "1,000");
        assert_eq!(format_with_thousands(1234), "1,234");
        assert_eq!(format_with_thousands(7371), "7,371");
        assert_eq!(format_with_thousands(12345), "12,345");
        assert_eq!(format_with_thousands(123456), "123,456");
    }

    #[test]
    fn test_format_with_thousands_millions() {
        assert_eq!(format_with_thousands(1000000), "1,000,000");
        assert_eq!(format_with_thousands(1234567), "1,234,567");
        assert_eq!(format_with_thousands(123456789), "123,456,789");
    }

    #[test]
    fn test_format_with_thousands_billions() {
        assert_eq!(format_with_thousands(1000000000), "1,000,000,000");
    }
}
