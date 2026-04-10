//! Init modal — project scaffolding initialization.

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use similar::TextDiff;
use tracing::debug;

use crate::app::App;
use crate::config::Config;
use crate::templates;
use crate::ui::centered_rect;

/// Status of a file for the init modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitFileStatus {
    /// File will be created (doesn't exist).
    WillCreate,
    /// File already exists and matches template (will be skipped).
    Exists,
    /// File exists and differs from template — will be overwritten.
    WillRegenerate,
}

/// A file entry for the init modal.
#[derive(Debug, Clone)]
pub struct InitFileEntry {
    /// Display path (relative for readability).
    pub display_path: String,
    /// Full path for file operations.
    pub full_path: PathBuf,
    /// Current status.
    pub status: InitFileStatus,
    /// Unified diff lines (only for WillRegenerate).
    pub diff_lines: Vec<String>,
}

/// Which field is focused in the init modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitModalField {
    InitializeButton,
    CancelButton,
}

impl InitModalField {
    pub fn next(self) -> Self {
        match self {
            Self::InitializeButton => Self::CancelButton,
            Self::CancelButton => Self::InitializeButton,
        }
    }

    pub fn prev(self) -> Self {
        self.next() // Only two options, so prev == next
    }
}

/// State for the init modal.
#[derive(Debug, Clone)]
pub struct InitModalState {
    /// Files to be initialized with their status.
    pub files: Vec<InitFileEntry>,
    /// Current focused field.
    pub focus: InitModalField,
    /// Error message to display.
    pub error: Option<String>,
    /// Success message to display (briefly before closing).
    pub success: Option<String>,
}

/// Return the compiled-in template for a skill file path.
fn template_for_path(display_path: &str) -> Option<&'static str> {
    if display_path.contains("brain-dump") {
        Some(templates::BRAIN_DUMP_SKILL_MD)
    } else if display_path.contains("shape") {
        Some(templates::SHAPE_SKILL_MD)
    } else if display_path.contains("capture") {
        Some(templates::CAPTURE_SKILL_MD)
    } else {
        None
    }
}

/// Compute unified diff lines between existing content and template.
fn compute_diff(existing: &str, template: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(existing, template);
    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header("current", "template")
        .to_string();
    unified.lines().map(|l| l.to_string()).collect()
}

/// The list of skill files managed by init.
fn skill_files() -> Vec<(String, PathBuf)> {
    vec![
        (
            ".claude/skills/brain-dump/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/brain-dump/SKILL.md"),
        ),
        (
            ".claude/skills/shape/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/shape/SKILL.md"),
        ),
        (
            ".claude/skills/capture/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/capture/SKILL.md"),
        ),
    ]
}

impl InitModalState {
    /// Create a new init modal state by checking file existence and diffs.
    ///
    /// Files that exist and match the template are skipped. Files that exist
    /// but differ show a unified diff and will be overwritten.
    pub fn new(_config: &Config) -> Self {
        let files = skill_files()
            .into_iter()
            .map(|(display, full)| {
                if full.exists() {
                    let existing = std::fs::read_to_string(&full).unwrap_or_default();
                    let template = template_for_path(&display).unwrap_or("");
                    if existing == template {
                        InitFileEntry {
                            display_path: display,
                            full_path: full,
                            status: InitFileStatus::Exists,
                            diff_lines: vec![],
                        }
                    } else {
                        InitFileEntry {
                            diff_lines: compute_diff(&existing, template),
                            display_path: display,
                            full_path: full,
                            status: InitFileStatus::WillRegenerate,
                        }
                    }
                } else {
                    InitFileEntry {
                        display_path: display,
                        full_path: full,
                        status: InitFileStatus::WillCreate,
                        diff_lines: vec![],
                    }
                }
            })
            .collect();

        Self {
            files,
            focus: InitModalField::InitializeButton,
            error: None,
            success: None,
        }
    }

    /// Check if all files are up to date (nothing to create or regenerate).
    pub fn all_up_to_date(&self) -> bool {
        self.files
            .iter()
            .all(|f| f.status == InitFileStatus::Exists)
    }

