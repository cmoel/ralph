//! Init modal — project scaffolding initialization.

use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use tracing::debug;

use crate::app::App;
use crate::config::Config;
use crate::prompt_sniff;
use crate::templates;
use crate::ui::centered_rect;

/// Status of a file for the init modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitFileStatus {
    /// File will be created (doesn't exist).
    WillCreate,
    /// File already exists (will be skipped).
    Exists,
    /// File exists but contains stale content (will be regenerated with backup).
    Stale,
    /// File exists and will be force-regenerated (reinit mode).
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

impl InitModalState {
    /// Create a new init modal state by checking file existence.
    pub fn new(config: &Config) -> Self {
        let prompt_path = config.prompt_path();

        let mut files_to_check = vec![(config.paths.prompt.clone(), prompt_path.clone())];

        files_to_check.push((
            ".claude/skills/brain-dump/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/brain-dump/SKILL.md"),
        ));
        files_to_check.push((
            ".claude/skills/shape/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/shape/SKILL.md"),
        ));
        let files = files_to_check
            .into_iter()
            .map(|(display, full)| {
                let status = if !full.exists() {
                    InitFileStatus::WillCreate
                } else if display.ends_with("PROMPT.md") {
                    // Check if PROMPT.md contains stale specs-mode content
                    if let Ok(content) = std::fs::read_to_string(&full) {
                        if !prompt_sniff::sniff_prompt(&content).is_empty() {
                            InitFileStatus::Stale
                        } else {
                            InitFileStatus::Exists
                        }
                    } else {
                        InitFileStatus::Exists
                    }
                } else {
                    InitFileStatus::Exists
                };
                InitFileEntry {
                    display_path: display,
                    full_path: full,
                    status,
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
    pub fn all_exist(&self) -> bool {
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

    /// Create a reinit state: same files as init but marks
    /// existing files as `WillRegenerate` instead of `Exists`.
    pub fn new_reinit(config: &Config) -> Self {
        let prompt_path = config.prompt_path();

        let mut files_to_check = vec![(config.paths.prompt.clone(), prompt_path.clone())];

        files_to_check.push((
            ".claude/skills/brain-dump/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/brain-dump/SKILL.md"),
        ));
        files_to_check.push((
            ".claude/skills/shape/SKILL.md".to_string(),
            PathBuf::from(".claude/skills/shape/SKILL.md"),
        ));

        let files = files_to_check
            .into_iter()
            .map(|(display, full)| {
                let status = if full.exists() {
                    InitFileStatus::WillRegenerate
                } else {
                    InitFileStatus::WillCreate
                };
                InitFileEntry {
                    display_path: display,
                    full_path: full,
                    status,
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

    /// Create all files. Returns Ok(()) on success, Err(message) on failure.
    pub fn create_files(&self) -> Result<(), String> {
        for file in &self.files {
            // Skip files that already exist and are up to date
            if file.status == InitFileStatus::Exists {
                continue;
            }

            // Backup stale or regenerated files before overwriting
            if file.status == InitFileStatus::Stale || file.status == InitFileStatus::WillRegenerate
            {
                let backup_ext = if file.display_path.ends_with(".md") {
                    "md.bak"
                } else {
                    "bak"
                };
                let backup_path = file.full_path.with_extension(backup_ext);
                std::fs::rename(&file.full_path, &backup_path)
                    .map_err(|e| format!("Failed to backup {}: {}", file.display_path, e))?;
            }

            // Determine template content based on file path
            let content =
                if file.display_path.ends_with("PROMPT.md") || file.display_path == "./PROMPT.md" {
                    templates::PROMPT_MD
                } else if file.display_path.contains("brain-dump") {
                    templates::BRAIN_DUMP_SKILL_MD
                } else if file.display_path.contains("shape") {
                    templates::SHAPE_SKILL_MD
                } else {
                    return Err(format!("Unknown template for: {}", file.display_path));
                };

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
                if !state.all_exist() {
                    let created = state.create_count();
                    let skipped = state.skip_count();
                    match state.create_files() {
                        Ok(()) => {
                            debug!("Project initialized: created {created}, skipped {skipped}");
                            state.success = Some(format!(
                                "Created {created} files, skipped {skipped} existing"
                            ));
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
    let all_exist = state.all_exist();

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
        let (icon, icon_style, label) = match file.status {
            InitFileStatus::WillCreate => ("✓", Style::default().fg(Color::Green), None),
            InitFileStatus::Exists => (
                "—",
                Style::default().fg(Color::DarkGray),
                Some(" (exists, skipped)"),
            ),
            InitFileStatus::Stale => (
                "↻",
                Style::default().fg(Color::Yellow),
                Some(" (stale, will regenerate)"),
            ),
            InitFileStatus::WillRegenerate => (
                "↻",
                Style::default().fg(Color::Cyan),
                Some(" (will regenerate)"),
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
    }

    content.push(Line::from(""));

    // Show status messages
    if all_exist {
        content.push(Line::from(Span::styled(
            "  Nothing to create — all files already exist.",
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
    fn test_init_includes_prompt_and_skills() {
        let config = Config::default();
        let state = InitModalState::new(&config);

        let paths: Vec<&str> = state
            .files
            .iter()
            .map(|f| f.display_path.as_str())
            .collect();
        assert!(paths.iter().any(|p| p.ends_with("PROMPT.md")));
        assert!(paths.iter().any(|p| p.contains("brain-dump")));
        assert!(paths.iter().any(|p| p.contains("shape")));
    }

    #[test]
    fn test_init_has_three_files() {
        let config = Config::default();
        let state = InitModalState::new(&config);
        assert_eq!(state.files.len(), 3);
    }
}
