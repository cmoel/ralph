//! Workers stream modal — live output viewer for concurrent workers.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;

/// State for the workers stream modal.
#[derive(Debug)]
pub struct WorkersStreamState {
    /// Index into app.workers for the selected worker.
    pub selected: usize,
    /// Scroll offset for the worker list (left pane).
    pub scroll_offset: usize,
    /// Scroll offset for the output stream (right pane).
    pub stream_scroll: usize,
    /// Whether to auto-scroll the output stream to the bottom.
    pub auto_scroll: bool,
}

impl WorkersStreamState {
    /// Create a new state, starting on the given worker index.
    pub fn new(selected: usize) -> Self {
        Self {
            selected,
            scroll_offset: 0,
            stream_scroll: 0,
            auto_scroll: true,
        }
    }

    /// Move selection to the previous worker, clamping at 0.
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
        self.reset_stream();
    }

    /// Move selection to the next worker, clamping at max.
    pub fn select_next(&mut self, worker_count: usize) {
        if worker_count > 0 && self.selected < worker_count - 1 {
            self.selected += 1;
            self.reset_stream();
        }
    }

    /// Clamp the selected index to valid bounds after workers change.
    pub fn clamp_selected(&mut self, worker_count: usize) {
        if worker_count == 0 {
            self.selected = 0;
        } else if self.selected >= worker_count {
            self.selected = worker_count - 1;
        }
    }

    /// Reset stream scroll and re-enable auto-scroll (when switching workers).
    fn reset_stream(&mut self) {
        self.stream_scroll = 0;
        self.auto_scroll = true;
    }

    /// Scroll the output stream up by `amount` lines, disabling auto-scroll.
    pub fn scroll_up(&mut self, amount: usize) {
        self.stream_scroll = self.stream_scroll.saturating_sub(amount);
        self.auto_scroll = false;
    }

    /// Scroll the output stream down by `amount` lines, disabling auto-scroll.
    pub fn scroll_down(&mut self, amount: usize, max_scroll: usize) {
        self.stream_scroll = (self.stream_scroll + amount).min(max_scroll);
        self.auto_scroll = false;
    }

    /// Jump to the top of the output stream.
    pub fn scroll_to_top(&mut self) {
        self.stream_scroll = 0;
        self.auto_scroll = false;
    }

    /// Jump to the bottom of the output stream and re-enable auto-scroll.
    #[allow(dead_code)]
    pub fn scroll_to_bottom(&mut self, max_scroll: usize) {
        self.stream_scroll = max_scroll;
        self.auto_scroll = true;
    }
}

/// Handle keyboard input for the workers stream modal.
pub fn handle_workers_stream_input(app: &mut App, key_code: KeyCode, modifiers: KeyModifiers) {
    let worker_count = app.workers.len();
    let Some(state) = &mut app.workers_stream_state else {
        return;
    };

    match key_code {
        KeyCode::Esc => {
            app.show_workers_stream = false;
            app.workers_stream_state = None;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.select_prev();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.select_next(worker_count);
        }
        KeyCode::Char('g') => {
            state.scroll_to_top();
        }
        KeyCode::Char('G') => {
            // max_scroll will be computed during render; use a large value here
            state.auto_scroll = true;
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            state.scroll_up(10);
        }
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            // Use a generous max; render will clamp
            state.scroll_down(10, usize::MAX);
            // Still disable auto-scroll for Ctrl+d
            state.auto_scroll = false;
        }
        _ => {}
    }
}

/// Draw the workers stream modal (full-screen overlay).
pub fn draw_workers_stream(f: &mut Frame, app: &mut App) {
    let Some(state) = &mut app.workers_stream_state else {
        return;
    };

    // Clamp selection in case workers changed
    state.clamp_selected(app.workers.len());

    let area = f.area();
    // Leave a 1-cell margin on each side for visual breathing room
    let modal_area = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    f.render_widget(Clear, modal_area);

    let outer_block = Block::default()
        .borders(Borders::ALL)
        .title(" Workers ")
        .style(Style::default().fg(Color::White));
    let inner_area = outer_block.inner(modal_area);
    f.render_widget(outer_block, modal_area);

    if app.workers.is_empty() {
        let msg = Paragraph::new("No workers running").style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, inner_area);
        return;
    }

    // Split into left (worker list) and right (output stream)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
        .split(inner_area);

    draw_worker_list(f, app, chunks[0]);
    draw_worker_output(f, app, chunks[1]);
}

/// Draw the worker list in the left pane.
fn draw_worker_list(f: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.workers_stream_state else {
        return;
    };

    let list_block = Block::default()
        .borders(Borders::RIGHT)
        .style(Style::default().fg(Color::DarkGray));
    let list_inner = list_block.inner(area);
    f.render_widget(list_block, area);

    let visible_height = list_inner.height as usize;
    let mut lines: Vec<Line> = Vec::new();

    for (i, worker) in app.workers.iter().enumerate() {
        let status_icon = if worker.child_process.is_some() {
            "▶"
        } else {
            "○"
        };

        let bead_title = worker.hooked_bead_id.as_deref().unwrap_or("idle");

        let max_title_len = area.width.saturating_sub(6) as usize; // icon + space + index + padding
        let truncated = if bead_title.len() > max_title_len {
            &bead_title[..max_title_len]
        } else {
            bead_title
        };

        let is_selected = i == state.selected;
        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default()
        };

        let icon_color = if worker.child_process.is_some() {
            Color::Green
        } else {
            Color::DarkGray
        };

        let line = if is_selected {
            // For selected line, use inverted colors throughout
            Line::from(vec![
                Span::styled(format!(" {status_icon} "), style),
                Span::styled(truncated.to_string(), style),
                // Pad to full width for highlight effect
                Span::styled(
                    " ".repeat(max_title_len.saturating_sub(truncated.len())),
                    style,
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!(" {status_icon} "), Style::default().fg(icon_color)),
                Span::raw(truncated.to_string()),
            ])
        };

        lines.push(line);
    }

    // Apply scroll offset for the list
    let start = state.scroll_offset.min(lines.len());
    let end = (start + visible_height).min(lines.len());
    let visible_lines: Vec<Line> = lines[start..end].to_vec();

    let list_widget = Paragraph::new(visible_lines);
    f.render_widget(list_widget, list_inner);
}

