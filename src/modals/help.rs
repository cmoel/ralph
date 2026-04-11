//! Context-aware help modal — shows relevant keys for the current view/modal.

use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::ui::centered_rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpContext {
    Board,
    Preview,
    WorkersStream,
    Config,
    Init,
}

fn header(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {title}"),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ))
}

fn kv(key: &str, desc: &str) -> Line<'static> {
    let pad = 14usize.saturating_sub(key.len());
    Line::from(vec![
        Span::raw("    "),
        Span::styled(key.to_string(), Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("{}{}", " ".repeat(pad), desc),
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

fn system_section() -> Vec<Line<'static>> {
    vec![
        header("System"),
        kv("S", "Start/Stop loop"),
        kv("q", "Quit"),
        kv("?", "This help"),
    ]
}

fn navigate_section() -> Vec<Line<'static>> {
    vec![
        header("Navigate"),
        kv("w", "Workers stream"),
        kv("c", "Configuration"),
        kv("i", "Initialize project"),
    ]
}

pub fn content_for(ctx: HelpContext) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    match ctx {
        HelpContext::Board => {
            lines.push(header("This view"));
            lines.push(kv("h / \u{2190}", "Previous column"));
            lines.push(kv("l / \u{2192}", "Next column"));
            lines.push(kv("k / \u{2191}", "Previous card"));
            lines.push(kv("j / \u{2193}", "Next card"));
            lines.push(kv("Enter", "Focus preview pane"));
            lines.push(kv("r", "Refresh board"));
            lines.push(kv("X", "Close bead"));
            lines.push(kv("d", "Defer bead"));
            lines.push(kv("b", "Add dependency"));
            lines.push(kv("+ / =", "Raise priority"));
            lines.push(kv("-", "Lower priority"));
            lines.push(kv("H", "Toggle human label"));
            lines.push(kv("u", "Undo last action"));
            lines.push(kv("Ctrl+r", "Redo"));
            lines.push(Line::from(""));
            lines.extend(navigate_section());
            lines.push(Line::from(""));
            lines.extend(system_section());
        }
        HelpContext::Preview => {
            lines.push(header("This view"));
            lines.push(kv("j / \u{2193}", "Scroll down"));
            lines.push(kv("k / \u{2191}", "Scroll up"));
            lines.push(kv("Esc / Enter", "Return to board"));
            lines.push(Line::from(""));
            lines.extend(navigate_section());
            lines.push(Line::from(""));
            lines.extend(system_section());
        }
        HelpContext::WorkersStream => {
            lines.push(header("This view"));
            lines.push(kv("k / \u{2191}", "Previous worker"));
            lines.push(kv("j / \u{2193}", "Next worker"));
            lines.push(kv("g", "Scroll to top"));
            lines.push(kv("G", "Scroll to bottom (auto-follow)"));
            lines.push(kv("Ctrl+u", "Scroll up 10 lines"));
            lines.push(kv("Ctrl+d", "Scroll down 10 lines"));
            lines.push(kv("Esc", "Close modal"));
            lines.push(Line::from(""));
            lines.extend(system_section());
        }
        HelpContext::Config => {
            lines.push(header("This view"));
            lines.push(kv("Tab", "Next field"));
            lines.push(kv("Shift+Tab", "Previous field"));
            lines.push(kv("\u{2190} / \u{2192}", "Adjust field or move cursor"));
            lines.push(kv("\u{2191} / \u{2193}", "Field nav or cycle options"));
            lines.push(kv("Enter", "Save / Cancel / next field"));
            lines.push(kv("Home / End", "Cursor to start/end"));
            lines.push(kv("Esc", "Close without saving"));
            lines.push(Line::from(""));
            lines.extend(system_section());
        }
        HelpContext::Init => {
            lines.push(header("This view"));
            lines.push(kv("Tab / \u{2190} / \u{2192}", "Switch buttons"));
            lines.push(kv("Enter", "Confirm focused button"));
            lines.push(kv("Esc", "Close"));
            lines.push(Line::from(""));
            lines.extend(system_section());
        }
    }

    lines
}

