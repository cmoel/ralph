//! Manage Claude tool permissions across settings files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The permissions block inside a Claude settings file.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
    #[serde(flatten)]
    other: serde_json::Value,
}

/// A Claude settings file (preserves unknown top-level fields).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SettingsFile {
    #[serde(default)]
    pub permissions: Option<Permissions>,
    #[serde(flatten)]
    other: serde_json::Value,
}

/// Where a settings file lives in the chain.
#[derive(Debug, Clone, Copy)]
pub enum SettingsLevel {
    User,
    Project,
    Local,
}

impl std::fmt::Display for SettingsLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsLevel::User => write!(f, "~/.claude/settings.json"),
            SettingsLevel::Project => write!(f, ".claude/settings.json"),
            SettingsLevel::Local => write!(f, ".claude/settings.local.json"),
        }
    }
}

/// A tool rule with its source file.
pub struct ToolRule {
    pub pattern: String,
    pub kind: RuleKind,
    pub source: SettingsLevel,
}

#[derive(Debug, Clone, Copy)]
pub enum RuleKind {
    Allow,
    Deny,
}

impl std::fmt::Display for RuleKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleKind::Allow => write!(f, "allow"),
            RuleKind::Deny => write!(f, "deny"),
        }
    }
}

/// Resolve the path for a settings level.
fn settings_path(level: SettingsLevel) -> Result<PathBuf> {
    match level {
        SettingsLevel::User => {
            let home = std::env::var("HOME").context("HOME not set")?;
            Ok(PathBuf::from(home).join(".claude").join("settings.json"))
        }
        SettingsLevel::Project => Ok(PathBuf::from(".claude").join("settings.json")),
        SettingsLevel::Local => Ok(PathBuf::from(".claude").join("settings.local.json")),
    }
}

/// Read and parse a settings file. Returns None if the file doesn't exist.
fn read_settings(path: &Path) -> Result<Option<SettingsFile>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            let settings: SettingsFile = serde_json::from_str(&contents)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
            Ok(Some(settings))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Failed to read {}", path.display())),
    }
}

/// Write a settings file back to disk with pretty formatting.
fn write_settings(path: &Path, settings: &SettingsFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(settings).context("Failed to serialize settings")?;
    std::fs::write(path, format!("{json}\n"))
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Add a pattern to the allow list in the given settings file.
pub fn allow_pattern(pattern: &str, project: bool) -> Result<()> {
    let level = if project {
        SettingsLevel::Project
    } else {
        SettingsLevel::Local
    };
    let path = settings_path(level)?;
    let mut settings = read_settings(&path)?.unwrap_or_default();
    let perms = settings
        .permissions
        .get_or_insert_with(Permissions::default);

    if perms.allow.iter().any(|p| p == pattern) {
        println!("{pattern} is already in {level} allow list");
        return Ok(());
    }

    perms.allow.push(pattern.to_string());
    write_settings(&path, &settings)?;
    println!("Added {pattern} to {level} allow list");
    Ok(())
}

/// Add a pattern to the deny list in the given settings file.
pub fn deny_pattern(pattern: &str, project: bool) -> Result<()> {
    let level = if project {
        SettingsLevel::Project
    } else {
        SettingsLevel::Local
    };
    let path = settings_path(level)?;
    let mut settings = read_settings(&path)?.unwrap_or_default();
    let perms = settings
        .permissions
        .get_or_insert_with(Permissions::default);

    if perms.deny.iter().any(|p| p == pattern) {
        println!("{pattern} is already in {level} deny list");
        return Ok(());
    }

    perms.deny.push(pattern.to_string());
    write_settings(&path, &settings)?;
    println!("Added {pattern} to {level} deny list");
    Ok(())
}

/// List all tool rules across all settings files with provenance.
pub fn list_rules() -> Result<()> {
    let levels = [
        SettingsLevel::User,
        SettingsLevel::Project,
        SettingsLevel::Local,
    ];

    let mut rules: Vec<ToolRule> = Vec::new();

    for level in levels {
        let path = settings_path(level)?;
        if let Some(settings) = read_settings(&path)?
            && let Some(perms) = settings.permissions
        {
            for pattern in &perms.allow {
                rules.push(ToolRule {
                    pattern: pattern.clone(),
                    kind: RuleKind::Allow,
                    source: level,
                });
            }
            for pattern in &perms.deny {
                rules.push(ToolRule {
                    pattern: pattern.clone(),
                    kind: RuleKind::Deny,
                    source: level,
                });
            }
        }
    }

    if rules.is_empty() {
        println!("No tool permissions configured.");
        return Ok(());
    }

    // Calculate column widths
    let kind_width = 5; // "allow" or "deny"
    let pattern_width = rules.iter().map(|r| r.pattern.len()).max().unwrap_or(0);

    for rule in &rules {
        println!(
            "{:<kw$}  {:<pw$}  ({})",
            rule.kind,
            rule.pattern,
            rule.source,
            kw = kind_width,
            pw = pattern_width,
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_read_settings_missing_file() {
        let result = read_settings(Path::new("/nonexistent/settings.json")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_read_settings_valid() {
        let dir = setup_temp_dir();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{"permissions": {"allow": ["Read", "Edit"], "deny": ["Bash(rm:*)"]}}"#,
        )
        .unwrap();

        let settings = read_settings(&path).unwrap().unwrap();
        let perms = settings.permissions.unwrap();
        assert_eq!(perms.allow, vec!["Read", "Edit"]);
        assert_eq!(perms.deny, vec!["Bash(rm:*)"]);
    }

    #[test]
    fn test_read_settings_preserves_unknown_fields() {
        let dir = setup_temp_dir();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{"permissions": {"allow": ["Read"]}, "hooks": {"PreToolUse": []}}"#,
        )
        .unwrap();

        let settings = read_settings(&path).unwrap().unwrap();
        // Write it back and verify hooks are preserved
        let serialized = serde_json::to_string(&settings).unwrap();
        assert!(serialized.contains("PreToolUse"));
    }

    #[test]
    fn test_write_settings_creates_parent_dirs() {
        let dir = setup_temp_dir();
        let path = dir.path().join("nested").join("dir").join("settings.json");
        let settings = SettingsFile::default();
        write_settings(&path, &settings).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_write_settings_roundtrip() {
        let dir = setup_temp_dir();
        let path = dir.path().join("settings.json");
        let original = r#"{
  "permissions": {
    "allow": [
      "Read"
    ],
    "deny": [
      "Bash(rm:*)"
    ]
  }
}
"#;
        fs::write(&path, original).unwrap();

        let settings = read_settings(&path).unwrap().unwrap();
        write_settings(&path, &settings).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        assert_eq!(written, original);
    }

    #[test]
    fn test_read_settings_empty_permissions() {
        let dir = setup_temp_dir();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"permissions": {}}"#).unwrap();

        let settings = read_settings(&path).unwrap().unwrap();
        let perms = settings.permissions.unwrap();
        assert!(perms.allow.is_empty());
        assert!(perms.deny.is_empty());
    }

    #[test]
    fn test_read_settings_no_permissions() {
        let dir = setup_temp_dir();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"hooks": {}}"#).unwrap();

        let settings = read_settings(&path).unwrap().unwrap();
        assert!(settings.permissions.is_none());
    }
}
