//! Bead picker modal — filtering list overlay for selecting a bead by ID or title.

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use serde::Deserialize;

use crate::app::App;
use crate::ui::centered_rect;

/// Minimal bead data for the picker list.
#[derive(Debug, Clone, Deserialize)]
pub struct BeadPickerItem {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: Option<u64>,
}

/// State for the bead picker modal.
#[derive(Debug)]
pub struct BeadPickerState {
    /// All loaded items.
    pub items: Vec<BeadPickerItem>,
    /// Indices into `items` that match the current filter.
    pub filtered: Vec<usize>,
    /// Filter text input.
    pub filter: String,
    /// Cursor position (byte offset) within `filter`.
    pub cursor_pos: usize,
    /// Selected index within `filtered`.
    pub selected: usize,
    /// Scroll offset for the list.
    pub scroll_offset: usize,
    /// Whether data is still loading.
    pub is_loading: bool,
    /// Error message if loading failed.
    pub error: Option<String>,
}

impl BeadPickerState {
    /// Create a new state in loading mode.
    pub fn new_loading() -> Self {
        Self {
            items: Vec::new(),
            filtered: Vec::new(),
            filter: String::new(),
            cursor_pos: 0,
            selected: 0,
            scroll_offset: 0,
            is_loading: true,
            error: None,
        }
    }

    /// Populate with loaded data.
    pub fn populate(&mut self, result: Result<Vec<BeadPickerItem>, String>) {
        self.is_loading = false;
        match result {
            Ok(items) => {
                self.items = items;
                self.update_filter();
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    /// Recompute `filtered` indices based on current filter text.
    fn update_filter(&mut self) {
        let query = self.filter.to_lowercase();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if query.is_empty() {
                    return true;
                }
                let id_lower = item.id.to_lowercase();
                // Match against full ID and short ID (after last hyphen)
                let short_id = id_lower.rsplit('-').next().unwrap_or("");
                id_lower.contains(&query)
                    || short_id.contains(&query)
                    || item.title.to_lowercase().contains(&query)
            })
            .map(|(i, _)| i)
            .collect();
        // Reset selection when filter changes
        self.selected = 0;
        self.scroll_offset = 0;
    }

    fn insert_char(&mut self, c: char) {
        self.filter.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.update_filter();
    }

    fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.filter[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
            self.filter.remove(self.cursor_pos);
            self.update_filter();
        }
    }

    fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.filter[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
        }
    }

    fn cursor_right(&mut self) {
        if self.cursor_pos < self.filter.len() {
            let next = self.filter[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += next;
        }
    }

    fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn select_next(&mut self) {
        if !self.filtered.is_empty() && self.selected < self.filtered.len() - 1 {
            self.selected += 1;
        }
    }

    /// Ensure the selected item is visible given a list height.
    fn ensure_visible(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected - visible_height + 1;
        }
    }
}

