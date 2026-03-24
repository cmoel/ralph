//! Specs/work panel modal — list view with preview.

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;
use crate::work_source::{WorkItem, WorkItemStatus};

/// State for the specs/work panel modal.
#[derive(Debug)]
pub struct SpecsPanelState {
    /// List of work items.
    pub specs: Vec<WorkItem>,
    /// Currently selected index.
    pub selected: usize,
    /// Scroll offset for the list.
    pub scroll_offset: usize,
    /// Error message if parsing failed.
    pub error: Option<String>,
    /// Directory where spec files are located.
    pub specs_dir: PathBuf,
    /// Label for the panel title (e.g., "Specs", "Beads").
    pub panel_label: String,
    /// Whether data is still loading from a background thread.
    pub is_loading: bool,
}

impl SpecsPanelState {
    /// Create a panel in loading state (data will arrive via populate()).
    pub fn new_loading(label: &str, specs_dir: &std::path::Path) -> Self {
        Self {
            specs: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            error: None,
            specs_dir: specs_dir.to_path_buf(),
            panel_label: label.to_string(),
            is_loading: true,
        }
    }

    /// Populate the panel with results from a background list_items call.
    pub fn populate(&mut self, result: Result<Vec<WorkItem>, String>) {
        self.is_loading = false;
        match result {
            Ok(mut items) => {
                Self::sort_items(&mut items);
                self.specs = items;
            }
            Err(e) => {
                self.error = Some(e);
            }
        }
    }

    /// Sort work items by status then timestamp.
    fn sort_items(items: &mut [WorkItem]) {
        items.sort_by(|a, b| match a.status.cmp(&b.status) {
            std::cmp::Ordering::Equal => {
                // Within same status, sort by timestamp descending (newest first)
                // None values go to the end
                match (&b.timestamp, &a.timestamp) {
                    (Some(b_ts), Some(a_ts)) => b_ts.cmp(a_ts),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
            }
            other => other,
        });
    }

    /// Get the path to the currently selected spec file.
    pub fn selected_spec_path(&self) -> Option<PathBuf> {
        self.specs
            .get(self.selected)
            .map(|spec| self.specs_dir.join(format!("{}.md", spec.name)))
    }

    /// Read the head of the selected spec file.
    pub fn read_selected_spec_head(&self, max_lines: usize) -> Result<Vec<String>, String> {
        let path = self.selected_spec_path().ok_or("No spec selected")?;

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err("File not found".to_string());
            }
            Err(e) => {
                return Err(format!("Error reading file: {}", e));
            }
        };

        Ok(contents.lines().take(max_lines).map(String::from).collect())
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.specs.is_empty() && self.selected < self.specs.len() - 1 {
            self.selected += 1;
        }
    }

    /// Count of blocked items.
    pub fn blocked_count(&self) -> usize {
        self.specs
            .iter()
            .filter(|s| s.status == WorkItemStatus::Blocked)
            .count()
    }

    /// Ensure selected item is visible, adjusting scroll_offset if needed.
    pub fn ensure_visible(&mut self, visible_height: usize) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected - visible_height + 1;
        }
    }
}

/// Handle keyboard input for the specs panel.
pub fn handle_specs_panel_input(app: &mut App, key_code: KeyCode) {
    let Some(state) = &mut app.specs_panel_state else {
        return;
    };

    match key_code {
        KeyCode::Esc => {
            app.show_specs_panel = false;
            app.specs_panel_state = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
        }
        _ => {}
    }
}

