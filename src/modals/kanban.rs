//! Kanban board modal — work board view for beads mode.

use std::collections::{HashMap, HashSet};

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui::centered_rect;

/// The status category of a card, used to determine its emoji icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Ready,
    InProgress,
    Blocked,
    Human,
    Deferred,
}

impl CardKind {
    /// Returns the emoji icon for this card kind in the Triage column.
    pub fn triage_icon(self) -> &'static str {
        match self {
            CardKind::Human => "\u{2753}",    // ❓
            CardKind::Blocked => "\u{1f534}", // 🔴
            _ => "\u{1f916}",                 // 🤖 (fallback)
        }
    }
}

/// Emoji icon for epic beads (has children).
const EPIC_ICON: &str = "\u{26f0}\u{fe0f}"; // ⛰️

/// A bead item for the kanban board.
#[derive(Debug, Clone)]
pub struct KanbanCard {
    pub id: String,
    pub title: String,
    pub priority: u64,
    /// If true, this is a non-selectable section header (used in Human column).
    pub is_header: bool,
    /// Short IDs of beads blocking this one (empty if not blocked).
    pub blockers: Vec<String>,
    /// The status category, determines the emoji icon shown.
    pub kind: CardKind,
    /// Whether this bead is an epic (has children).
    pub is_epic: bool,
    /// Whether this bead has the "human" label.
    pub has_human_label: bool,
}

/// Data fetched from multiple bd commands for board population.
pub struct KanbanBoardData {
    pub in_progress: Vec<serde_json::Value>,
    pub deferred: Vec<serde_json::Value>,
    pub human: Vec<serde_json::Value>,
    pub blocked: Vec<serde_json::Value>,
    pub ready: Vec<serde_json::Value>,
    pub open_count: u64,
    pub closed_count: u64,
}

/// Strip the project prefix from a bead ID, returning just the short suffix.
/// e.g., "ralph-y3t" → "y3t", "private-lessons-gac" → "gac"
fn short_id(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(_, short)| short)
}

/// Truncate a string to fit within `max_width` display columns.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0;
    let mut result = String::new();
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > max_width {
            break;
        }
        result.push(ch);
        width += w;
    }
    // Pad with spaces if we stopped short (e.g., skipped a 2-wide char)
    while width < max_width {
        result.push(' ');
        width += 1;
    }
    result
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
    Deferred,
    Human,
    Ready,
    InProgress,
}

impl KanbanColumn {
    pub const ALL: [KanbanColumn; 4] = [
        KanbanColumn::Deferred,
        KanbanColumn::Human,
        KanbanColumn::Ready,
        KanbanColumn::InProgress,
    ];

    pub const COUNT: usize = 4;

    pub fn label(self) -> &'static str {
        match self {
            KanbanColumn::Deferred => "Deferred",
            KanbanColumn::Human => "Triage",
            KanbanColumn::Ready => "Ready",
            KanbanColumn::InProgress => "In Progress",
        }
    }
}

/// State for the kanban board modal.
#[derive(Debug)]
pub struct KanbanBoardState {
    /// Cards grouped by column (Deferred, Triage, Ready, InProgress).
    pub columns: Vec<Vec<KanbanCard>>,
    /// Currently focused column index.
    pub selected_column: usize,
    /// Currently selected card index within each column (skipping headers).
    pub selected_row: Vec<usize>,
    /// Whether data is still loading.
    pub is_loading: bool,
    /// Error message if loading failed.
    pub error: Option<String>,
    /// Detail drill-down view (when Some, renders detail instead of board).
    pub detail_view: Option<BeadDetailState>,
    /// Total open issues for footer.
    pub open_count: u64,
    /// Total closed issues for footer.
    pub closed_count: u64,
    /// Maps each bead ID to the set of its direct dependency neighbors (both directions).
    pub dep_neighbors: HashMap<String, HashSet<String>>,
}

impl KanbanBoardState {
    pub fn new_loading() -> Self {
        Self {
            columns: vec![Vec::new(); KanbanColumn::COUNT],
            selected_column: 2, // Start on Ready column
            selected_row: vec![0; KanbanColumn::COUNT],
            is_loading: true,
            error: None,
            detail_view: None,
            open_count: 0,
            closed_count: 0,
            dep_neighbors: HashMap::new(),
        }
    }

    /// Returns the currently selected card, if any (skipping headers).
    pub fn selected_card(&self) -> Option<&KanbanCard> {
        let col = self.selected_column;
        let row = self.selected_row[col];
        let card = self.columns[col].get(row)?;
        if card.is_header { None } else { Some(card) }
    }

