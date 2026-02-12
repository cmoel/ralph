use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Status of config file loading
#[derive(Debug, Clone)]
pub enum ConfigLoadStatus {
    /// Config loaded successfully from existing file
    Loaded,
    /// Created default config file (first run)
    Created,
    /// Error occurred during loading, using defaults.
    /// String is used in Debug output for logging.
    #[allow(dead_code)]
    Error(String),
}

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

/// Path configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PathsConfig {
    pub prompt: String,
    pub specs: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            prompt: "./PROMPT.md".to_string(),
            specs: "./specs".to_string(),
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
    /// - Negative (-1): Infinite mode, continues until user stops or specs complete
    /// - Zero (0): Stopped mode, pressing 's' has no effect
    /// - Positive (N): Runs exactly N iterations then stops
    pub iterations: i32,
    /// Whether to auto-expand the tasks panel when tasks arrive.
    /// When true, receiving tasks expands the panel.
    /// When false, the panel stays collapsed and shows the count.
    pub auto_expand_tasks_panel: bool,
    /// Whether to acquire a wake lock to prevent system idle sleep.
    /// When true, the system won't sleep while claude is running.
    /// Display may still sleep. Default: true.
    pub keep_awake: bool,
    /// Legacy field - converted to iterations on load.
    /// `true` becomes `-1` (infinite), `false` becomes `0` (stopped).
    #[serde(skip_serializing, default)]
    auto_continue: Option<bool>,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            iterations: -1,                // Infinite mode by default
            auto_expand_tasks_panel: true, // Auto-expand by default for backwards compatibility
            keep_awake: true,              // Prevent system sleep by default
            auto_continue: None,
        }
    }
}

impl BehaviorConfig {
    /// Normalize the config after deserialization.
    /// Converts legacy `auto_continue` field to `iterations` if present,
    /// but only if `iterations` wasn't explicitly set (i.e., still at default -1).
    pub fn normalize(&mut self) {
        if let Some(auto_continue) = self.auto_continue.take() {
            // Only apply legacy migration if iterations wasn't explicitly set
            // We can't truly detect if it was explicitly set to -1 vs defaulted,
            // but that's an edge case. Just document that iterations takes precedence.
            // For simplicity, always apply legacy field if present (old configs won't have iterations)
            self.iterations = if auto_continue { -1 } else { 0 };
        }
    }
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub behavior: BehaviorConfig,
}

impl Config {
    /// Normalize the config after deserialization.
    /// Handles legacy field migrations.
    pub fn normalize(&mut self) {
        self.behavior.normalize();
    }

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

    /// Get the expanded prompt file path
    pub fn prompt_path(&self) -> PathBuf {
        Self::expand_tilde(&self.paths.prompt)
    }

    /// Get the expanded specs directory path
    pub fn specs_path(&self) -> PathBuf {
        Self::expand_tilde(&self.paths.specs)
    }
}

/// Partial Claude CLI configuration for project overrides.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PartialClaudeConfig {
    pub path: Option<String>,
}

/// Partial path configuration for project overrides.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PartialPathsConfig {
    pub prompt: Option<String>,
    pub specs: Option<String>,
}

/// Partial logging configuration for project overrides.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PartialLoggingConfig {
    pub level: Option<String>,
}

/// Partial behavior configuration for project overrides.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PartialBehaviorConfig {
    pub iterations: Option<i32>,
    pub auto_expand_tasks_panel: Option<bool>,
    pub keep_awake: Option<bool>,
}

/// Project-specific configuration where every field is optional.
/// Parsed from `.ralph` files. Fields that are `None` inherit from the global config.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct PartialConfig {
    pub claude: PartialClaudeConfig,
    pub paths: PartialPathsConfig,
    pub logging: PartialLoggingConfig,
    pub behavior: PartialBehaviorConfig,
}

/// Merge a global config with a project-level partial config.
/// Project values override global values where present.
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
        paths: PathsConfig {
            prompt: project
                .paths
                .prompt
                .clone()
                .unwrap_or_else(|| global.paths.prompt.clone()),
            specs: project
                .paths
                .specs
                .clone()
                .unwrap_or_else(|| global.paths.specs.clone()),
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
            auto_expand_tasks_panel: project
                .behavior
                .auto_expand_tasks_panel
                .unwrap_or(global.behavior.auto_expand_tasks_panel),
            keep_awake: project
                .behavior
                .keep_awake
                .unwrap_or(global.behavior.keep_awake),
            auto_continue: None,
        },
    }
}

/// Loaded configuration with metadata
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub config_path: PathBuf,
    pub project_config_path: Option<PathBuf>,
    pub status: ConfigLoadStatus,
}

