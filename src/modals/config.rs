//! Configuration modal — settings editor for per-project config.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use tracing::debug;

use crate::app::App;
use crate::config::{Config, PartialConfig, save_partial_config};
use crate::get_file_mtime;
use crate::ui::centered_rect;
use crate::validators::validate_executable_path;

/// Log level options for the dropdown.
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Per-tab form state storing field values and validation state.
#[derive(Debug, Clone)]
pub struct TabFormState {
    pub claude_path: String,
    pub log_level_index: usize,
    pub iterations: i32,
    pub keep_awake: bool,
    pub cursor_pos: usize,
    pub error: Option<String>,
    pub validation_errors: HashMap<ConfigModalField, String>,
    /// Fields explicitly set in this tab (only meaningful for project tab).
    pub explicit_fields: HashSet<ConfigModalField>,
}

/// Which field is focused in the config modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigModalField {
    ClaudePath,
    LogLevel,
    Iterations,
    KeepAwake,
    SaveButton,
    CancelButton,
}

impl ConfigModalField {
    pub fn next(self) -> Self {
        match self {
            Self::ClaudePath => Self::LogLevel,
            Self::LogLevel => Self::Iterations,
            Self::Iterations => Self::KeepAwake,
            Self::KeepAwake => Self::SaveButton,
            Self::SaveButton => Self::CancelButton,
            Self::CancelButton => Self::ClaudePath,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::ClaudePath => Self::CancelButton,
            Self::LogLevel => Self::ClaudePath,
            Self::Iterations => Self::LogLevel,
            Self::KeepAwake => Self::Iterations,
            Self::SaveButton => Self::KeepAwake,
            Self::CancelButton => Self::SaveButton,
        }
    }
}

/// State for the config modal form.
#[derive(Debug, Clone)]
pub struct ConfigModalState {
    /// Current focused field.
    pub focus: ConfigModalField,
    /// Form state for editing config values.
    pub form: TabFormState,
    /// Path to the per-project config file, if available.
    pub project_config_path: Option<PathBuf>,
}

impl TabFormState {
    /// Create form state from a PartialConfig (to know which fields are explicit)
    /// and the merged Config (for displaying resolved values).
    fn from_partial_config(partial: &PartialConfig, merged: &Config) -> Self {
        let mut explicit_fields = HashSet::new();

        if partial.claude.path.is_some() {
            explicit_fields.insert(ConfigModalField::ClaudePath);
        }
        if partial.logging.level.is_some() {
            explicit_fields.insert(ConfigModalField::LogLevel);
        }
        if partial.behavior.iterations.is_some() {
            explicit_fields.insert(ConfigModalField::Iterations);
        }
        if partial.behavior.keep_awake.is_some() {
            explicit_fields.insert(ConfigModalField::KeepAwake);
        }

        // Display merged values (so inherited fields show their effective value)
        let log_level_index = LOG_LEVELS
            .iter()
            .position(|&l| l == merged.logging.level)
            .unwrap_or(2);

        Self {
            claude_path: merged.claude.path.clone(),
            log_level_index,
            iterations: merged.behavior.iterations,
            keep_awake: merged.behavior.keep_awake,
            cursor_pos: merged.claude.path.len(),
            error: None,
            validation_errors: HashMap::new(),
            explicit_fields,
        }
    }

    /// Build a Config from the current form values.
    fn to_config(&self) -> Config {
        let mut config = Config {
            claude: crate::config::ClaudeConfig {
                path: self.claude_path.clone(),
                args: None,
            },
            logging: crate::config::LoggingConfig {
                level: self.selected_log_level().to_string(),
            },
            behavior: crate::config::BehaviorConfig::default(),
        };
        config.behavior.iterations = self.iterations;
        config.behavior.keep_awake = self.keep_awake;
        config
    }