pub fn draw_help_modal(f: &mut Frame, ctx: HelpContext) {
    let modal_width: u16 = 50;
    let content = content_for(ctx);
    let inner_width = modal_width.saturating_sub(4) as usize;
    let footer_text = "? or Esc to close";
    let footer_padding = inner_width.saturating_sub(footer_text.len());

    let mut lines = content;
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(footer_padding)),
        Span::styled(footer_text, Style::default().fg(Color::DarkGray)),
    ]));

    let modal_height = (lines.len() as u16) + 2;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    f.render_widget(Clear, modal_area);

    let modal = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Help ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_contains(ctx: HelpContext, needle: &str) -> bool {
        content_for(ctx).iter().any(|line| {
            let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            text.contains(needle)
        })
    }

    #[test]
    fn board_contains_all_keys() {
        for key in [
            "h", "l", "k", "j", "Enter", "X", "d", "b", "+", "-", "H", "u", "Ctrl+r",
        ] {
            assert!(
                content_contains(HelpContext::Board, key),
                "Board help missing key: {key}"
            );
        }
    }

    #[test]
    fn preview_contains_scroll_keys() {
        for key in ["j", "k", "Esc", "Enter"] {
            assert!(
                content_contains(HelpContext::Preview, key),
                "Preview help missing key: {key}"
            );
        }
    }

    #[test]
    fn workers_stream_contains_all_keys() {
        for key in ["k", "j", "g", "G", "Ctrl+u", "Ctrl+d", "Esc"] {
            assert!(
                content_contains(HelpContext::WorkersStream, key),
                "Workers help missing key: {key}"
            );
        }
    }

    #[test]
    fn config_contains_all_keys() {
        for key in ["Tab", "Shift+Tab", "Enter", "Home", "End", "Esc"] {
            assert!(
                content_contains(HelpContext::Config, key),
                "Config help missing key: {key}"
            );
        }
    }

    #[test]
    fn init_contains_all_keys() {
        for key in ["Tab", "Enter", "Esc"] {
            assert!(
                content_contains(HelpContext::Init, key),
                "Init help missing key: {key}"
            );
        }
    }

    #[test]
    fn all_contexts_have_system_section() {
        for ctx in [
            HelpContext::Board,
            HelpContext::Preview,
            HelpContext::WorkersStream,
            HelpContext::Config,
            HelpContext::Init,
        ] {
            assert!(content_contains(ctx, "S"), "{ctx:?} missing system key S");
            assert!(content_contains(ctx, "q"), "{ctx:?} missing system key q");
            assert!(content_contains(ctx, "?"), "{ctx:?} missing system key ?");
        }
    }

    #[test]
    fn board_and_preview_have_navigate_section() {
        for ctx in [HelpContext::Board, HelpContext::Preview] {
            assert!(content_contains(ctx, "w"), "{ctx:?} missing navigate key w");
            assert!(content_contains(ctx, "c"), "{ctx:?} missing navigate key c");
            assert!(content_contains(ctx, "i"), "{ctx:?} missing navigate key i");
        }
    }

    #[test]
    fn modal_contexts_lack_navigate_section() {
        for ctx in [
            HelpContext::WorkersStream,
            HelpContext::Config,
            HelpContext::Init,
        ] {
            let has_navigate = content_for(ctx).iter().any(|line| {
                let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                text.contains("Navigate")
            });
            assert!(!has_navigate, "{ctx:?} should not have Navigate section");
        }
    }

    #[test]
    fn no_stale_keys() {
        let board = content_for(HelpContext::Board);
        let text: String = board
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            !text.contains("Toggle tool panel"),
            "stale key: t / toggle tool panel"
        );
        assert!(!text.contains("Allow tool"), "stale key: A / allow tool");
        assert!(
            !text.contains("Switch panel focus"),
            "stale key: Tab / switch panel focus"
        );
        assert!(!text.contains("Full page"), "stale key: Ctrl+b/f full page");
    }
}
