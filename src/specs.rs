//! Spec parsing and detection functions.

use ratatui::style::Color;
use tracing::{debug, warn};

use crate::modals::SpecEntry;

/// Status of a spec in the README table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SpecStatus {
    /// Blocked - needs attention (sorted first)
    Blocked,
    /// Ready - can be worked on
    Ready,
    /// In Progress - currently being worked on
    InProgress,
    /// Done - completed (sorted last)
    Done,
}

impl SpecStatus {
    /// Parse status from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim() {
            "Blocked" => Some(Self::Blocked),
            "Ready" => Some(Self::Ready),
            "In Progress" => Some(Self::InProgress),
            "Done" => Some(Self::Done),
            _ => None,
        }
    }

    /// Get the display color for this status.
    pub fn color(&self) -> Color {
        match self {
            Self::Blocked => Color::Red,
            Self::Ready => Color::Cyan,
            Self::InProgress => Color::Green,
            Self::Done => Color::DarkGray,
        }
    }

    /// Get the display label for this status.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Blocked => "Blocked",
            Self::Ready => "Ready",
            Self::InProgress => "In Progress",
            Self::Done => "Done",
        }
    }
}

/// Result of checking if there's more spec work to do.
pub enum SpecsRemaining {
    /// There are specs with Ready or In Progress status.
    Yes,
    /// All specs are Done or Blocked.
    No,
    /// README file doesn't exist.
    Missing,
    /// README file couldn't be read (permissions, etc.).
    ReadError(String),
}

/// Parse specs from the README.md file.
pub fn parse_specs_readme(specs_dir: &std::path::Path) -> Result<Vec<SpecEntry>, String> {
    let readme_path = specs_dir.join("README.md");

    let contents = match std::fs::read_to_string(&readme_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("specs/README.md not found".to_string());
        }
        Err(e) => {
            return Err(format!("Failed to read specs/README.md: {}", e));
        }
    };

    let mut specs = Vec::new();

    for line in contents.lines() {
        // Skip non-table lines and header rows
        if !line.starts_with('|') || line.contains("---") || line.contains("Spec") {
            continue;
        }

        // Parse table row: | [spec-name](spec-name.md) | Status | Summary | Depends On |
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 3 {
            continue;
        }

        // Extract spec name from markdown link in first column
        let name_col = parts.get(1).map(|s| s.trim()).unwrap_or("");
        let name = if let Some(start) = name_col.find('[') {
            let after_bracket = &name_col[start + 1..];
            if let Some(end) = after_bracket.find(']') {
                after_bracket[..end].to_string()
            } else {
                continue;
            }
        } else {
            continue;
        };

        // Extract status from second column
        let status_str = parts.get(2).map(|s| s.trim()).unwrap_or("");
        let status = match SpecStatus::from_str(status_str) {
            Some(s) => s,
            None => continue,
        };

        // Get file timestamp
        let spec_path = specs_dir.join(format!("{}.md", name));
        let timestamp = std::fs::metadata(&spec_path).ok().and_then(|m| {
            // Try created time first, fall back to modified time
            m.created().ok().or_else(|| m.modified().ok())
        });

        specs.push(SpecEntry {
            name,
            status,
            timestamp,
        });
    }

    if specs.is_empty() {
        return Err("No specs found in README.md".to_string());
    }

    Ok(specs)
}

/// Check if there are specs remaining (Ready or In Progress status).
pub fn check_specs_remaining(specs_dir: &std::path::Path) -> SpecsRemaining {
    let readme_path = specs_dir.join("README.md");

    let contents = match std::fs::read_to_string(&readme_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return SpecsRemaining::Missing;
        }
        Err(e) => {
            return SpecsRemaining::ReadError(e.to_string());
        }
    };

    for line in contents.lines() {
        // Look for table rows with "Ready" or "In Progress" status
        if line.contains("| Ready |")
            || line.contains("| Ready|")
            || line.contains("| In Progress |")
            || line.contains("| In Progress|")
        {
            return SpecsRemaining::Yes;
        }
    }

    SpecsRemaining::No
}

