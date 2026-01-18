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
    /// Error occurred during loading, using defaults
    #[allow(dead_code)] // Will be used in later UI slices
    Error(String),
}

/// Claude CLI configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub path: String,
    pub args: String,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            path: "~/.claude/local/claude".to_string(),
            args: "--output-format=stream-json --verbose --print --include-partial-messages"
                .to_string(),
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

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
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

    /// Get the expanded prompt file path
    pub fn prompt_path(&self) -> PathBuf {
        Self::expand_tilde(&self.paths.prompt)
    }

    /// Get the expanded specs directory path
    #[allow(dead_code)] // Used by future current-spec-detection spec
    pub fn specs_path(&self) -> PathBuf {
        Self::expand_tilde(&self.paths.specs)
    }
}

/// Loaded configuration with metadata
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: Config,
    pub config_path: PathBuf,
    pub status: ConfigLoadStatus,
}

/// Get the platform-appropriate config directory
fn get_config_dir() -> Option<PathBuf> {
    ProjectDirs::from("dev", "cmoel", "ralph").map(|dirs| dirs.config_dir().to_path_buf())
}

/// Get the full path to the config file
pub fn get_config_path() -> Option<PathBuf> {
    get_config_dir().map(|dir| dir.join("config.toml"))
}

/// Ensure the config file exists, creating it with defaults if necessary.
/// Returns the path to the config file, or None if it couldn't be determined/created.
pub fn ensure_config_exists() -> Option<PathBuf> {
    let config_path = get_config_path()?;

    if config_path.exists() {
        return Some(config_path);
    }

    // Create default config
    let (_, status) = create_default_config(&config_path);
    match status {
        ConfigLoadStatus::Created | ConfigLoadStatus::Loaded => Some(config_path),
        ConfigLoadStatus::Error(_) => {
            // Even if we couldn't create it, return the path so the editor can show an error
            Some(config_path)
        }
    }
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
                status: ConfigLoadStatus::Error("Could not determine config directory".to_string()),
            };
        }
    };

    debug!("Config path: {:?}", config_path);

    let (config, status) = load_or_create_config(&config_path);
    let config = apply_env_overrides(config);

    LoadedConfig {
        config,
        config_path,
        status,
    }
}

/// Reload configuration from the given path.
/// Returns Ok(Config) if reload succeeded, or Err(String) with error message.
/// On error, the caller should keep the previous config.
pub fn reload_config(config_path: &PathBuf) -> Result<Config, String> {
    let contents = fs::read_to_string(config_path).map_err(|e| {
        warn!(path = ?config_path, error = %e, "config_reload_read_failed");
        format!("Failed to read config: {}", e)
    })?;

    let config = toml::from_str::<Config>(&contents).map_err(|e| {
        warn!(path = ?config_path, error = %e, "config_reload_parse_failed");
        format!("Invalid config: {}", e)
    })?;

    let config = apply_env_overrides(config);
    info!(path = ?config_path, "config_reloaded");
    Ok(config)
}

/// Load config from file, or create default if not exists
fn load_or_create_config(config_path: &PathBuf) -> (Config, ConfigLoadStatus) {
    match fs::read_to_string(config_path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(config) => {
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
        assert!(config.claude.args.contains("--output-format=stream-json"));
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
args = "--custom-args"

[paths]
prompt = "./custom-prompt.md"
specs = "./custom-specs"

[logging]
level = "debug"
"#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.claude.path, "/custom/claude");
        assert_eq!(config.claude.args, "--custom-args");
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
        // args should be default since not specified
        assert!(config.claude.args.contains("--output-format=stream-json"));
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
}
