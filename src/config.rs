use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Claude CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub path: String,
    /// Legacy field - ignored on load, not serialized.
    /// CLI args are hardcoded in main.rs as Ralph depends on specific args for streaming.
    #[serde(skip_serializing, default)]
    #[allow(dead_code)]
    pub args: Option<String>,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            path: "~/.claude/local/claude".to_string(),
            args: None,
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

/// Behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Number of iterations to run:
    /// - Negative (-1): Infinite mode, continues until user stops or all beads complete
    /// - Zero (0): Stopped mode, pressing 's' has no effect
    /// - Positive (N): Runs exactly N iterations then stops
    pub iterations: i32,
    /// Whether to acquire a wake lock to prevent system idle sleep.
    /// When true, the system won't sleep while claude is running.
    /// Display may still sleep. Default: true.
    pub keep_awake: bool,
    /// Path to the `bd` CLI binary. Default: "bd".
    pub bd_path: String,
    /// How often to send agent heartbeats (seconds). Default: 30.
    pub heartbeat_interval: u64,
    /// How long before an agent is considered stale (seconds). Default: 180.
    pub stale_threshold: u64,
    /// Number of concurrent Claude Code workers to spawn on S press. Default: 1.
    pub workers: u32,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            iterations: -1,   // Infinite mode by default
            keep_awake: true, // Prevent system sleep by default
            bd_path: "bd".to_string(),
            heartbeat_interval: 30,
            stale_threshold: 180,
            workers: 1,
        }
    }
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub behavior: BehaviorConfig,
}

impl Config {
    /// Expand `~` to home directory in a path string
    pub fn expand_tilde(path: &str) -> PathBuf {
        if let Some(stripped) = path.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(stripped);
        }
        PathBuf::from(path)
    }

    /// Get the expanded Claude CLI path
    pub fn claude_path(&self) -> PathBuf {
        Self::expand_tilde(&self.claude.path)
    }
}

/// Partial Claude CLI configuration for project overrides.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PartialClaudeConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Partial logging configuration for project overrides.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PartialLoggingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// Partial behavior configuration for project overrides.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PartialBehaviorConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_awake: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bd_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_interval: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_threshold: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workers: Option<u32>,
}

/// Project-specific configuration where every field is optional.
/// Fields that are `None` inherit from compiled-in defaults.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PartialConfig {
    #[serde(skip_serializing_if = "is_partial_claude_empty")]
    pub claude: PartialClaudeConfig,
    #[serde(skip_serializing_if = "is_partial_logging_empty")]
    pub logging: PartialLoggingConfig,
    #[serde(skip_serializing_if = "is_partial_behavior_empty")]
    pub behavior: PartialBehaviorConfig,
}

fn is_partial_claude_empty(c: &PartialClaudeConfig) -> bool {
    c.path.is_none()
}

fn is_partial_logging_empty(l: &PartialLoggingConfig) -> bool {
    l.level.is_none()
}

fn is_partial_behavior_empty(b: &PartialBehaviorConfig) -> bool {
    b.iterations.is_none()
        && b.keep_awake.is_none()
        && b.bd_path.is_none()
        && b.heartbeat_interval.is_none()
        && b.stale_threshold.is_none()
        && b.workers.is_none()
}

/// Merge a base config with a project-level partial config.
/// Project values override base values where present.
pub fn merge_config(global: &Config, project: &PartialConfig) -> Config {
    Config {
        claude: ClaudeConfig {
            path: project
                .claude
                .path
                .clone()
                .unwrap_or_else(|| global.claude.path.clone()),
            args: None,
        },
        logging: LoggingConfig {
            level: project
                .logging
                .level
                .clone()
                .unwrap_or_else(|| global.logging.level.clone()),
        },
        behavior: BehaviorConfig {
            iterations: project
                .behavior
                .iterations
                .unwrap_or(global.behavior.iterations),
            keep_awake: project
                .behavior
                .keep_awake
                .unwrap_or(global.behavior.keep_awake),
            bd_path: project
                .behavior
                .bd_path
                .clone()
                .unwrap_or_else(|| global.behavior.bd_path.clone()),
            heartbeat_interval: project
                .behavior
                .heartbeat_interval
                .unwrap_or(global.behavior.heartbeat_interval),
            stale_threshold: project
                .behavior
                .stale_threshold
                .unwrap_or(global.behavior.stale_threshold),
            workers: project
                .behavior
                .workers
                .unwrap_or(global.behavior.workers)
                .max(1),
        },
    }
}

