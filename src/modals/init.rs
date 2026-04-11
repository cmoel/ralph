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

/// Return the compiled-in template for a managed file path.
fn template_for_path(display_path: &str) -> Option<&'static str> {
    if display_path.contains("brain-dump") {
        Some(templates::BRAIN_DUMP_SKILL_MD)
    } else if display_path.contains("shape") {
        Some(templates::SHAPE_SKILL_MD)
    } else if display_path.contains("capture") {
        Some(templates::CAPTURE_SKILL_MD)
    } else if display_path.ends_with("bd-retry.sh") {
        Some(templates::BD_RETRY_SH)
    } else if display_path.ends_with("intercept-bd.sh") {
        Some(templates::INTERCEPT_BD_SH)
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

/// The list of files managed by init.
fn managed_files() -> Vec<(String, PathBuf)> {
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
        (
            "scripts/bd-retry.sh".to_string(),
            PathBuf::from("scripts/bd-retry.sh"),
        ),
        (
            "scripts/intercept-bd.sh".to_string(),
            PathBuf::from("scripts/intercept-bd.sh"),
        ),
    ]
}

/// Path to the settings file that init merges the PreToolUse hook into.
const SETTINGS_PATH: &str = ".claude/settings.json";

/// Command string used in the PreToolUse hook entry for `intercept-bd.sh`.
const INTERCEPT_BD_HOOK_COMMAND: &str = "\"$CLAUDE_PROJECT_DIR\"/scripts/intercept-bd.sh";

/// Merge the intercept-bd PreToolUse hook entry into `.claude/settings.json`.
///
/// Creates the file if missing. If an existing entry with matcher `"Bash"` is
/// present, appends our hook alongside its current hooks (preserving any
/// build-intercept entry). Otherwise a new matcher entry is added. No-op if
/// our hook is already registered.
fn ensure_intercept_bd_hook_registered() -> Result<(), String> {
    let path = PathBuf::from(SETTINGS_PATH);

    let mut json: serde_json::Value = if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {SETTINGS_PATH}: {e}"))?;
        serde_json::from_str(&contents)
            .map_err(|e| format!("Failed to parse {SETTINGS_PATH}: {e}"))?
    } else {
        serde_json::json!({})
    };

    let updated = merge_intercept_bd_hook(&mut json)?;
    if !updated {
        return Ok(());
    }

    if let Some(parent) = path.parent()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    let serialized = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("Failed to serialize {SETTINGS_PATH}: {e}"))?;
    std::fs::write(&path, format!("{serialized}\n"))
        .map_err(|e| format!("Failed to write {SETTINGS_PATH}: {e}"))?;

    Ok(())
}

/// Pure merge step: inserts the intercept-bd hook entry into the given JSON
/// value. Returns `Ok(true)` if `json` was modified, `Ok(false)` if the hook
/// was already registered. Fails on unexpected JSON shapes.
fn merge_intercept_bd_hook(json: &mut serde_json::Value) -> Result<bool, String> {
    let hook_entry = serde_json::json!({
        "type": "command",
        "command": INTERCEPT_BD_HOOK_COMMAND,
    });

    let root = json
        .as_object_mut()
        .ok_or_else(|| format!("{SETTINGS_PATH} is not a JSON object"))?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{SETTINGS_PATH} `hooks` is not a JSON object"))?;
    let pre_tool_use = hooks
        .entry("PreToolUse")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("{SETTINGS_PATH} `hooks.PreToolUse` is not a JSON array"))?;

    for entry in pre_tool_use.iter() {
        if let Some(hook_list) = entry.get("hooks").and_then(|h| h.as_array())
            && hook_list.iter().any(|h| {
                h.get("command").and_then(|c| c.as_str()) == Some(INTERCEPT_BD_HOOK_COMMAND)
            })
        {
            return Ok(false);
        }
    }

    for entry in pre_tool_use.iter_mut() {
        if entry.get("matcher").and_then(|m| m.as_str()) == Some("Bash")
            && let Some(hook_list) = entry.get_mut("hooks").and_then(|h| h.as_array_mut())
        {
            hook_list.push(hook_entry);
            return Ok(true);
        }
    }

    pre_tool_use.push(serde_json::json!({
        "matcher": "Bash",
        "hooks": [hook_entry],
    }));
    Ok(true)
}