/// Draw the specs panel modal.
pub fn draw_specs_panel(f: &mut Frame, app: &mut App) {
    let modal_width: u16 = 70;
    let modal_height: u16 = 24;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    let Some(state) = &mut app.specs_panel_state else {
        return;
    };

    // Calculate inner area (minus borders)
    let inner_height = modal_height.saturating_sub(2) as usize;
    let inner_width = modal_width.saturating_sub(2) as usize;
    let blocked_count = state.blocked_count();

    // Reserve space for warning banner if there are blocked specs
    let banner_height = if blocked_count > 0 { 3 } else { 0 };

    // Split layout: list (~40%), separator (1), preview (~60%)
    let list_area_height = ((inner_height - banner_height) * 40 / 100).max(3);
    let separator_height = 1;
    let preview_area_height =
        inner_height.saturating_sub(banner_height + list_area_height + separator_height);

    // Ensure selected item is visible
    state.ensure_visible(list_area_height);

    let mut content: Vec<Line> = Vec::new();

    // Warning banner for blocked specs
    if blocked_count > 0 {
        let banner_width = inner_width;
        let banner_fill = "\u{2588}".repeat(banner_width);
        let warning_text = format!(
            "\u{2588}\u{2588}  \u{26a0} {} BLOCKED SPEC{} - ACTION REQUIRED",
            blocked_count,
            if blocked_count == 1 { "" } else { "S" }
        );
        let padding = banner_width.saturating_sub(warning_text.chars().count());
        let padded_warning = format!("{}{}", warning_text, " ".repeat(padding.saturating_sub(2)));
        let padded_warning = format!("{}\u{2588}\u{2588}", padded_warning);

        content.push(Line::from(Span::styled(
            banner_fill.clone(),
            Style::default().fg(Color::White).bg(Color::Red),
        )));
        content.push(Line::from(Span::styled(
            padded_warning,
            Style::default().fg(Color::Yellow).bg(Color::Red),
        )));
        content.push(Line::from(Span::styled(
            banner_fill,
            Style::default().fg(Color::White).bg(Color::Red),
        )));
    }

    // Handle loading/error/empty cases
    if state.is_loading {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(error) = &state.error {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!("  Error: {}", error),
            Style::default().fg(Color::Red),
        )));
    } else if state.specs.is_empty() {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  No specs found",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Render visible specs
        let visible_start = state.scroll_offset;
        let visible_end = (state.scroll_offset + list_area_height).min(state.specs.len());

        for spec_idx in visible_start..visible_end {
            let spec = &state.specs[spec_idx];
            let is_selected = spec_idx == state.selected;

            // Build the line: "  [Status] spec-name"
            let status_label = format!("[{}]", spec.status.label());
            let status_width = 14; // Fixed width for alignment
            let padded_status = format!("{:width$}", status_label, width = status_width);

            let line_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };

            let status_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default().fg(spec.status.color())
            };

            let name_style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };

            // Calculate padding needed for full-width selection highlight
            let line_content = format!("  {}{}", padded_status, spec.name);
            let padding = inner_width.saturating_sub(line_content.len());

            content.push(Line::from(vec![
                Span::styled("  ", line_style),
                Span::styled(padded_status, status_style),
                Span::styled(&spec.name, name_style),
                Span::styled(" ".repeat(padding), line_style),
            ]));
        }

        // Fill remaining list space if list is shorter than allocated height
        let rendered_lines = visible_end - visible_start;
        for _ in rendered_lines..list_area_height {
            content.push(Line::from(""));
        }

        // Horizontal separator between list and preview
        let separator = "\u{2500}".repeat(inner_width);
        content.push(Line::from(Span::styled(
            separator,
            Style::default().fg(Color::DarkGray),
        )));

        // Preview pane
        match state.read_selected_spec_head(preview_area_height) {
            Ok(lines) => {
                for line in lines.iter().take(preview_area_height) {
                    // Truncate long lines to fit width
                    let display_line: String = line.chars().take(inner_width).collect();
                    content.push(Line::from(Span::styled(
                        display_line,
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                // Fill remaining preview space
                for _ in lines.len()..preview_area_height {
                    content.push(Line::from(""));
                }
            }
            Err(error) => {
                content.push(Line::from(Span::styled(
                    format!("  {}", error),
                    Style::default().fg(Color::Yellow),
                )));
                // Fill remaining preview space
                for _ in 1..preview_area_height {
                    content.push(Line::from(""));
                }
            }
        }
    }

    let panel_title = format!(" {} ", state.panel_label);
    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(panel_title)
            .title_alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
