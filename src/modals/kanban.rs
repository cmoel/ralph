//! Kanban board modal — work board view for beads mode.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// A bead item for the kanban board.
#[derive(Debug, Clone)]
pub struct KanbanCard {
    pub id: String,
    pub title: String,
}

/// Kanban board columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KanbanColumn {
    Blocked,
    NeedsBrainDump,
    NeedsShaping,
    Ready,
    InProgress,
}

impl KanbanColumn {
    pub const ALL: [KanbanColumn; 5] = [
        KanbanColumn::Blocked,
        KanbanColumn::NeedsBrainDump,
        KanbanColumn::NeedsShaping,
        KanbanColumn::Ready,
        KanbanColumn::InProgress,
    ];

    pub fn label(self) -> &'static str {
        match self {
            KanbanColumn::Blocked => "Blocked",
            KanbanColumn::NeedsBrainDump => "Brain Dump",
            KanbanColumn::NeedsShaping => "Shaping",
            KanbanColumn::Ready => "Ready",
            KanbanColumn::InProgress => "In Progress",
        }
    }
}

/// State for the kanban board modal.
#[derive(Debug)]
pub struct KanbanBoardState {
    /// Cards grouped by column.
    pub columns: Vec<Vec<KanbanCard>>,
    /// Currently focused column index.
    pub selected_column: usize,
    /// Currently selected card index within each column.
    pub selected_row: Vec<usize>,
    /// Whether data is still loading.
    pub is_loading: bool,
    /// Error message if loading failed.
    pub error: Option<String>,
}

impl KanbanBoardState {
    pub fn new_loading() -> Self {
        Self {
            columns: vec![Vec::new(); 5],
            selected_column: 3, // Start on Ready column
            selected_row: vec![0; 5],
            is_loading: true,
            error: None,
        }
    }

