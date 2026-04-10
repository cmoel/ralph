use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::state::{short_id, BeadDetailState, BoardFocus, KanbanBoardState};

fn build_detail_content(detail: &BeadDetailState) -> Vec<Line<'_>> {
    let mut content: Vec<Line> = Vec::new();

    if detail.is_loading {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "  Loading...",
            Style::default().fg(Color::DarkGray),
        )));
        return content;
    }
    if let Some(error) = &detail.error {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            format!("  Error: {error}"),
            Style::default().fg(Color::Red),
        )));
        return content;
    }

    // Title
    content.push(Line::from(vec![Span::styled(
        &detail.title,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));
    content.push(Line::from(""));

    // Metadata line: ID · status · priority · type
    let mut meta: Vec<Span> = vec![Span::styled(
        short_id(&detail.id),
        Style::default().fg(Color::Cyan),
    )];
    if !detail.status.is_empty() {
        meta.push(Span::styled(
            " \u{b7} ",
            Style::default().fg(Color::DarkGray),
        ));
        let status_color = match detail.status.as_str() {
            "open" => Color::Green,
            "in_progress" => Color::Yellow,
            "closed" => Color::DarkGray,
            "blocked" => Color::Red,
            "deferred" => Color::DarkGray,
            _ => Color::White,
        };
        meta.push(Span::styled(
            &detail.status,
            Style::default().fg(status_color),
        ));
    }
    if !detail.priority.is_empty() {
        meta.push(Span::styled(
            " \u{b7} ",
            Style::default().fg(Color::DarkGray),
        ));
        meta.push(Span::styled(
            &detail.priority,
            Style::default().fg(Color::Magenta),
        ));
    }
    if !detail.issue_type.is_empty() {
        meta.push(Span::styled(
            " \u{b7} ",
            Style::default().fg(Color::DarkGray),
        ));
        meta.push(Span::styled(
            &detail.issue_type,
            Style::default().fg(Color::Gray),
        ));
    }
    content.push(Line::from(meta));

    // Labels
    if !detail.labels.is_empty() {
        let mut label_spans: Vec<Span> = vec![Span::styled(
            "Labels: ",
            Style::default().fg(Color::DarkGray),
        )];
        for (i, label) in detail.labels.iter().enumerate() {
            if i > 0 {
                label_spans.push(Span::styled(", ", Style::default().fg(Color::DarkGray)));
            }
            label_spans.push(Span::styled(label, Style::default().fg(Color::Yellow)));
        }
        content.push(Line::from(label_spans));
    }

    // Dependencies
    if !detail.dependencies.is_empty() {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "Dependencies",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for dep in &detail.dependencies {
            let status_icon = match dep.status.as_str() {
                "closed" => "\u{2713}",
                "in_progress" => "\u{25d0}",
                "blocked" => "\u{25cf}",
                _ => "\u{25cb}",
            };
            let arrow = if dep.dep_type == "blocks" {
                "\u{2190}" // ←  this issue is blocked by dep
            } else {
                "\u{2192}" // →  this issue blocks dep
            };
            let status_color = match dep.status.as_str() {
                "closed" => Color::DarkGray,
                "blocked" => Color::Red,
                _ => Color::White,
            };
            content.push(Line::from(vec![
                Span::styled(
                    format!("  {arrow} {status_icon} "),
                    Style::default().fg(status_color),
                ),
                Span::styled(&dep.id, Style::default().fg(Color::Cyan)),
                Span::styled(" ", Style::default()),
                Span::styled(&dep.title, Style::default().fg(status_color)),
            ]));
        }
    }

    // Description
    if !detail.description.is_empty() {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "Description",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for line in detail.description.lines() {
            content.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    // Design
    if !detail.design.is_empty() {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "Design",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for line in detail.design.lines() {
            content.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    // Notes
    if !detail.notes.is_empty() {
        content.push(Line::from(""));
        content.push(Line::from(Span::styled(
            "Notes",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        for line in detail.notes.lines() {
            content.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    content
}

pub(super) fn draw_preview_pane(f: &mut Frame, state: &KanbanBoardState, area: Rect) {
    let has_focus = state.focus == BoardFocus::Preview;
    let border_style = if has_focus {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    match &state.preview_detail {
        Some(detail) => {
            let content = build_detail_content(detail);
            let title = format!(" {} ", short_id(&detail.id));
            let pane = Paragraph::new(content)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .style(border_style),
                )
                .wrap(Wrap { trim: false })
                .scroll((detail.scroll_offset, 0));
            f.render_widget(pane, area);
        }
        None => {
            let content = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  Select a bead to see details",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let pane = Paragraph::new(content).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Preview ")
                    .title_alignment(Alignment::Center)
                    .style(border_style),
            );
            f.render_widget(pane, area);
        }
    }
}
