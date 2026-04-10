//! Tool allow modal — pattern-based tool permission granting.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use tracing::debug;

use crate::app::App;
use crate::tool_settings;
use crate::ui::centered_rect;

/// Which field is focused in the tool allow modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAllowField {
    Pattern,
    AllowButton,
    CancelButton,
}

impl ToolAllowField {
    pub fn next(self) -> Self {
        match self {
            Self::Pattern => Self::AllowButton,
            Self::AllowButton => Self::CancelButton,
            Self::CancelButton => Self::Pattern,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Pattern => Self::CancelButton,
            Self::AllowButton => Self::Pattern,
            Self::CancelButton => Self::AllowButton,
        }
    }
}

/// State for the tool allow modal.
#[derive(Debug, Clone)]
pub struct ToolAllowModalState {
    /// The tool name (e.g., "Bash").
    pub tool_name: String,
    /// The editable pattern (e.g., "Bash(git status)").
    pub pattern: String,
    /// Cursor position within the pattern string.
    pub cursor_pos: usize,
    /// Currently focused field.
    pub focus: ToolAllowField,
    /// Error message from a failed allow attempt.
    pub error: Option<String>,
}

impl ToolAllowModalState {
    /// Create a new state pre-filled with the tool name and summary.
    #[allow(dead_code)]
    pub fn new(tool_name: &str, summary: &str) -> Self {
        let pattern = if summary.is_empty() {
            tool_name.to_string()
        } else {
            format!("{}({})", tool_name, summary)
        };
        let cursor_pos = pattern.len();
        Self {
            tool_name: tool_name.to_string(),
            pattern,
            cursor_pos,
            focus: ToolAllowField::Pattern,
            error: None,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.pattern.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.pattern[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
            self.pattern.remove(self.cursor_pos);
        }
    }

    pub fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.pattern[..self.cursor_pos]
                .chars()
                .last()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos -= prev;
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor_pos < self.pattern.len() {
            let next = self.pattern[self.cursor_pos..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor_pos += next;
        }
    }

    pub fn cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn cursor_end(&mut self) {
        self.cursor_pos = self.pattern.len();
    }
}

/// Handle input for the tool allow modal.
pub fn handle_tool_allow_modal_input(app: &mut App, key_code: KeyCode, _modifiers: KeyModifiers) {
    let Some(state) = &mut app.tool_allow_modal_state else {
        return;
    };

    match key_code {
        KeyCode::Esc => {
            app.show_tool_allow_modal = false;
            app.tool_allow_modal_state = None;
        }
        KeyCode::Tab => {
            let next = state.focus.next();
            state.focus = next;
        }
        KeyCode::BackTab => {
            let prev = state.focus.prev();
            state.focus = prev;
        }
        KeyCode::Enter => match state.focus {
            ToolAllowField::Pattern | ToolAllowField::AllowButton => {
                let pattern = state.pattern.clone();
                if pattern.is_empty() {
                    state.error = Some("Pattern cannot be empty".to_string());
                    return;
                }
                match tool_settings::allow_pattern(&pattern, false) {
                    Ok(()) => {
                        debug!("Allowed tool pattern: {}", pattern);
                        app.show_tool_allow_modal = false;
                        app.tool_allow_modal_state = None;
                    }
                    Err(e) => {
                        state.error = Some(format!("Failed: {e}"));
                    }
                }
            }
            ToolAllowField::CancelButton => {
                app.show_tool_allow_modal = false;
                app.tool_allow_modal_state = None;
            }
        },
        _ => match state.focus {
            ToolAllowField::Pattern => match key_code {
                KeyCode::Char(c) => state.insert_char(c),
                KeyCode::Backspace => state.delete_char_before(),
                KeyCode::Left => state.cursor_left(),
                KeyCode::Right => state.cursor_right(),
                KeyCode::Home => state.cursor_home(),
                KeyCode::End => state.cursor_end(),
                _ => {}
            },
            ToolAllowField::AllowButton | ToolAllowField::CancelButton => match key_code {
                KeyCode::Left => {
                    let prev = state.focus.prev();
                    state.focus = prev;
                }
                KeyCode::Right => {
                    let next = state.focus.next();
                    state.focus = next;
                }
                _ => {}
            },
        },
    }
}

/// Render a text field with cursor.
fn render_text_field(value: &str, cursor_pos: usize, field_width: usize) -> Vec<Span<'static>> {
    let display_value: String = if value.len() > field_width {
        let start = cursor_pos.saturating_sub(field_width / 2);
        let end = (start + field_width).min(value.len());
        let start = end.saturating_sub(field_width);
        value[start..end].to_string()
    } else {
        value.to_string()
    };

