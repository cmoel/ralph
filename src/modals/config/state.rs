use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::config::{Config, PartialConfig};
use crate::validators::validate_executable_path;

/// Log level options for the dropdown.
pub const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

const HEARTBEAT_MIN: u64 = 1;
const HEARTBEAT_MAX: u64 = 9999;
const STALE_MIN: u64 = 1;
const STALE_MAX: u64 = 9999;

/// Per-tab form state storing field values and validation state.
#[derive(Debug, Clone)]
pub struct TabFormState {
    pub claude_path: String,
    pub bd_path: String,
    pub log_level_index: usize,
    pub iterations: i32,
    pub heartbeat_interval: u64,
    pub stale_threshold: u64,
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
    BdPath,
    LogLevel,
    Iterations,
    HeartbeatInterval,
    StaleThreshold,
    KeepAwake,
    SaveButton,
    CancelButton,
}

impl ConfigModalField {
    pub fn next(self) -> Self {
        match self {
            Self::ClaudePath => Self::BdPath,
            Self::BdPath => Self::LogLevel,
            Self::LogLevel => Self::Iterations,
            Self::Iterations => Self::HeartbeatInterval,
            Self::HeartbeatInterval => Self::StaleThreshold,
            Self::StaleThreshold => Self::KeepAwake,
            Self::KeepAwake => Self::SaveButton,
            Self::SaveButton => Self::CancelButton,
            Self::CancelButton => Self::ClaudePath,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::ClaudePath => Self::CancelButton,
            Self::BdPath => Self::ClaudePath,
            Self::LogLevel => Self::BdPath,
            Self::Iterations => Self::LogLevel,
            Self::HeartbeatInterval => Self::Iterations,
            Self::StaleThreshold => Self::HeartbeatInterval,
            Self::KeepAwake => Self::StaleThreshold,
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
    /// Form state for editing config values.
    pub form: TabFormState,
    /// Path to the per-project config file, if available.
    pub project_config_path: Option<PathBuf>,
}

impl TabFormState {
    /// Create form state from a PartialConfig (to know which fields are explicit)
    /// and the merged Config (for displaying resolved values).
    pub(super) fn from_partial_config(partial: &PartialConfig, merged: &Config) -> Self {
        let mut explicit_fields = HashSet::new();

        if partial.claude.path.is_some() {
            explicit_fields.insert(ConfigModalField::ClaudePath);
        }
        if partial.logging.level.is_some() {
            explicit_fields.insert(ConfigModalField::LogLevel);
        }
        if partial.behavior.iterations.is_some() {
            explicit_fields.insert(ConfigModalField::Iterations);
        }
        if partial.behavior.keep_awake.is_some() {
            explicit_fields.insert(ConfigModalField::KeepAwake);
        }
        if partial.behavior.bd_path.is_some() {
            explicit_fields.insert(ConfigModalField::BdPath);
        }
        if partial.behavior.heartbeat_interval.is_some() {
            explicit_fields.insert(ConfigModalField::HeartbeatInterval);
        }
        if partial.behavior.stale_threshold.is_some() {
            explicit_fields.insert(ConfigModalField::StaleThreshold);
        }

        // Display merged values (so inherited fields show their effective value)
        let log_level_index = LOG_LEVELS
            .iter()
            .position(|&l| l == merged.logging.level)
            .unwrap_or(2);

        Self {
            claude_path: merged.claude.path.clone(),
            bd_path: merged.behavior.bd_path.clone(),
            log_level_index,
            iterations: merged.behavior.iterations,
            heartbeat_interval: merged.behavior.heartbeat_interval,
            stale_threshold: merged.behavior.stale_threshold,
            keep_awake: merged.behavior.keep_awake,
            cursor_pos: merged.claude.path.len(),
            error: None,
            validation_errors: HashMap::new(),
            explicit_fields,
        }
    }

    /// Build a Config from the current form values.
    pub(super) fn to_config(&self) -> Config {
        let mut config = Config {
            claude: crate::config::ClaudeConfig {
                path: self.claude_path.clone(),
                args: None,
            },
            logging: crate::config::LoggingConfig {
                level: self.selected_log_level().to_string(),
            },
            behavior: crate::config::BehaviorConfig::default(),
        };
        config.behavior.iterations = self.iterations;
        config.behavior.keep_awake = self.keep_awake;
        config.behavior.bd_path = self.bd_path.clone();
        config.behavior.heartbeat_interval = self.heartbeat_interval;
        config.behavior.stale_threshold = self.stale_threshold;
        config
    }

    /// Build a PartialConfig from the current form values, including only explicit fields.
    pub(super) fn to_partial_config(&self) -> PartialConfig {
        PartialConfig {
            claude: crate::config::PartialClaudeConfig {
                path: if self.explicit_fields.contains(&ConfigModalField::ClaudePath) {
                    Some(self.claude_path.clone())
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
                keep_awake: if self.explicit_fields.contains(&ConfigModalField::KeepAwake) {
                    Some(self.keep_awake)
                } else {
                    None
                },
                bd_path: if self.explicit_fields.contains(&ConfigModalField::BdPath) {
                    Some(self.bd_path.clone())
                } else {
                    None
                },
                heartbeat_interval: if self
                    .explicit_fields
                    .contains(&ConfigModalField::HeartbeatInterval)
                {
                    Some(self.heartbeat_interval)
                } else {
                    None
                },
                stale_threshold: if self
                    .explicit_fields
                    .contains(&ConfigModalField::StaleThreshold)
                {
                    Some(self.stale_threshold)
                } else {
                    None
                },
                workers: None,
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
    /// Create modal state showing resolved config (compiled-in defaults + per-project overrides).
    pub fn from_config(
        partial: &PartialConfig,
        merged_config: &Config,
        project_config_path: Option<PathBuf>,
    ) -> Self {
        Self {
            focus: ConfigModalField::ClaudePath,
            form: TabFormState::from_partial_config(partial, merged_config),
            project_config_path,
        }
    }

    /// Get a reference to the form state.
    pub fn active_form(&self) -> &TabFormState {
        &self.form
    }

    /// Get a mutable reference to the form state.
    fn active_form_mut(&mut self) -> &mut TabFormState {
        &mut self.form
    }

    /// Get a reference to the currently focused text field's value.
    pub fn current_field_value(&self) -> Option<&String> {
        let form = self.active_form();
        match self.focus {
            ConfigModalField::ClaudePath => Some(&form.claude_path),
            ConfigModalField::BdPath => Some(&form.bd_path),
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

    /// Mark a field as explicitly set (overriding the compiled-in default).
    fn mark_explicit(&mut self) {
        self.form.explicit_fields.insert(self.focus);
    }

    /// Insert a character at the current cursor position.
    pub fn insert_char(&mut self, c: char) {
        let cursor = self.active_form().cursor_pos;
        let field_changed = match self.focus {
            ConfigModalField::ClaudePath | ConfigModalField::BdPath => {
                let focus = self.focus;
                let form = self.active_form_mut();
                let field = if focus == ConfigModalField::ClaudePath {
                    &mut form.claude_path
                } else {
                    &mut form.bd_path
                };
                if cursor >= field.len() {
                    field.push(c);
                } else {
                    field.insert(cursor, c);
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
            ConfigModalField::ClaudePath | ConfigModalField::BdPath => {
                let focus = self.focus;
                let form = self.active_form_mut();
                let field = if focus == ConfigModalField::ClaudePath {
                    &mut form.claude_path
                } else {
                    &mut form.bd_path
                };
                field.remove(cursor - 1);
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
            ConfigModalField::ClaudePath | ConfigModalField::BdPath => {
                let focus = self.focus;
                let form = self.active_form_mut();
                let field = if focus == ConfigModalField::ClaudePath {
                    &mut form.claude_path
                } else {
                    &mut form.bd_path
                };
                if cursor < field.len() {
                    field.remove(cursor);
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

    pub fn heartbeat_increment(&mut self) {
        let form = self.active_form_mut();
        if form.heartbeat_interval < HEARTBEAT_MAX {
            form.heartbeat_interval += 1;
        }
        self.mark_explicit();
    }

    pub fn heartbeat_decrement(&mut self) {
        let form = self.active_form_mut();
        if form.heartbeat_interval > HEARTBEAT_MIN {
            form.heartbeat_interval -= 1;
        }
        self.mark_explicit();
    }

    pub fn stale_increment(&mut self) {
        let form = self.active_form_mut();
        if form.stale_threshold < STALE_MAX {
            form.stale_threshold += 1;
        }
        self.mark_explicit();
    }

    pub fn stale_decrement(&mut self) {
        let form = self.active_form_mut();
        if form.stale_threshold > STALE_MIN {
            form.stale_threshold -= 1;
        }
        self.mark_explicit();
    }

    /// Check if there are any validation errors.
    pub fn has_validation_errors(&self) -> bool {
        self.active_form().has_validation_errors()
    }

    /// Validate a specific field and update validation_errors.
    /// Skips validation for inherited (non-explicit) fields.
    pub fn validate_field(&mut self, field: ConfigModalField) {
        if !self.form.explicit_fields.contains(&field) {
            return;
        }

        let form = self.active_form();
        let error = match field {
            ConfigModalField::ClaudePath => validate_executable_path(&form.claude_path),
            ConfigModalField::BdPath => {
                if form.bd_path.trim().is_empty() {
                    Some("Path cannot be empty".to_string())
                } else {
                    None
                }
            }
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

    /// Toggle the keep-awake setting.
    pub fn toggle_keep_awake(&mut self) {
        let form = self.active_form_mut();
        form.keep_awake = !form.keep_awake;
        self.mark_explicit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, PartialConfig};

    fn make_state(partial: &PartialConfig, merged: &Config) -> ConfigModalState {
        ConfigModalState::from_config(partial, merged, None)
    }

    fn default_state() -> ConfigModalState {
        make_state(&PartialConfig::default(), &Config::default())
    }

    // -- BdPath round-trip tests --

    #[test]
    fn bd_path_round_trips_through_partial() {
        let mut partial = PartialConfig::default();
        partial.behavior.bd_path = Some("/usr/local/bin/bd".to_string());
        let mut merged = Config::default();
        merged.behavior.bd_path = "/usr/local/bin/bd".to_string();

        let state = make_state(&partial, &merged);
        let out = state.to_partial_config();
        assert_eq!(out.behavior.bd_path, Some("/usr/local/bin/bd".to_string()));
    }

    #[test]
    fn bd_path_not_in_partial_when_not_explicit() {
        let state = default_state();
        let out = state.to_partial_config();
        assert_eq!(out.behavior.bd_path, None);
    }

    #[test]
    fn bd_path_in_partial_when_explicit() {
        let mut state = default_state();
        state.focus = ConfigModalField::BdPath;
        state.insert_char('x');
        let out = state.to_partial_config();
        assert!(out.behavior.bd_path.is_some());
    }

    #[test]
    fn bd_path_empty_validation() {
        let mut state = default_state();
        // Navigate to BdPath (updates cursor position)
        state.focus_next(); // ClaudePath -> BdPath
        assert_eq!(state.focus, ConfigModalField::BdPath);

        let default_len = state.active_form().bd_path.len();
        for _ in 0..default_len {
            state.delete_char_before();
        }
        state.validate_field(ConfigModalField::BdPath);
        assert!(
            state
                .active_form()
                .validation_errors
                .contains_key(&ConfigModalField::BdPath)
        );

        // Typing something clears the error
        state.insert_char('b');
        assert!(
            !state
                .active_form()
                .validation_errors
                .contains_key(&ConfigModalField::BdPath)
        );
    }

    // -- HeartbeatInterval tests --

    #[test]
    fn heartbeat_interval_round_trips_through_partial() {
        let mut partial = PartialConfig::default();
        partial.behavior.heartbeat_interval = Some(60);
        let mut merged = Config::default();
        merged.behavior.heartbeat_interval = 60;

        let state = make_state(&partial, &merged);
        let out = state.to_partial_config();
        assert_eq!(out.behavior.heartbeat_interval, Some(60));
    }

    #[test]
    fn heartbeat_interval_clamps_min() {
        let mut state = default_state();
        state.focus = ConfigModalField::HeartbeatInterval;
        state.active_form_mut().heartbeat_interval = 1;
        state.heartbeat_decrement();
        assert_eq!(state.active_form().heartbeat_interval, 1);
    }

    #[test]
    fn heartbeat_interval_clamps_max() {
        let mut state = default_state();
        state.focus = ConfigModalField::HeartbeatInterval;
        state.active_form_mut().heartbeat_interval = 9999;
        state.heartbeat_increment();
        assert_eq!(state.active_form().heartbeat_interval, 9999);
    }

    #[test]
    fn heartbeat_interval_not_in_partial_when_not_explicit() {
        let state = default_state();
        let out = state.to_partial_config();
        assert_eq!(out.behavior.heartbeat_interval, None);
    }

    #[test]
    fn heartbeat_interval_in_partial_when_explicit() {
        let mut state = default_state();
        state.focus = ConfigModalField::HeartbeatInterval;
        state.heartbeat_increment();
        let out = state.to_partial_config();
        assert!(out.behavior.heartbeat_interval.is_some());
    }

    // -- StaleThreshold tests --

    #[test]
    fn stale_threshold_round_trips_through_partial() {
        let mut partial = PartialConfig::default();
        partial.behavior.stale_threshold = Some(300);
        let mut merged = Config::default();
        merged.behavior.stale_threshold = 300;

        let state = make_state(&partial, &merged);
        let out = state.to_partial_config();
        assert_eq!(out.behavior.stale_threshold, Some(300));
    }

    #[test]
    fn stale_threshold_clamps_min() {
        let mut state = default_state();
        state.focus = ConfigModalField::StaleThreshold;
        state.active_form_mut().stale_threshold = 1;
        state.stale_decrement();
        assert_eq!(state.active_form().stale_threshold, 1);
    }

    #[test]
    fn stale_threshold_clamps_max() {
        let mut state = default_state();
        state.focus = ConfigModalField::StaleThreshold;
        state.active_form_mut().stale_threshold = 9999;
        state.stale_increment();
        assert_eq!(state.active_form().stale_threshold, 9999);
    }

    #[test]
    fn stale_threshold_not_in_partial_when_not_explicit() {
        let state = default_state();
        let out = state.to_partial_config();
        assert_eq!(out.behavior.stale_threshold, None);
    }

    #[test]
    fn stale_threshold_in_partial_when_explicit() {
        let mut state = default_state();
        state.focus = ConfigModalField::StaleThreshold;
        state.stale_increment();
        let out = state.to_partial_config();
        assert!(out.behavior.stale_threshold.is_some());
    }
}
