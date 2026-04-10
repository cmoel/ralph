//! Kanban board modal — pipeline-based work board view.
//!
//! Columns are defined in `board_columns.toml`. Each column has a name and a list
//! of shell pipeline sources that return JSON arrays. Ralph renders the results
//! with zero knowledge of beads internals.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use serde::Deserialize;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::ui::centered_rect;

// ---------------------------------------------------------------------------
// Column definitions
// ---------------------------------------------------------------------------

/// Board configuration: a list of column definitions.
#[derive(Debug, Clone, Deserialize)]
pub struct BoardConfig {
    pub columns: Vec<ColumnDef>,
}

/// A column definition with a name and list of pipeline sources.
#[derive(Debug, Clone, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    pub sources: Vec<SourceDef>,
}

/// A pipeline source: a shell command that returns a JSON array, plus an emoji.
#[derive(Debug, Clone, Deserialize)]
pub struct SourceDef {
    pub command: String,
    pub emoji: String,
}

/// Load board column definitions.
///
/// Cascade: per-project `board_columns.toml` in the config dir → compiled-in default.
/// If the external file exists but fails to parse, falls back to the compiled-in
/// default and logs a warning.
pub fn load_board_config() -> Result<BoardConfig, toml::de::Error> {
    if let Some(path) = crate::config::resolve_board_columns_path() {
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<BoardConfig>(&contents) {
                Ok(config) => {
                    tracing::info!("Loaded custom board columns from {}", path.display());
                    return Ok(config);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to parse {}: {e}. Falling back to compiled-in default.",
                        path.display()
                    );
                }
            },
            Err(e) => {
                tracing::warn!(
                    "Failed to read {}: {e}. Falling back to compiled-in default.",
                    path.display()
                );
            }
        }
    }
    toml::from_str(include_str!("board_columns.toml"))
}

// ---------------------------------------------------------------------------
// Card type
// ---------------------------------------------------------------------------

/// A bead item for the kanban board.
#[derive(Debug, Clone)]
pub struct KanbanCard {
    pub id: String,
    pub title: String,
    pub priority: u64,
    /// Short IDs of beads blocking this one (empty if not blocked).
    pub blockers: Vec<String>,
    /// Emoji from the source definition.
    pub emoji: String,
    /// Whether this bead is an epic (has children).
    pub is_epic: bool,
    /// Whether this card represents a pipeline error (non-selectable).
    pub is_error: bool,
    /// Labels attached to this bead.
    pub labels: Vec<String>,
    /// The bead's status (e.g. "open", "blocked", "in_progress").
    pub status: String,
}

/// Data fetched from pipeline sources for board population.
pub struct KanbanBoardData {
    pub columns: Vec<Vec<KanbanCard>>,
    pub open_count: u64,
    pub closed_count: u64,
    pub dep_neighbors: HashMap<String, HashSet<String>>,
    /// Bead IDs with status=blocked but no actual blocking dependencies.
    pub manual_blocked_ids: HashSet<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Bead detail state (drill-down view)
// ---------------------------------------------------------------------------

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

                // Clamp scroll_offset so it stays within content bounds
                let approx_lines = 4 // title + metadata + spacing
                    + self.description.lines().count()
                    + self.notes.lines().count()
                    + self.design.lines().count()
                    + self.dependencies.len();
                let max_scroll = approx_lines.saturating_sub(1) as u16;
                if self.scroll_offset > max_scroll {
                    self.scroll_offset = max_scroll;
                }
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Board state
// ---------------------------------------------------------------------------

/// State for the kanban board modal.
#[derive(Debug)]
pub struct KanbanBoardState {
    /// Column definitions from the board config.
    pub column_defs: Vec<ColumnDef>,
    /// Cards grouped by column.
    pub columns: Vec<Vec<KanbanCard>>,
    /// Currently focused column index.
    pub selected_column: usize,
    /// Currently selected card index within each column (skipping error cards).
    pub selected_row: Vec<usize>,
    /// Whether data is still loading.
    pub is_loading: bool,
    /// Error message if loading failed.
    pub error: Option<String>,
    /// Which pane has keyboard focus (board columns or preview pane).
    pub focus: BoardFocus,
    /// Preview pane detail state (loaded on cursor movement).
    pub preview_detail: Option<BeadDetailState>,
    /// Bead ID currently shown in the preview pane.
    pub preview_bead_id: Option<String>,
    /// When the cursor last moved — used to debounce preview fetches.
    pub preview_cursor_moved: Option<Instant>,
    /// Bead ID that the debounce timer is waiting to fetch.
    pub preview_pending_id: Option<String>,
    /// Total open issues for footer.
    pub open_count: u64,
    /// Total closed issues for footer.
    pub closed_count: u64,
    /// Maps each bead ID to the set of its direct dependency neighbors (both directions).
    pub dep_neighbors: HashMap<String, HashSet<String>>,
    /// Close confirmation overlay state.
    pub close_confirm: Option<CloseConfirmState>,
    /// Defer input overlay state.
    pub defer_input: Option<DeferState>,
    /// Bead IDs with status=blocked but no actual blocking dependencies.
    pub manual_blocked_ids: HashSet<String>,
    /// Dependency direction picker overlay state.
    pub dep_direction: Option<DepDirectionState>,
    /// Undo stack — push on every forward action, pop on undo.
    pub undo_stack: Vec<BoardAction>,
    /// Redo stack — push when undoing, clear on new forward action.
    pub redo_stack: Vec<BoardAction>,
    /// Transient status message with timestamp for auto-dismiss.
    pub status_message: Option<(String, Instant)>,
    /// Whether the help overlay is shown.
    pub show_help: bool,
}

/// Which direction the dependency goes.
#[derive(Debug, Clone, Copy)]
pub enum DepDirection {
    /// The selected bead is blocked by the picked bead.
    BlockedBy,
    /// The selected bead blocks the picked bead.
    Blocks,
}

/// Which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardFocus {
    /// Board columns have focus — cursor navigates cards.
    Board,
    /// Preview pane has focus — j/k scrolls detail content.
    Preview,
}

