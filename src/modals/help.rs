//! Help modal — keyboard shortcut reference.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// Draw the help modal.
pub fn draw_help_modal(f: &mut Frame, _app: &App) {
    let modal_width: u16 = 50;
    let modal_height: u16 = 25;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(Color::Cyan);
    let desc_style = Style::default().fg(Color::DarkGray);
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let inner_width = modal_width.saturating_sub(4) as usize;

    // Footer - right aligned "? or Esc to close"
    let footer_text = "? or Esc to close";
    let footer_padding = inner_width.saturating_sub(footer_text.len());

    let content: Vec<Line> = vec![
        // Control section
        Line::from(Span::styled("  Control", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("S", key_style),
            Span::styled("  Start/Stop claude", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("q", key_style),
            Span::styled("  Quit", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("D", key_style),
            Span::styled("  Toggle Dolt server (beads)", desc_style),
        ]),
        Line::from(""),
        // Panels section
        Line::from(Span::styled("  Panels", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("c", key_style),
            Span::styled("  Configuration", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("l", key_style),
            Span::styled("  Specs list (specs)", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("B", key_style),
            Span::styled("  Work board (beads)", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("i", key_style),
            Span::styled("  Initialize project", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("t", key_style),
            Span::styled("  Toggle tool panel", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Tab", key_style),
            Span::styled("  Switch panel focus", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("A", key_style),
            Span::styled("  Allow tool (tools panel)", desc_style),
        ]),
        Line::from(""),
        // Scroll section
        Line::from(Span::styled("  Scroll", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("j/k", key_style),
            Span::styled("  Scroll down/up", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("↑/↓", key_style),
            Span::styled("  Scroll down/up", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Ctrl+u/d", key_style),
            Span::styled("  Half page up/down", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Ctrl+b/f", key_style),
            Span::styled("  Full page up/down", desc_style),
        ]),
        // Footer
        Line::from(""),
        Line::from(vec![
            Span::raw(" ".repeat(footer_padding)),
            Span::styled(footer_text, desc_style),
        ]),
    ];

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
