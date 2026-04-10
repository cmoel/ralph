use std::collections::{HashMap, HashSet};
use std::time::Instant;

use serde::Deserialize;

use super::overlays::{CloseConfirmState, DeferState, DepDirectionState};

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
    toml::from_str(include_str!("../board_columns.toml"))
}

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

/// Strip the project prefix from a bead ID, returning just the short suffix.
/// e.g., "ralph-y3t" → "y3t", "private-lessons-gac" → "gac"
pub(super) fn short_id(id: &str) -> &str {
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
    pub(super) fn describe(&self) -> String {
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
    pub(super) fn execute_forward(&self, bd_path: &str) {
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
    pub(super) fn execute_reverse(&self, bd_path: &str) {
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
pub(super) fn spawn_bd(bd_path: &str, args: &[&str]) {
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
            column_defs,
        }
    }

    pub(super) fn col_count(&self) -> usize {
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
    pub(super) fn find_card(&self, bead_id: &str) -> Option<&KanbanCard> {
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
        let config: BoardConfig = toml::from_str(include_str!("../board_columns.toml"))
            .expect("embedded TOML should parse");
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