/// An undoable board action, stored in the undo/redo stacks.
#[derive(Debug, Clone)]
pub enum BoardAction {
    ToggleHumanLabel {
        bead_id: String,
        was_present: bool,
    },
    Defer {
        bead_id: String,
        previous_status: String,
    },
    Close {
        bead_id: String,
        previous_status: String,
    },
    ChangePriority {
        bead_id: String,
        old_priority: u64,
        new_priority: u64,
    },
    AddDependency {
        issue: String,
        depends_on: String,
    },
}

impl BoardAction {
    /// Human-readable description of the forward action.
    fn describe(&self) -> String {
        match self {
            BoardAction::ToggleHumanLabel {
                bead_id,
                was_present,
            } => {
                if *was_present {
                    format!("removed human label from {bead_id}")
                } else {
                    format!("added human label to {bead_id}")
                }
            }
            BoardAction::Defer { bead_id, .. } => format!("deferred {bead_id}"),
            BoardAction::Close { bead_id, .. } => format!("closed {bead_id}"),
            BoardAction::ChangePriority {
                bead_id,
                new_priority,
                ..
            } => format!("priority P{new_priority} on {bead_id}"),
            BoardAction::AddDependency { issue, depends_on } => {
                format!("dep {issue} -> {depends_on}")
            }
        }
    }

    /// Execute the forward (original) action via bd CLI.
    fn execute_forward(&self, bd_path: &str) {
        let bd = bd_path.to_string();
        match self.clone() {
            BoardAction::ToggleHumanLabel {
                bead_id,
                was_present,
            } => {
                let flag = if was_present {
                    "--remove-label=human"
                } else {
                    "--add-label=human"
                };
                spawn_bd(&bd, &["update", &bead_id, flag]);
            }
            BoardAction::Defer { bead_id, .. } => {
                spawn_bd(&bd, &["update", &bead_id, "--status=deferred"]);
            }
            BoardAction::Close { bead_id, .. } => {
                spawn_bd(&bd, &["close", &bead_id]);
            }
            BoardAction::ChangePriority {
                bead_id,
                new_priority,
                ..
            } => {
                let p = new_priority.to_string();
                spawn_bd(&bd, &["update", &bead_id, "--priority", &p]);
            }
            BoardAction::AddDependency { issue, depends_on } => {
                spawn_bd(&bd, &["dep", "add", &issue, &depends_on]);
            }
        }
    }

    /// Execute the reverse (undo) action via bd CLI.
    fn execute_reverse(&self, bd_path: &str) {
        let bd = bd_path.to_string();
        match self.clone() {
            BoardAction::ToggleHumanLabel {
                bead_id,
                was_present,
            } => {
                let flag = if was_present {
                    "--add-label=human"
                } else {
                    "--remove-label=human"
                };
                spawn_bd(&bd, &["update", &bead_id, flag]);
            }
            BoardAction::Defer {
                bead_id,
                previous_status,
            } => {
                let status_flag = format!("--status={previous_status}");
                spawn_bd(&bd, &["update", &bead_id, &status_flag]);
            }
            BoardAction::Close {
                bead_id,
                previous_status,
            } => {
                let status_flag = format!("--status={previous_status}");
                spawn_bd(&bd, &["update", &bead_id, &status_flag]);
            }
            BoardAction::ChangePriority {
                bead_id,
                old_priority,
                ..
            } => {
                let p = old_priority.to_string();
                spawn_bd(&bd, &["update", &bead_id, "--priority", &p]);
            }
            BoardAction::AddDependency { issue, depends_on } => {
                spawn_bd(&bd, &["dep", "remove", &issue, &depends_on]);
            }
        }
    }
}

/// Spawn a fire-and-forget bd command in a background thread.
fn spawn_bd(bd_path: &str, args: &[&str]) {
    let bd = bd_path.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        std::process::Command::new(&bd)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok();
    });
}

/// State for the dependency direction picker overlay (b).
#[derive(Debug)]
pub struct DepDirectionState {
    /// The bead ID we pressed 'b' on.
    pub bead_id: String,
}

/// State for the close confirmation overlay (Shift+X).
#[derive(Debug)]
pub struct CloseConfirmState {
    /// The bead ID to close.
    pub bead_id: String,
    /// Optional reason text being typed.
    pub reason: String,
    /// Cursor position (byte offset) within `reason`.
    pub cursor_pos: usize,
}