    let visible_cursor = if value.len() > field_width {
        let start = cursor_pos.saturating_sub(field_width / 2);
        let end = (start + field_width).min(value.len());
        let start = end.saturating_sub(field_width);
        cursor_pos - start
    } else {
        cursor_pos
    };

    let char_indices: Vec<_> = display_value.char_indices().collect();
    let (before, cursor_char, rest) = if visible_cursor < char_indices.len() {
        let (idx, _) = char_indices[visible_cursor];
        let before = display_value[..idx].to_string();
        let cc = display_value[idx..]
            .chars()
            .next()
            .unwrap_or(' ')
            .to_string();
        let rest_start = idx + cc.len();
        let rest = if rest_start < display_value.len() {
            display_value[rest_start..].to_string()
        } else {
            String::new()
        };
        (before, cc, rest)
    } else {
        (display_value.clone(), " ".to_string(), String::new())
    };

    vec![
        Span::styled(before, Style::default().fg(Color::White)),
        Span::styled(
            cursor_char,
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(rest, Style::default().fg(Color::White)),
    ]
}

/// Draw the tool allow modal.
pub fn draw_tool_allow_modal(f: &mut Frame, app: &App) {
    let Some(state) = &app.tool_allow_modal_state else {
        return;
    };

    let modal_width: u16 = 60;
    let modal_height: u16 = 11;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    f.render_widget(Clear, modal_area);

    let label_style = Style::default().fg(Color::DarkGray);
    let field_width = modal_width.saturating_sub(6) as usize;

    // Pattern field with cursor
    let pattern_focused = state.focus == ToolAllowField::Pattern;
    let pattern_spans = if pattern_focused {
        render_text_field(&state.pattern, state.cursor_pos, field_width)
    } else {
        let display = if state.pattern.len() > field_width {
            format!("{}…", &state.pattern[..field_width - 1])
        } else {
            state.pattern.clone()
        };
        vec![Span::styled(display, Style::default().fg(Color::White))]
    };

    // Buttons
    let allow_focused = state.focus == ToolAllowField::AllowButton;
    let cancel_focused = state.focus == ToolAllowField::CancelButton;
    let allow_style = if allow_focused {
        Style::default().fg(Color::Black).bg(Color::Green)
    } else {
        Style::default().fg(Color::Green)
    };
    let cancel_style = if cancel_focused {
        Style::default().fg(Color::Black).bg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let mut content: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![Span::raw("  "), Span::styled("Pattern", label_style)]),
        Line::from(
            std::iter::once(Span::raw("  "))
                .chain(pattern_spans)
                .collect::<Vec<_>>(),
        ),
    ];

    // Error line
    if let Some(ref error) = state.error {
        content.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(error.clone(), Style::default().fg(Color::Red)),
        ]));
    } else {
        content.push(Line::from(""));
    }

    // Hint
    content.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Tip: use * for wildcards, e.g. Bash(git:*)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    content.push(Line::from(""));

    // Buttons row
    content.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(" Allow ", allow_style),
        Span::raw("  "),
        Span::styled(" Cancel ", cancel_style),
    ]));

    let title = format!(" Allow {} ", state.tool_name);
    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