    pub fn populate(&mut self, result: Result<Vec<serde_json::Value>, String>) {
        self.is_loading = false;
        match result {
            Ok(items) => {
                let mut cols: Vec<Vec<KanbanCard>> = vec![Vec::new(); 5];
                for item in &items {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let title = item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let status = item
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("open");
                    let labels: Vec<&str> = item
                        .get("labels")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter().filter_map(|l| l.as_str()).collect())
                        .unwrap_or_default();

                    // Skip closed/deferred items
                    if status == "closed" || status == "deferred" {
                        continue;
                    }

                    let card = KanbanCard { id, title };

                    // Priority: labels first, then status
                    if labels.contains(&"needs-brain-dump") {
                        cols[1].push(card);
                    } else if labels.contains(&"needs-shaping")
                        || labels.contains(&"shaping-required")
                    {
                        cols[2].push(card);
                    } else if status == "blocked" {
                        cols[0].push(card);
                    } else if status == "in_progress" {
                        cols[4].push(card);
                    } else {
                        // open with no special labels → Ready
                        cols[3].push(card);
                    }
                }
                self.columns = cols;
                self.selected_row = vec![0; 5];
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    pub fn move_left(&mut self) {
        if self.selected_column > 0 {
            self.selected_column -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.selected_column < 4 {
            self.selected_column += 1;
        }
    }

    pub fn move_up(&mut self) {
        let col = self.selected_column;
        if self.selected_row[col] > 0 {
            self.selected_row[col] -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let col = self.selected_column;
        let len = self.columns[col].len();
        if len > 0 && self.selected_row[col] < len - 1 {
            self.selected_row[col] += 1;
        }
    }
}

/// Handle keyboard input for the kanban board modal.
pub fn handle_kanban_input(app: &mut App, key_code: KeyCode) {
    let Some(state) = &mut app.kanban_board_state else {
        return;
    };

    match key_code {
        KeyCode::Esc => {
            app.show_kanban_board = false;
            app.kanban_board_state = None;
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.move_left();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.move_right();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_up();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.move_down();
        }
        _ => {}
    }
}

/// Draw the kanban board modal.
pub fn draw_kanban_board(f: &mut Frame, app: &App) {
    let Some(state) = &app.kanban_board_state else {
        return;
    };

    // Use most of the screen
    let area = f.area();
    let modal_width = area.width.saturating_sub(4).min(120);
    let modal_height = area.height.saturating_sub(4).min(40);
    let modal_area = centered_rect(modal_width, modal_height, area);

    f.render_widget(Clear, modal_area);

    let inner_height = modal_height.saturating_sub(2) as usize;
    let inner_width = modal_width.saturating_sub(2) as usize;

    let mut content: Vec<Line> = Vec::new();

    if state.is_loading {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(error) = &state.error {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!("  Error: {error}"),
            Style::default().fg(Color::Red),
        )));
    } else {
        let col_count = KanbanColumn::ALL.len();
        // Each column gets equal width with separators between
        let separators = col_count.saturating_sub(1);
        let col_width = inner_width.saturating_sub(separators) / col_count;

        // Header row
        let mut header_spans: Vec<Span> = Vec::new();
        for (i, col) in KanbanColumn::ALL.iter().enumerate() {
            let is_selected = i == state.selected_column;
            let count = state.columns[i].len();
            let label = format!("{} ({})", col.label(), count);
            let padded = format!("{:^width$}", label, width = col_width);

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            };
            header_spans.push(Span::styled(padded, style));
            if i < col_count - 1 {
                header_spans.push(Span::styled(
                    "\u{2502}",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        content.push(Line::from(header_spans));

        // Separator line
        let mut sep = String::new();
        for i in 0..col_count {
            sep.push_str(&"\u{2500}".repeat(col_width));
            if i < col_count - 1 {
                sep.push('\u{253c}');
            }
        }
        content.push(Line::from(Span::styled(
            sep,
            Style::default().fg(Color::DarkGray),
        )));

        // Card rows
        let max_rows = inner_height.saturating_sub(2); // header + separator
        let max_cards = state.columns.iter().map(|c| c.len()).max().unwrap_or(0);
        let visible_rows = max_cards.min(max_rows);

        for row in 0..visible_rows {
            let mut row_spans: Vec<Span> = Vec::new();
            for (col_idx, column) in state.columns.iter().enumerate() {
                let is_active_col = col_idx == state.selected_column;
                let is_selected_row = is_active_col && row == state.selected_row[col_idx];

                let cell_text = if row < column.len() {
                    let card = &column[row];
                    let id_width = card.id.len() + 1; // "id "
                    let title_max = col_width.saturating_sub(id_width + 1); // margin
                    let title = if card.title.len() > title_max {
                        format!("{}..", &card.title[..title_max.saturating_sub(2)])
                    } else {
                        card.title.clone()
                    };
                    format!("{} {}", card.id, title)
                } else {
                    String::new()
                };

                // Pad/truncate to column width
                let padded = if cell_text.len() >= col_width {
                    cell_text[..col_width].to_string()
                } else {
                    format!("{:<width$}", cell_text, width = col_width)
                };

                let style = if is_selected_row {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else if is_active_col && row < column.len() {
                    Style::default().fg(Color::White)
                } else if row < column.len() {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default()
                };

                row_spans.push(Span::styled(padded, style));
                if col_idx < KanbanColumn::ALL.len() - 1 {
                    row_spans.push(Span::styled(
                        "\u{2502}",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            content.push(Line::from(row_spans));
        }

        // Fill remaining height with empty rows
        for _ in visible_rows..max_rows {
            let mut row_spans: Vec<Span> = Vec::new();
            for col_idx in 0..col_count {
                row_spans.push(Span::raw(" ".repeat(col_width)));
                if col_idx < col_count - 1 {
                    row_spans.push(Span::styled(
                        "\u{2502}",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            content.push(Line::from(row_spans));
        }
    }

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Work Board ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