/// Fetch bead list data by running `bd list --json`.
pub fn fetch_bead_picker_data(bd_path: &str) -> Result<Vec<BeadPickerItem>, String> {
    let output = crate::bd_lock::with_lock(|| {
        std::process::Command::new(bd_path)
            .args(["list", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .map_err(|e| format!("Failed to run bd list: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("bd list failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }

    serde_json::from_str::<Vec<BeadPickerItem>>(trimmed)
        .map_err(|e| format!("Failed to parse bd list output: {e}"))
}

/// Handle key input for the bead picker modal.
pub fn handle_bead_picker_input(app: &mut App, key_code: KeyCode) {
    let Some(state) = &mut app.bead_picker_state else {
        return;
    };

    match key_code {
        KeyCode::Esc => {
            app.show_bead_picker = false;
            app.bead_picker_state = None;
            app.pending_dep = None;
            // No result — cancelled
        }
        KeyCode::Enter => {
            // Confirm selection
            if let Some(&idx) = state.filtered.get(state.selected) {
                app.bead_picker_result = Some(
                    app.bead_picker_state.as_ref().unwrap().items[idx]
                        .id
                        .clone(),
                );
            }
            app.show_bead_picker = false;
            app.bead_picker_state = None;
        }
        KeyCode::Up | KeyCode::BackTab => {
            state.select_prev();
        }
        KeyCode::Down | KeyCode::Tab => {
            state.select_next();
        }
        KeyCode::Left => {
            state.cursor_left();
        }
        KeyCode::Right => {
            state.cursor_right();
        }
        KeyCode::Home => {
            state.cursor_pos = 0;
        }
        KeyCode::End => {
            state.cursor_pos = state.filter.len();
        }
        KeyCode::Backspace => {
            state.delete_char_before();
        }
        KeyCode::Char(c) => {
            state.insert_char(c);
        }
        _ => {}
    }
}

/// Draw the bead picker modal.
pub fn draw_bead_picker(f: &mut Frame, app: &mut App) {
    let Some(state) = &mut app.bead_picker_state else {
        return;
    };

    let modal_width: u16 = 70;
    let modal_height: u16 = 20;
    let modal_area = centered_rect(modal_width, modal_height, f.area());
    f.render_widget(Clear, modal_area);

    let mut content: Vec<Line> = Vec::new();

    // Filter input line with cursor
    let filter_display = if state.filter.is_empty() && state.cursor_pos == 0 {
        // Show placeholder when empty
        let cursor = Span::styled(" ", Style::default().fg(Color::Black).bg(Color::White));
        let placeholder = Span::styled(" type to filter...", Style::default().fg(Color::DarkGray));
        Line::from(vec![Span::raw("  > "), cursor, placeholder])
    } else {
        // Build text with cursor
        let char_indices: Vec<(usize, char)> = state.filter.char_indices().collect();
        let visible_cursor = char_indices
            .iter()
            .position(|(byte_idx, _)| *byte_idx == state.cursor_pos)
            .unwrap_or(char_indices.len());

        let (before, cursor_char, after) = if visible_cursor < char_indices.len() {
            let (idx, _) = char_indices[visible_cursor];
            let before = &state.filter[..idx];
            let c = state.filter[idx..].chars().next().unwrap_or(' ');
            let rest_start = idx + c.len_utf8();
            let after = &state.filter[rest_start..];
            (before.to_string(), c.to_string(), after.to_string())
        } else {
            (state.filter.clone(), " ".to_string(), String::new())
        };

        Line::from(vec![
            Span::raw("  > "),
            Span::styled(before, Style::default().fg(Color::White)),
            Span::styled(
                cursor_char,
                Style::default().fg(Color::Black).bg(Color::White),
            ),
            Span::styled(after, Style::default().fg(Color::White)),
        ])
    };
    content.push(filter_display);
    content.push(Line::from(""));

    if state.is_loading {
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(error) = &state.error {
        content.push(Line::from(Span::styled(
            format!("  Error: {error}"),
            Style::default().fg(Color::Yellow),
        )));
    } else if state.filtered.is_empty() {
        let msg = if state.filter.is_empty() {
            "  No beads found"
        } else {
            "  No matches"
        };
        content.push(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // List height = modal inner height - filter line (1) - blank line (1) - border (2)
        let list_height = (modal_height as usize).saturating_sub(6);
        state.ensure_visible(list_height);

        // Column widths
        let id_width = 12;

        let visible_items = state
            .filtered
            .iter()
            .skip(state.scroll_offset)
            .take(list_height);

        for (view_idx, &item_idx) in visible_items.enumerate() {
            let item = &state.items[item_idx];
            let is_selected = state.scroll_offset + view_idx == state.selected;

            let line_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };

            // Status indicator
            let status_char = match item.status.as_str() {
                "open" => "○",
                "in_progress" => "◐",
                "blocked" => "●",
                "closed" => "✓",
                "deferred" => "❄",
                "pinned" => "📌",
                _ => "·",
            };

            // Priority label
            let priority_str = item.priority.map(|p| format!("P{p}")).unwrap_or_default();

            // Truncate title to fit
            let inner_width = (modal_width as usize).saturating_sub(4); // borders + padding
            let prefix_width = 2 + 2 + id_width + 1 + 3 + 1; // "  " + status + " " + id + " " + priority + " "
            let title_max = inner_width.saturating_sub(prefix_width);
            let title = if item.title.len() > title_max {
                format!("{}…", &item.title[..title_max.saturating_sub(1)])
            } else {
                item.title.clone()
            };

            // Pad to full width for highlight
            let line_text = format!(
                "  {status_char} {id:<id_width$} {priority:<3} {title}",
                id = item.id,
                priority = priority_str,
            );
            let padded_len = inner_width;
            let pad = padded_len.saturating_sub(line_text.chars().count());

            let padded = format!("{line_text}{}", " ".repeat(pad));
            content.push(Line::from(Span::styled(padded, line_style)));
        }

        // Scroll indicator
        if state.filtered.len() > list_height {
            let total = state.filtered.len();
            let showing_end = (state.scroll_offset + list_height).min(total);
            content.push(Line::from(""));
            content.push(Line::from(Span::styled(
                format!("  {}-{} of {total}", state.scroll_offset + 1, showing_end,),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Select Bead ")
        .title_alignment(ratatui::layout::Alignment::Center)
        .style(Style::default().fg(Color::White));

    let widget = Paragraph::new(content).block(block);
    f.render_widget(widget, modal_area);
}
