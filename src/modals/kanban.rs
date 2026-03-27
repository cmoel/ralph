//! Kanban board modal — work board view for beads mode.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::App;
use crate::ui::centered_rect;

/// A bead item for the kanban board.
#[derive(Debug, Clone)]
pub struct KanbanCard {
    pub id: String,
    pub title: String,
    pub priority: u64,
}

/// Strip the project prefix from a bead ID, returning just the short suffix.
/// e.g., "ralph-y3t" → "y3t", "private-lessons-gac" → "gac"
fn short_id(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(_, short)| short)
}

/// Parsed detail data for a single bead.
#[derive(Debug)]
pub struct BeadDetailState {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: String,
    pub issue_type: String,
    pub labels: Vec<String>,
    pub notes: String,
    pub design: String,
    pub dependencies: Vec<BeadDependency>,
    pub scroll_offset: u16,
    pub is_loading: bool,
    pub error: Option<String>,
}

/// A dependency entry in a bead detail.
#[derive(Debug)]
pub struct BeadDependency {
    pub id: String,
    pub title: String,
    pub status: String,
    pub dep_type: String,
}

impl BeadDetailState {
    pub fn new_loading(id: String) -> Self {
        Self {
            id,
            title: String::new(),
            description: String::new(),
            status: String::new(),
            priority: String::new(),
            issue_type: String::new(),
            labels: Vec::new(),
            notes: String::new(),
            design: String::new(),
            dependencies: Vec::new(),
            scroll_offset: 0,
            is_loading: true,
            error: None,
        }
    }