impl CloseConfirmState {
    fn insert_char(&mut self, c: char) {
        self.reason.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.reason[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
            self.reason.remove(self.cursor_pos);
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.reason[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
        }
    }

    fn cursor_right(&mut self) {
        if self.cursor_pos < self.reason.len() {
            let next = self.reason[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += next;
        }
    }
}

/// State for the defer input overlay (d).
#[derive(Debug)]
pub struct DeferState {
    /// The bead ID to defer.
    pub bead_id: String,
    /// Optional "until" date text being typed.
    pub until: String,
    /// Cursor position (byte offset) within `until`.
    pub cursor_pos: usize,
}

impl DeferState {
    fn insert_char(&mut self, c: char) {
        self.until.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.until[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
            self.until.remove(self.cursor_pos);
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.until[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
        }
    }

    fn cursor_right(&mut self) {
        if self.cursor_pos < self.until.len() {
            let next = self.until[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += next;
        }
    }
}

impl KanbanBoardState {
    pub fn new_loading(column_defs: Vec<ColumnDef>) -> Self {
        let col_count = column_defs.len();
        let default_col = column_defs
            .iter()
            .position(|c| c.name == "Ready")
            .unwrap_or(0);
        Self {
            columns: vec![Vec::new(); col_count],
            selected_column: default_col,
            selected_row: vec![0; col_count],
            is_loading: true,
            error: None,
            focus: BoardFocus::Board,
            preview_detail: None,
            preview_bead_id: None,
            preview_cursor_moved: None,
            preview_pending_id: None,
            open_count: 0,
            closed_count: 0,
            dep_neighbors: HashMap::new(),
            close_confirm: None,
            defer_input: None,
            manual_blocked_ids: HashSet::new(),
            dep_direction: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            status_message: None,
            show_help: false,
            column_defs,
        }
    }

    fn col_count(&self) -> usize {
        self.column_defs.len()
    }

    /// Returns the currently selected card, if any (skipping error cards).
    pub fn selected_card(&self) -> Option<&KanbanCard> {
        let col = self.selected_column;
        let row = self.selected_row[col];
        let card = self.columns[col].get(row)?;
        if card.is_error { None } else { Some(card) }
    }

    pub fn populate(&mut self, result: Result<KanbanBoardData, String>) {
        self.is_loading = false;
        match result {
            Ok(data) => {
                self.open_count = data.open_count;
                self.closed_count = data.closed_count;
                self.dep_neighbors = data.dep_neighbors;
                self.manual_blocked_ids = data.manual_blocked_ids;
                self.columns = data.columns;

                // Preserve cursor positions across refreshes, clamping to new bounds
                let col_count = self.col_count();
                if self.selected_row.len() != col_count {
                    self.selected_row = vec![0; col_count];
                }
                for col_idx in 0..col_count {
                    let len = self.columns[col_idx].len();
                    if len == 0 {
                        self.selected_row[col_idx] = 0;
                    } else if self.selected_row[col_idx] >= len {
                        self.selected_row[col_idx] = len - 1;
                    }
                    self.advance_to_card(col_idx);
                }

                // Schedule preview fetch for initially selected card
                self.schedule_preview_fetch();
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    /// Advance selected_row for a column to the next non-error card.
    fn advance_to_card(&mut self, col: usize) {
        let len = self.columns[col].len();
        while self.selected_row[col] < len && self.columns[col][self.selected_row[col]].is_error {
            self.selected_row[col] += 1;
        }
        // If we went past the end, reset to 0 (column might be all errors)
        if self.selected_row[col] >= len {
            self.selected_row[col] = 0;
        }
    }

    /// Find a card by bead ID across all columns.
    fn find_card(&self, bead_id: &str) -> Option<&KanbanCard> {
        self.columns.iter().flatten().find(|c| c.id == bead_id)
    }

    /// Record a forward action: push to undo stack and clear redo stack.
    pub fn push_action(&mut self, action: BoardAction) {
        self.set_status(action.describe());
        self.undo_stack.push(action);
        self.redo_stack.clear();
    }

    /// Set a transient status message that auto-dismisses after a few seconds.
    pub fn set_status(&mut self, msg: String) {
        self.status_message = Some((msg, Instant::now()));
    }

    pub fn move_left(&mut self) {
        if self.selected_column > 0 {
            self.selected_column -= 1;
        }
    }

    pub fn move_right(&mut self) {
        if self.selected_column < self.col_count() - 1 {
            self.selected_column += 1;
        }
    }

    pub fn move_up(&mut self) {
        let col = self.selected_column;
        let mut row = self.selected_row[col];
        // Move up, skipping error cards
        loop {
            if row == 0 {
                break;
            }
            row -= 1;
            if !self.columns[col][row].is_error {
                self.selected_row[col] = row;
                break;
            }
        }
    }

    pub fn move_down(&mut self) {
        let col = self.selected_column;
        let len = self.columns[col].len();
        let mut row = self.selected_row[col];
        // Move down, skipping error cards
        loop {
            if row + 1 >= len {
                break;
            }
            row += 1;
            if !self.columns[col][row].is_error {
                self.selected_row[col] = row;
                break;
            }
        }
    }

    /// Schedule a debounced preview fetch for the currently selected card.
    pub fn schedule_preview_fetch(&mut self) {
        if let Some(card) = self.selected_card() {
            let id = card.id.clone();
            // Don't schedule if we're already showing this bead
            if self.preview_bead_id.as_deref() == Some(&id) && self.preview_pending_id.is_none() {
                return;
            }
            self.preview_pending_id = Some(id);
            self.preview_cursor_moved = Some(Instant::now());
            // Reset scroll when selecting a different bead
            if let Some(ref mut detail) = self.preview_detail {
                detail.scroll_offset = 0;
            }
        } else {
            // No card selected (empty column or error card)
            self.preview_pending_id = None;
            self.preview_cursor_moved = None;
        }
    }
}

/// Parse a JSON bead item into a KanbanCard with the given source emoji.
fn parse_card(item: &serde_json::Value, emoji: &str) -> Option<KanbanCard> {
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
    let labels = item
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let status = item
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(KanbanCard {
        id,
        title,
        priority,
        blockers,
        emoji: emoji.to_string(),
        is_epic: false, // Set later in fetch_board_data after collecting parent IDs
        is_error: false,
        labels,
        status,
    })
}

// ---------------------------------------------------------------------------
// Input handling
// ---------------------------------------------------------------------------

/// Handle keyboard input for the kanban board (primary view).
pub fn handle_kanban_input(app: &mut App, key_code: KeyCode, modifiers: KeyModifiers) {
    let state = &mut app.kanban_board_state;

    // If close confirmation is open, handle its input
    if let Some(confirm) = &mut state.close_confirm {
        match key_code {
            KeyCode::Esc => {
                state.close_confirm = None;
            }
            KeyCode::Enter => {
                let bead_id = confirm.bead_id.clone();
                let reason = confirm.reason.trim().to_string();
                let bd_path = app.config.behavior.bd_path.clone();
                let previous_status = state
                    .find_card(&bead_id)
                    .map(|c| c.status.clone())
                    .unwrap_or_else(|| "open".to_string());
                state.close_confirm = None;
                state.push_action(BoardAction::Close {
                    bead_id: bead_id.clone(),
                    previous_status,
                });
                std::thread::spawn(move || {
                    let mut cmd = std::process::Command::new(&bd_path);
                    cmd.arg("close").arg(&bead_id);
                    if !reason.is_empty() {
                        cmd.arg("--reason").arg(&reason);
                    }
                    cmd.stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .ok();
                });
            }
            KeyCode::Backspace => {
                confirm.delete_char_before();
            }
            KeyCode::Left => {
                confirm.cursor_left();
            }
            KeyCode::Right => {
                confirm.cursor_right();
            }
            KeyCode::Char(c) => {
                confirm.insert_char(c);
            }
            _ => {}
        }
        return;
    }

    // If dep direction picker is open, handle its input
    if let Some(dep_dir) = &state.dep_direction {
        match key_code {
            KeyCode::Esc => {
                state.dep_direction = None;
            }
            KeyCode::Char('1') => {
                app.pending_dep = Some(crate::app::PendingDep {
                    bead_id: dep_dir.bead_id.clone(),
                    direction: DepDirection::BlockedBy,
                });
                state.dep_direction = None;
                app.open_bead_picker();
            }
            KeyCode::Char('2') => {
                app.pending_dep = Some(crate::app::PendingDep {
                    bead_id: dep_dir.bead_id.clone(),
                    direction: DepDirection::Blocks,
                });
                state.dep_direction = None;
                app.open_bead_picker();
            }
            _ => {}
        }
        return;
    }

    // If defer input is open, handle its input
    if let Some(defer) = &mut state.defer_input {
        match key_code {
            KeyCode::Esc => {
                state.defer_input = None;
            }
            KeyCode::Enter => {
                let bead_id = defer.bead_id.clone();
                let until = defer.until.trim().to_string();
                let bd_path = app.config.behavior.bd_path.clone();
                let previous_status = state
                    .find_card(&bead_id)
                    .map(|c| c.status.clone())
                    .unwrap_or_else(|| "open".to_string());
                state.defer_input = None;
                state.push_action(BoardAction::Defer {
                    bead_id: bead_id.clone(),
                    previous_status,
                });
                std::thread::spawn(move || {
                    let mut cmd = std::process::Command::new(&bd_path);
                    if until.is_empty() {
                        cmd.args(["update", &bead_id, "--status=deferred"]);
                    } else {
                        cmd.args(["defer", &bead_id, "--until"]).arg(&until);
                    }
                    cmd.stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .ok();
                });
            }
            KeyCode::Backspace => {
                defer.delete_char_before();
            }
            KeyCode::Left => {
                defer.cursor_left();
            }
            KeyCode::Right => {
                defer.cursor_right();
            }
            KeyCode::Char(c) => {
                defer.insert_char(c);
            }
            _ => {}
        }
        return;
    }

    // If help overlay is open, only ? and Esc dismiss it
    if state.show_help {
        match key_code {
            KeyCode::Char('?') | KeyCode::Esc => {
                state.show_help = false;
            }
            _ => {}
        }
        return;
    }

    // If preview pane has focus, handle preview input
    if state.focus == BoardFocus::Preview {
        match key_code {
            KeyCode::Esc | KeyCode::Enter => {
                state.focus = BoardFocus::Board;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut detail) = state.preview_detail {
                    detail.scroll_offset = detail.scroll_offset.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut detail) = state.preview_detail {
                    detail.scroll_offset = detail.scroll_offset.saturating_sub(1);
                }
            }
            KeyCode::Char('?') => {
                state.show_help = true;
            }
            _ => {}
        }
        return;
    }

    match key_code {
        KeyCode::Esc => {
            // Board is the primary view — Esc is a no-op
        }
        KeyCode::Enter => {
            // Move focus to preview pane if there's a selected card
            if state.selected_card().is_some() && state.preview_detail.is_some() {
                state.focus = BoardFocus::Preview;
            }
        }
        KeyCode::Char('X') => {
            if let Some(card) = state.selected_card() {
                state.close_confirm = Some(CloseConfirmState {
                    bead_id: card.id.clone(),
                    reason: String::new(),
                    cursor_pos: 0,
                });
            }
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if let Some(card) = state.selected_card()
                && card.priority > 0
            {
                let bead_id = card.id.clone();
                let old_priority = card.priority;
                let new_priority = old_priority - 1;
                let bd_path = app.config.behavior.bd_path.clone();
                state.push_action(BoardAction::ChangePriority {
                    bead_id: bead_id.clone(),
                    old_priority,
                    new_priority,
                });
                let p = new_priority.to_string();
                spawn_bd(&bd_path, &["update", &bead_id, "--priority", &p]);
            }
        }
        KeyCode::Char('-') => {
            if let Some(card) = state.selected_card()
                && card.priority < 4
            {
                let bead_id = card.id.clone();
                let old_priority = card.priority;
                let new_priority = old_priority + 1;
                let bd_path = app.config.behavior.bd_path.clone();
                state.push_action(BoardAction::ChangePriority {
                    bead_id: bead_id.clone(),
                    old_priority,
                    new_priority,
                });
                let p = new_priority.to_string();
                spawn_bd(&bd_path, &["update", &bead_id, "--priority", &p]);
            }
        }
        KeyCode::Char('H') => {
            if let Some(card) = state.selected_card() {
                let bead_id = card.id.clone();
                let has_human = card.labels.contains(&"human".to_string());
                let bd_path = app.config.behavior.bd_path.clone();
                state.push_action(BoardAction::ToggleHumanLabel {
                    bead_id: bead_id.clone(),
                    was_present: has_human,
                });
                let flag = if has_human {
                    "--remove-label=human"
                } else {
                    "--add-label=human"
                };
                spawn_bd(&bd_path, &["update", &bead_id, flag]);
            }
        }
        KeyCode::Char('d') => {
            if let Some(card) = state.selected_card() {
                state.defer_input = Some(DeferState {
                    bead_id: card.id.clone(),
                    until: String::new(),
                    cursor_pos: 0,
                });
            }
        }
        KeyCode::Char('b') => {
            if let Some(card) = state.selected_card() {
                state.dep_direction = Some(DepDirectionState {
                    bead_id: card.id.clone(),
                });
            }
        }
        KeyCode::Char('u') => {
            if let Some(action) = state.undo_stack.pop() {
                let bd_path = app.config.behavior.bd_path.clone();
                action.execute_reverse(&bd_path);
                state.set_status(format!("Undid: {}", action.describe()));
                state.redo_stack.push(action);
            }
        }
        KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(action) = state.redo_stack.pop() {
                let bd_path = app.config.behavior.bd_path.clone();
                action.execute_forward(&bd_path);
                state.set_status(format!("Redid: {}", action.describe()));
                state.undo_stack.push(action);
            }
        }
        KeyCode::Char('?') => {
            state.show_help = true;
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.move_left();
            state.schedule_preview_fetch();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.move_right();
            state.schedule_preview_fetch();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_up();
            state.schedule_preview_fetch();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.move_down();
            state.schedule_preview_fetch();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Board rendering
// ---------------------------------------------------------------------------

/// Draw the kanban board in the given content area.
pub fn draw_kanban_board(f: &mut Frame, app: &App, board_area: Rect) {
    let state = &app.kanban_board_state;

    // Split content area: top for board columns, bottom for preview pane.
    // If terminal is very short (< 12 lines), hide preview and show board only.
    let (columns_area, preview_area) = if board_area.height < 12 {
        (board_area, None)
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
            .split(board_area);
        (chunks[0], Some(chunks[1]))
    };

    // Draw the preview pane in the bottom area
    if let Some(area) = preview_area {
        draw_preview_pane(f, state, area);
    }

    let inner_height = columns_area.height.saturating_sub(2) as usize;
    let inner_width = columns_area.width.saturating_sub(2) as usize;

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
        let col_count = state.col_count();
        let separators = col_count.saturating_sub(1);
        let usable = inner_width.saturating_sub(separators);

        // Accordion layout: selected column gets ~45% of width, others split the rest
        let (expanded_width, collapsed_width) = if col_count <= 1 {
            (usable, 0)
        } else {
            let exp = usable * 45 / 100;
            let coll = usable.saturating_sub(exp) / (col_count - 1);
            (exp, coll)
        };
        let leftover = if col_count <= 1 {
            0
        } else {
            usable.saturating_sub(expanded_width + collapsed_width * (col_count - 1))
        };
        let col_widths: Vec<usize> = (0..col_count)
            .map(|i| {
                if i == state.selected_column {
                    expanded_width + leftover
                } else {
                    collapsed_width
                }
            })
            .collect();

        // Count real cards (not error cards) per column for display
        let card_counts: Vec<usize> = state
            .columns
            .iter()
            .map(|col| col.iter().filter(|c| !c.is_error).count())
            .collect();

        // Header row
        let mut header_spans: Vec<Span> = Vec::new();
        for (i, col_def) in state.column_defs.iter().enumerate() {
            let is_selected = i == state.selected_column;
            let w = col_widths[i];
            let label = format!("{} ({})", col_def.name, card_counts[i]);
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
                header_spans.push(Span::styled(
                    "\u{2502}",
                    Style::default().fg(Color::DarkGray),
                ));
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
                sep_spans.push(Span::styled(
                    "\u{253c}",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        content.push(Line::from(sep_spans));

        // Warning banner for manual-blocked beads
        let manual_blocked_count = state.manual_blocked_ids.len();
        let has_banner = manual_blocked_count > 0;
        if has_banner {
            let noun = if manual_blocked_count == 1 {
                "bead has"
            } else {
                "beads have"
            };
            let banner_text = format!(
                " {manual_blocked_count} {noun} 'blocked' status without dependencies \u{2014} Ralph won't pick these up"
            );
            let banner_padded = format!("{:<width$}", banner_text, width = inner_width);
            content.push(Line::from(Span::styled(
                banner_padded,
                Style::default().fg(Color::Yellow),
            )));
        }

        // Card rows
        let banner_rows = if has_banner { 1 } else { 0 };
        let max_rows = inner_height.saturating_sub(3 + banner_rows); // header + separator + footer + banner
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

                    if card.is_error {
                        // Error card — render with error style, not selectable
                        let cell_text = format!(" {} {}", card.emoji, card.title);
                        let display_width = UnicodeWidthStr::width(cell_text.as_str());
                        let padded = if display_width >= w {
                            truncate_to_width(&cell_text, w)
                        } else {
                            let padding = w - display_width;
                            format!("{}{}", cell_text, " ".repeat(padding))
                        };
                        let style = Style::default().fg(Color::Red);
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
                        let is_manual_blocked = state.manual_blocked_ids.contains(&card.id);
                        let style = if is_manual_blocked {
                            Style::default().fg(Color::Yellow)
                        } else if is_dep_neighbor {
                            Style::default().fg(Color::Gray).bg(Color::Rgb(25, 35, 60))
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        row_spans.push(Span::styled(padded, style));
                    } else {
                        // Expanded column: source emoji, id, title, blockers
                        let icon_prefix = &card.emoji;
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
                        let is_manual_blocked = state.manual_blocked_ids.contains(&card.id);
                        let base_style = if is_selected_row {
                            Style::default().fg(Color::Black).bg(Color::White)
                        } else if is_manual_blocked {
                            Style::default().fg(Color::Yellow)
                        } else if is_dep_neighbor {
                            Style::default().fg(Color::White).bg(Color::Rgb(25, 35, 60))
                        } else {
                            Style::default().fg(Color::White)
                        };
                        // Epics get bold styling but keep the source emoji
                        let style = if card.is_epic {
                            base_style.add_modifier(Modifier::BOLD)
                        } else {
                            base_style
                        };

                        row_spans.push(Span::styled(padded, style));
                    }
                } else {
                    row_spans.push(Span::raw(" ".repeat(w)));
                }

                if col_idx < col_count - 1 {
                    row_spans.push(Span::styled(
                        "\u{2502}",
                        Style::default().fg(Color::DarkGray),
                    ));
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
                    row_spans.push(Span::styled(
                        "\u{2502}",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            content.push(Line::from(row_spans));
        }

        // Footer — show status message if fresh, otherwise key hints
        let status_msg_timeout = std::time::Duration::from_secs(3);
        let active_status = state
            .status_message
            .as_ref()
            .filter(|(_, ts)| ts.elapsed() < status_msg_timeout)
            .map(|(msg, _)| msg.clone());

        if let Some(msg) = active_status {
            let padded = format!(" {msg:<width$}", width = inner_width.saturating_sub(1));
            content.push(Line::from(Span::styled(
                padded,
                Style::default().fg(Color::Yellow),
            )));
        } else {
            let sep = Style::default().fg(Color::DarkGray);
            let key = Style::default().fg(Color::Cyan);
            let desc = Style::default().fg(Color::DarkGray);
            content.push(Line::from(vec![
                Span::styled(" hjkl", key),
                Span::styled(" navigate", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("Enter", key),
                Span::styled(" preview", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("X", key),
                Span::styled(" close", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("b", key),
                Span::styled(" dep", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("d", key),
                Span::styled(" defer", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("+/-", key),
                Span::styled(" pri", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("H", key),
                Span::styled(" human", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("?", key),
                Span::styled(" help", desc),
            ]));
        }
    }

    let stats_title = format!(
        " {} open \u{b7} {} closed ",
        state.open_count, state.closed_count
    );

    let board = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Work Board ")
            .title_alignment(Alignment::Center)
            .title_top(Line::from(stats_title).right_aligned())
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(board, columns_area);

    // Close confirmation overlay
    if let Some(confirm) = &state.close_confirm {
        draw_close_confirm(f, confirm);
    }

    // Defer input overlay
    if let Some(defer) = &state.defer_input {
        draw_defer_input(f, defer);
    }

    // Dependency direction picker overlay
    if let Some(dep_dir) = &state.dep_direction {
        draw_dep_direction(f, dep_dir);
    }

    // Help overlay
    if state.show_help {
        draw_board_help(f);
    }
}

/// Draw the close confirmation overlay with optional reason input.
fn draw_close_confirm(f: &mut Frame, confirm: &CloseConfirmState) {
    let area = f.area();
    let overlay = centered_rect(50, 5, area);
    f.render_widget(Clear, overlay);

    let prompt = format!("Close {}? Reason (optional):", confirm.bead_id);

    // Build the text input line with cursor
    let before = &confirm.reason[..confirm.cursor_pos];
    let at_end = confirm.cursor_pos >= confirm.reason.len();
    let cursor_char = if at_end {
        ' '
    } else {
        confirm.reason[confirm.cursor_pos..].chars().next().unwrap()
    };
    let after = if at_end {
        ""
    } else {
        &confirm.reason[confirm.cursor_pos + cursor_char.len_utf8()..]
    };

    let input_line = Line::from(vec![
        Span::styled(before, Style::default().fg(Color::White)),
        Span::styled(
            cursor_char.to_string(),
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(after, Style::default().fg(Color::White)),
    ]);

    let content = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        input_line,
        Line::from(Span::styled(
            "Enter to confirm \u{b7} Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Close Bead ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Red)),
    );

    f.render_widget(widget, overlay);
}

/// Draw the defer input overlay with optional until-date input.
fn draw_defer_input(f: &mut Frame, defer: &DeferState) {
    let area = f.area();
    let overlay = centered_rect(50, 5, area);
    f.render_widget(Clear, overlay);

    let prompt = format!("Defer {}. Until (optional):", defer.bead_id);

    // Build the text input line with cursor
    let before = &defer.until[..defer.cursor_pos];
    let at_end = defer.cursor_pos >= defer.until.len();
    let cursor_char = if at_end {
        ' '
    } else {
        defer.until[defer.cursor_pos..].chars().next().unwrap()
    };
    let after = if at_end {
        ""
    } else {
        &defer.until[defer.cursor_pos + cursor_char.len_utf8()..]
    };

    let input_line = Line::from(vec![
        Span::styled(before, Style::default().fg(Color::White)),
        Span::styled(
            cursor_char.to_string(),
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(after, Style::default().fg(Color::White)),
    ]);

    let content = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        input_line,
        Line::from(Span::styled(
            "Enter to defer \u{b7} Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Defer Bead ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(widget, overlay);
}

/// Draw the dependency direction picker overlay.
fn draw_dep_direction(f: &mut Frame, dep_dir: &DepDirectionState) {
    let area = f.area();
    let overlay = centered_rect(50, 6, area);
    f.render_widget(Clear, overlay);

    let prompt = format!("Add dependency for {}", dep_dir.bead_id);

    let content = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(vec![
            Span::styled("1", Style::default().fg(Color::Cyan)),
            Span::styled("  This is blocked by...", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("2", Style::default().fg(Color::Cyan)),
            Span::styled("  This blocks...", Style::default().fg(Color::White)),
        ]),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Add Dependency ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(widget, overlay);
}

/// Draw the board help overlay with keybinding reference.
fn draw_board_help(f: &mut Frame) {
    let modal_width: u16 = 50;
    let modal_height: u16 = 26;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    f.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(Color::Cyan);
    let desc_style = Style::default().fg(Color::DarkGray);
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let inner_width = modal_width.saturating_sub(4) as usize;
    let footer_text = "? or Esc to close";
    let footer_padding = inner_width.saturating_sub(footer_text.len());

    let content: Vec<Line> = vec![
        Line::from(Span::styled("  Navigation", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("hjkl/\u{2190}\u{2191}\u{2192}\u{2193}", key_style),
            Span::styled("  Move between cards", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Enter", key_style),
            Span::styled("      Focus preview pane", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Preview Pane", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("j/k", key_style),
            Span::styled("          Scroll preview", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Esc/Enter", key_style),
            Span::styled("    Return to board", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Card Actions", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("X", key_style),
            Span::styled("            Close bead", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("b", key_style),
            Span::styled("            Add dependency", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("d", key_style),
            Span::styled("            Defer bead", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("+/-", key_style),
            Span::styled("          Change priority", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("H", key_style),
            Span::styled("            Toggle human label", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Undo/Redo", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("u", key_style),
            Span::styled("            Undo last action", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Ctrl+r", key_style),
            Span::styled("       Redo", desc_style),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Board", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("?", key_style),
            Span::styled("            Toggle help", desc_style),
        ]),
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

// ---------------------------------------------------------------------------
// Preview pane rendering
// ---------------------------------------------------------------------------

/// Build the detail content lines for a bead (shared by preview pane).
fn build_detail_content(detail: &BeadDetailState) -> Vec<Line<'_>> {
    let mut content: Vec<Line> = Vec::new();

    if detail.is_loading {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
        return content;
    }
    if let Some(error) = &detail.error {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!("  Error: {error}"),
            Style::default().fg(Color::Red),
        )));
        return content;
    }

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

    content
}

/// Draw the preview pane showing bead detail below the board.
fn draw_preview_pane(f: &mut Frame, state: &KanbanBoardState, area: Rect) {
    let has_focus = state.focus == BoardFocus::Preview;
    let border_style = if has_focus {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    match &state.preview_detail {
        Some(detail) => {
            let content = build_detail_content(detail);
            let title = format!(" {} ", short_id(&detail.id));
            let pane = Paragraph::new(content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .style(border_style),
                )
                .wrap(Wrap { trim: false })
                .scroll((detail.scroll_offset, 0));
            f.render_widget(pane, area);
        }
        None => {
            let content = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Select a bead to see details",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let pane = Paragraph::new(content).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Preview ")
                    .title_alignment(Alignment::Center)
                    .style(border_style),
            );
            f.render_widget(pane, area);
        }
    }
}

// ---------------------------------------------------------------------------
// Pipeline execution
// ---------------------------------------------------------------------------

/// Run a shell pipeline and parse the JSON array output.
fn run_shell_pipeline(command: &str, bd_path: &str) -> Result<Vec<serde_json::Value>, String> {
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", command])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Ensure the directory containing bd is on PATH so pipeline commands
    // can find the bd binary even when bd_path is an absolute path.
    let bd_abs = std::path::Path::new(bd_path);
    if let Some(parent) = bd_abs.parent().filter(|p| !p.as_os_str().is_empty())
        && let Ok(current_path) = std::env::var("PATH")
    {
        cmd.env("PATH", format!("{}:{current_path}", parent.display()));
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run pipeline: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Pipeline failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
        .map_err(|e| format!("Failed to parse pipeline output: {e}"))
}

// ---------------------------------------------------------------------------
// Filesystem watcher
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Data fetching
// ---------------------------------------------------------------------------

/// Fetch board data from pipeline sources (called from background thread).
pub fn fetch_board_data(
    bd_path: &str,
    column_defs: &[ColumnDef],
) -> Result<KanbanBoardData, String> {
    use std::thread;

    // Collect all (col_idx, emoji, command) tuples
    let mut tasks: Vec<(usize, String, String)> = Vec::new();
    for (col_idx, col_def) in column_defs.iter().enumerate() {
        for source in &col_def.sources {
            tasks.push((col_idx, source.emoji.clone(), source.command.clone()));
        }
    }

    // Spawn all source commands in parallel
    type PipelineHandle = (
        usize,
        String,
        thread::JoinHandle<Result<Vec<serde_json::Value>, String>>,
    );
    let bd = bd_path.to_string();
    let handles: Vec<PipelineHandle> = tasks
        .into_iter()
        .map(|(col_idx, emoji, command)| {
            let bd_clone = bd.clone();
            let handle = thread::spawn(move || run_shell_pipeline(&command, &bd_clone));
            (col_idx, emoji, handle)
        })
        .collect();

    // Also fetch stats in parallel
    let p = bd_path.to_string();
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

    // Collect results into columns
    let col_count = column_defs.len();
    let mut columns: Vec<Vec<KanbanCard>> = vec![Vec::new(); col_count];
    let mut all_items: Vec<serde_json::Value> = Vec::new();

    for (col_idx, emoji, handle) in handles {
        match handle.join().map_err(|_| "thread panic".to_string())? {
            Ok(items) => {
                all_items.extend(items.iter().cloned());
                for item in &items {
                    if let Some(card) = parse_card(item, &emoji) {
                        columns[col_idx].push(card);
                    }
                }
            }
            Err(err) => {
                // Render an error card in the column; other sources still render
                columns[col_idx].push(KanbanCard {
                    id: String::new(),
                    title: format!("Error: {err}"),
                    priority: 999,
                    blockers: Vec::new(),
                    emoji: "\u{26a0}\u{fe0f}".to_string(), // ⚠️
                    is_epic: false,
                    is_error: true,
                    labels: Vec::new(),
                    status: String::new(),
                });
            }
        }
    }

    // Dedup within each column by ID
    for column in &mut columns {
        let mut seen = HashSet::new();
        column.retain(|card| card.is_error || card.id.is_empty() || seen.insert(card.id.clone()));
    }

    // Detect epics (beads that are parents of other beads)
    let parent_ids: HashSet<String> = all_items
        .iter()
        .filter_map(|item| {
            item.get("parent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    for column in &mut columns {
        for card in column.iter_mut() {
            if parent_ids.contains(&card.id) {
                card.is_epic = true;
            }
        }
    }

    // Sort each column by priority (error cards sort to the end)
    for column in &mut columns {
        column.sort_by_key(|c| if c.is_error { u64::MAX } else { c.priority });
    }

    // Build bidirectional dependency neighbor map
    let mut dep_neighbors: HashMap<String, HashSet<String>> = HashMap::new();
    for item in &all_items {
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

    // Detect manual-blocked beads: status=blocked but no actual blocking dependencies
    let manual_blocked_ids: HashSet<String> = columns
        .iter()
        .flat_map(|col| col.iter())
        .filter(|card| card.status == "blocked" && card.blockers.is_empty())
        .map(|card| card.id.clone())
        .collect();

    // Stats
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
        columns,
        open_count,
        closed_count,
        dep_neighbors,
        manual_blocked_ids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_board_config_returns_compiled_default_when_no_external_file() {
        // With no external file, load_board_config should succeed with compiled-in default
        let config = load_board_config().expect("compiled-in default should parse");
        assert!(
            !config.columns.is_empty(),
            "default config should have columns"
        );
    }

    #[test]
    fn compiled_default_parses_correctly() {
        let config: BoardConfig =
            toml::from_str(include_str!("board_columns.toml")).expect("embedded TOML should parse");
        assert!(!config.columns.is_empty());
        for col in &config.columns {
            assert!(!col.name.is_empty(), "column name should not be empty");
            assert!(
                !col.sources.is_empty(),
                "column should have at least one source"
            );
            for src in &col.sources {
                assert!(
                    !src.command.is_empty(),
                    "source command should not be empty"
                );
                assert!(!src.emoji.is_empty(), "source emoji should not be empty");
            }
        }
    }

    #[test]
    fn board_config_deserializes_valid_toml() {
        let toml_str = r#"
[[columns]]
name = "Test Column"

[[columns.sources]]
command = "echo '[]'"
emoji = "✓"
"#;
        let config: BoardConfig = toml::from_str(toml_str).expect("valid TOML should parse");
        assert_eq!(config.columns.len(), 1);
        assert_eq!(config.columns[0].name, "Test Column");
        assert_eq!(config.columns[0].sources.len(), 1);
        assert_eq!(config.columns[0].sources[0].emoji, "✓");
    }

    #[test]
    fn board_config_rejects_invalid_toml() {
        let bad_toml = "this is not valid TOML [[[";
        let result = toml::from_str::<BoardConfig>(bad_toml);
        assert!(result.is_err());
    }
}