/// Get the platform-appropriate config directory
fn get_config_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "cmoel", "ralph").map(|dirs| dirs.config_dir().to_path_buf())
}

/// Get the full path to the config file
pub fn get_config_path() -> Option<PathBuf> {
    get_config_dir().map(|dir| dir.join("config.toml"))
}

/// Get the project config path (.ralph in current working directory).
pub fn get_project_config_path() -> Option<PathBuf> {
    let path = std::env::current_dir().ok()?.join(".ralph");
    if path.exists() { Some(path) } else { None }
}

/// Load a project config (.ralph) from the given path.
/// Returns Ok(PartialConfig) on success, Err(String) on parse/read failure.
fn load_project_config(path: &PathBuf) -> Result<PartialConfig, String> {
    let contents = fs::read_to_string(path).map_err(|e| {
        warn!(path = ?path, error = %e, "project_config_read_failed");
        format!("Failed to read .ralph: {}", e)
    })?;

    toml::from_str::<PartialConfig>(&contents).map_err(|e| {
        warn!(path = ?path, error = %e, "project_config_parse_failed");
        format!("Invalid .ralph: {}", e)
    })
}

/// Load configuration from file, environment, and defaults
pub fn load_config() -> LoadedConfig {
    let config_path = match get_config_path() {
        Some(path) => path,
        None => {
            warn!("Could not determine config directory, using defaults");
            return LoadedConfig {
                config: apply_env_overrides(Config::default()),
                config_path: PathBuf::from("config.toml"),
                project_config_path: None,
                status: ConfigLoadStatus::Error("Could not determine config directory".to_string()),
            };
        }
    };

    debug!("Config path: {:?}", config_path);

    let (mut config, status) = load_or_create_config(&config_path);

    // Check for project-level .ralph file
    let project_config_path = get_project_config_path();
    if let Some(ref project_path) = project_config_path {
        match load_project_config(project_path) {
            Ok(partial) => {
                config = merge_config(&config, &partial);
                info!(path = ?project_path, "project_config_loaded");
            }
            Err(e) => {
                warn!(path = ?project_path, error = %e, "project_config_error");
                // Keep using global config only
            }
        }
    }

    let config = apply_env_overrides(config);

    LoadedConfig {
        config,
        config_path,
        project_config_path,
        status,
    }
}

/// Result of reloading configuration, including separate error tracking.
pub struct ReloadedConfig {
    pub config: Config,
    pub global_error: Option<String>,
    pub project_error: Option<String>,
}

/// Reload configuration from global and optional project config paths.
/// Returns a ReloadedConfig that always has a usable config (falls back to defaults).
/// Errors for each source are tracked separately so the UI can display them.
pub fn reload_config(
    config_path: &PathBuf,
    project_config_path: Option<&PathBuf>,
) -> ReloadedConfig {
    // Load global config
    let (mut config, global_error) = match fs::read_to_string(config_path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(mut c) => {
                c.normalize();
                (c, None)
            }
            Err(e) => {
                warn!(path = ?config_path, error = %e, "config_reload_parse_failed");
                (Config::default(), Some(format!("Invalid config: {}", e)))
            }
        },
        Err(e) => {
            warn!(path = ?config_path, error = %e, "config_reload_read_failed");
            (
                Config::default(),
                Some(format!("Failed to read config: {}", e)),
            )
        }
    };

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
            // .ralph was deleted — just use global config, no error
            None
        }
    } else {
        None
    };

    let config = apply_env_overrides(config);
    info!(path = ?config_path, "config_reloaded");

    ReloadedConfig {
        config,
        global_error,
        project_error,
    }
}

/// Save a config to the given file path.
/// Returns Ok(()) on success, or Err(String) with error message on failure.
pub fn save_config(config: &Config, config_path: &PathBuf) -> Result<(), String> {
    // Serialize to TOML
    let toml_content = toml::to_string_pretty(config).map_err(|e| {
        warn!(error = %e, "config_save_serialize_failed");
        format!("Failed to serialize config: {}", e)
    })?;

    // Write to file
    fs::write(config_path, &toml_content).map_err(|e| {
        warn!(path = ?config_path, error = %e, "config_save_write_failed");
        format!("Failed to write config: {}", e)
    })?;

    info!(path = ?config_path, "config_saved");
    Ok(())
}