    pub fn populate(&mut self, result: Result<serde_json::Value, String>) {
        self.is_loading = false;
        match result {
            Ok(item) => {
                self.title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.description = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.status = item
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.priority = match item.get("priority").and_then(|v| v.as_u64()) {
                    Some(p) => format!("P{p}"),
                    None => String::new(),
                };
                self.issue_type = item
                    .get("issue_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.labels = item
                    .get("labels")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|l| l.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                self.notes = item
                    .get("notes")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.design = item
                    .get("design")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                self.dependencies = item
                    .get("dependencies")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|dep| BeadDependency {
                                id: dep
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                title: dep
                                    .get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                status: dep
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                dep_type: dep
                                    .get("dependency_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }
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
    /// Detail drill-down view (when Some, renders detail instead of board).
    pub detail_view: Option<BeadDetailState>,
}

impl KanbanBoardState {
    pub fn new_loading() -> Self {
        Self {
            columns: vec![Vec::new(); 5],
            selected_column: 3, // Start on Ready column
            selected_row: vec![0; 5],
            is_loading: true,
            error: None,
            detail_view: None,
        }
    }

    /// Returns the currently selected card, if any.
    pub fn selected_card(&self) -> Option<&KanbanCard> {
        let col = self.selected_column;
        let row = self.selected_row[col];
        self.columns[col].get(row)
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

                    let priority = item
                        .get("priority")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(4);

                    // Skip closed/deferred items
                    if status == "closed" || status == "deferred" {
                        continue;
                    }

                    let card = KanbanCard { id, title, priority };

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
                for col in &mut cols {
                    col.sort_by_key(|card| card.priority);
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

    // If detail view is open, handle detail input
    if let Some(detail) = &mut state.detail_view {
        match key_code {
            KeyCode::Esc => {
                state.detail_view = None;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                detail.scroll_offset = detail.scroll_offset.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                detail.scroll_offset = detail.scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
        return;
    }

    match key_code {
        KeyCode::Esc => {
            app.show_kanban_board = false;
            app.kanban_board_state = None;
        }
        KeyCode::Enter => {
            if let Some(card) = state.selected_card() {
                let bead_id = card.id.clone();
                state.detail_view = Some(BeadDetailState::new_loading(bead_id.clone()));
                let bd_path = app.config.behavior.bd_path.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let output = std::process::Command::new(&bd_path)
                        .args(["show", &bead_id, "--json"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .output();
                    let result = match output {
                        Ok(out) if out.status.success() => {
                            let stdout = String::from_utf8_lossy(&out.stdout);
                            serde_json::from_str::<serde_json::Value>(&stdout)
                                .map(|val| {
                                    // bd show --json returns an array; take the first element
                                    if let Some(arr) = val.as_array() {
                                        arr.first().cloned().unwrap_or(val)
                                    } else {
                                        val
                                    }
                                })
                                .map_err(|e| e.to_string())
                        }
                        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
                        Err(e) => Err(e.to_string()),
                    };
                    let _ = tx.send(result);
                });
                app.bead_detail_rx = Some(rx);
            }
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

    // If detail view is active, draw that instead
    if let Some(detail) = &state.detail_view {
        draw_bead_detail(f, detail);
        return;
    }

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
                    let sid = short_id(&card.id);
                    let id_width = sid.len() + 1; // "id "
                    let title_max = col_width.saturating_sub(id_width + 1); // margin
                    let title = if card.title.len() > title_max {
                        format!("{}..", &card.title[..title_max.saturating_sub(2)])
                    } else {
                        card.title.clone()
                    };
                    format!("{} {}", sid, title)
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

/// Draw the bead detail drill-down view.
fn draw_bead_detail(f: &mut Frame, detail: &BeadDetailState) {
    let area = f.area();
    let modal_width = area.width.saturating_sub(4).min(100);
    let modal_height = area.height.saturating_sub(2);
    let modal_area = centered_rect(modal_width, modal_height, area);

    f.render_widget(Clear, modal_area);

    let mut content: Vec<Line> = Vec::new();

    if detail.is_loading {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(error) = &detail.error {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!("  Error: {error}"),
            Style::default().fg(Color::Red),
        )));
    } else {
        // Title
        content.push(Line::from(vec![Span::styled(
            &detail.title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));
        content.push(Line::from(""));

        // Metadata line: ID · status · priority · type
        let mut meta: Vec<Span> = vec![Span::styled(short_id(&detail.id), Style::default().fg(Color::Cyan))];
        if !detail.status.is_empty() {
            meta.push(Span::styled(
                " \u{b7} ",
                Style::default().fg(Color::DarkGray),
            ));
            let status_color = match detail.status.as_str() {
                "open" => Color::Green,
                "in_progress" => Color::Yellow,
                "closed" => Color::DarkGray,
                "blocked" => Color::Red,
                "deferred" => Color::DarkGray,
                _ => Color::White,
            };
            meta.push(Span::styled(
                &detail.status,
                Style::default().fg(status_color),
            ));
        }
        if !detail.priority.is_empty() {
            meta.push(Span::styled(
                " \u{b7} ",
                Style::default().fg(Color::DarkGray),
            ));
            meta.push(Span::styled(
                &detail.priority,
                Style::default().fg(Color::Magenta),
            ));
        }
        if !detail.issue_type.is_empty() {
            meta.push(Span::styled(
                " \u{b7} ",
                Style::default().fg(Color::DarkGray),
            ));
            meta.push(Span::styled(
                &detail.issue_type,
                Style::default().fg(Color::Gray),
            ));
        }
        content.push(Line::from(meta));

        // Labels
        if !detail.labels.is_empty() {
            let mut label_spans: Vec<Span> = vec![Span::styled(
                "Labels: ",
                Style::default().fg(Color::DarkGray),
            )];
            for (i, label) in detail.labels.iter().enumerate() {
                if i > 0 {
                    label_spans.push(Span::styled(", ", Style::default().fg(Color::DarkGray)));
                }
                label_spans.push(Span::styled(label, Style::default().fg(Color::Yellow)));
            }
            content.push(Line::from(label_spans));
        }

        // Dependencies
        if !detail.dependencies.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                "Dependencies",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for dep in &detail.dependencies {
                let status_icon = match dep.status.as_str() {
                    "closed" => "\u{2713}",
                    "in_progress" => "\u{25d0}",
                    "blocked" => "\u{25cf}",
                    _ => "\u{25cb}",
                };
                let arrow = if dep.dep_type == "blocks" {
                    "\u{2190}" // ←  this issue is blocked by dep
                } else {
                    "\u{2192}" // →  this issue blocks dep
                };
                let status_color = match dep.status.as_str() {
                    "closed" => Color::DarkGray,
                    "blocked" => Color::Red,
                    _ => Color::White,
                };
                content.push(Line::from(vec![
                    Span::styled(
                        format!("  {arrow} {status_icon} "),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(&dep.id, Style::default().fg(Color::Cyan)),
                    Span::styled(" ", Style::default()),
                    Span::styled(&dep.title, Style::default().fg(status_color)),
                ]));
            }
        }

        // Description
        if !detail.description.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                "Description",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in detail.description.lines() {
                content.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        // Design
        if !detail.design.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                "Design",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in detail.design.lines() {
                content.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Gray),
                )));
            }
        }

        // Notes
        if !detail.notes.is_empty() {
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                "Notes",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in detail.notes.lines() {
                content.push(Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Gray),
                )));
            }
        }
    }

    let modal = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", short_id(&detail.id)))
                .title_alignment(Alignment::Center)
                .style(Style::default().fg(Color::White)),
        )
        .wrap(Wrap { trim: false })
        .scroll((detail.scroll_offset, 0));

    f.render_widget(modal, modal_area);
}