    /// Count files that will be created.
    pub fn create_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.status == InitFileStatus::WillCreate)
            .count()
    }

    /// Count files that already exist (will be skipped).
    pub fn skip_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.status == InitFileStatus::Exists)
            .count()
    }

    /// Move focus to next field.
    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    /// Move focus to previous field.
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
    }

    /// Count files that will be regenerated.
    pub fn regenerate_count(&self) -> usize {
        self.files
            .iter()
            .filter(|f| f.status == InitFileStatus::WillRegenerate)
            .count()
    }

    /// Return a hint message describing scaffolding drift, or `None` if up to date.
    pub fn hint_message(&self) -> Option<&'static str> {
        match (self.create_count() > 0, self.regenerate_count() > 0) {
            (true, false) => Some("Skills not installed \u{2014} press `i` to run init."),
            (false, true) => Some("Skill updates available \u{2014} press `i` to refresh."),
            (true, true) => Some("Skills out of date \u{2014} press `i` to init."),
            (false, false) => None,
        }
    }

    /// Create all files. Returns Ok(()) on success, Err(message) on failure.
    pub fn create_files(&self) -> Result<(), String> {
        for file in &self.files {
            // Skip files that already exist and match the template
            if file.status == InitFileStatus::Exists {
                continue;
            }

            // Determine template content based on file path
            let content = template_for_path(&file.display_path)
                .ok_or_else(|| format!("Unknown template for: {}", file.display_path))?;

            // Create parent directories if needed
            if let Some(parent) = file.full_path.parent()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!("Failed to create directory {}: {}", parent.display(), e)
                })?;
            }

            // Write the file
            std::fs::write(&file.full_path, content)
                .map_err(|e| format!("Failed to write {}: {}", file.display_path, e))?;
        }

        Ok(())
    }

    /// Print diffs for all files that will be regenerated (CLI output).
    pub fn print_diffs(&self) {
        for file in &self.files {
            if file.diff_lines.is_empty() {
                continue;
            }
            println!("Changes for {}:", file.display_path);
            for line in &file.diff_lines {
                println!("{line}");
            }
            println!();
        }
    }
}

/// Handle keyboard input for the init modal.
pub fn handle_init_modal_input(app: &mut App, key_code: KeyCode) {
    let Some(state) = &mut app.init_modal_state else {
        return;
    };

    // Clear any previous error when user takes action
    if state.error.is_some() && key_code != KeyCode::Esc {
        state.error = None;
    }

    match key_code {
        // Navigation between buttons
        KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
            state.focus_next();
        }
        KeyCode::BackTab => {
            state.focus_prev();
        }

        // Cancel / close
        KeyCode::Esc => {
            app.show_init_modal = false;
            app.init_modal_state = None;
        }

        // Enter - context-dependent
        KeyCode::Enter => match state.focus {
            InitModalField::InitializeButton => {
                // Disabled when all files already exist
                if !state.all_up_to_date() {
                    let created = state.create_count();
                    let regenerated = state.regenerate_count();
                    let skipped = state.skip_count();
                    match state.create_files() {
                        Ok(()) => {
                            let mut parts = Vec::new();
                            if created > 0 {
                                parts.push(format!("created {created}"));
                            }
                            if regenerated > 0 {
                                parts.push(format!("updated {regenerated}"));
                            }
                            if skipped > 0 {
                                parts.push(format!("skipped {skipped}"));
                            }
                            debug!(
                                "Project initialized: created {created}, updated {regenerated}, skipped {skipped}"
                            );
                            state.success = Some(parts.join(", "));
                            // Close modal after showing success
                            app.show_init_modal = false;
                            app.init_modal_state = None;
                        }
                        Err(e) => {
                            state.error = Some(e);
                        }
                    }
                }
            }
            InitModalField::CancelButton => {
                app.show_init_modal = false;
                app.init_modal_state = None;
            }
        },

        _ => {}
    }
}

const MAX_DIFF_LINES: usize = 15;