    pub fn populate(&mut self, result: Result<KanbanBoardData, String>) {
        self.is_loading = false;
        match result {
            Ok(data) => {
                self.open_count = data.open_count;
                self.closed_count = data.closed_count;

                let mut cols: Vec<Vec<KanbanCard>> = vec![Vec::new(); KanbanColumn::COUNT];
                let mut seen: HashSet<String> = HashSet::new();

                // Collect all parent IDs to detect epics
                let all_items_iter = data
                    .in_progress
                    .iter()
                    .chain(data.deferred.iter())
                    .chain(data.human.iter())
                    .chain(data.blocked.iter())
                    .chain(data.ready.iter());
                let parent_ids: HashSet<String> = all_items_iter
                    .filter_map(|item| {
                        item.get("parent")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                // 1. in_progress → InProgress (index 3)
                for item in &data.in_progress {
                    if let Some(mut card) = parse_card(item, CardKind::InProgress) {
                        card.is_epic = parent_ids.contains(&card.id);
                        seen.insert(card.id.clone());
                        cols[3].push(card);
                    }
                }

                // 2. deferred → Deferred (index 0)
                for item in &data.deferred {
                    if let Some(mut card) = parse_card(item, CardKind::Deferred)
                        && seen.insert(card.id.clone())
                    {
                        card.is_epic = parent_ids.contains(&card.id);
                        cols[0].push(card);
                    }
                }

                // 3. human → Human / Decisions section (index 1)
                let mut human_decisions: Vec<KanbanCard> = Vec::new();
                for item in &data.human {
                    if let Some(mut card) = parse_card(item, CardKind::Human)
                        && seen.insert(card.id.clone())
                    {
                        card.is_epic = parent_ids.contains(&card.id);
                        human_decisions.push(card);
                    }
                }

                // 4. blocked → Human / Blocked section (index 1)
                let mut human_blocked: Vec<KanbanCard> = Vec::new();
                for item in &data.blocked {
                    if let Some(mut card) = parse_card(item, CardKind::Blocked)
                        && seen.insert(card.id.clone())
                    {
                        card.is_epic = parent_ids.contains(&card.id);
                        human_blocked.push(card);
                    }
                }

                // Sort each section by priority
                human_decisions.sort_by_key(|c| c.priority);
                human_blocked.sort_by_key(|c| c.priority);

                // Build Triage column: decisions on top, separator, blocked on bottom
                cols[1].extend(human_decisions);
                if !cols[1].is_empty() && !human_blocked.is_empty() {
                    cols[1].push(KanbanCard {
                        id: String::new(),
                        title: String::new(),
                        priority: 0,
                        is_header: true,
                        blockers: Vec::new(),
                        kind: CardKind::Blocked,
                        is_epic: false,
                        has_human_label: false,
                    });
                }
                cols[1].extend(human_blocked);

                // 5. ready → Ready (index 2)
                for item in &data.ready {
                    if let Some(mut card) = parse_card(item, CardKind::Ready)
                        && seen.insert(card.id.clone())
                    {
                        card.is_epic = parent_ids.contains(&card.id);
                        cols[2].push(card);
                    }
                }

                // Sort non-Human columns by priority
                cols[0].sort_by_key(|c| c.priority);
                cols[2].sort_by_key(|c| c.priority);
                cols[3].sort_by_key(|c| c.priority);

                // Build bidirectional dependency neighbor map from raw JSON
                let mut dep_neighbors: HashMap<String, HashSet<String>> = HashMap::new();
                let all_items = data
                    .in_progress
                    .iter()
                    .chain(data.deferred.iter())
                    .chain(data.human.iter())
                    .chain(data.blocked.iter())
                    .chain(data.ready.iter());
                for item in all_items {
                    if let Some(id) = item.get("id").and_then(|v| v.as_str())
                        && let Some(blockers) = item.get("blocked_by").and_then(|v| v.as_array())
                    {
                        for b in blockers {
                            if let Some(bid) = b.as_str() {
                                dep_neighbors
                                    .entry(id.to_string())
                                    .or_default()
                                    .insert(bid.to_string());
                                dep_neighbors
                                    .entry(bid.to_string())
                                    .or_default()
                                    .insert(id.to_string());
                            }
                        }
                    }
                }
                self.dep_neighbors = dep_neighbors;

                self.columns = cols;
                self.selected_row = vec![0; KanbanColumn::COUNT];

                // Ensure selected_row starts on a non-header card
                for col_idx in 0..KanbanColumn::COUNT {
                    self.advance_to_card(col_idx);
                }
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    /// Advance selected_row for a column to the next non-header card.
    fn advance_to_card(&mut self, col: usize) {
        let len = self.columns[col].len();
        while self.selected_row[col] < len && self.columns[col][self.selected_row[col]].is_header {
            self.selected_row[col] += 1;
        }
        // If we went past the end, reset to 0 (column might be all headers)
        if self.selected_row[col] >= len {
            self.selected_row[col] = 0;
        }
    }

    pub fn move_left(&mut self) {
        if self.selected_column > 0 {
            self.selected_column -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.selected_column < KanbanColumn::COUNT - 1 {
            self.selected_column += 1;
        }
    }

    pub fn move_up(&mut self) {
        let col = self.selected_column;
        let mut row = self.selected_row[col];
        // Move up, skipping headers
        loop {
            if row == 0 {
                break;
            }
            row -= 1;
            if !self.columns[col][row].is_header {
                self.selected_row[col] = row;
                break;
            }
        }
    }

    pub fn move_down(&mut self) {
        let col = self.selected_column;
        let len = self.columns[col].len();
        let mut row = self.selected_row[col];
        // Move down, skipping headers
        loop {
            if row + 1 >= len {
                break;
            }
            row += 1;
            if !self.columns[col][row].is_header {
                self.selected_row[col] = row;
                break;
            }
        }
    }
}

/// Parse a JSON bead item into a KanbanCard with the given kind.
fn parse_card(item: &serde_json::Value, kind: CardKind) -> Option<KanbanCard> {
    let id = item.get("id").and_then(|v| v.as_str())?.to_string();
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let priority = item.get("priority").and_then(|v| v.as_u64()).unwrap_or(4);
    let blockers = item
        .get("blocked_by")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| short_id(s).to_string()))
                .collect()
        })
        .unwrap_or_default();
    let has_human_label = item
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|v| v.as_str() == Some("human")))
        .unwrap_or(false);
    Some(KanbanCard {
        id,
        title,
        priority,
        is_header: false,
        blockers,
        kind,
        is_epic: false, // Set later in populate() after collecting parent IDs
        has_human_label,
    })
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
            app.stop_kanban_watcher();
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
        let col_count = KanbanColumn::COUNT;
        // Divider between Human (index 1) and Ready (index 2) is double-line (║)
        // Other separators are single-line (│) — each is 1 char wide
        let separators = col_count.saturating_sub(1);
        let usable = inner_width.saturating_sub(separators);