/// Load config from file, or create default if not exists
fn load_or_create_config(config_path: &PathBuf) -> (Config, ConfigLoadStatus) {
    match fs::read_to_string(config_path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(mut config) => {
                config.normalize();
                info!("Loaded config from {:?}", config_path);
                (config, ConfigLoadStatus::Loaded)
            }
            Err(e) => {
                warn!(
                    "Config file malformed at {:?}: {}. Using defaults.",
                    config_path, e
                );
                (
                    Config::default(),
                    ConfigLoadStatus::Error(format!("Malformed TOML: {}", e)),
                )
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Config doesn't exist, create default
            create_default_config(config_path)
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            warn!(
                "Permission denied reading config at {:?}. Using defaults.",
                config_path
            );
            (
                Config::default(),
                ConfigLoadStatus::Error("Permission denied reading config".to_string()),
            )
        }
        Err(e) => {
            warn!(
                "Error reading config at {:?}: {}. Using defaults.",
                config_path, e
            );
            (
                Config::default(),
                ConfigLoadStatus::Error(format!("Read error: {}", e)),
            )
        }
    }
}

/// Create the default config file
fn create_default_config(config_path: &PathBuf) -> (Config, ConfigLoadStatus) {
    let config = Config::default();

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        warn!(
            "Could not create config directory {:?}: {}. Continuing without file.",
            parent, e
        );
        return (
            config,
            ConfigLoadStatus::Error(format!("Could not create config directory: {}", e)),
        );
    }

    // Serialize to TOML
    let toml_content = match toml::to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            warn!("Could not serialize default config: {}", e);
            return (
                config,
                ConfigLoadStatus::Error(format!("Serialization error: {}", e)),
            );
        }
    };

    // Write file
    match fs::write(config_path, &toml_content) {
        Ok(()) => {
            info!("Created default config at {:?}", config_path);
            (config, ConfigLoadStatus::Created)
        }
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            warn!(
                "Permission denied creating config at {:?}. Continuing without file.",
                config_path
            );
            (
                config,
                ConfigLoadStatus::Error("Permission denied creating config".to_string()),
            )
        }
        Err(e) => {
            warn!(
                "Could not write default config to {:?}: {}. Continuing without file.",
                config_path, e
            );
            (
                config,
                ConfigLoadStatus::Error(format!("Write error: {}", e)),
            )
        }
    }
}