impl InitModalState {
    /// Create a new init modal state by checking file existence and diffs.
    ///
    /// Files that exist and match the template are skipped. Files that exist
    /// but differ show a unified diff and will be overwritten.
    pub fn new(_config: &Config) -> Self {
        let files = managed_files()
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
            if file.status == InitFileStatus::Exists {
                continue;
            }

            let content = template_for_path(&file.display_path)
                .ok_or_else(|| format!("Unknown template for: {}", file.display_path))?;

            if let Some(parent) = file.full_path.parent()
                && !parent.exists()
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!("Failed to create directory {}: {}", parent.display(), e)
                })?;
            }

            std::fs::write(&file.full_path, content)
                .map_err(|e| format!("Failed to write {}: {}", file.display_path, e))?;

            #[cfg(unix)]
            if file.display_path.ends_with(".sh") {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&file.full_path)
                    .map_err(|e| format!("Failed to read {}: {}", file.display_path, e))?
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&file.full_path, perms)
                    .map_err(|e| format!("Failed to chmod {}: {}", file.display_path, e))?;
            }
        }

        ensure_intercept_bd_hook_registered()?;

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

        KeyCode::Char('?') => {
            app.help_context = Some(crate::modals::HelpContext::Init);
        }
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
    fn test_init_includes_expected_files() {
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
        assert!(paths.iter().any(|p| p.ends_with("bd-retry.sh")));
        assert!(paths.iter().any(|p| p.ends_with("intercept-bd.sh")));
        assert!(!paths.iter().any(|p| p.ends_with("PROMPT.md")));
    }

    #[test]
    fn test_init_manages_five_files() {
        let config = Config::default();
        let state = InitModalState::new(&config);
        assert_eq!(state.files.len(), 5);
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
        assert!(template_for_path("scripts/bd-retry.sh").is_some());
        assert!(template_for_path("scripts/intercept-bd.sh").is_some());
        assert!(template_for_path("unknown/path.md").is_none());
    }

    #[test]
    fn merge_into_empty_settings_creates_hook_entry() {
        let mut json = serde_json::json!({});
        let changed = merge_intercept_bd_hook(&mut json).unwrap();
        assert!(changed);
        let entry = json["hooks"]["PreToolUse"][0].clone();
        assert_eq!(entry["matcher"], "Bash");
        assert_eq!(
            entry["hooks"][0]["command"].as_str(),
            Some(INTERCEPT_BD_HOOK_COMMAND)
        );
    }

    #[test]
    fn merge_is_idempotent_when_hook_already_present() {
        let mut json = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": INTERCEPT_BD_HOOK_COMMAND,
                    }]
                }]
            }
        });
        let changed = merge_intercept_bd_hook(&mut json).unwrap();
        assert!(!changed);
        assert_eq!(
            json["hooks"]["PreToolUse"][0]["hooks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn merge_appends_to_existing_bash_matcher_preserving_other_hooks() {
        let mut json = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "\"$CLAUDE_PROJECT_DIR\"/scripts/intercept-build.sh",
                    }]
                }]
            }
        });
        let changed = merge_intercept_bd_hook(&mut json).unwrap();
        assert!(changed);
        let hooks = json["hooks"]["PreToolUse"][0]["hooks"].as_array().unwrap();
        assert_eq!(hooks.len(), 2);
        let commands: Vec<&str> = hooks
            .iter()
            .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
            .collect();
        assert!(commands.iter().any(|c| c.contains("intercept-build.sh")));
        assert!(commands.iter().any(|c| c.contains("intercept-bd.sh")));
    }

    #[test]
    fn merge_preserves_unrelated_settings() {
        let mut json = serde_json::json!({
            "permissions": {
                "allow": ["Edit", "Write"]
            }
        });
        let changed = merge_intercept_bd_hook(&mut json).unwrap();
        assert!(changed);
        assert_eq!(json["permissions"]["allow"][0], "Edit");
        assert!(json["hooks"]["PreToolUse"].is_array());
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
