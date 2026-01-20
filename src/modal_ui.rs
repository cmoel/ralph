//! Modal UI rendering functions.

use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::modals::ConfigModalField;
use crate::ui::centered_rect;

/// Draw the configuration modal.
pub fn draw_config_modal(f: &mut Frame, app: &App) {
    let modal_width = 70;
    // Increased height to accommodate validation error lines
    let modal_height = 24;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    // Get form state (fall back to read-only view if no state)
    let state = app.config_modal_state.as_ref();

    // Build the modal content
    let config_path_display = app.config_path.display().to_string();
    let log_dir_display = app
        .log_directory
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not configured)".to_string());

    let separator = "─".repeat(modal_width.saturating_sub(4) as usize);
    let field_width = 40;

    // Helper to render a text input field - returns owned Spans
    let render_field = |value: &str, focused: bool, cursor_pos: usize| -> Vec<Span<'static>> {
        let display_value: String = if value.len() > field_width {
            // Show the portion around the cursor
            let start = cursor_pos.saturating_sub(field_width / 2);
            let end = (start + field_width).min(value.len());
            let start = end.saturating_sub(field_width);
            value[start..end].to_string()
        } else {
            value.to_string()
        };

        // Calculate cursor position within displayed text
        let visible_cursor = if value.len() > field_width {
            let start = cursor_pos.saturating_sub(field_width / 2);
            let end = (start + field_width).min(value.len());
            let start = end.saturating_sub(field_width);
            cursor_pos - start
        } else {
            cursor_pos
        };

        if focused {
            // Split at cursor for visual indication
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
            vec![Span::styled(
                display_value,
                Style::default().fg(Color::White),
            )]
        }
    };

    // Helper for label styling
    let label_style = Style::default().fg(Color::DarkGray);
    let focused_label_style = Style::default().fg(Color::Cyan);

    // Get values from state or config
    let (claude_path, prompt_file, specs_dir, log_level, iterations, cursor_pos, focus, has_errors) =
        if let Some(s) = state {
            (
                s.claude_path.as_str(),
                s.prompt_file.as_str(),
                s.specs_dir.as_str(),
                s.selected_log_level(),
                s.iterations,
                s.cursor_pos,
                Some(s.focus),
                s.has_validation_errors(),
            )
        } else {
            (
                app.config.claude.path.as_str(),
                app.config.paths.prompt.as_str(),
                app.config.paths.specs.as_str(),
                app.config.logging.level.as_str(),
                app.config.behavior.iterations,
                0,
                None,
                false,
            )
        };

    // Helper to get validation error for a field
    let get_field_error = |field: ConfigModalField| -> Option<&str> {
        state.and_then(|s| s.validation_errors.get(&field).map(|e| e.as_str()))
    };

    // Style for validation error messages
    let error_style = Style::default().fg(Color::Yellow);

    // Build content lines
    let mut content = vec![
        Line::from(vec![
            Span::styled("  Config file: ", label_style),
            Span::raw(&config_path_display),
        ]),
        Line::from(vec![
            Span::styled("  Log directory: ", label_style),
            Span::raw(&log_dir_display),
        ]),
        Line::from(format!("  {separator}")),
    ];

    // Claude CLI path field
    let path_focused = focus == Some(ConfigModalField::ClaudePath);
    let path_label_style = if path_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut path_line = vec![Span::styled("  Claude CLI path: ", path_label_style)];
    path_line.extend(render_field(claude_path, path_focused, cursor_pos));
    content.push(Line::from(path_line));
    // Validation error for Claude CLI path
    if let Some(error) = get_field_error(ConfigModalField::ClaudePath) {
        content.push(Line::from(Span::styled(
            format!("                     \u{26a0} {}", error),
            error_style,
        )));
    }

    // Prompt file field
    let prompt_focused = focus == Some(ConfigModalField::PromptFile);
    let prompt_label_style = if prompt_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut prompt_line = vec![Span::styled("  Prompt file:     ", prompt_label_style)];
    prompt_line.extend(render_field(prompt_file, prompt_focused, cursor_pos));
    content.push(Line::from(prompt_line));
    // Validation error for Prompt file
    if let Some(error) = get_field_error(ConfigModalField::PromptFile) {
        content.push(Line::from(Span::styled(
            format!("                     \u{26a0} {}", error),
            error_style,
        )));
    }

    // Specs directory field
    let specs_focused = focus == Some(ConfigModalField::SpecsDirectory);
    let specs_label_style = if specs_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut specs_line = vec![Span::styled("  Specs directory: ", specs_label_style)];
    specs_line.extend(render_field(specs_dir, specs_focused, cursor_pos));
    content.push(Line::from(specs_line));
    // Validation error for Specs directory
    if let Some(error) = get_field_error(ConfigModalField::SpecsDirectory) {
        content.push(Line::from(Span::styled(
            format!("                     \u{26a0} {}", error),
            error_style,
        )));
    }

    // Log level dropdown
    let level_focused = focus == Some(ConfigModalField::LogLevel);
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
    } else {
        Style::default().fg(Color::White)
    };
    content.push(Line::from(vec![
        Span::styled("  Log level:       ", level_label_style),
        Span::styled(level_display, level_value_style),
    ]));

    // Iterations field
    let iter_focused = focus == Some(ConfigModalField::Iterations);
    let iter_label_style = if iter_focused {
        focused_label_style
    } else {
        label_style
    };
    // Display -1 as infinity symbol
    let iter_value = if iterations < 0 {
        "∞".to_string()
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
    } else {
        Style::default().fg(Color::White)
    };
    content.push(Line::from(vec![
        Span::styled("  Iterations:      ", iter_label_style),
        Span::styled(iter_display, iter_value_style),
    ]));

    content.push(Line::from(""));

    // Error message if any
    if let Some(s) = state {
        if let Some(error) = &s.error {
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

    // Save button is dimmed when there are validation errors
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

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Configuration ")
            .title_alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
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

    // Handle error case
    if let Some(error) = &state.error {
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

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Specs ")
            .title_alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
