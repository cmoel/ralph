use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::state::App;

impl App {
    pub fn visual_line_count(&mut self) -> u16 {
        if self.main_pane_width == 0 {
            return 0;
        }
        if let Some(cached) = self.cached_visual_line_count {
            return cached;
        }
        // Include both completed lines and the current partial line
        let w = self.selected_worker;
        let mut content: Vec<Line> = self.workers[w].output_lines.to_vec();
        if !self.workers[w].current_line.is_empty() {
            content.push(Line::raw(&self.workers[w].current_line));
        }
        let paragraph = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        let count = paragraph.line_count(self.main_pane_width) as u16;
        self.cached_visual_line_count = Some(count);
        count
    }

    pub fn max_scroll(&mut self) -> u16 {
        self.visual_line_count()
            .saturating_sub(self.main_pane_height)
    }

    #[allow(dead_code)]
    pub fn scroll_up(&mut self, amount: u16) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(amount);
            self.is_auto_following = false;
        }
    }

    #[allow(dead_code)]
    pub fn scroll_down(&mut self, amount: u16) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
        if self.scroll_offset >= max {
            self.is_auto_following = true;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll();
        self.is_auto_following = true;
    }

    /// Adds a styled line to the output.
    pub fn add_line(&mut self, line: Line<'static>) {
        let w = self.selected_worker;
        self.workers[w].output_lines.push(line);
        self.cached_visual_line_count = None;
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    /// Adds a plain text line to the output (convenience method).
    pub fn add_text_line(&mut self, text: String) {
        self.add_line(Line::raw(text));
    }

    /// Appends text with indentation to the current line, flushing complete lines to output.
    /// Used for assistant text which should be indented under the header.
    pub fn append_indented_text(&mut self, text: &str) {
        self.in_indented_text = true;
        let w = self.selected_worker;
        for ch in text.chars() {
            if ch == '\n' {
                // Flush current line to output (with indentation prefix)
                let line = std::mem::take(&mut self.workers[w].current_line);
                self.add_text_line(format!("  {}", line));
            } else {
                self.workers[w].current_line.push(ch);
            }
        }
        // Update display with partial line if auto-following
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    /// Flushes any remaining text in current_line to output.
    /// Uses indentation if we're in an indented text block.
    pub fn flush_current_line(&mut self) {
        let w = self.selected_worker;
        if !self.workers[w].current_line.is_empty() {
            let line = std::mem::take(&mut self.workers[w].current_line);
            self.cached_visual_line_count = None;
            if self.in_indented_text {
                self.add_text_line(format!("  {}", line));
            } else {
                self.add_text_line(line);
            }
        }
        self.in_indented_text = false;
    }
}