        // Accordion layout: selected column gets ~45% of width, others split the rest
        let expanded_width = usable * 45 / 100;
        let collapsed_width = usable.saturating_sub(expanded_width) / (col_count - 1);
        // Distribute any rounding remainder to the expanded column
        let leftover = usable.saturating_sub(expanded_width + collapsed_width * (col_count - 1));
        let col_widths: Vec<usize> = (0..col_count)
            .map(|i| {
                if i == state.selected_column {
                    expanded_width + leftover
                } else {
                    collapsed_width
                }
            })
            .collect();

        // Count real cards (not headers) per column for display
        let card_counts: Vec<usize> = state
            .columns
            .iter()
            .map(|col| col.iter().filter(|c| !c.is_header).count())
            .collect();

        // Header row
        let mut header_spans: Vec<Span> = Vec::new();
        for (i, col) in KanbanColumn::ALL.iter().enumerate() {
            let is_selected = i == state.selected_column;
            let w = col_widths[i];
            let label = format!("{} ({})", col.label(), card_counts[i]);
            let padded = format!("{:^width$}", label, width = w);

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
                let sep_char = if i == 1 { "\u{2551}" } else { "\u{2502}" };
                header_spans.push(Span::styled(sep_char, Style::default().fg(Color::DarkGray)));
            }
        }
        content.push(Line::from(header_spans));

        // Separator line
        let mut sep_spans: Vec<Span> = Vec::new();
        for (i, &w) in col_widths.iter().enumerate() {
            sep_spans.push(Span::styled(
                "\u{2500}".repeat(w),
                Style::default().fg(Color::DarkGray),
            ));
            if i < col_count - 1 {
                let junction = if i == 1 { "\u{256b}" } else { "\u{253c}" };
                sep_spans.push(Span::styled(junction, Style::default().fg(Color::DarkGray)));
            }
        }
        content.push(Line::from(sep_spans));

        // Card rows
        let max_rows = inner_height.saturating_sub(3); // header + separator + footer
        let max_cards = state.columns.iter().map(|c| c.len()).max().unwrap_or(0);
        let visible_rows = max_cards.min(max_rows);

        // Compute highlighted dependency neighbors for the selected card
        let highlighted_ids: HashSet<&str> = state
            .selected_card()
            .and_then(|card| state.dep_neighbors.get(&card.id))
            .map(|neighbors| neighbors.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        for row in 0..visible_rows {
            let mut row_spans: Vec<Span> = Vec::new();
            for (col_idx, column) in state.columns.iter().enumerate() {
                let is_active_col = col_idx == state.selected_column;
                let is_selected_row = is_active_col && row == state.selected_row[col_idx];
                let w = col_widths[col_idx];

                if row < column.len() {
                    let card = &column[row];

                    if card.is_header {
                        // Section header — render with dimmer style, not selectable
                        let cell_text = format!(" {}", card.title);
                        let padded = if cell_text.len() >= w {
                            cell_text[..w].to_string()
                        } else {
                            format!("{:<width$}", cell_text, width = w)
                        };
                        let style = Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD);
                        row_spans.push(Span::styled(padded, style));
                    } else if !is_active_col {
                        // Collapsed column: short ID + truncated title
                        let sid = short_id(&card.id);
                        let cell_text = format!(" {} {}", sid, card.title);
                        let display_width = UnicodeWidthStr::width(cell_text.as_str());
                        let padded = if display_width >= w {
                            truncate_to_width(&cell_text, w)
                        } else {
                            let padding = w - display_width;
                            format!("{}{}", cell_text, " ".repeat(padding))
                        };

                        let is_dep_neighbor = highlighted_ids.contains(card.id.as_str());
                        let style = if is_dep_neighbor {
                            Style::default().fg(Color::Gray).bg(Color::Rgb(25, 35, 60))
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        row_spans.push(Span::styled(padded, style));
                    } else {
                        // Expanded column: full card with icon, id, title, blockers
                        let is_triage = col_idx == 1;
                        let icon_prefix = if card.is_epic {
                            EPIC_ICON.to_string()
                        } else if is_triage {
                            card.kind.triage_icon().to_string()
                        } else if card.has_human_label {
                            "\u{1f464}".to_string() // 👤
                        } else {
                            "\u{1f916}".to_string() // 🤖
                        };
                        let icon_width = UnicodeWidthStr::width(icon_prefix.as_str());

                        let sid = short_id(&card.id);
                        let id_width = sid.len() + 1; // "id "
                        let blocker_suffix = if card.blockers.is_empty() {
                            String::new()
                        } else {
                            format!(" \u{2190} {}", card.blockers.join(", "))
                        };
                        // icon + space + id + space + title + blocker_suffix
                        let fixed_width = icon_width + 1 + id_width + blocker_suffix.width();
                        let title_max = w.saturating_sub(fixed_width);
                        let title_display_width = UnicodeWidthStr::width(card.title.as_str());
                        let title = if title_display_width > title_max {
                            let truncated =
                                truncate_to_width(&card.title, title_max.saturating_sub(2));
                            format!("{}..", truncated.trim_end())
                        } else {
                            card.title.clone()
                        };
                        let cell_text =
                            format!("{} {} {}{}", icon_prefix, sid, title, blocker_suffix);

                        let display_width = UnicodeWidthStr::width(cell_text.as_str());
                        let padded = if display_width >= w {
                            truncate_to_width(&cell_text, w)
                        } else {
                            let padding = w - display_width;
                            format!("{}{}", cell_text, " ".repeat(padding))
                        };

                        let is_dep_neighbor = highlighted_ids.contains(card.id.as_str());
                        let style = if is_selected_row {
                            Style::default().fg(Color::Black).bg(Color::White)
                        } else if is_dep_neighbor {
                            Style::default().fg(Color::White).bg(Color::Rgb(25, 35, 60))
                        } else {
                            Style::default().fg(Color::White)
                        };

                        row_spans.push(Span::styled(padded, style));
                    }
                } else {
                    row_spans.push(Span::raw(" ".repeat(w)));
                }

                if col_idx < col_count - 1 {
                    let sep_char = if col_idx == 1 { "\u{2551}" } else { "\u{2502}" };
                    row_spans.push(Span::styled(sep_char, Style::default().fg(Color::DarkGray)));
                }
            }
            content.push(Line::from(row_spans));
        }

        // Fill remaining height with empty rows (leaving room for footer)
        for _ in visible_rows..max_rows {
            let mut row_spans: Vec<Span> = Vec::new();
            for (col_idx, &w) in col_widths.iter().enumerate() {
                row_spans.push(Span::raw(" ".repeat(w)));
                if col_idx < col_count - 1 {
                    let sep_char = if col_idx == 1 { "\u{2551}" } else { "\u{2502}" };
                    row_spans.push(Span::styled(sep_char, Style::default().fg(Color::DarkGray)));
                }
            }
            content.push(Line::from(row_spans));
        }

        // Footer
        let footer_text = format!(
            " {} open \u{b7} {} closed \u{b7} bd list --all",
            state.open_count, state.closed_count
        );
        let footer_padded = format!("{:<width$}", footer_text, width = inner_width);
        content.push(Line::from(Span::styled(
            footer_padded,
            Style::default().fg(Color::DarkGray),
        )));
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
        let mut meta: Vec<Span> = vec![Span::styled(
            short_id(&detail.id),
            Style::default().fg(Color::Cyan),
        )];
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

/// Run a bd command and parse the JSON array output.
fn run_bd_json(bd_path: &str, args: &[&str]) -> Result<Vec<serde_json::Value>, String> {
    let output = std::process::Command::new(bd_path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run bd: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("bd {} failed: {stderr}", args.join(" ")));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
        .map_err(|e| format!("Failed to parse bd output: {e}"))
}

/// Watch .beads/ directory for changes and send notifications (called from background thread).
/// Debounces events — waits 200ms after the last change before notifying.
pub fn watch_beads_directory(
    tx: std::sync::mpsc::Sender<()>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use notify::{Config, RecursiveMode, Watcher};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let beads_dir = match std::env::current_dir() {
        Ok(dir) => dir.join(".beads"),
        Err(_) => return,
    };

    if !beads_dir.exists() {
        return;
    }

    let (event_tx, event_rx) = mpsc::channel();
    let mut watcher = match notify::RecommendedWatcher::new(event_tx, Config::default()) {
        Ok(w) => w,
        Err(_) => return,
    };

    if watcher.watch(&beads_dir, RecursiveMode::Recursive).is_err() {
        return;
    }

    let debounce_duration = Duration::from_millis(200);
    let mut last_event: Option<Instant> = None;

    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        match event_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(_)) => {
                last_event = Some(Instant::now());
            }
            Ok(Err(_)) => {} // notify error, ignore
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(last) = last_event
            && last.elapsed() >= debounce_duration
        {
            let _ = tx.send(());
            last_event = None;
        }
    }
    // watcher is dropped here, stopping the OS-level watch
}

