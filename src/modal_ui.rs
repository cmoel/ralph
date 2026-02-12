//! Modal UI rendering functions.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::modals::{ConfigModalField, ConfigTab, InitFileStatus, InitModalField};
use crate::ui::centered_rect;

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

    // Determine if we're on the project tab and which fields are inherited
    let on_project_tab = state
        .map(|s| s.active_tab() == ConfigTab::Project)
        .unwrap_or(false);
    let explicit_fields = if on_project_tab {
        state.and_then(|s| s.project_form.as_ref().map(|f| &f.explicit_fields))
    } else {
        None
    };

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
    let (
        claude_path,
        prompt_file,
        specs_dir,
        log_level,
        iterations,
        auto_expand_tasks,
        keep_awake,
        cursor_pos,
        focus,
        has_errors,
    ): (
        &str,
        &str,
        &str,
        &str,
        i32,
        bool,
        bool,
        usize,
        Option<ConfigModalField>,
        bool,
    ) = if let Some(s) = state {
        let f = s.active_form();
        (
            f.claude_path.as_str(),
            f.prompt_file.as_str(),
            f.specs_dir.as_str(),
            f.selected_log_level(),
            f.iterations,
            f.auto_expand_tasks,
            f.keep_awake,
            f.cursor_pos,
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
            app.config.behavior.auto_expand_tasks_panel,
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

    // Tab bar (only when .ralph exists)
    if let Some(s) = state
        && s.has_tabs()
    {
        let active = s.active_tab();
        let project_style = if active == ConfigTab::Project {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let global_style = if active == ConfigTab::Global {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        content.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(" Project ", project_style),
            Span::raw(" "),
            Span::styled(" Global ", global_style),
            Span::styled("                  [ / ] switch tabs", label_style),
        ]));
        content.push(Line::from(format!("  {separator}")));
    }

    // Config file path display
    let config_path_display = if on_project_tab {
        state
            .and_then(|s| s.project_config_path.as_ref())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".ralph".to_string())
    } else {
        app.config_path.display().to_string()
    };
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

    // Prompt file field
    let prompt_focused = focus == Some(ConfigModalField::PromptFile);
    let prompt_inherited = is_inherited(ConfigModalField::PromptFile);
    let prompt_label_style = if prompt_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut prompt_line = vec![Span::styled("  Prompt file:     ", prompt_label_style)];
    prompt_line.extend(render_field(
        prompt_file,
        prompt_focused,
        cursor_pos,
        prompt_inherited,
    ));
    if prompt_inherited && !prompt_focused {
        prompt_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(prompt_line));
    if let Some(error) = get_field_error(ConfigModalField::PromptFile) {
        content.push(Line::from(Span::styled(
            format!("                     \u{26a0} {}", error),
            error_style,
        )));
    }

    // Specs directory field
    let specs_focused = focus == Some(ConfigModalField::SpecsDirectory);
    let specs_inherited = is_inherited(ConfigModalField::SpecsDirectory);
    let specs_label_style = if specs_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut specs_line = vec![Span::styled("  Specs directory: ", specs_label_style)];
    specs_line.extend(render_field(
        specs_dir,
        specs_focused,
        cursor_pos,
        specs_inherited,
    ));
    if specs_inherited && !specs_focused {
        specs_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(specs_line));
    if let Some(error) = get_field_error(ConfigModalField::SpecsDirectory) {
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

    // Auto-expand tasks toggle
    let auto_expand_focused = focus == Some(ConfigModalField::AutoExpandTasks);
    let auto_expand_inherited = is_inherited(ConfigModalField::AutoExpandTasks);
    let auto_expand_label_style = if auto_expand_focused {
        focused_label_style
    } else {
        label_style
    };
    let auto_expand_value = if auto_expand_tasks { "ON" } else { "OFF" };
    let auto_expand_display = if auto_expand_focused {
        format!("< {} >", auto_expand_value)
    } else {
        auto_expand_value.to_string()
    };
    let auto_expand_value_style = if auto_expand_focused {
        Style::default().fg(Color::Cyan)
    } else if auto_expand_inherited {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    let mut auto_expand_line = vec![
        Span::styled("  Auto-expand tasks: ", auto_expand_label_style),
        Span::styled(auto_expand_display, auto_expand_value_style),
    ];
    if auto_expand_inherited && !auto_expand_focused {
        auto_expand_line.push(Span::styled(" (inherited)", label_style));
    }
    content.push(Line::from(auto_expand_line));

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

    // Title shows which config we're editing
    let title = if on_project_tab {
        " Configuration (Project) "
    } else if state.map(|s| s.has_tabs()).unwrap_or(false) {
        " Configuration (Global) "
    } else {
        " Configuration "
    };

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
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

/// Draw the project init modal.
pub fn draw_init_modal(f: &mut Frame, app: &App) {
    let modal_width: u16 = 60;
    let modal_height: u16 = 18;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    let Some(state) = &app.init_modal_state else {
        return;
    };

    let label_style = Style::default().fg(Color::DarkGray);
    let warning_style = Style::default().fg(Color::Yellow);
    let has_conflicts = state.has_conflicts();

    let mut content: Vec<Line> = Vec::new();

    // Title/description
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Initialize project scaffolding:",
        label_style,
    )));
    content.push(Line::from(""));

    // File list with status indicators
    for file in &state.files {
        let (icon, icon_style) = match file.status {
            InitFileStatus::WillCreate => ("✓", Style::default().fg(Color::Green)),
            InitFileStatus::Conflict => ("✗", Style::default().fg(Color::Red)),
        };

        content.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(&file.display_path, Style::default().fg(Color::White)),
        ]));
    }

    content.push(Line::from(""));

    // Show conflict warning or error/success messages
    if has_conflicts {
        // Warning panel for conflicts
        content.push(Line::from(Span::styled(
            "  ⚠ Cannot initialize — files already exist:",
            warning_style,
        )));
        for file in state.conflicting_files() {
            content.push(Line::from(Span::styled(
                format!("    ✗ {}", file.display_path),
                warning_style,
            )));
        }
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Rename them or update config (press `c`).",
            warning_style,
        )));
    } else if let Some(error) = &state.error {
        content.push(Line::from(Span::styled(
            format!("  Error: {}", error),
            Style::default().fg(Color::Red),
        )));
    } else if let Some(success) = &state.success {
        content.push(Line::from(Span::styled(
            format!("  {}", success),
            Style::default().fg(Color::Green),
        )));
    } else {
        content.push(Line::from(""));
    }

    content.push(Line::from(""));

    // Buttons - only show Initialize button when no conflicts
    let cancel_focused = state.focus == InitModalField::CancelButton;

    if has_conflicts {
        // Only Cancel button when conflicts exist
        let cancel_style = if cancel_focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::White)
        };

        content.push(Line::from(vec![
            Span::raw("                      "),
            Span::styled(" Cancel ", cancel_style),
        ]));
    } else {
        // Both buttons when no conflicts
        let init_focused = state.focus == InitModalField::InitializeButton;

        let init_style = if init_focused {
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
            Span::raw("              "),
            Span::styled(" Initialize ", init_style),
            Span::raw("    "),
            Span::styled(" Cancel ", cancel_style),
        ]));
    }

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Initialize Project ")
            .title_alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}

