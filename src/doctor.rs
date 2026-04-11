//! Health check functions for the `ralph doctor` subcommand.

use std::process::Command;

use crate::config::{Config, LoadedConfig};
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

/// Check that config loaded successfully.
pub fn check_config(loaded: &LoadedConfig) -> CheckResult {
    match &loaded.project_config_path {
        Some(path) => CheckResult::pass(format!("Config loaded (project: {})", path.display())),
        None => CheckResult::pass("Config loaded (compiled-in defaults)"),
    }
}

/// Check that the Claude CLI binary works.
pub fn check_claude(config: &Config) -> CheckResult {
    let path = config.claude_path();
    match crate::bd_lock::with_lock(|| Command::new(&path).arg("--version").output()) {
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

/// Check that the bd CLI binary works.
pub fn check_bd(config: &Config) -> CheckResult {
    let path = &config.behavior.bd_path;
    match crate::bd_lock::with_lock(|| Command::new(path).arg("--version").output()) {
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

/// Check prompt resolution — per-project PROMPT.md or compiled-in default.
pub fn check_prompt(_config: &Config) -> CheckResult {
    if crate::config::resolve_prompt_path().is_some() {
        CheckResult::pass("PROMPT.md found (per-project override)")
    } else {
        CheckResult::pass("Using compiled-in PROMPT.md")
    }
}

/// Check that work items exist.
pub fn check_work_items(config: &Config) -> CheckResult {
    let source = work_source::BeadsWorkSource::new(config.behavior.bd_path.clone());

    match source.list_items() {
        Ok(items) if !items.is_empty() => {
            CheckResult::pass(format!("{} work item(s) found", items.len()))
        }
        Ok(_) => CheckResult::fail(
            "No work items found — create beads with: bd create --title=\"...\"".to_string(),
        ),
        Err(e) => CheckResult::fail(format!("Could not list work items: {}", e)),
    }
}

/// Check that a SessionStart hook running `bd prime` is installed in Claude Code settings.
pub fn check_bd_prime_hook() -> CheckResult {
    let paths = bd_prime_hook_search_paths();
    for path in &paths {
        if let Ok(contents) = std::fs::read_to_string(path)
            && settings_has_bd_prime_hook(&contents)
        {
            return CheckResult::pass("bd prime SessionStart hook installed");
        }
    }
    CheckResult::fail(
        "bd prime SessionStart hook not found — add to ~/.claude/settings.json:\n    \
         \"hooks\": { \"SessionStart\": [{ \"matcher\": \"\", \
         \"hooks\": [{ \"type\": \"command\", \"command\": \"bd prime\" }] }] }",
    )
}

fn bd_prime_hook_search_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        paths.push(home.join(".claude").join("settings.json"));
    }
    paths.push(std::path::PathBuf::from(".claude/settings.json"));
    paths.push(std::path::PathBuf::from(".claude/settings.local.json"));
    paths
}

fn settings_has_bd_prime_hook(contents: &str) -> bool {
    let json: serde_json::Value = match serde_json::from_str(contents) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let session_start = match json.get("hooks").and_then(|h| h.get("SessionStart")) {
        Some(v) => v,
        None => return false,
    };

    let entries = match session_start.as_array() {
        Some(a) => a,
        None => return false,
    };

    for entry in entries {
        if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
            for hook in hooks {
                if let Some(cmd) = hook.get("command").and_then(|c| c.as_str())
                    && cmd == "bd prime"
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Check that the `bd` retry hook is fully installed: both scripts exist and
/// the PreToolUse entry is registered in `.claude/settings.json`.
pub fn check_bd_retry_hook() -> CheckResult {
    let retry = std::path::Path::new("scripts/bd-retry.sh");
    let intercept = std::path::Path::new("scripts/intercept-bd.sh");
    if !retry.exists() || !intercept.exists() {
        return CheckResult::fail("bd retry hook scripts missing — run `ralph init` to install");
    }

    let settings_path = std::path::PathBuf::from(".claude/settings.json");
    let contents = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::fail(
                "bd retry hook not registered — run `ralph init` to register in .claude/settings.json",
            );
        }
    };

    if settings_has_intercept_bd_hook(&contents) {
        CheckResult::pass("bd retry hook installed (scripts + PreToolUse registration)")
    } else {
        CheckResult::fail(
            "bd retry hook not registered — run `ralph init` to register in .claude/settings.json",
        )
    }
}

fn settings_has_intercept_bd_hook(contents: &str) -> bool {
    let json: serde_json::Value = match serde_json::from_str(contents) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let pre_tool_use = match json.get("hooks").and_then(|h| h.get("PreToolUse")) {
        Some(v) => v,
        None => return false,
    };

    let entries = match pre_tool_use.as_array() {
        Some(a) => a,
        None => return false,
    };

    for entry in entries {
        if let Some(hooks) = entry.get("hooks").and_then(|h| h.as_array()) {
            for hook in hooks {
                if let Some(cmd) = hook.get("command").and_then(|c| c.as_str())
                    && cmd.contains("intercept-bd.sh")
                {
                    return true;
                }
            }
        }
    }

    false
}

/// Check that scaffolded skill files are present and match compiled-in templates.
pub fn check_scaffolding_drift(config: &Config) -> CheckResult {
    let state = crate::modals::InitModalState::new(config);
    if state.all_up_to_date() {
        CheckResult::pass("Scaffolded skills up to date")
    } else {
        let missing = state.create_count();
        let drifted = state.regenerate_count();
        let msg = match (missing > 0, drifted > 0) {
            (true, false) => format!("{missing} skill file(s) not installed — run `ralph init`"),
            (false, true) => {
                format!("{drifted} skill file(s) have updates — run `ralph init` to refresh")
            }
            _ => format!("{missing} missing, {drifted} drifted — run `ralph init`"),
        };
        CheckResult::fail(msg)
    }
}

/// Check board column TOML validity.
///
/// If a per-project `board_columns.toml` exists, validates it and reports the path.
/// Otherwise validates the compiled-in default.
pub fn check_board_toml() -> CheckResult {
    if let Some(path) = crate::config::resolve_board_columns_path() {
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<crate::modals::BoardConfig>(&contents) {
                Ok(config) => {
                    if config.columns.is_empty() {
                        return CheckResult::fail(format!(
                            "Custom board TOML has no columns ({})",
                            path.display()
                        ));
                    }
                    CheckResult::pass(format!(
                        "Custom board TOML is valid ({}, {} columns)",
                        path.display(),
                        config.columns.len()
                    ))
                }
                Err(e) => CheckResult::fail(format!(
                    "Custom board TOML is invalid ({}): {e}",
                    path.display()
                )),
            },
            Err(e) => CheckResult::fail(format!(
                "Cannot read custom board TOML ({}): {e}",
                path.display()
            )),
        }
    } else {
        match toml::from_str::<crate::modals::BoardConfig>(include_str!(
            "modals/board_columns.toml"
        )) {
            Ok(_) => CheckResult::pass("Board column TOML is valid (compiled-in default)"),
            Err(e) => CheckResult::fail(format!("Compiled-in board column TOML is invalid: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn check_config_passes_with_defaults() {
        let loaded = LoadedConfig {
            config: Config::default(),
            project_config_path: None,
        };
        let result = check_config(&loaded);
        assert!(result.passed);
        assert!(result.message.contains("compiled-in defaults"));
    }

    #[test]
    fn check_config_passes_with_project() {
        let loaded = LoadedConfig {
            config: Config::default(),
            project_config_path: Some(PathBuf::from("/tmp/project/config.toml")),
        };
        let result = check_config(&loaded);
        assert!(result.passed);
        assert!(result.message.contains("project"));
    }

    #[test]
    fn check_prompt_always_passes() {
        let config = Config::default();
        let result = check_prompt(&config);
        assert!(result.passed);
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

    #[test]
    fn bd_prime_hook_detected_in_settings() {
        let settings = r#"{
            "hooks": {
                "SessionStart": [{
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "bd prime"}]
                }]
            }
        }"#;
        assert!(settings_has_bd_prime_hook(settings));
    }

    #[test]
    fn bd_prime_hook_missing_when_no_hooks() {
        let settings = r#"{"permissions": {"allow": []}}"#;
        assert!(!settings_has_bd_prime_hook(settings));
    }

    #[test]
    fn bd_prime_hook_missing_when_different_command() {
        let settings = r#"{
            "hooks": {
                "SessionStart": [{
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "echo hello"}]
                }]
            }
        }"#;
        assert!(!settings_has_bd_prime_hook(settings));
    }

    #[test]
    fn bd_prime_hook_missing_when_no_session_start() {
        let settings = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "bd prime"}]
                }]
            }
        }"#;
        assert!(!settings_has_bd_prime_hook(settings));
    }

    #[test]
    fn bd_prime_hook_detected_among_multiple_hooks() {
        let settings = r#"{
            "hooks": {
                "SessionStart": [{
                    "matcher": "",
                    "hooks": [
                        {"type": "command", "command": "echo warmup"},
                        {"type": "command", "command": "bd prime"}
                    ]
                }]
            }
        }"#;
        assert!(settings_has_bd_prime_hook(settings));
    }

    #[test]
    fn bd_prime_hook_handles_invalid_json() {
        assert!(!settings_has_bd_prime_hook("not json"));
    }

    #[test]
    fn intercept_bd_hook_detected_in_settings() {
        let settings = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{
                        "type": "command",
                        "command": "\"$CLAUDE_PROJECT_DIR\"/scripts/intercept-bd.sh"
                    }]
                }]
            }
        }"#;
        assert!(settings_has_intercept_bd_hook(settings));
    }

    #[test]
    fn intercept_bd_hook_missing_when_no_hooks() {
        let settings = r#"{"permissions": {"allow": []}}"#;
        assert!(!settings_has_intercept_bd_hook(settings));
    }

    #[test]
    fn intercept_bd_hook_detected_among_multiple_hooks() {
        let settings = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [
                        {"type": "command", "command": "\"$CLAUDE_PROJECT_DIR\"/scripts/intercept-build.sh"},
                        {"type": "command", "command": "\"$CLAUDE_PROJECT_DIR\"/scripts/intercept-bd.sh"}
                    ]
                }]
            }
        }"#;
        assert!(settings_has_intercept_bd_hook(settings));
    }

    #[test]
    fn intercept_bd_hook_missing_when_only_build_intercept() {
        let settings = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "\"$CLAUDE_PROJECT_DIR\"/scripts/intercept-build.sh"}]
                }]
            }
        }"#;
        assert!(!settings_has_intercept_bd_hook(settings));
    }

    #[test]
    fn intercept_bd_hook_handles_invalid_json() {
        assert!(!settings_has_intercept_bd_hook("not json"));
    }

    #[test]
    fn board_toml_check_passes_for_embedded_toml() {
        let result = check_board_toml();
        assert!(result.passed);
        assert!(result.message.contains("valid"));
        // When no external file, should report compiled-in default
        assert!(result.message.contains("compiled-in default"));
    }

    #[test]
    fn scaffolding_drift_returns_valid_result() {
        let config = Config::default();
        let result = check_scaffolding_drift(&config);
        // Whether it passes or fails depends on local file state,
        // but it should always produce a non-empty message
        assert!(!result.message.is_empty());
    }
}
