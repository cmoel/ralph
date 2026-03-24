//! Quit confirmation modal.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// Draw the quit confirmation modal.
pub fn draw_quit_modal(f: &mut Frame, _app: &App) {
    let modal_width: u16 = 30;
    let modal_height: u16 = 5;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    f.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(Color::Cyan);

    let content: Vec<Line> = vec![
        Line::from(""),
        Line::from("  Quit ralph?"),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", key_style),
            Span::raw(" yes  "),
            Span::styled("n", key_style),
            Span::raw(" no"),
        ]),
    ];

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Quit ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