/// Draw the help modal.
pub fn draw_help_modal(f: &mut Frame, _app: &App) {
    let modal_width: u16 = 50;
    let modal_height: u16 = 20;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    let key_style = Style::default().fg(Color::Cyan);
    let desc_style = Style::default().fg(Color::DarkGray);
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);

    let inner_width = modal_width.saturating_sub(4) as usize;

    // Footer - right aligned "? or Esc to close"
    let footer_text = "? or Esc to close";
    let footer_padding = inner_width.saturating_sub(footer_text.len());

    let content: Vec<Line> = vec![
        // Control section
        Line::from(Span::styled("  Control", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("s", key_style),
            Span::styled("  Start/Stop claude", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("q", key_style),
            Span::styled("  Quit", desc_style),
        ]),
        Line::from(""),
        // Panels section
        Line::from(Span::styled("  Panels", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("c", key_style),
            Span::styled("  Configuration", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("l", key_style),
            Span::styled("  Specs list", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("i", key_style),
            Span::styled("  Initialize project", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("t", key_style),
            Span::styled("  Toggle tasks panel", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Tab", key_style),
            Span::styled("  Switch panel focus", desc_style),
        ]),
        Line::from(""),
        // Scroll section
        Line::from(Span::styled("  Scroll", header_style)),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("j/k", key_style),
            Span::styled("  Scroll down/up", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("↑/↓", key_style),
            Span::styled("  Scroll down/up", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Ctrl+u/d", key_style),
            Span::styled("  Half page up/down", desc_style),
        ]),
        Line::from(vec![
            Span::raw("    "),
            Span::styled("Ctrl+b/f", key_style),
            Span::styled("  Full page up/down", desc_style),
        ]),
        // Footer
        Line::from(""),
        Line::from(vec![
            Span::raw(" ".repeat(footer_padding)),
            Span::styled(footer_text, desc_style),
        ]),
    ];

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}
