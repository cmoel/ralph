//! Health check functions for the `ralph doctor` subcommand.

use std::process::Command;

use crate::config::{Config, ConfigLoadStatus, LoadedConfig};
use crate::work_source;

/// Result of a single health check.
pub struct CheckResult {
    pub passed: bool,
    pub message: String,
}

impl CheckResult {
    fn pass(message: impl Into<String>) -> Self {
        Self {
            passed: true,
            message: message.into(),
        }
    }

    fn fail(message: impl Into<String>) -> Self {
        Self {
            passed: false,
            message: message.into(),
        }
    }
}

/// Check that the config file was loaded successfully.
pub fn check_config(loaded: &LoadedConfig) -> CheckResult {
    match &loaded.status {
        ConfigLoadStatus::Loaded => CheckResult::pass("Config loaded"),
        ConfigLoadStatus::Created => CheckResult::pass(format!(
            "Config created at {}",
            loaded.config_path.display()
        )),
        ConfigLoadStatus::Error(e) => CheckResult::fail(format!(
            "Config error: {} — check {}",
            e,
            loaded.config_path.display()
        )),
    }
}

/// Check that the Claude CLI binary works.
pub fn check_claude(config: &Config) -> CheckResult {
    let path = config.claude_path();
    match Command::new(&path).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            CheckResult::pass(format!("Claude CLI ({})", version.trim()))
        }
        Ok(_) => CheckResult::fail(format!("Claude CLI failed — check {}", path.display())),
        Err(_) => CheckResult::fail(
            "Claude CLI not found — install from https://claude.ai/download or set claude.path in config",
        ),
    }
}

/// Check that the bd CLI binary works (beads mode only).
pub fn check_bd(config: &Config) -> CheckResult {
    let path = &config.behavior.bd_path;
    match Command::new(path).arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            CheckResult::pass(format!("Beads CLI ({})", version.trim()))
        }
        Ok(_) => CheckResult::fail(format!(
            "Beads CLI failed — check bd_path in config (currently: {})",
            path
        )),
        Err(_) => CheckResult::fail(
            "Beads CLI not found — install beads or set behavior.bd_path in config",
        ),
    }
}

/// Check that PROMPT.md exists.
pub fn check_prompt(config: &Config) -> CheckResult {
    let path = config.prompt_path();
    if path.exists() {
        CheckResult::pass("PROMPT.md found")
    } else {
        CheckResult::fail("PROMPT.md not found — run: ralph init")
    }
}

/// Check that the Dolt server is running (beads mode only).
pub fn check_dolt_status(config: &Config) -> CheckResult {
    let path = &config.behavior.bd_path;
    match Command::new(path).args(["dolt", "status"]).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("server: running") {
                CheckResult::pass("Dolt server running")
            } else {
                CheckResult::fail(
                    "Dolt server not running — press D to start it in ralph",
                )
            }
        }
        Ok(_) => CheckResult::fail(
            "Dolt server not running — press D to start it in ralph",
        ),
        Err(_) => CheckResult::fail("Could not check Dolt status"),
    }
}

/// Check that work items exist.
pub fn check_work_items(config: &Config) -> CheckResult {
    let source = work_source::create_work_source(
        &config.behavior.mode,
        config.specs_path(),
        &config.behavior.bd_path,
    );

    match source.list_items() {
        Ok(items) if !items.is_empty() => {
            CheckResult::pass(format!("{} work item(s) found", items.len()))
        }
        Ok(_) => {
            let fix = match config.behavior.mode.as_str() {
                "beads" => "create beads with: bd create --title=\"...\"",
                _ => "add specs to your specs directory",
            };
            CheckResult::fail(format!("No work items found — {}", fix))
        }
        Err(e) => CheckResult::fail(format!("Could not list work items: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigLoadStatus;
    use std::path::PathBuf;

    fn default_loaded_config(status: ConfigLoadStatus) -> LoadedConfig {
        LoadedConfig {
            config: Config::default(),
            config_path: PathBuf::from("/tmp/config.toml"),
            project_config_path: None,
            status,
        }
    }

    #[test]
    fn check_config_passes_when_loaded() {
        let loaded = default_loaded_config(ConfigLoadStatus::Loaded);
        let result = check_config(&loaded);
        assert!(result.passed);
        assert!(result.message.contains("Config loaded"));
    }

    #[test]
    fn check_config_passes_when_created() {
        let loaded = default_loaded_config(ConfigLoadStatus::Created);
        let result = check_config(&loaded);
        assert!(result.passed);
        assert!(result.message.contains("Config created"));
    }

    #[test]
    fn check_config_fails_on_error() {
        let loaded = default_loaded_config(ConfigLoadStatus::Error("bad toml".to_string()));
        let result = check_config(&loaded);
        assert!(!result.passed);
        assert!(result.message.contains("bad toml"));
    }

    #[test]
    fn check_prompt_fails_when_missing() {
        let mut config = Config::default();
        config.paths.prompt = "/nonexistent/PROMPT.md".to_string();
        let result = check_prompt(&config);
        assert!(!result.passed);
        assert!(result.message.contains("ralph init"));
    }

    #[test]
    fn check_result_pass_sets_fields() {
        let r = CheckResult::pass("ok");
        assert!(r.passed);
        assert_eq!(r.message, "ok");
    }

    #[test]
    fn check_result_fail_sets_fields() {
        let r = CheckResult::fail("bad");
        assert!(!r.passed);
        assert_eq!(r.message, "bad");
    }
}