/// Loaded configuration with metadata
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub project_config_path: Option<PathBuf>,
}

impl LoadedConfig {
    /// Create a minimal LoadedConfig for tests (no side effects).
    #[cfg(test)]
    pub fn default_for_test() -> Self {
        Self {
            config: Config::default(),
            project_config_path: None,
        }
    }
}

/// Get the platform-appropriate config directory
fn get_config_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "cmoel", "ralph").map(|dirs| dirs.config_dir().to_path_buf())
}

/// Derive a project key from an absolute path.
/// Replaces path separators with dashes (e.g. `/Users/me/code/foo` → `-Users-me-code-foo`).
fn project_key_from_path(path: &std::path::Path) -> String {
    path.to_string_lossy().replace(['/', '\\'], "-")
}

/// Compute the per-project config path (deterministic, may not exist yet).
/// Returns `<config-dir>/projects/<key>/config.toml` based on the current working directory.
pub fn compute_project_config_path() -> Option<PathBuf> {
    let config_dir = get_config_dir()?;
    let cwd = std::env::current_dir().ok()?;
    let key = project_key_from_path(&cwd);
    Some(config_dir.join("projects").join(key).join("config.toml"))
}

/// Get the per-project config path if the file exists.
pub fn get_project_config_path() -> Option<PathBuf> {
    let path = compute_project_config_path()?;
    if path.exists() { Some(path) } else { None }
}

/// Resolve the per-project PROMPT.md path if the file exists.
/// Returns the path to `<per-project-config-dir>/PROMPT.md` when present, or None
/// (meaning the compiled-in default should be used).
pub fn resolve_prompt_path() -> Option<PathBuf> {
    let config_path = compute_project_config_path()?;
    let prompt_path = config_path.with_file_name("PROMPT.md");
    if prompt_path.exists() {
        Some(prompt_path)
    } else {
        None
    }
}

/// Resolve the per-project board_columns.toml path if the file exists.
/// Returns the path to `<per-project-config-dir>/board_columns.toml` when present,
/// or None (meaning the compiled-in default should be used).
pub fn resolve_board_columns_path() -> Option<PathBuf> {
    let config_path = compute_project_config_path()?;
    let board_path = config_path.with_file_name("board_columns.toml");
    if board_path.exists() {
        Some(board_path)
    } else {
        None
    }
}

/// Load a per-project config from the given path.
/// Returns Ok(PartialConfig) on success, Err(String) on parse/read failure.
pub fn load_project_config(path: &PathBuf) -> Result<PartialConfig, String> {
    let contents = fs::read_to_string(path).map_err(|e| {
        warn!(path = ?path, error = %e, "project_config_read_failed");
        format!("Failed to read project config: {}", e)
    })?;

    toml::from_str::<PartialConfig>(&contents).map_err(|e| {
        warn!(path = ?path, error = %e, "project_config_parse_failed");
        format!("Invalid project config: {}", e)
    })
}

/// Load configuration from compiled-in defaults, per-project overrides, and env vars.
pub fn load_config() -> LoadedConfig {
    let mut config = Config::default();

    // Check for per-project config file
    let project_config_path = get_project_config_path();
    if let Some(ref project_path) = project_config_path {
        match load_project_config(project_path) {
            Ok(partial) => {
                config = merge_config(&config, &partial);
                info!(path = ?project_path, "project_config_loaded");
            }
            Err(e) => {
                warn!(path = ?project_path, error = %e, "project_config_error");
            }
        }
    }

    let config = apply_env_overrides(config);

    LoadedConfig {
        config,
        project_config_path,
    }
}

