use std::collections::HashMap;

use ratatui::text::Line;

/// Tracks accumulated state for a content block being streamed.
#[derive(Debug, Default)]
pub struct ContentBlockState {
    /// For text blocks: accumulated text content.
    pub text: String,
    /// For tool_use blocks: the tool name.
    pub tool_name: Option<String>,
    /// For tool_use blocks: the tool use ID (for correlating with results).
    pub tool_use_id: Option<String>,
    /// For tool_use blocks: accumulated JSON input string.
    pub input_json: String,
    /// Whether we've shown the assistant header for this text block.
    pub header_shown: bool,
}

/// A pending tool call waiting for its result.
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    /// The tool name (e.g., "Read", "Bash").
    pub tool_name: String,
    /// The styled line to display.
    pub styled_line: Line<'static>,
}

/// Status of a tool call in the panel display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// Tool call sent, waiting for result.
    Pending,
    /// Tool call completed successfully.
    Success,
    /// Tool call returned an error.
    Error,
}

/// A tool call entry for the panel display.
#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    /// The tool name (e.g., "Read", "Bash").
    pub tool_name: String,
    /// Summary of the key argument (e.g., "git status", "/path/to/file.rs").
    pub summary: String,
    /// Current status.
    pub status: ToolCallStatus,
    /// Tool use ID for correlating with results.
    pub tool_use_id: Option<String>,
}

/// Which panel is currently focused for scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedPanel {
    Main,
    Tools,
}

impl SelectedPanel {
    pub fn toggle(&mut self) {
        *self = match self {
            SelectedPanel::Main => SelectedPanel::Tools,
            SelectedPanel::Tools => SelectedPanel::Main,
        }
    }
}

/// Tool call tracking and panel display state.
pub struct ToolPanel {
    /// Tool call entries for the panel display.
    pub entries: Vec<ToolCallEntry>,
    /// Scroll offset for the tool panel.
    pub scroll_offset: u16,
    /// Cached height of the tool panel (set during draw).
    pub height: u16,
    /// Selected tool index when tools panel is focused (None = no selection).
    pub selected: Option<usize>,
    /// Maps tool_use_id to tool name for correlating results with calls.
    pub id_to_name: HashMap<String, String>,
    /// Pending tool calls waiting for their results (keyed by tool_use_id).
    pub pending_calls: HashMap<String, PendingToolCall>,
    /// Whether the tool panel is collapsed.
    pub collapsed: bool,
    /// Which panel is currently focused for scrolling.
    pub selected_panel: SelectedPanel,
}

impl ToolPanel {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scroll_offset: 0,
            height: 0,
            selected: None,
            id_to_name: HashMap::new(),
            pending_calls: HashMap::new(),
            collapsed: false,
            selected_panel: SelectedPanel::Main,
        }
    }

    /// Add a tool call entry to the panel.
    pub fn add_entry(&mut self, entry: ToolCallEntry) {
        self.entries.push(entry);
    }

    /// Update the status of a tool call entry by tool_use_id.
    pub fn update_status(&mut self, tool_use_id: &str, status: ToolCallStatus) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.tool_use_id.as_deref() == Some(tool_use_id))
        {
            entry.status = status;
        }
    }

    pub fn scroll_up(&mut self, amount: u16) {
        if amount == 1 {
            // Single-step: move selection
            self.select_prev();
        } else {
            // Page scroll
            self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        }
    }

    pub fn scroll_down(&mut self, amount: u16) {
        if amount == 1 {
            // Single-step: move selection
            self.select_next();
        } else {
            // Page scroll
            let max =
                self.entries
                    .len()
                    .saturating_sub(self.height.saturating_sub(2) as usize) as u16;
            self.scroll_offset = (self.scroll_offset + amount).min(max);
        }
    }

    /// Move tool panel selection up.
    fn select_prev(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let current = self.selected.unwrap_or(0);
        let new = current.saturating_sub(1);
        self.selected = Some(new);
        self.ensure_selection_visible();
    }

    /// Move tool panel selection down.
    fn select_next(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len().saturating_sub(1);
        let current = self.selected.unwrap_or(0);
        let new = (current + 1).min(max);
        self.selected = Some(new);
        self.ensure_selection_visible();
    }

    /// Ensure the selected tool is visible in the scroll viewport.
    fn ensure_selection_visible(&mut self) {
        let Some(selected) = self.selected else {
            return;
        };
        let inner_height = self.height.saturating_sub(2) as usize;
        if inner_height == 0 {
            return;
        }
        let offset = self.scroll_offset as usize;
        if selected < offset {
            self.scroll_offset = selected as u16;
        } else if selected >= offset + inner_height {
            self.scroll_offset = (selected - inner_height + 1) as u16;
        }
    }
}