    /// Build a PartialConfig from the current form values, including only explicit fields.
    fn to_partial_config(&self) -> PartialConfig {
        PartialConfig {
            claude: crate::config::PartialClaudeConfig {
                path: if self.explicit_fields.contains(&ConfigModalField::ClaudePath) {
                    Some(self.claude_path.clone())
                } else {
                    None
                },
            },
            logging: crate::config::PartialLoggingConfig {
                level: if self.explicit_fields.contains(&ConfigModalField::LogLevel) {
                    Some(self.selected_log_level().to_string())
                } else {
                    None
                },
            },
            behavior: crate::config::PartialBehaviorConfig {
                iterations: if self.explicit_fields.contains(&ConfigModalField::Iterations) {
                    Some(self.iterations)
                } else {
                    None
                },
                keep_awake: if self.explicit_fields.contains(&ConfigModalField::KeepAwake) {
                    Some(self.keep_awake)
                } else {
                    None
                },
                bd_path: None,
                heartbeat_interval: None,
                stale_threshold: None,
                workers: None,
            },
        }
    }

    pub fn selected_log_level(&self) -> &'static str {
        LOG_LEVELS[self.log_level_index]
    }

    /// Check if there are any validation errors.
    pub fn has_validation_errors(&self) -> bool {
        !self.validation_errors.is_empty()
    }
}

impl ConfigModalState {
    /// Create modal state showing resolved config (compiled-in defaults + per-project overrides).
    pub fn from_config(
        partial: &PartialConfig,
        merged_config: &Config,
        project_config_path: Option<PathBuf>,
    ) -> Self {
        Self {
            focus: ConfigModalField::ClaudePath,
            form: TabFormState::from_partial_config(partial, merged_config),
            project_config_path,
        }
    }

    /// Get a reference to the form state.
    pub fn active_form(&self) -> &TabFormState {
        &self.form
    }

    /// Get a mutable reference to the form state.
    fn active_form_mut(&mut self) -> &mut TabFormState {
        &mut self.form
    }

    /// Get a reference to the currently focused text field's value.
    pub fn current_field_value(&self) -> Option<&String> {
        let form = self.active_form();
        match self.focus {
            ConfigModalField::ClaudePath => Some(&form.claude_path),
            _ => None,
        }
    }

    /// Move focus to the next field, resetting cursor position.
    /// Validates the field being left (blur validation).
    pub fn focus_next(&mut self) {
        let leaving_field = self.focus;
        self.focus = self.focus.next();
        self.update_cursor_for_new_focus();
        self.validate_field(leaving_field);
    }

    /// Move focus to the previous field, resetting cursor position.
    /// Validates the field being left (blur validation).
    pub fn focus_prev(&mut self) {
        let leaving_field = self.focus;
        self.focus = self.focus.prev();
        self.update_cursor_for_new_focus();
        self.validate_field(leaving_field);
    }

    /// Update cursor position when focus changes to a new field.
    fn update_cursor_for_new_focus(&mut self) {
        if let Some(value) = self.current_field_value() {
            // Need to clone to avoid borrow issues
            let len = value.len();
            self.active_form_mut().cursor_pos = len;
        } else {
            self.active_form_mut().cursor_pos = 0;
        }
    }

    /// Mark a field as explicitly set (overriding the compiled-in default).
    fn mark_explicit(&mut self) {
        self.form.explicit_fields.insert(self.focus);
    }

    /// Insert a character at the current cursor position.
    pub fn insert_char(&mut self, c: char) {
        let cursor = self.active_form().cursor_pos;
        let field_changed = match self.focus {
            ConfigModalField::ClaudePath => {
                let form = self.active_form_mut();
                if cursor >= form.claude_path.len() {
                    form.claude_path.push(c);
                } else {
                    form.claude_path.insert(cursor, c);
                }
                form.cursor_pos += 1;
                true
            }
            _ => false,
        };
        if field_changed {
            self.mark_explicit();
            self.clear_current_field_error();
        }
    }

    /// Delete the character before the cursor (backspace).
    pub fn delete_char_before(&mut self) {
        if self.active_form().cursor_pos == 0 {
            return;
        }
        let cursor = self.active_form().cursor_pos;
        let field_changed = match self.focus {
            ConfigModalField::ClaudePath => {
                let form = self.active_form_mut();
                form.claude_path.remove(cursor - 1);
                form.cursor_pos -= 1;
                true
            }
            _ => false,
        };
        if field_changed {
            self.mark_explicit();
            self.clear_current_field_error();
        }
    }

