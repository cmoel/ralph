//! Stale agent recovery modal.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::agent::StaleAgent;
use crate::app::App;
use crate::ui::centered_rect;

/// State for the stale agent recovery modal.
pub struct StaleModalState {
    /// List of stale agents with hooked beads.
    pub agents: Vec<StaleAgent>,
    /// Currently selected agent index.
    pub selected: usize,
    /// Status message after an action.
    pub message: Option<String>,
}

impl StaleModalState {
    pub fn new(agents: Vec<StaleAgent>) -> Self {
        Self {
            agents,
            selected: 0,
            message: None,
        }
    }

    /// Advance to next stale agent (or close if none left).
    pub fn advance(&mut self) {
        self.agents.remove(self.selected);
        if !self.agents.is_empty() && self.selected >= self.agents.len() {
            self.selected = self.agents.len() - 1;
        }
        self.message = None;
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub fn current(&self) -> Option<&StaleAgent> {
        self.agents.get(self.selected)
    }
}

/// Draw the stale agent recovery modal.
pub fn draw_stale_modal(f: &mut Frame, app: &App) {
    let state = match &app.stale_modal_state {
        Some(s) => s,
        None => return,
    };

    let agent = match state.current() {
        Some(a) => a,
        None => return,
    };

    let modal_width: u16 = 52;
    let modal_height: u16 = 8;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    f.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(Color::Cyan);
    let dim_style = Style::default().fg(Color::DarkGray);

    let title = format!(
        " Stale Agent ({}/{}) ",
        state.selected + 1,
        state.agents.len()
    );

    // Truncate bead info to fit modal width
    let max_inner = (modal_width as usize).saturating_sub(6);
    let bead_display = format!("{} \"{}\"", agent.hooked_bead_id, agent.hooked_bead_title);
    let bead_display = if bead_display.len() > max_inner {
        format!("{}...", &bead_display[..max_inner.saturating_sub(3)])
    } else {
        bead_display
    };

    let content: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Agent "),
            Span::styled(agent.agent_bead_id.clone(), dim_style),
        ]),
        Line::from(vec![
            Span::raw("  Hooked: "),
            Span::styled(bead_display, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("r", key_style),
            Span::raw(" resume  "),
            Span::styled("x", key_style),
            Span::raw(" release  "),
            Span::styled("n", key_style),
            Span::raw(" skip  "),
            Span::styled("Esc", key_style),
            Span::raw(" close"),
        ]),
    ];

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