/// Draw the project init modal.
pub fn draw_init_modal(f: &mut Frame, app: &App) {
    let Some(state) = &app.init_modal_state else {
        return;
    };

    let label_style = Style::default().fg(Color::DarkGray);
    let all_exist = state.all_up_to_date();

    let mut content: Vec<Line> = Vec::new();

    // Title/description
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "  Scaffold skill files:",
        label_style,
    )));
    content.push(Line::from(""));

    // File list with status indicators
    for file in &state.files {
        let (icon, icon_style, label) = match file.status {
            InitFileStatus::WillCreate => ("✓", Style::default().fg(Color::Green), None),
            InitFileStatus::Exists => (
                "—",
                Style::default().fg(Color::DarkGray),
                Some(" (up to date)"),
            ),
            InitFileStatus::WillRegenerate => (
                "↻",
                Style::default().fg(Color::Cyan),
                Some(" (will update)"),
            ),
        };

        let mut spans = vec![
            Span::raw("    "),
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(&file.display_path, Style::default().fg(Color::White)),
        ];
        if let Some(label) = label {
            spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
        }

        content.push(Line::from(spans));

        // Show diff for files that will be regenerated
        if !file.diff_lines.is_empty() {
            content.push(Line::from(""));
            let show_count = file.diff_lines.len().min(MAX_DIFF_LINES);
            for diff_line in &file.diff_lines[..show_count] {
                let style = if diff_line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if diff_line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else if diff_line.starts_with('@') {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                content.push(Line::from(Span::styled(
                    format!("      {diff_line}"),
                    style,
                )));
            }
            if file.diff_lines.len() > MAX_DIFF_LINES {
                content.push(Line::from(Span::styled(
                    format!(
                        "      ... ({} more lines)",
                        file.diff_lines.len() - MAX_DIFF_LINES
                    ),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    content.push(Line::from(""));

    // Show status messages
    if all_exist {
        content.push(Line::from(Span::styled(
            "  All skill files are up to date.",
            label_style,
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

    // Buttons - Initialize is disabled when all files exist
    let init_focused = state.focus == InitModalField::InitializeButton;
    let cancel_focused = state.focus == InitModalField::CancelButton;

    let init_style = if all_exist {
        Style::default().fg(Color::DarkGray)
    } else if init_focused {
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

    // Dynamic modal height based on content
    let modal_width: u16 = 72;
    let modal_height = (content.len() as u16 + 3).min(f.area().height.saturating_sub(2));
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Initialize Project ")
            .title_alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_includes_skills_only() {
        let config = Config::default();
        let state = InitModalState::new(&config);

        let paths: Vec<&str> = state
            .files
            .iter()
            .map(|f| f.display_path.as_str())
            .collect();
        assert!(paths.iter().any(|p| p.contains("brain-dump")));
        assert!(paths.iter().any(|p| p.contains("shape")));
        assert!(paths.iter().any(|p| p.contains("capture")));
        // PROMPT.md is no longer managed by init
        assert!(!paths.iter().any(|p| p.ends_with("PROMPT.md")));
    }

    #[test]
    fn test_init_has_two_files() {
        let config = Config::default();
        let state = InitModalState::new(&config);
        assert_eq!(state.files.len(), 3);
    }

    #[test]
    fn test_compute_diff_identical() {
        let text = "line 1\nline 2\nline 3\n";
        let diff = compute_diff(text, text);
        // Identical content produces only header lines (--- and +++)
        assert!(diff.iter().all(|l| !l.starts_with('@')));
    }

    #[test]
    fn test_compute_diff_different() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nchanged\nline 3\n";
        let diff = compute_diff(old, new);
        assert!(diff.iter().any(|l| l.starts_with('-')));
        assert!(diff.iter().any(|l| l.starts_with('+')));
    }

    #[test]
    fn test_template_for_path() {
        assert!(template_for_path(".claude/skills/brain-dump/SKILL.md").is_some());
        assert!(template_for_path(".claude/skills/shape/SKILL.md").is_some());
        assert!(template_for_path(".claude/skills/capture/SKILL.md").is_some());
        assert!(template_for_path("unknown/path.md").is_none());
    }

    fn make_state(statuses: &[InitFileStatus]) -> InitModalState {
        let files = statuses
            .iter()
            .enumerate()
            .map(|(i, &status)| InitFileEntry {
                display_path: format!("file_{i}"),
                full_path: PathBuf::from(format!("file_{i}")),
                status,
                diff_lines: vec![],
            })
            .collect();
        InitModalState {
            files,
            focus: InitModalField::InitializeButton,
            error: None,
            success: None,
        }
    }

    #[test]
    fn hint_message_none_when_all_up_to_date() {
        let state = make_state(&[InitFileStatus::Exists, InitFileStatus::Exists]);
        assert!(state.hint_message().is_none());
    }

    #[test]
    fn hint_message_missing_only() {
        let state = make_state(&[InitFileStatus::WillCreate, InitFileStatus::WillCreate]);
        let msg = state.hint_message().unwrap();
        assert!(msg.contains("not installed"));
        assert!(msg.contains("press `i`"));
    }

    #[test]
    fn hint_message_drifted_only() {
        let state = make_state(&[InitFileStatus::WillRegenerate, InitFileStatus::Exists]);
        let msg = state.hint_message().unwrap();
        assert!(msg.contains("updates available"));
    }

    #[test]
    fn hint_message_both_missing_and_drifted() {
        let state = make_state(&[InitFileStatus::WillCreate, InitFileStatus::WillRegenerate]);
        let msg = state.hint_message().unwrap();
        assert!(msg.contains("out of date"));
    }
}