    /// Delete the character at the cursor position (delete key).
    pub fn delete_char_at(&mut self) {
        let cursor = self.active_form().cursor_pos;
        let field_changed = match self.focus {
            ConfigModalField::ClaudePath => {
                let form = self.active_form_mut();
                if cursor < form.claude_path.len() {
                    form.claude_path.remove(cursor);
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if field_changed {
            self.mark_explicit();
            self.clear_current_field_error();
        }
    }

    /// Move cursor left within the current field.
    pub fn cursor_left(&mut self) {
        let form = self.active_form_mut();
        if form.cursor_pos > 0 {
            form.cursor_pos -= 1;
        }
    }

    /// Move cursor right within the current field.
    pub fn cursor_right(&mut self) {
        if let Some(value) = self.current_field_value() {
            let len = value.len();
            let form = self.active_form_mut();
            if form.cursor_pos < len {
                form.cursor_pos += 1;
            }
        }
    }

    /// Move to beginning of current field.
    pub fn cursor_home(&mut self) {
        self.active_form_mut().cursor_pos = 0;
    }

    /// Move to end of current field.
    pub fn cursor_end(&mut self) {
        if let Some(value) = self.current_field_value() {
            let len = value.len();
            self.active_form_mut().cursor_pos = len;
        }
    }

    /// Cycle log level selection up.
    pub fn log_level_prev(&mut self) {
        let form = self.active_form_mut();
        if form.log_level_index > 0 {
            form.log_level_index -= 1;
        } else {
            form.log_level_index = LOG_LEVELS.len() - 1;
        }
        self.mark_explicit();
    }

    /// Cycle log level selection down.
    pub fn log_level_next(&mut self) {
        let form = self.active_form_mut();
        if form.log_level_index < LOG_LEVELS.len() - 1 {
            form.log_level_index += 1;
        } else {
            form.log_level_index = 0;
        }
        self.mark_explicit();
    }

    /// Increment iterations value (towards positive/larger countdown).
    pub fn iterations_increment(&mut self) {
        let form = self.active_form_mut();
        if form.iterations < 999 {
            form.iterations += 1;
        }
        self.mark_explicit();
    }

    /// Decrement iterations value (towards -1/infinite).
    pub fn iterations_decrement(&mut self) {
        let form = self.active_form_mut();
        if form.iterations > -1 {
            form.iterations -= 1;
        }
        self.mark_explicit();
    }

    /// Check if there are any validation errors.
    pub fn has_validation_errors(&self) -> bool {
        self.active_form().has_validation_errors()
    }

    /// Validate a specific field and update validation_errors.
    /// Skips validation for inherited (non-explicit) fields.
    pub fn validate_field(&mut self, field: ConfigModalField) {
        if !self.form.explicit_fields.contains(&field) {
            return;
        }

        let form = self.active_form();
        let error = match field {
            ConfigModalField::ClaudePath => validate_executable_path(&form.claude_path),
            _ => None,
        };

        let form = self.active_form_mut();
        if let Some(msg) = error {
            form.validation_errors.insert(field, msg);
        } else {
            form.validation_errors.remove(&field);
        }
    }

    /// Clear validation error for the current field (called when value changes).
    fn clear_current_field_error(&mut self) {
        let focus = self.focus;
        self.active_form_mut().validation_errors.remove(&focus);
    }

    /// Build a Config from the current form values.
    pub fn to_config(&self) -> Config {
        self.active_form().to_config()
    }

    /// Build a PartialConfig from the project tab form values.
    pub fn to_partial_config(&self) -> PartialConfig {
        self.active_form().to_partial_config()
    }

    /// Get the error from the active form.
    pub fn error(&self) -> Option<&String> {
        self.active_form().error.as_ref()
    }

    /// Set error on the active form.
    pub fn set_error(&mut self, error: Option<String>) {
        self.active_form_mut().error = error;
    }

    /// Toggle the keep-awake setting.
    pub fn toggle_keep_awake(&mut self) {
        let form = self.active_form_mut();
        form.keep_awake = !form.keep_awake;
        self.mark_explicit();
    }
}

/// Handle keyboard input for the config modal.
pub fn handle_config_modal_input(app: &mut App, key_code: KeyCode, modifiers: KeyModifiers) {
    let Some(state) = &mut app.config_modal_state else {
        return;
    };

    // Clear any previous error when user takes action
    if state.error().is_some() && key_code != KeyCode::Esc {
        state.set_error(None);
    }

    match key_code {
        // Navigation between fields
        KeyCode::Tab => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                state.focus_prev();
            } else {
                state.focus_next();
            }
        }
        KeyCode::BackTab => {
            state.focus_prev();
        }

        // Cancel / close
        KeyCode::Esc => {
            app.show_config_modal = false;
            app.config_modal_state = None;
        }

        // Enter - context-dependent
        KeyCode::Enter => match state.focus {
            ConfigModalField::SaveButton => {
                // Don't save if there are validation errors
                if state.has_validation_errors() {
                    return;
                }
                // Save as per-project partial config
                let partial = state.to_partial_config();
                let save_result = if let Some(ref path) = state.project_config_path {
                    save_partial_config(&partial, path)
                } else {
                    Err("No project config path".to_string())
                };
                match save_result {
                    Ok(()) => {
                        // Re-merge and update app config
                        let new_merged = state.to_config();
                        app.config = new_merged;
                        // Update mtime so we don't trigger a reload
                        if let Some(ref path) = state.project_config_path {
                            app.project_config_mtime = get_file_mtime(path);
                            // Track the project config path for hot-reload
                            app.project_config_path = Some(path.clone());
                        }
                        app.show_config_modal = false;
                        app.config_modal_state = None;
                        debug!("Config saved successfully via modal");
                    }
                    Err(e) => {
                        state.set_error(Some(e));
                    }
                }
            }
            ConfigModalField::CancelButton => {
                app.show_config_modal = false;
                app.config_modal_state = None;
            }
            _ => {
                // Enter in text fields moves to next field
                state.focus_next();
            }
        },

        // Text input handling
        KeyCode::Char(c) => {
            if matches!(state.focus, ConfigModalField::ClaudePath) {
                state.insert_char(c);
            }
        }

        KeyCode::Backspace => {
            state.delete_char_before();
        }

        KeyCode::Delete => {
            state.delete_char_at();
        }

        // Cursor movement within text fields
        KeyCode::Left => match state.focus {
            ConfigModalField::LogLevel => state.log_level_prev(),
            ConfigModalField::Iterations => state.iterations_decrement(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            _ => state.cursor_left(),
        },

        KeyCode::Right => match state.focus {
            ConfigModalField::LogLevel => state.log_level_next(),
            ConfigModalField::Iterations => state.iterations_increment(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            _ => state.cursor_right(),
        },

        KeyCode::Home => {
            state.cursor_home();
        }

        KeyCode::End => {
            state.cursor_end();
        }

        // Up/Down for log level dropdown, iterations field, and button navigation
        KeyCode::Up => match state.focus {
            ConfigModalField::LogLevel => state.log_level_prev(),
            ConfigModalField::Iterations => state.iterations_increment(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            ConfigModalField::SaveButton | ConfigModalField::CancelButton => state.focus_prev(),
            _ => {}
        },

        KeyCode::Down => match state.focus {
            ConfigModalField::LogLevel => state.log_level_next(),
            ConfigModalField::Iterations => state.iterations_decrement(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            ConfigModalField::SaveButton | ConfigModalField::CancelButton => state.focus_next(),
            _ => {}
        },

        _ => {}
    }
}

/// Draw the configuration modal.
pub fn draw_config_modal(f: &mut Frame, app: &App) {
    let modal_width = 70;
    let modal_height = 28;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    // Get form state (fall back to read-only view if no state)
    let state = app.config_modal_state.as_ref();

    // Build the modal content
    let log_dir_display = app
        .log_directory
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not configured)".to_string());

    let separator = "─".repeat(modal_width.saturating_sub(4) as usize);
    let field_width = 40;

    // Determine which fields are explicitly set (vs inherited from defaults)
    let explicit_fields = state.map(|s| &s.form.explicit_fields);

    // Check if a field is inherited (not explicitly set in project config)
    let is_inherited = |field: ConfigModalField| -> bool {
        if let Some(fields) = explicit_fields {
            !fields.contains(&field)
        } else {
            false
        }
    };

    // Helper to render a text input field - returns owned Spans
    let render_field =
        |value: &str, focused: bool, cursor_pos: usize, inherited: bool| -> Vec<Span<'static>> {
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

            if focused {
                let char_indices: Vec<_> = display_value.char_indices().collect();
                let (before, cursor_char, rest) = if visible_cursor < char_indices.len() {
                    let (idx, _) = char_indices[visible_cursor];
                    let before = display_value[..idx].to_string();
                    let cursor_char = display_value[idx..]
                        .chars()
                        .next()
                        .unwrap_or(' ')
                        .to_string();
                    let rest_start = idx + cursor_char.len();
                    let rest = if rest_start < display_value.len() {
                        display_value[rest_start..].to_string()
                    } else {
                        String::new()
                    };
                    (before, cursor_char, rest)
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
            } else {
                let fg = if inherited {
                    Color::DarkGray
                } else {
                    Color::White
                };
                vec![Span::styled(display_value, Style::default().fg(fg))]
            }
        };

    // Helper for label styling
    let label_style = Style::default().fg(Color::DarkGray);
    let focused_label_style = Style::default().fg(Color::Cyan);

    // Get active form values
    let form = state.map(|s| s.active_form());
    let (claude_path, log_level, iterations, keep_awake, cursor_pos, focus, has_errors): (
        &str,
        &str,
        i32,
        bool,
        usize,
        Option<ConfigModalField>,
        bool,
    ) = if let Some(s) = state {
        let f = s.active_form();
        (
            f.claude_path.as_str(),
            f.selected_log_level(),
            f.iterations,
            f.keep_awake,
            f.cursor_pos,
            Some(s.focus),
            s.has_validation_errors(),
        )
    } else {
        (
            app.config.claude.path.as_str(),
            app.config.logging.level.as_str(),
            app.config.behavior.iterations,
            app.config.behavior.keep_awake,
            0,
            None,
            false,
        )
    };

    // Helper to get validation error for a field
    let get_field_error = |field: ConfigModalField| -> Option<&str> {
        form.and_then(|f| f.validation_errors.get(&field).map(|e| e.as_str()))
    };

    // Style for validation error messages
    let error_style = Style::default().fg(Color::Yellow);

    // Build content lines
    let mut content: Vec<Line> = Vec::new();

    // Config file path display
    let config_path_display = state
        .and_then(|s| s.project_config_path.as_ref())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no project config path)".to_string());
    content.push(Line::from(vec![
        Span::styled("  Config file: ", label_style),
        Span::raw(config_path_display),
    ]));
    content.push(Line::from(vec![
        Span::styled("  Log directory: ", label_style),
        Span::raw(&log_dir_display),
    ]));
    content.push(Line::from(format!("  {separator}")));

    // Claude CLI path field
    let path_focused = focus == Some(ConfigModalField::ClaudePath);
    let path_inherited = is_inherited(ConfigModalField::ClaudePath);
    let path_label_style = if path_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut path_line = vec![Span::styled("  Claude CLI path: ", path_label_style)];
    path_line.extend(render_field(
        claude_path,
        path_focused,
        cursor_pos,
        path_inherited,
    ));
    if path_inherited && !path_focused {
        path_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(path_line));
    if let Some(error) = get_field_error(ConfigModalField::ClaudePath) {
        content.push(Line::from(Span::styled(
            format!("                     \u{26a0} {}", error),
            error_style,
        )));
    }

    // Log level dropdown
    let level_focused = focus == Some(ConfigModalField::LogLevel);
    let level_inherited = is_inherited(ConfigModalField::LogLevel);
    let level_label_style = if level_focused {
        focused_label_style
    } else {
        label_style
    };
    let level_display = if level_focused {
        format!("< {} >", log_level)
    } else {
        log_level.to_string()
    };
    let level_value_style = if level_focused {
        Style::default().fg(Color::Cyan)
    } else if level_inherited {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let mut level_line = vec![
        Span::styled("  Log level:       ", level_label_style),
        Span::styled(level_display, level_value_style),
    ];
    if level_inherited && !level_focused {
        level_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(level_line));

    // Iterations field
    let iter_focused = focus == Some(ConfigModalField::Iterations);
    let iter_inherited = is_inherited(ConfigModalField::Iterations);
    let iter_label_style = if iter_focused {
        focused_label_style
    } else {
        label_style
    };
    let iter_value = if iterations < 0 {
        "\u{221e}".to_string()
    } else {
        iterations.to_string()
    };
    let iter_display = if iter_focused {
        format!("< {} >", iter_value)
    } else {
        iter_value
    };
    let iter_value_style = if iter_focused {
        Style::default().fg(Color::Cyan)
    } else if iter_inherited {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let mut iter_line = vec![
        Span::styled("  Iterations:      ", iter_label_style),
        Span::styled(iter_display, iter_value_style),
    ];
    if iter_inherited && !iter_focused {
        iter_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(iter_line));

    // Keep awake toggle
    let keep_awake_focused = focus == Some(ConfigModalField::KeepAwake);
    let keep_awake_inherited = is_inherited(ConfigModalField::KeepAwake);
    let keep_awake_label_style = if keep_awake_focused {
        focused_label_style
    } else {
        label_style
    };
    let keep_awake_value = if keep_awake { "ON" } else { "OFF" };
    let keep_awake_display = if keep_awake_focused {
        format!("< {} >", keep_awake_value)
    } else {
        keep_awake_value.to_string()
    };
    let keep_awake_value_style = if keep_awake_focused {
        Style::default().fg(Color::Cyan)
    } else if keep_awake_inherited {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let mut keep_awake_line = vec![
        Span::styled("  Keep awake:        ", keep_awake_label_style),
        Span::styled(keep_awake_display, keep_awake_value_style),
    ];
    if keep_awake_inherited && !keep_awake_focused {
        keep_awake_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(keep_awake_line));

    content.push(Line::from(""));

    // Error message if any
    if let Some(s) = state {
        if let Some(error) = s.error() {
            content.push(Line::from(Span::styled(
                format!("  Error: {}", error),
                Style::default().fg(Color::Red),
            )));
        } else {
            content.push(Line::from(""));
        }
    } else {
        content.push(Line::from(""));
    }

    // Buttons
    let save_focused = focus == Some(ConfigModalField::SaveButton);
    let cancel_focused = focus == Some(ConfigModalField::CancelButton);

    let save_style = if has_errors {
        Style::default().fg(Color::DarkGray)
    } else if save_focused {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let cancel_style = if cancel_focused {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };

    content.push(Line::from(vec![
        Span::raw("                    "),
        Span::styled(" Save ", save_style),
        Span::raw("    "),
        Span::styled(" Cancel ", cancel_style),
    ]));

    content.push(Line::from(""));

    let title = " Configuration ";

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_modal_field_next_full_cycle() {
        let field = ConfigModalField::ClaudePath;
        let field = field.next();
        assert_eq!(field, ConfigModalField::LogLevel);
        let field = field.next();
        assert_eq!(field, ConfigModalField::Iterations);
        let field = field.next();
        assert_eq!(field, ConfigModalField::KeepAwake);
        let field = field.next();
        assert_eq!(field, ConfigModalField::SaveButton);
        let field = field.next();
        assert_eq!(field, ConfigModalField::CancelButton);
        // Wraparound
        let field = field.next();
        assert_eq!(field, ConfigModalField::ClaudePath);
    }

    #[test]
    fn test_config_modal_field_next_wraparound() {
        assert_eq!(
            ConfigModalField::CancelButton.next(),
            ConfigModalField::ClaudePath
        );
    }

    #[test]
    fn test_config_modal_field_prev_full_cycle() {
        let field = ConfigModalField::ClaudePath;
        let field = field.prev();
        assert_eq!(field, ConfigModalField::CancelButton);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::SaveButton);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::KeepAwake);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::Iterations);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::LogLevel);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::ClaudePath);
    }

    #[test]
    fn test_config_modal_field_prev_wraparound() {
        assert_eq!(
            ConfigModalField::ClaudePath.prev(),
            ConfigModalField::CancelButton
        );
    }

    // next() and prev() are inverses of each other
    #[test]
    fn test_config_modal_field_next_prev_inverse() {
        let all_fields = [
            ConfigModalField::ClaudePath,
            ConfigModalField::LogLevel,
            ConfigModalField::Iterations,
            ConfigModalField::KeepAwake,
            ConfigModalField::SaveButton,
            ConfigModalField::CancelButton,
        ];

        for field in all_fields {
            assert_eq!(field.next().prev(), field);
            assert_eq!(field.prev().next(), field);
        }
    }
}