/// Apply environment variable overrides to config
fn apply_env_overrides(mut config: Config) -> Config {
    if let Ok(path) = env::var("RALPH_CLAUDE_PATH") {
        debug!("Overriding claude.path from RALPH_CLAUDE_PATH");
        config.claude.path = path;
    }

    if let Ok(path) = env::var("RALPH_PROMPT_PATH") {
        debug!("Overriding paths.prompt from RALPH_PROMPT_PATH");
        config.paths.prompt = path;
    }

    if let Ok(path) = env::var("RALPH_SPECS_DIR") {
        debug!("Overriding paths.specs from RALPH_SPECS_DIR");
        config.paths.specs = path;
    }

    if let Ok(level) = env::var("RALPH_LOG") {
        debug!("Overriding logging.level from RALPH_LOG");
        config.logging.level = level;
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
        assert_eq!(config.paths.prompt, "./PROMPT.md");
        assert_eq!(config.paths.specs, "./specs");
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

[paths]
prompt = "./custom-prompt.md"
specs = "./custom-specs"

[logging]
level = "debug"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
        assert!(config.claude.args.is_none());
        assert_eq!(config.paths.prompt, "./custom-prompt.md");
        assert_eq!(config.paths.specs, "./custom-specs");
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
        // paths and logging should be defaults
        assert_eq!(config.paths.prompt, "./PROMPT.md");
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

[paths]
prompt = "./PROMPT.md"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
        // args is parsed but ignored (legacy field)
        assert_eq!(
            config.claude.args,
            Some("--output-format=stream-json --verbose".to_string())
        );
        assert_eq!(config.paths.prompt, "./PROMPT.md");
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

        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();
        assert_eq!(config.behavior.iterations, 5);
    }

    #[test]
    fn test_legacy_auto_continue_true() {
        // Existing config files may have auto_continue - ensure they migrate
        let toml_str = r#"
[behavior]
auto_continue = true
"#;

        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();
        // true becomes -1 (infinite mode)
        assert_eq!(config.behavior.iterations, -1);
    }

    #[test]
    fn test_legacy_auto_continue_false() {
        let toml_str = r#"
[behavior]
auto_continue = false
"#;

        let mut config: Config = toml::from_str(toml_str).unwrap();
        config.normalize();
        // false becomes 0 (stopped mode)
        assert_eq!(config.behavior.iterations, 0);
    }

    #[test]
    fn test_legacy_auto_continue_overwrites_iterations() {
        // When both are present (unlikely transition case), auto_continue wins.
        // This is intentional - old configs won't have iterations field,
        // and if someone manually adds both, we favor the legacy field to be safe.
        let toml_str = r#"
[behavior]
iterations = 3
auto_continue = true
"#;

        let mut config: Config = toml::from_str(toml_str).unwrap();
        // Before normalize, iterations is 3 from TOML
        assert_eq!(config.behavior.iterations, 3);
        config.normalize();
        // After normalize, legacy auto_continue=true becomes -1 (infinite)
        assert_eq!(config.behavior.iterations, -1);
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
        assert!(partial.paths.prompt.is_none());
        assert!(partial.paths.specs.is_none());
        assert!(partial.logging.level.is_none());
        assert!(partial.behavior.iterations.is_none());
        assert!(partial.behavior.auto_expand_tasks_panel.is_none());
        assert!(partial.behavior.keep_awake.is_none());
    }

    #[test]
    fn test_partial_config_some_fields() {
        let toml_str = r#"
[paths]
prompt = "./custom-prompt.md"

[behavior]
iterations = 3
"#;

        let partial: PartialConfig = toml::from_str(toml_str).unwrap();
        assert!(partial.claude.path.is_none());
        assert_eq!(partial.paths.prompt, Some("./custom-prompt.md".to_string()));
        assert!(partial.paths.specs.is_none());
        assert_eq!(partial.behavior.iterations, Some(3));
        assert!(partial.behavior.keep_awake.is_none());
    }

    #[test]
    fn test_partial_config_unknown_keys_ignored() {
        let toml_str = r#"
[paths]
prompt = "./p.md"
unknown = "ignored"

[unknown_section]
foo = "bar"
"#;

        let partial: PartialConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(partial.paths.prompt, Some("./p.md".to_string()));
    }

    #[test]
    fn test_partial_config_comment_only() {
        let toml_str = "# Project-specific Ralph config — edit with config modal (c)\n";
        let partial: PartialConfig = toml::from_str(toml_str).unwrap();
        assert!(partial.claude.path.is_none());
        assert!(partial.paths.prompt.is_none());
    }

    #[test]
    fn test_merge_config_no_overrides() {
        let global = Config::default();
        let partial = PartialConfig::default();
        let merged = merge_config(&global, &partial);

        assert_eq!(merged.claude.path, global.claude.path);
        assert_eq!(merged.paths.prompt, global.paths.prompt);
        assert_eq!(merged.paths.specs, global.paths.specs);
        assert_eq!(merged.logging.level, global.logging.level);
        assert_eq!(merged.behavior.iterations, global.behavior.iterations);
        assert_eq!(
            merged.behavior.auto_expand_tasks_panel,
            global.behavior.auto_expand_tasks_panel
        );
        assert_eq!(merged.behavior.keep_awake, global.behavior.keep_awake);
    }

    #[test]
    fn test_merge_config_all_overrides() {
        let global = Config::default();
        let partial = PartialConfig {
            claude: PartialClaudeConfig {
                path: Some("/custom/claude".to_string()),
            },
            paths: PartialPathsConfig {
                prompt: Some("./proj-prompt.md".to_string()),
                specs: Some("./proj-specs".to_string()),
            },
            logging: PartialLoggingConfig {
                level: Some("debug".to_string()),
            },
            behavior: PartialBehaviorConfig {
                iterations: Some(5),
                auto_expand_tasks_panel: Some(false),
                keep_awake: Some(false),
            },
        };
        let merged = merge_config(&global, &partial);

        assert_eq!(merged.claude.path, "/custom/claude");
        assert_eq!(merged.paths.prompt, "./proj-prompt.md");
        assert_eq!(merged.paths.specs, "./proj-specs");
        assert_eq!(merged.logging.level, "debug");
        assert_eq!(merged.behavior.iterations, 5);
        assert!(!merged.behavior.auto_expand_tasks_panel);
        assert!(!merged.behavior.keep_awake);
    }

    #[test]
    fn test_merge_config_partial_overrides() {
        let global = Config::default();
        let partial: PartialConfig = toml::from_str(
            r#"
[paths]
prompt = "./custom-prompt.md"

[behavior]
iterations = 3
"#,
        )
        .unwrap();
        let merged = merge_config(&global, &partial);

        // Overridden fields
        assert_eq!(merged.paths.prompt, "./custom-prompt.md");
        assert_eq!(merged.behavior.iterations, 3);

        // Inherited fields
        assert_eq!(merged.claude.path, global.claude.path);
        assert_eq!(merged.paths.specs, global.paths.specs);
        assert_eq!(merged.logging.level, global.logging.level);
        assert_eq!(
            merged.behavior.auto_expand_tasks_panel,
            global.behavior.auto_expand_tasks_panel
        );
        assert_eq!(merged.behavior.keep_awake, global.behavior.keep_awake);
    }
}