/// Result of reloading configuration.
pub struct ReloadedConfig {
    pub config: Config,
    pub project_error: Option<String>,
}

/// Reload configuration from compiled-in defaults and optional project config.
/// Returns a ReloadedConfig that always has a usable config (falls back to defaults).
pub fn reload_config(project_config_path: Option<&PathBuf>) -> ReloadedConfig {
    let mut config = Config::default();

    // Merge with project config if present
    let project_error = if let Some(project_path) = project_config_path {
        if project_path.exists() {
            match load_project_config(project_path) {
                Ok(partial) => {
                    config = merge_config(&config, &partial);
                    None
                }
                Err(e) => Some(e),
            }
        } else {
            // Project config was deleted — just use defaults, no error
            None
        }
    } else {
        None
    };

    let config = apply_env_overrides(config);
    info!("config_reloaded");

    ReloadedConfig {
        config,
        project_error,
    }
}

/// Save a partial config to the given file path (per-project config).
/// Creates parent directories if needed. Prepends a comment header.
/// Only writes fields that are Some.
pub fn save_partial_config(partial: &PartialConfig, config_path: &PathBuf) -> Result<(), String> {
    let toml_content = toml::to_string_pretty(partial).map_err(|e| {
        warn!(error = %e, "partial_config_save_serialize_failed");
        format!("Failed to serialize config: {}", e)
    })?;

    // Prepend comment header, then TOML content (skip if all fields are None)
    let content = if toml_content.trim().is_empty() {
        "# Project-specific Ralph config — edit with config modal (c)\n".to_string()
    } else {
        format!(
            "# Project-specific Ralph config — edit with config modal (c)\n\n{}",
            toml_content
        )
    };

    // Ensure parent directory exists (creates projects/<key>/ on first save)
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            warn!(path = ?parent, error = %e, "project_config_dir_create_failed");
            format!("Failed to create project config directory: {}", e)
        })?;
    }

    fs::write(config_path, &content).map_err(|e| {
        warn!(path = ?config_path, error = %e, "partial_config_save_write_failed");
        format!("Failed to write config: {}", e)
    })?;

    info!(path = ?config_path, "partial_config_saved");
    Ok(())
}