/// Draw the selected worker's output in the right pane.
fn draw_worker_output(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(state) = &mut app.workers_stream_state else {
        return;
    };

    let output_block = Block::default()
        .borders(Borders::NONE)
        .style(Style::default());
    let output_inner = output_block.inner(area);
    f.render_widget(output_block, area);

    let visible_height = output_inner.height as usize;

    if state.selected >= app.workers.len() {
        return;
    }

    let worker = &app.workers[state.selected];
    let total_lines = worker.output_lines.len();

    // Compute max scroll
    let max_scroll = total_lines.saturating_sub(visible_height);

    // Auto-scroll: pin to bottom
    if state.auto_scroll {
        state.stream_scroll = max_scroll;
    } else {
        // Clamp manual scroll
        state.stream_scroll = state.stream_scroll.min(max_scroll);
    }

    let start = state.stream_scroll;
    let end = (start + visible_height).min(total_lines);
    let visible: Vec<Line> = if start < total_lines {
        worker.output_lines[start..end].to_vec()
    } else {
        Vec::new()
    };

    let output_widget = Paragraph::new(visible);
    f.render_widget(output_widget, output_inner);

    // Scroll indicator when not at bottom
    if state.stream_scroll < max_scroll && visible_height > 0 {
        let indicator = Span::styled(
            " ▼ more ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        let indicator_area = Rect {
            x: output_inner.x + output_inner.width.saturating_sub(10),
            y: output_inner.y + output_inner.height.saturating_sub(1),
            width: 10.min(output_inner.width),
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(indicator)), indicator_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_with_auto_scroll_enabled() {
        let state = WorkersStreamState::new(0);
        assert!(state.auto_scroll);
        assert_eq!(state.selected, 0);
        assert_eq!(state.stream_scroll, 0);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn select_next_advances_and_resets_stream() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 42;
        state.auto_scroll = false;

        state.select_next(3);

        assert_eq!(state.selected, 1);
        assert_eq!(state.stream_scroll, 0);
        assert!(state.auto_scroll);
    }

    #[test]
    fn select_next_clamps_at_last_worker() {
        let mut state = WorkersStreamState::new(2);

        state.select_next(3); // already at index 2 with 3 workers
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn select_next_noop_with_zero_workers() {
        let mut state = WorkersStreamState::new(0);
        state.select_next(0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn select_prev_decrements_and_resets_stream() {
        let mut state = WorkersStreamState::new(2);
        state.stream_scroll = 10;
        state.auto_scroll = false;

        state.select_prev();

        assert_eq!(state.selected, 1);
        assert_eq!(state.stream_scroll, 0);
        assert!(state.auto_scroll);
    }

    #[test]
    fn select_prev_clamps_at_zero() {
        let mut state = WorkersStreamState::new(0);
        state.select_prev();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn single_worker_cannot_navigate_past_bounds() {
        let mut state = WorkersStreamState::new(0);

        state.select_prev();
        assert_eq!(state.selected, 0);

        state.select_next(1);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn clamp_selected_when_workers_shrink() {
        let mut state = WorkersStreamState::new(2);
        assert_eq!(state.selected, 2);

        state.clamp_selected(2); // workers Vec shrunk to length 2
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn clamp_selected_when_workers_empty() {
        let mut state = WorkersStreamState::new(2);
        state.clamp_selected(0);
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn clamp_selected_noop_when_in_bounds() {
        let mut state = WorkersStreamState::new(1);
        state.clamp_selected(5);
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn scroll_up_disables_auto_scroll() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 10;
        assert!(state.auto_scroll);

        state.scroll_up(3);

        assert_eq!(state.stream_scroll, 7);
        assert!(!state.auto_scroll);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 2;

        state.scroll_up(5);

        assert_eq!(state.stream_scroll, 0);
        assert!(!state.auto_scroll);
    }

    #[test]
    fn scroll_down_disables_auto_scroll() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 5;

        state.scroll_down(3, 100);

        assert_eq!(state.stream_scroll, 8);
        assert!(!state.auto_scroll);
    }

    #[test]
    fn scroll_down_clamps_at_max() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 95;

        state.scroll_down(10, 100);

        assert_eq!(state.stream_scroll, 100);
        assert!(!state.auto_scroll);
    }

    #[test]
    fn scroll_to_top_disables_auto_scroll() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 50;
        state.auto_scroll = true;

        state.scroll_to_top();

        assert_eq!(state.stream_scroll, 0);
        assert!(!state.auto_scroll);
    }

    #[test]
    fn scroll_to_bottom_enables_auto_scroll() {
        let mut state = WorkersStreamState::new(0);
        state.auto_scroll = false;

        state.scroll_to_bottom(100);

        assert_eq!(state.stream_scroll, 100);
        assert!(state.auto_scroll);
    }

    #[test]
    fn switching_worker_resets_scroll_and_enables_auto_scroll() {
        let mut state = WorkersStreamState::new(0);
        state.stream_scroll = 50;
        state.auto_scroll = false;

        state.select_next(3); // switch to worker 1

        assert_eq!(state.selected, 1);
        assert_eq!(state.stream_scroll, 0);
        assert!(state.auto_scroll);
    }
}
