//! Modal dialog state and input handling.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyModifiers};
use tracing::debug;

use crate::app::App;
use crate::config::{Config, PartialConfig, save_config, save_partial_config};
use crate::get_file_mtime;
use crate::specs::{SpecStatus, parse_specs_readme};
use crate::templates;
use crate::validators::{
    validate_directory_exists, validate_executable_path, validate_file_exists,
};

/// Status of a file for the init modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitFileStatus {
    /// File will be created (doesn't exist).
    WillCreate,
    /// File already exists (conflict).
    Conflict,
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
        let specs_path = config.specs_path();

        // Build list of files to check
        let files_to_check = vec![
            (config.paths.prompt.clone(), prompt_path.clone()),
            (
                format!("{}/README.md", config.paths.specs),
                specs_path.join("README.md"),
            ),
            (
                format!("{}/TEMPLATE.md", config.paths.specs),
                specs_path.join("TEMPLATE.md"),
            ),
            (
                ".claude/commands/ralph-spec.md".to_string(),
                PathBuf::from(".claude/commands/ralph-spec.md"),
            ),
            (".ralph".to_string(), PathBuf::from(".ralph")),
        ];

        let files = files_to_check
            .into_iter()
            .map(|(display, full)| {
                let status = if full.exists() {
                    InitFileStatus::Conflict
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

    /// Check if there are any conflicts.
    pub fn has_conflicts(&self) -> bool {
        self.files
            .iter()
            .any(|f| f.status == InitFileStatus::Conflict)
    }

    /// Get list of conflicting files.
    pub fn conflicting_files(&self) -> Vec<&InitFileEntry> {
        self.files
            .iter()
            .filter(|f| f.status == InitFileStatus::Conflict)
            .collect()
    }

    /// Move focus to next field.
    pub fn focus_next(&mut self) {
        self.focus = self.focus.next();
    }

    /// Move focus to previous field.
    pub fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
    }

    /// Create all files. Returns Ok(()) on success, Err(message) on failure.
    pub fn create_files(&self) -> Result<(), String> {
        for file in &self.files {
            // Skip files that already exist (conflicts)
            if file.status == InitFileStatus::Conflict {
                continue;
            }

            // Determine template content based on file path
            let content =
                if file.display_path.ends_with("PROMPT.md") || file.display_path == "./PROMPT.md" {
                    templates::PROMPT_MD
                } else if file.display_path.ends_with("README.md") {
                    templates::SPECS_README_MD
                } else if file.display_path.ends_with("TEMPLATE.md") {
                    templates::SPECS_TEMPLATE_MD
                } else if file.display_path.ends_with("ralph-spec.md") {
                    templates::RALPH_SPEC_MD
                } else if file.display_path == ".ralph" {
                    templates::RALPH_CONFIG
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

/// Log level options for the dropdown.
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Which tab is active in the config modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTab {
    Project,
    Global,
}

/// Per-tab form state storing field values and validation state.
#[derive(Debug, Clone)]
pub struct TabFormState {
    pub claude_path: String,
    pub prompt_file: String,
    pub specs_dir: String,
    pub log_level_index: usize,
    pub iterations: i32,
    pub auto_expand_tasks: bool,
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
    PromptFile,
    SpecsDirectory,
    LogLevel,
    Iterations,
    AutoExpandTasks,
    KeepAwake,
    SaveButton,
    CancelButton,
}

impl ConfigModalField {
    pub fn next(self) -> Self {
        match self {
            Self::ClaudePath => Self::PromptFile,
            Self::PromptFile => Self::SpecsDirectory,
            Self::SpecsDirectory => Self::LogLevel,
            Self::LogLevel => Self::Iterations,
            Self::Iterations => Self::AutoExpandTasks,
            Self::AutoExpandTasks => Self::KeepAwake,
            Self::KeepAwake => Self::SaveButton,
            Self::SaveButton => Self::CancelButton,
            Self::CancelButton => Self::ClaudePath,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::ClaudePath => Self::CancelButton,
            Self::PromptFile => Self::ClaudePath,
            Self::SpecsDirectory => Self::PromptFile,
            Self::LogLevel => Self::SpecsDirectory,
            Self::Iterations => Self::LogLevel,
            Self::AutoExpandTasks => Self::Iterations,
            Self::KeepAwake => Self::AutoExpandTasks,
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
    /// Active tab (Project or Global). None when no .ralph exists.
    pub tab: Option<ConfigTab>,
    /// Project tab form state (only present when .ralph exists).
    pub project_form: Option<TabFormState>,
    /// Global tab form state.
    pub global_form: TabFormState,
    /// Path to the project config file (.ralph), if it exists.
    pub project_config_path: Option<PathBuf>,
}

impl TabFormState {
    /// Create form state from a Config, with all fields marked as explicit.
    fn from_config(config: &Config) -> Self {
        let log_level_index = LOG_LEVELS
            .iter()
            .position(|&l| l == config.logging.level)
            .unwrap_or(2);

        Self {
            claude_path: config.claude.path.clone(),
            prompt_file: config.paths.prompt.clone(),
            specs_dir: config.paths.specs.clone(),
            log_level_index,
            iterations: config.behavior.iterations,
            auto_expand_tasks: config.behavior.auto_expand_tasks_panel,
            keep_awake: config.behavior.keep_awake,
            cursor_pos: config.claude.path.len(),
            error: None,
            validation_errors: HashMap::new(),
            explicit_fields: HashSet::new(),
        }
    }

    /// Create form state for the project tab from a PartialConfig (values)
    /// and the merged Config (for inherited values to display).
    fn from_partial_config(partial: &PartialConfig, merged: &Config) -> Self {
        let mut explicit_fields = HashSet::new();

        if partial.claude.path.is_some() {
            explicit_fields.insert(ConfigModalField::ClaudePath);
        }
        if partial.paths.prompt.is_some() {
            explicit_fields.insert(ConfigModalField::PromptFile);
        }
        if partial.paths.specs.is_some() {
            explicit_fields.insert(ConfigModalField::SpecsDirectory);
        }
        if partial.logging.level.is_some() {
            explicit_fields.insert(ConfigModalField::LogLevel);
        }
        if partial.behavior.iterations.is_some() {
            explicit_fields.insert(ConfigModalField::Iterations);
        }
        if partial.behavior.auto_expand_tasks_panel.is_some() {
            explicit_fields.insert(ConfigModalField::AutoExpandTasks);
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
            prompt_file: merged.paths.prompt.clone(),
            specs_dir: merged.paths.specs.clone(),
            log_level_index,
            iterations: merged.behavior.iterations,
            auto_expand_tasks: merged.behavior.auto_expand_tasks_panel,
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
            paths: crate::config::PathsConfig {
                prompt: self.prompt_file.clone(),
                specs: self.specs_dir.clone(),
            },
            logging: crate::config::LoggingConfig {
                level: self.selected_log_level().to_string(),
            },
            behavior: crate::config::BehaviorConfig::default(),
        };
        config.behavior.iterations = self.iterations;
        config.behavior.auto_expand_tasks_panel = self.auto_expand_tasks;
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
            paths: crate::config::PartialPathsConfig {
                prompt: if self.explicit_fields.contains(&ConfigModalField::PromptFile) {
                    Some(self.prompt_file.clone())
                } else {
                    None
                },
                specs: if self
                    .explicit_fields
                    .contains(&ConfigModalField::SpecsDirectory)
                {
                    Some(self.specs_dir.clone())
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
                auto_expand_tasks_panel: if self
                    .explicit_fields
                    .contains(&ConfigModalField::AutoExpandTasks)
                {
                    Some(self.auto_expand_tasks)
                } else {
                    None
                },
                keep_awake: if self.explicit_fields.contains(&ConfigModalField::KeepAwake) {
                    Some(self.keep_awake)
                } else {
                    None
                },
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
    /// Create a new modal state for global config only (no .ralph).
    pub fn from_config(config: &Config) -> Self {
        Self {
            focus: ConfigModalField::ClaudePath,
            tab: None,
            project_form: None,
            global_form: TabFormState::from_config(config),
            project_config_path: None,
        }
    }

    /// Create a new modal state with project and global tabs.
    pub fn from_config_with_project(
        global_config: &Config,
        partial: &PartialConfig,
        merged_config: &Config,
        project_config_path: PathBuf,
    ) -> Self {
        Self {
            focus: ConfigModalField::ClaudePath,
            tab: Some(ConfigTab::Project),
            project_form: Some(TabFormState::from_partial_config(partial, merged_config)),
            global_form: TabFormState::from_config(global_config),
            project_config_path: Some(project_config_path),
        }
    }

    /// Get the active tab, defaulting to Global when no tabs.
    pub fn active_tab(&self) -> ConfigTab {
        self.tab.unwrap_or(ConfigTab::Global)
    }

    /// Whether tabs are shown (i.e., a .ralph exists).
    pub fn has_tabs(&self) -> bool {
        self.tab.is_some()
    }

    /// Switch to the other tab, preserving form state.
    pub fn switch_tab(&mut self) {
        if let Some(ref mut tab) = self.tab {
            // Save cursor state for the current form
            *tab = match tab {
                ConfigTab::Project => ConfigTab::Global,
                ConfigTab::Global => ConfigTab::Project,
            };
            self.focus = ConfigModalField::ClaudePath;
            self.update_cursor_for_new_focus();
        }
    }

    /// Get a reference to the active form state.
    pub fn active_form(&self) -> &TabFormState {
        match self.active_tab() {
            ConfigTab::Project => self.project_form.as_ref().unwrap_or(&self.global_form),
            ConfigTab::Global => &self.global_form,
        }
    }

    /// Get a mutable reference to the active form state.
    fn active_form_mut(&mut self) -> &mut TabFormState {
        match self.active_tab() {
            ConfigTab::Project => {
                if self.project_form.is_some() {
                    self.project_form.as_mut().unwrap()
                } else {
                    &mut self.global_form
                }
            }
            ConfigTab::Global => &mut self.global_form,
        }
    }

    /// Get a reference to the currently focused text field's value.
    pub fn current_field_value(&self) -> Option<&String> {
        let form = self.active_form();
        match self.focus {
            ConfigModalField::ClaudePath => Some(&form.claude_path),
            ConfigModalField::PromptFile => Some(&form.prompt_file),
            ConfigModalField::SpecsDirectory => Some(&form.specs_dir),
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

    /// Mark a field as explicitly set in the project tab.
    fn mark_explicit(&mut self) {
        if self.active_tab() == ConfigTab::Project
            && let Some(ref mut form) = self.project_form
        {
            form.explicit_fields.insert(self.focus);
        }
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
            ConfigModalField::PromptFile => {
                let form = self.active_form_mut();
                if cursor >= form.prompt_file.len() {
                    form.prompt_file.push(c);
                } else {
                    form.prompt_file.insert(cursor, c);
                }
                form.cursor_pos += 1;
                true
            }
            ConfigModalField::SpecsDirectory => {
                let form = self.active_form_mut();
                if cursor >= form.specs_dir.len() {
                    form.specs_dir.push(c);
                } else {
                    form.specs_dir.insert(cursor, c);
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
            ConfigModalField::PromptFile => {
                let form = self.active_form_mut();
                form.prompt_file.remove(cursor - 1);
                form.cursor_pos -= 1;
                true
            }
            ConfigModalField::SpecsDirectory => {
                let form = self.active_form_mut();
                form.specs_dir.remove(cursor - 1);
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
            ConfigModalField::PromptFile => {
                let form = self.active_form_mut();
                if cursor < form.prompt_file.len() {
                    form.prompt_file.remove(cursor);
                    true
                } else {
                    false
                }
            }
            ConfigModalField::SpecsDirectory => {
                let form = self.active_form_mut();
                if cursor < form.specs_dir.len() {
                    form.specs_dir.remove(cursor);
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
    pub fn validate_field(&mut self, field: ConfigModalField) {
        let form = self.active_form();
        let error = match field {
            ConfigModalField::ClaudePath => validate_executable_path(&form.claude_path),
            ConfigModalField::PromptFile => validate_file_exists(&form.prompt_file),
            ConfigModalField::SpecsDirectory => validate_directory_exists(&form.specs_dir),
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

    /// Toggle the auto-expand tasks setting.
    pub fn toggle_auto_expand_tasks(&mut self) {
        let form = self.active_form_mut();
        form.auto_expand_tasks = !form.auto_expand_tasks;
        self.mark_explicit();
    }

    /// Toggle the keep-awake setting.
    pub fn toggle_keep_awake(&mut self) {
        let form = self.active_form_mut();
        form.keep_awake = !form.keep_awake;
        self.mark_explicit();
    }
}

/// A single spec entry parsed from the README.
#[derive(Debug, Clone)]
pub struct SpecEntry {
    /// Name of the spec (from markdown link).
    pub name: String,
    /// Current status.
    pub status: SpecStatus,
    /// File creation/modification timestamp for sorting.
    pub timestamp: Option<SystemTime>,
}

/// State for the specs panel modal.
#[derive(Debug)]
pub struct SpecsPanelState {
    /// List of specs parsed from README.
    pub specs: Vec<SpecEntry>,
    /// Currently selected index.
    pub selected: usize,
    /// Scroll offset for the list.
    pub scroll_offset: usize,
    /// Error message if parsing failed.
    pub error: Option<String>,
    /// Directory where spec files are located.
    pub specs_dir: PathBuf,
}

impl SpecsPanelState {
    /// Create a new specs panel state by parsing the README.
    pub fn new(specs_dir: &std::path::Path) -> Self {
        match parse_specs_readme(specs_dir) {
            Ok(mut specs) => {
                // Sort by status (Blocked→Ready→InProgress→Done), then by timestamp (newest first)
                specs.sort_by(|a, b| match a.status.cmp(&b.status) {
                    std::cmp::Ordering::Equal => {
                        // Within same status, sort by timestamp descending (newest first)
                        // None values go to the end
                        match (&b.timestamp, &a.timestamp) {
                            (Some(b_ts), Some(a_ts)) => b_ts.cmp(a_ts),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    }
                    other => other,
                });
                Self {
                    specs,
                    selected: 0,
                    scroll_offset: 0,
                    error: None,
                    specs_dir: specs_dir.to_path_buf(),
                }
            }
            Err(e) => Self {
                specs: Vec::new(),
                selected: 0,
                scroll_offset: 0,
                error: Some(e),
                specs_dir: specs_dir.to_path_buf(),
            },
        }
    }

    /// Get the path to the currently selected spec file.
    pub fn selected_spec_path(&self) -> Option<PathBuf> {
        self.specs
            .get(self.selected)
            .map(|spec| self.specs_dir.join(format!("{}.md", spec.name)))
    }

    /// Read the head of the selected spec file.
    pub fn read_selected_spec_head(&self, max_lines: usize) -> Result<Vec<String>, String> {
        let path = self.selected_spec_path().ok_or("No spec selected")?;

        let contents = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err("File not found".to_string());
            }
            Err(e) => {
                return Err(format!("Error reading file: {}", e));
            }
        };

        Ok(contents.lines().take(max_lines).map(String::from).collect())
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if !self.specs.is_empty() && self.selected < self.specs.len() - 1 {
            self.selected += 1;
        }
    }

    /// Count of blocked specs.
    pub fn blocked_count(&self) -> usize {
        self.specs
            .iter()
            .filter(|s| s.status == SpecStatus::Blocked)
            .count()
    }

    /// Ensure selected item is visible, adjusting scroll_offset if needed.
    pub fn ensure_visible(&mut self, visible_height: usize) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected - visible_height + 1;
        }
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
        // Tab switching with [ and ]
        KeyCode::Char('[') if state.has_tabs() => {
            state.switch_tab();
        }
        KeyCode::Char(']') if state.has_tabs() => {
            state.switch_tab();
        }

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
                // Save based on active tab
                let save_result = match state.active_tab() {
                    ConfigTab::Project => {
                        let partial = state.to_partial_config();
                        if let Some(ref path) = state.project_config_path {
                            save_partial_config(&partial, path)
                        } else {
                            Err("No project config path".to_string())
                        }
                    }
                    ConfigTab::Global => {
                        let new_config = state.to_config();
                        save_config(&new_config, &app.config_path)
                    }
                };
                match save_result {
                    Ok(()) => {
                        // Re-merge and update app config
                        let new_merged = state.to_config();
                        app.config = new_merged;
                        // Update mtimes so we don't trigger a reload
                        app.config_mtime = get_file_mtime(&app.config_path);
                        if let Some(ref path) = state.project_config_path {
                            app.project_config_mtime = get_file_mtime(path);
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
            if matches!(
                state.focus,
                ConfigModalField::ClaudePath
                    | ConfigModalField::PromptFile
                    | ConfigModalField::SpecsDirectory
            ) {
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
            ConfigModalField::AutoExpandTasks => state.toggle_auto_expand_tasks(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            _ => state.cursor_left(),
        },

        KeyCode::Right => match state.focus {
            ConfigModalField::LogLevel => state.log_level_next(),
            ConfigModalField::Iterations => state.iterations_increment(),
            ConfigModalField::AutoExpandTasks => state.toggle_auto_expand_tasks(),
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
            ConfigModalField::AutoExpandTasks => state.toggle_auto_expand_tasks(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            ConfigModalField::SaveButton | ConfigModalField::CancelButton => state.focus_prev(),
            _ => {}
        },

        KeyCode::Down => match state.focus {
            ConfigModalField::LogLevel => state.log_level_next(),
            ConfigModalField::Iterations => state.iterations_decrement(),
            ConfigModalField::AutoExpandTasks => state.toggle_auto_expand_tasks(),
            ConfigModalField::KeepAwake => state.toggle_keep_awake(),
            ConfigModalField::SaveButton | ConfigModalField::CancelButton => state.focus_next(),
            _ => {}
        },

        _ => {}
    }
}

/// Handle keyboard input for the specs panel.
pub fn handle_specs_panel_input(app: &mut App, key_code: KeyCode) {
    let Some(state) = &mut app.specs_panel_state else {
        return;
    };

    match key_code {
        KeyCode::Esc => {
            app.show_specs_panel = false;
            app.specs_panel_state = None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
        }
        _ => {}
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
                // Only allow initialize when no conflicts
                if !state.has_conflicts() {
                    match state.create_files() {
                        Ok(()) => {
                            // Show success message and close modal
                            debug!("Project initialized successfully");
                            app.show_init_modal = false;
                            app.init_modal_state = None;
                        }
                        Err(e) => {
                            // Show error in modal
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

#[cfg(test)]
mod tests {
    use super::*;

    // ConfigModalField::next tests

    #[test]
    fn test_config_modal_field_next_full_cycle() {
        let field = ConfigModalField::ClaudePath;
        let field = field.next();
        assert_eq!(field, ConfigModalField::PromptFile);
        let field = field.next();
        assert_eq!(field, ConfigModalField::SpecsDirectory);
        let field = field.next();
        assert_eq!(field, ConfigModalField::LogLevel);
        let field = field.next();
        assert_eq!(field, ConfigModalField::Iterations);
        let field = field.next();
        assert_eq!(field, ConfigModalField::AutoExpandTasks);
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

    // ConfigModalField::prev tests

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
        assert_eq!(field, ConfigModalField::AutoExpandTasks);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::Iterations);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::LogLevel);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::SpecsDirectory);
        let field = field.prev();
        assert_eq!(field, ConfigModalField::PromptFile);
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
            ConfigModalField::PromptFile,
            ConfigModalField::SpecsDirectory,
            ConfigModalField::LogLevel,
            ConfigModalField::Iterations,
            ConfigModalField::AutoExpandTasks,
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