/// Apply environment variable overrides to config
fn apply_env_overrides(mut config: Config) -> Config {
    if let Ok(path) = env::var("RALPH_CLAUDE_PATH") {
        debug!("Overriding claude.path from RALPH_CLAUDE_PATH");
        config.claude.path = path;
    }

    if let Ok(level) = env::var("RALPH_LOG") {
        debug!("Overriding logging.level from RALPH_LOG");
        config.logging.level = level;
    }

    if let Ok(path) = env::var("RALPH_BD_PATH") {
        debug!("Overriding behavior.bd_path from RALPH_BD_PATH");
        config.behavior.bd_path = path;
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.claude.path, "~/.claude/local/claude");
        assert!(config.claude.args.is_none());
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = Config::expand_tilde("~/.config/test");
        assert!(!expanded.to_string_lossy().starts_with('~'));

        let no_tilde = Config::expand_tilde("/absolute/path");
        assert_eq!(no_tilde, PathBuf::from("/absolute/path"));

        let relative = Config::expand_tilde("./relative/path");
        assert_eq!(relative, PathBuf::from("./relative/path"));
    }

    #[test]
    fn test_config_deserialization() {
        let toml_str = r#"
[claude]
path = "/custom/claude"

[logging]
level = "debug"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
        assert!(config.claude.args.is_none());
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_config_partial_deserialization() {
        // Only claude section specified, others should use defaults
        let toml_str = r#"
[claude]
path = "/custom/claude"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
        // args should be None since not specified (legacy field)
        assert!(config.claude.args.is_none());
        // logging should be defaults
        assert_eq!(config.logging.level, "info");
    }

    #[test]
    fn test_unknown_keys_ignored() {
        let toml_str = r#"
[claude]
path = "/custom/claude"
unknown_key = "should be ignored"

[unknown_section]
foo = "bar"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
    }

    #[test]
    fn test_legacy_claude_args_ignored() {
        // Existing config files may have claude.args - ensure they still load
        let toml_str = r#"
[claude]
path = "/custom/claude"
args = "--output-format=stream-json --verbose"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
        // args is parsed but ignored (legacy field)
        assert_eq!(
            config.claude.args,
            Some("--output-format=stream-json --verbose".to_string())
        );
    }

    #[test]
    fn test_iterations_default() {
        let config = Config::default();
        assert_eq!(config.behavior.iterations, -1); // Infinite by default
    }

    #[test]
    fn test_iterations_explicit() {
        let toml_str = r#"
[behavior]
iterations = 5
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.behavior.iterations, 5);
    }

    #[test]
    fn test_keep_awake_default() {
        let config = Config::default();
        assert!(config.behavior.keep_awake); // Default: true
    }

    #[test]
    fn test_keep_awake_explicit() {
        let toml_str = r#"
[behavior]
keep_awake = false
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.behavior.keep_awake);
    }

    #[test]
    fn test_partial_config_empty() {
        let toml_str = "";
        let partial: PartialConfig = toml::from_str(toml_str).unwrap();
        assert!(partial.claude.path.is_none());
        assert!(partial.logging.level.is_none());
        assert!(partial.behavior.iterations.is_none());
        assert!(partial.behavior.keep_awake.is_none());
    }

    #[test]
    fn test_partial_config_some_fields() {
        let toml_str = r#"
[behavior]
iterations = 3
"#;

        let partial: PartialConfig = toml::from_str(toml_str).unwrap();
        assert!(partial.claude.path.is_none());
        assert_eq!(partial.behavior.iterations, Some(3));
        assert!(partial.behavior.keep_awake.is_none());
    }

    #[test]
    fn test_partial_config_unknown_keys_ignored() {
        let toml_str = r#"
[unknown_section]
foo = "bar"
"#;

        let _partial: PartialConfig = toml::from_str(toml_str).unwrap();
    }

    #[test]
    fn test_partial_config_comment_only() {
        let toml_str = "# Project-specific Ralph config — edit with config modal (c)\n";
        let partial: PartialConfig = toml::from_str(toml_str).unwrap();
        assert!(partial.claude.path.is_none());
    }

    #[test]
    fn test_merge_config_no_overrides() {
        let global = Config::default();
        let partial = PartialConfig::default();
        let merged = merge_config(&global, &partial);

        assert_eq!(merged.claude.path, global.claude.path);
        assert_eq!(merged.logging.level, global.logging.level);
        assert_eq!(merged.behavior.iterations, global.behavior.iterations);
        assert_eq!(merged.behavior.keep_awake, global.behavior.keep_awake);
    }

    #[test]
    fn test_merge_config_all_overrides() {
        let global = Config::default();
        let partial = PartialConfig {
            claude: PartialClaudeConfig {
                path: Some("/custom/claude".to_string()),
            },
            logging: PartialLoggingConfig {
                level: Some("debug".to_string()),
            },
            behavior: PartialBehaviorConfig {
                iterations: Some(5),
                keep_awake: Some(false),
                bd_path: None,
                heartbeat_interval: None,
                stale_threshold: None,
                workers: None,
            },
        };
        let merged = merge_config(&global, &partial);

        assert_eq!(merged.claude.path, "/custom/claude");
        assert_eq!(merged.logging.level, "debug");
        assert_eq!(merged.behavior.iterations, 5);
        assert!(!merged.behavior.keep_awake);
    }

    #[test]
    fn test_merge_config_partial_overrides() {
        let global = Config::default();
        let partial: PartialConfig = toml::from_str(
            r#"
[behavior]
iterations = 3
"#,
        )
        .unwrap();
        let merged = merge_config(&global, &partial);

        // Overridden fields
        assert_eq!(merged.behavior.iterations, 3);

        // Inherited fields
        assert_eq!(merged.claude.path, global.claude.path);
        assert_eq!(merged.logging.level, global.logging.level);
        assert_eq!(merged.behavior.keep_awake, global.behavior.keep_awake);
    }

    #[test]
    fn test_partial_config_serialize_empty() {
        let partial = PartialConfig::default();
        let toml_str = toml::to_string_pretty(&partial).unwrap();
        // Empty partial config should serialize to empty string (all sections skipped)
        assert_eq!(toml_str.trim(), "");
    }

    #[test]
    fn test_partial_config_serialize_some_fields() {
        let partial = PartialConfig {
            behavior: PartialBehaviorConfig {
                iterations: Some(3),
                ..Default::default()
            },
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&partial).unwrap();
        // Should contain only the set fields
        assert!(toml_str.contains("iterations = 3"));
        // Should not contain unset fields
        assert!(!toml_str.contains("claude"));
        assert!(!toml_str.contains("keep_awake"));
    }

    #[test]
    fn test_partial_config_serialize_roundtrip() {
        let partial = PartialConfig {
            claude: PartialClaudeConfig {
                path: Some("/custom/claude".to_string()),
            },
            logging: PartialLoggingConfig { level: None },
            behavior: PartialBehaviorConfig {
                iterations: Some(5),
                keep_awake: None,
                bd_path: None,
                heartbeat_interval: None,
                stale_threshold: None,
                workers: None,
            },
        };
        let toml_str = toml::to_string_pretty(&partial).unwrap();
        let deserialized: PartialConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(deserialized.claude.path, Some("/custom/claude".to_string()));
        assert!(deserialized.logging.level.is_none());
        assert_eq!(deserialized.behavior.iterations, Some(5));
        assert!(deserialized.behavior.keep_awake.is_none());
    }

    #[test]
    fn test_project_key_from_path() {
        let key = project_key_from_path(std::path::Path::new("/Users/me/code/foo"));
        assert_eq!(key, "-Users-me-code-foo");

        let key = project_key_from_path(std::path::Path::new("/"));
        assert_eq!(key, "-");
    }

    #[test]
    fn test_compute_project_config_path_structure() {
        // Verify the returned path has the expected structure
        if let Some(path) = compute_project_config_path() {
            let path_str = path.to_string_lossy();
            assert!(path_str.contains("projects"));
            assert!(path_str.ends_with("config.toml"));
        }
        // If None, config dir can't be determined (CI env) — that's ok
    }

    #[test]
    fn workers_default_is_one() {
        let config = BehaviorConfig::default();
        assert_eq!(config.workers, 1);
    }

    #[test]
    fn workers_parsed_from_toml() {
        let toml_str = r#"
[behavior]
workers = 4
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.behavior.workers, 4);
    }

    #[test]
    fn workers_explicit_values() {
        for n in [1, 2, 4, 8] {
            let toml_str = format!("[behavior]\nworkers = {}", n);
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.behavior.workers, n);
        }
    }

    #[test]
    fn workers_zero_clamped_to_one_on_merge() {
        let base = Config::default();
        let partial = PartialConfig {
            behavior: PartialBehaviorConfig {
                workers: Some(0),
                ..Default::default()
            },
            ..Default::default()
        };
        let merged = merge_config(&base, &partial);
        assert_eq!(merged.behavior.workers, 1);
    }

    #[test]
    fn workers_missing_defaults_to_one() {
        let toml_str = r#"
[behavior]
iterations = -1
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.behavior.workers, 1);
    }

    #[test]
    fn workers_merge_project_overrides_global() {
        let global = Config::default();
        let partial = PartialConfig {
            behavior: PartialBehaviorConfig {
                workers: Some(3),
                ..Default::default()
            },
            ..Default::default()
        };
        let merged = merge_config(&global, &partial);
        assert_eq!(merged.behavior.workers, 3);
    }

    #[test]
    fn workers_merge_project_none_inherits_global() {
        let mut global = Config::default();
        global.behavior.workers = 2;
        let partial = PartialConfig::default();
        let merged = merge_config(&global, &partial);
        assert_eq!(merged.behavior.workers, 2);
    }
}
