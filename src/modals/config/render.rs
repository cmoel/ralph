use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use tracing::debug;

use crate::app::App;
use crate::config::save_partial_config;
use crate::startup::get_file_mtime;
use crate::ui::centered_rect;

use super::ConfigModalField;

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
