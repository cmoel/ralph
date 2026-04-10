use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{App, AppStatus, DoltServerState};
use crate::modals::{
    draw_bead_picker, draw_config_modal, draw_help_modal, draw_init_modal, draw_kanban_board,
    draw_quit_modal, draw_tool_allow_modal, draw_workers_stream,
};

use super::tool_display::format_elapsed;

/// Calculate a centered rectangle within the given area.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

/// Draw the main UI.
pub fn draw_ui(f: &mut Frame, app: &mut App) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let command_height = 3u16; // Fixed: border + 1 content + border

    // Two-level layout: content area (flexible) + command bar (fixed)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),                 // Content area
            Constraint::Length(command_height), // Command panel
        ])
        .split(f.area());

    let content_area = outer[0];
    let command_area = outer[1];

    // === Board (primary content area) ===
    draw_kanban_board(f, app, content_area);

    // === Command Panel ===
    let w = app.selected_worker;
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
            if let Some(start_time) = app.workers[w].run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "RUNNING".to_string()
            }
        }
        AppStatus::Error => {
            if let Some(start_time) = app.workers[w].run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "ERROR".to_string()
            }
        }
    };
    let status_color = app.status.status_color();

    // Build command spans: "S Start  q Quit  ? Help"
    let command_spans = vec![
        Span::styled("S", key_style),
        Span::styled(format!(" {}  ", start_stop_label), label_style),
        Span::styled("q", key_style),
        Span::styled(" Quit  ", label_style),
        Span::styled("?", key_style),
        Span::styled(" Help", label_style),
    ];

    // Dolt server indicator
    let dim = Style::default().fg(Color::DarkGray);
    let dolt_spans: Vec<Span> = match app.dolt.state {
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
    let config_error = app.project_config_error.as_deref();

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

    // Workers stream modal
    if app.show_workers_stream {
        draw_workers_stream(f, app);
    }

    // Quit confirmation modal
    if app.show_quit_modal {
        draw_quit_modal(f, app);
    }
}