/// Fetch board data from multiple bd commands (called from background thread).
pub fn fetch_board_data(bd_path: &str) -> Result<KanbanBoardData, String> {
    use std::thread;

    let p = bd_path.to_string();
    let p1 = p.clone();
    let p2 = p.clone();
    let p3 = p.clone();
    let p4 = p.clone();
    let p5 = p.clone();

    // Run all commands in parallel
    let h_in_progress =
        thread::spawn(move || run_bd_json(&p1, &["list", "--json", "--status", "in_progress"]));
    let h_deferred = thread::spawn(move || run_bd_json(&p2, &["list", "--deferred", "--json"]));
    let h_human = thread::spawn(move || run_bd_json(&p3, &["human", "list", "--json"]));
    let h_blocked = thread::spawn(move || run_bd_json(&p4, &["blocked", "--json"]));
    let h_ready = thread::spawn(move || run_bd_json(&p5, &["ready", "--json"]));
    let h_stats = thread::spawn(move || {
        let output = std::process::Command::new(&p)
            .args(["stats", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                serde_json::from_str::<serde_json::Value>(&stdout).ok()
            }
            _ => None,
        }
    });

    let in_progress = h_in_progress.join().map_err(|_| "thread panic")??;
    let deferred = h_deferred.join().map_err(|_| "thread panic")??;
    let human = h_human.join().map_err(|_| "thread panic")??;
    let blocked = h_blocked.join().map_err(|_| "thread panic")??;
    let ready = h_ready.join().map_err(|_| "thread panic")??;
    let stats = h_stats.join().map_err(|_| "thread panic")?;

    let (open_count, closed_count) = stats
        .and_then(|s| s.get("summary").cloned())
        .map(|s| {
            let open = s.get("open_issues").and_then(|v| v.as_u64()).unwrap_or(0)
                + s.get("in_progress_issues")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                + s.get("blocked_issues")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                + s.get("deferred_issues")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            let closed = s.get("closed_issues").and_then(|v| v.as_u64()).unwrap_or(0);
            (open, closed)
        })
        .unwrap_or((0, 0));

    Ok(KanbanBoardData {
        in_progress,
        deferred,
        human,
        blocked,
        ready,
        open_count,
        closed_count,
    })
}