/// Detect the currently in-progress spec from specs/README.md.
pub fn detect_current_spec(specs_dir: &std::path::Path) -> Option<String> {
    let readme_path = specs_dir.join("README.md");

    let contents = match std::fs::read_to_string(&readme_path) {
        Ok(c) => c,
        Err(e) => {
            debug!(path = ?readme_path, error = %e, "spec_readme_read_failed");
            return None;
        }
    };

    let mut found_specs: Vec<String> = Vec::new();

    for line in contents.lines() {
        // Look for table rows with "In Progress" status
        // Pattern: | [spec-name](...)  | In Progress | ... |
        if line.contains("| In Progress |") || line.contains("| In Progress|") {
            // Extract spec name from the link: | [spec-name](spec-name.md) |
            if let Some(start) = line.find("| [") {
                let after_bracket = &line[start + 3..];
                if let Some(end) = after_bracket.find(']') {
                    let spec_name = after_bracket[..end].to_string();
                    found_specs.push(spec_name);
                }
            }
        }
    }

    if found_specs.len() > 1 {
        warn!(
            specs = ?found_specs,
            "multiple_specs_in_progress"
        );
    }

    found_specs.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    // SpecStatus::from_str tests

    #[test]
    fn test_spec_status_from_str_blocked() {
        assert_eq!(SpecStatus::from_str("Blocked"), Some(SpecStatus::Blocked));
    }

    #[test]
    fn test_spec_status_from_str_ready() {
        assert_eq!(SpecStatus::from_str("Ready"), Some(SpecStatus::Ready));
    }

    #[test]
    fn test_spec_status_from_str_in_progress() {
        assert_eq!(
            SpecStatus::from_str("In Progress"),
            Some(SpecStatus::InProgress)
        );
    }

    #[test]
    fn test_spec_status_from_str_done() {
        assert_eq!(SpecStatus::from_str("Done"), Some(SpecStatus::Done));
    }

    #[test]
    fn test_spec_status_from_str_invalid() {
        assert_eq!(SpecStatus::from_str("Invalid"), None);
        assert_eq!(SpecStatus::from_str(""), None);
        assert_eq!(SpecStatus::from_str("done"), None); // case sensitive
        assert_eq!(SpecStatus::from_str("DONE"), None);
    }

    #[test]
    fn test_spec_status_from_str_with_whitespace() {
        assert_eq!(
            SpecStatus::from_str("  Blocked  "),
            Some(SpecStatus::Blocked)
        );
        assert_eq!(SpecStatus::from_str("\tReady\t"), Some(SpecStatus::Ready));
        assert_eq!(
            SpecStatus::from_str(" In Progress "),
            Some(SpecStatus::InProgress)
        );
    }

    // SpecStatus::label tests

    #[test]
    fn test_spec_status_label_blocked() {
        assert_eq!(SpecStatus::Blocked.label(), "Blocked");
    }

    #[test]
    fn test_spec_status_label_ready() {
        assert_eq!(SpecStatus::Ready.label(), "Ready");
    }

    #[test]
    fn test_spec_status_label_in_progress() {
        assert_eq!(SpecStatus::InProgress.label(), "In Progress");
    }

    #[test]
    fn test_spec_status_label_done() {
        assert_eq!(SpecStatus::Done.label(), "Done");
    }

    #[test]
    fn test_spec_status_label_roundtrip() {
        // Verify that label() returns a value that from_str() can parse back
        for status in [
            SpecStatus::Blocked,
            SpecStatus::Ready,
            SpecStatus::InProgress,
            SpecStatus::Done,
        ] {
            assert_eq!(SpecStatus::from_str(status.label()), Some(status));
        }
    }
}
