use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::overlays::{CloseConfirmState, DeferState, DepDirectionState};
use super::preview::draw_preview_pane;
use super::state::short_id;
use crate::app::App;
use crate::ui::centered_rect;

fn truncate_to_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0;
    let mut result = String::new();
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + w > max_width {
            break;
        }
        result.push(ch);
        width += w;
    }
    while width < max_width {
        result.push(' ');
        width += 1;
    }
    result
}

/// Draw the kanban board in the given content area.
pub fn draw_kanban_board(f: &mut Frame, app: &App, board_area: Rect) {
    let state = &app.kanban_board_state;

    // Split content area: top for board columns, bottom for preview pane.
    // If terminal is very short (< 12 lines), hide preview and show board only.
    let (columns_area, preview_area) = if board_area.height < 12 {
        (board_area, None)
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(33), Constraint::Percentage(67)])
            .split(board_area);
        (chunks[0], Some(chunks[1]))
    };

    // Draw the preview pane in the bottom area
    if let Some(area) = preview_area {
        draw_preview_pane(f, state, area);
    }

    let inner_height = columns_area.height.saturating_sub(2) as usize;
    let inner_width = columns_area.width.saturating_sub(2) as usize;

    let mut content: Vec<Line> = Vec::new();

    if state.is_loading {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
    } else if let Some(error) = &state.error {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!("  Error: {error}"),
            Style::default().fg(Color::Red),
        )));
    } else {
        let col_count = state.col_count();
        let separators = col_count.saturating_sub(1);
        let usable = inner_width.saturating_sub(separators);

        // Accordion layout: selected column gets ~45% of width, others split the rest
        let (expanded_width, collapsed_width) = if col_count <= 1 {
            (usable, 0)
        } else {
            let exp = usable * 45 / 100;
            let coll = usable.saturating_sub(exp) / (col_count - 1);
            (exp, coll)
        };
        let leftover = if col_count <= 1 {
            0
        } else {
            usable.saturating_sub(expanded_width + collapsed_width * (col_count - 1))
        };
        let col_widths: Vec<usize> = (0..col_count)
            .map(|i| {
                if i == state.selected_column {
                    expanded_width + leftover
                } else {
                    collapsed_width
                }
            })
            .collect();

        // Count real cards (not error cards) per column for display
        let card_counts: Vec<usize> = state
            .columns
            .iter()
            .map(|col| col.iter().filter(|c| !c.is_error).count())
            .collect();

        // Header row
        let mut header_spans: Vec<Span> = Vec::new();
        for (i, col_def) in state.column_defs.iter().enumerate() {
            let is_selected = i == state.selected_column;
            let w = col_widths[i];
            let label = format!("{} ({})", col_def.name, card_counts[i]);
            let padded = format!("{:^width$}", label, width = w);

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            };
            header_spans.push(Span::styled(padded, style));
            if i < col_count - 1 {
                header_spans.push(Span::styled(
                    "\u{2502}",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        content.push(Line::from(header_spans));

        // Separator line
        let mut sep_spans: Vec<Span> = Vec::new();
        for (i, &w) in col_widths.iter().enumerate() {
            sep_spans.push(Span::styled(
                "\u{2500}".repeat(w),
                Style::default().fg(Color::DarkGray),
            ));
            if i < col_count - 1 {
                sep_spans.push(Span::styled(
                    "\u{253c}",
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
        content.push(Line::from(sep_spans));

        // Warning banner for manual-blocked beads
        let manual_blocked_count = state.manual_blocked_ids.len();
        let has_banner = manual_blocked_count > 0;
        if has_banner {
            let noun = if manual_blocked_count == 1 {
                "bead has"
            } else {
                "beads have"
            };
            let banner_text = format!(
                " {manual_blocked_count} {noun} 'blocked' status without dependencies \u{2014} Ralph won't pick these up"
            );
            let banner_padded = format!("{:<width$}", banner_text, width = inner_width);
            content.push(Line::from(Span::styled(
                banner_padded,
                Style::default().fg(Color::Yellow),
            )));
        }

        // Card rows
        let banner_rows = if has_banner { 1 } else { 0 };
        let max_rows = inner_height.saturating_sub(3 + banner_rows); // header + separator + footer + banner
        let max_cards = state.columns.iter().map(|c| c.len()).max().unwrap_or(0);
        let visible_rows = max_cards.min(max_rows);

        // Compute highlighted dependency neighbors for the selected card
        let highlighted_ids: HashSet<&str> = state
            .selected_card()
            .and_then(|card| state.dep_neighbors.get(&card.id))
            .map(|neighbors| neighbors.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        for row in 0..visible_rows {
            let mut row_spans: Vec<Span> = Vec::new();
            for (col_idx, column) in state.columns.iter().enumerate() {
                let is_active_col = col_idx == state.selected_column;
                let is_selected_row = is_active_col && row == state.selected_row[col_idx];
                let w = col_widths[col_idx];

                if row < column.len() {
                    let card = &column[row];

                    if card.is_error {
                        // Error card — render with error style, not selectable
                        let cell_text = format!(" {} {}", card.emoji, card.title);
                        let display_width = UnicodeWidthStr::width(cell_text.as_str());
                        let padded = if display_width >= w {
                            truncate_to_width(&cell_text, w)
                        } else {
                            let padding = w - display_width;
                            format!("{}{}", cell_text, " ".repeat(padding))
                        };
                        let style = Style::default().fg(Color::Red);
                        row_spans.push(Span::styled(padded, style));
                    } else if !is_active_col {
                        // Collapsed column: short ID + truncated title
                        let sid = short_id(&card.id);
                        let cell_text = format!(" {} {}", sid, card.title);
                        let display_width = UnicodeWidthStr::width(cell_text.as_str());
                        let padded = if display_width >= w {
                            truncate_to_width(&cell_text, w)
                        } else {
                            let padding = w - display_width;
                            format!("{}{}", cell_text, " ".repeat(padding))
                        };

                        let is_dep_neighbor = highlighted_ids.contains(card.id.as_str());
                        let is_manual_blocked = state.manual_blocked_ids.contains(&card.id);
                        let style = if is_manual_blocked {
                            Style::default().fg(Color::Yellow)
                        } else if is_dep_neighbor {
                            Style::default().fg(Color::Gray).bg(Color::Rgb(25, 35, 60))
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        row_spans.push(Span::styled(padded, style));
                    } else {
                        // Expanded column: source emoji, id, title, blockers
                        let icon_prefix = &card.emoji;
                        let icon_width = UnicodeWidthStr::width(icon_prefix.as_str());

                        let sid = short_id(&card.id);
                        let id_width = sid.len() + 1; // "id "
                        let blocker_suffix = if card.blockers.is_empty() {
                            String::new()
                        } else {
                            format!(" \u{2190} {}", card.blockers.join(", "))
                        };
                        // icon + space + id + space + title + blocker_suffix
                        let fixed_width = icon_width + 1 + id_width + blocker_suffix.width();
                        let title_max = w.saturating_sub(fixed_width);
                        let title_display_width = UnicodeWidthStr::width(card.title.as_str());
                        let title = if title_display_width > title_max {
                            let truncated =
                                truncate_to_width(&card.title, title_max.saturating_sub(2));
                            format!("{}..", truncated.trim_end())
                        } else {
                            card.title.clone()
                        };
                        let cell_text =
                            format!("{} {} {}{}", icon_prefix, sid, title, blocker_suffix);

                        let display_width = UnicodeWidthStr::width(cell_text.as_str());
                        let padded = if display_width >= w {
                            truncate_to_width(&cell_text, w)
                        } else {
                            let padding = w - display_width;
                            format!("{}{}", cell_text, " ".repeat(padding))
                        };

                        let is_dep_neighbor = highlighted_ids.contains(card.id.as_str());
                        let is_manual_blocked = state.manual_blocked_ids.contains(&card.id);
                        let base_style = if is_selected_row {
                            Style::default().fg(Color::Black).bg(Color::White)
                        } else if is_manual_blocked {
                            Style::default().fg(Color::Yellow)
                        } else if is_dep_neighbor {
                            Style::default().fg(Color::White).bg(Color::Rgb(25, 35, 60))
                        } else {
                            Style::default().fg(Color::White)
                        };
                        // Epics get bold styling but keep the source emoji
                        let style = if card.is_epic {
                            base_style.add_modifier(Modifier::BOLD)
                        } else {
                            base_style
                        };

                        row_spans.push(Span::styled(padded, style));
                    }
                } else {
                    row_spans.push(Span::raw(" ".repeat(w)));
                }

                if col_idx < col_count - 1 {
                    row_spans.push(Span::styled(
                        "\u{2502}",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            content.push(Line::from(row_spans));
        }

        // Fill remaining height with empty rows (leaving room for footer)
        for _ in visible_rows..max_rows {
            let mut row_spans: Vec<Span> = Vec::new();
            for (col_idx, &w) in col_widths.iter().enumerate() {
                row_spans.push(Span::raw(" ".repeat(w)));
                if col_idx < col_count - 1 {
                    row_spans.push(Span::styled(
                        "\u{2502}",
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
            content.push(Line::from(row_spans));
        }

        // Footer — show status message if fresh, otherwise key hints
        let status_msg_timeout = std::time::Duration::from_secs(3);
        let active_status = state
            .status_message
            .as_ref()
            .filter(|(_, ts)| ts.elapsed() < status_msg_timeout)
            .map(|(msg, _)| msg.clone());

        if let Some(msg) = active_status {
            let padded = format!(" {msg:<width$}", width = inner_width.saturating_sub(1));
            content.push(Line::from(Span::styled(
                padded,
                Style::default().fg(Color::Yellow),
            )));
        } else {
            let sep = Style::default().fg(Color::DarkGray);
            let key = Style::default().fg(Color::Cyan);
            let desc = Style::default().fg(Color::DarkGray);
            content.push(Line::from(vec![
                Span::styled(" hjkl", key),
                Span::styled(" navigate", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("Enter", key),
                Span::styled(" preview", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("X", key),
                Span::styled(" close", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("b", key),
                Span::styled(" dep", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("d", key),
                Span::styled(" defer", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("+/-", key),
                Span::styled(" pri", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("H", key),
                Span::styled(" human", desc),
                Span::styled(" \u{b7} ", sep),
                Span::styled("?", key),
                Span::styled(" help", desc),
            ]));
        }
    }

    let stats_title = format!(
        " {} open \u{b7} {} closed ",
        state.open_count, state.closed_count
    );

    let board = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Work Board ")
            .title_alignment(Alignment::Center)
            .title_top(Line::from(stats_title).right_aligned())
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(board, columns_area);

    // Close confirmation overlay
    if let Some(confirm) = &state.close_confirm {
        draw_close_confirm(f, confirm);
    }

    // Defer input overlay
    if let Some(defer) = &state.defer_input {
        draw_defer_input(f, defer);
    }

    // Dependency direction picker overlay
    if let Some(dep_dir) = &state.dep_direction {
        draw_dep_direction(f, dep_dir);
    }
}

fn draw_close_confirm(f: &mut Frame, confirm: &CloseConfirmState) {
    let area = f.area();
    let overlay = centered_rect(50, 5, area);
    f.render_widget(Clear, overlay);

    let prompt = format!("Close {}? Reason (optional):", confirm.bead_id);

    // Build the text input line with cursor
    let before = &confirm.reason[..confirm.cursor_pos];
    let at_end = confirm.cursor_pos >= confirm.reason.len();
    let cursor_char = if at_end {
        ' '
    } else {
        confirm.reason[confirm.cursor_pos..].chars().next().unwrap()
    };
    let after = if at_end {
        ""
    } else {
        &confirm.reason[confirm.cursor_pos + cursor_char.len_utf8()..]
    };

    let input_line = Line::from(vec![
        Span::styled(before, Style::default().fg(Color::White)),
        Span::styled(
            cursor_char.to_string(),
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(after, Style::default().fg(Color::White)),
    ]);

    let content = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        input_line,
        Line::from(Span::styled(
            "Enter to confirm \u{b7} Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Close Bead ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Red)),
    );

    f.render_widget(widget, overlay);
}

fn draw_defer_input(f: &mut Frame, defer: &DeferState) {
    let area = f.area();
    let overlay = centered_rect(50, 5, area);
    f.render_widget(Clear, overlay);

    let prompt = format!("Defer {}. Until (optional):", defer.bead_id);

    // Build the text input line with cursor
    let before = &defer.until[..defer.cursor_pos];
    let at_end = defer.cursor_pos >= defer.until.len();
    let cursor_char = if at_end {
        ' '
    } else {
        defer.until[defer.cursor_pos..].chars().next().unwrap()
    };
    let after = if at_end {
        ""
    } else {
        &defer.until[defer.cursor_pos + cursor_char.len_utf8()..]
    };

    let input_line = Line::from(vec![
        Span::styled(before, Style::default().fg(Color::White)),
        Span::styled(
            cursor_char.to_string(),
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(after, Style::default().fg(Color::White)),
    ]);

    let content = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        input_line,
        Line::from(Span::styled(
            "Enter to defer \u{b7} Esc to cancel",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Defer Bead ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(widget, overlay);
}

fn draw_dep_direction(f: &mut Frame, dep_dir: &DepDirectionState) {
    let area = f.area();
    let overlay = centered_rect(50, 6, area);
    f.render_widget(Clear, overlay);

    let prompt = format!("Add dependency for {}", dep_dir.bead_id);

    let content = vec![
        Line::from(Span::styled(prompt, Style::default().fg(Color::Yellow))),
        Line::from(""),
        Line::from(vec![
            Span::styled("1", Style::default().fg(Color::Cyan)),
            Span::styled("  This is blocked by...", Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled("2", Style::default().fg(Color::Cyan)),
            Span::styled("  This blocks...", Style::default().fg(Color::White)),
        ]),
    ];

    let widget = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Add Dependency ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(widget, overlay);
}
