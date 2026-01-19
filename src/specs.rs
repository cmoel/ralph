//! Spec parsing and detection functions.

use ratatui::style::Color;
use tracing::{debug, warn};

use crate::modals::SpecEntry;

/// A parsed spec from the README table (pure data, no timestamps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSpec {
    /// Name of the spec (from markdown link).
    pub name: String,
    /// Current status.
    pub status: SpecStatus,
}

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

/// Parse specs table from README content (pure function).
///
/// Extracts spec names and statuses from markdown table rows.
/// Skips header rows, separator lines, and malformed rows.
fn parse_specs_table(contents: &str) -> Vec<ParsedSpec> {
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

        specs.push(ParsedSpec { name, status });
    }

    specs
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

    let parsed = parse_specs_table(&contents);

    if parsed.is_empty() {
        return Err("No specs found in README.md".to_string());
    }

    // Add timestamps from filesystem
    let specs = parsed
        .into_iter()
        .map(|p| {
            let spec_path = specs_dir.join(format!("{}.md", p.name));
            let timestamp = std::fs::metadata(&spec_path).ok().and_then(|m| {
                // Try created time first, fall back to modified time
                m.created().ok().or_else(|| m.modified().ok())
            });
            SpecEntry {
                name: p.name,
                status: p.status,
                timestamp,
            }
        })
        .collect();

    Ok(specs)
}

/// Check if there are specs remaining (Ready or In Progress status).
pub fn check_specs_remaining(specs_dir: &std::path::Path) -> SpecsRemaining {
    match parse_specs_readme(specs_dir) {
        Ok(specs) => {
            if specs
                .iter()
                .any(|s| matches!(s.status, SpecStatus::Ready | SpecStatus::InProgress))
            {
                SpecsRemaining::Yes
            } else {
                SpecsRemaining::No
            }
        }
        Err(e) if e.contains("not found") => SpecsRemaining::Missing,
        Err(e) => SpecsRemaining::ReadError(e),
    }
}

/// Detect the currently in-progress spec from specs/README.md.
pub fn detect_current_spec(specs_dir: &std::path::Path) -> Option<String> {
    let specs = match parse_specs_readme(specs_dir) {
        Ok(s) => s,
        Err(e) => {
            debug!(error = %e, "spec_readme_read_failed");
            return None;
        }
    };

    let in_progress: Vec<_> = specs
        .iter()
        .filter(|s| s.status == SpecStatus::InProgress)
        .collect();

    if in_progress.len() > 1 {
        let names: Vec<_> = in_progress.iter().map(|s| &s.name).collect();
        warn!(
            specs = ?names,
            "multiple_specs_in_progress"
        );
    }

    in_progress.into_iter().next().map(|s| s.name.clone())
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

    // parse_specs_table tests

    #[test]
    fn test_parse_specs_table_valid_row() {
        let contents = "| [my-spec](my-spec.md) | Ready | Some summary | — |";
        let specs = parse_specs_table(contents);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "my-spec");
        assert_eq!(specs[0].status, SpecStatus::Ready);
    }

    #[test]
    fn test_parse_specs_table_all_status_variants() {
        let contents = "\
| [blocked-spec](blocked-spec.md) | Blocked | Blocked summary | — |
| [ready-spec](ready-spec.md) | Ready | Ready summary | — |
| [in-progress-spec](in-progress-spec.md) | In Progress | In progress summary | — |
| [done-spec](done-spec.md) | Done | Done summary | — |";

        let specs = parse_specs_table(contents);

        assert_eq!(specs.len(), 4);
        assert_eq!(specs[0].name, "blocked-spec");
        assert_eq!(specs[0].status, SpecStatus::Blocked);
        assert_eq!(specs[1].name, "ready-spec");
        assert_eq!(specs[1].status, SpecStatus::Ready);
        assert_eq!(specs[2].name, "in-progress-spec");
        assert_eq!(specs[2].status, SpecStatus::InProgress);
        assert_eq!(specs[3].name, "done-spec");
        assert_eq!(specs[3].status, SpecStatus::Done);
    }

    #[test]
    fn test_parse_specs_table_skips_header_and_separator() {
        let contents = "\
| Spec | Status | Summary | Depends On |
|------|--------|---------|------------|
| [my-spec](my-spec.md) | Ready | Summary | — |";

        let specs = parse_specs_table(contents);

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "my-spec");
    }

    #[test]
    fn test_parse_specs_table_skips_malformed_rows() {
        let contents = "\
| [valid-spec](valid-spec.md) | Ready | Summary | — |
| no-brackets | Ready | Summary | — |
| [missing-close-bracket | Ready | Summary | — |
|| Ready | Summary |
| [valid-spec-2](valid-spec-2.md) | Done | Summary | — |";

        let specs = parse_specs_table(contents);

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "valid-spec");
        assert_eq!(specs[1].name, "valid-spec-2");
    }

    #[test]
    fn test_parse_specs_table_skips_invalid_status() {
        let contents = "\
| [valid-spec](valid-spec.md) | Ready | Summary | — |
| [invalid-status](invalid-status.md) | Unknown | Summary | — |
| [another-valid](another-valid.md) | Done | Summary | — |";

        let specs = parse_specs_table(contents);

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].name, "valid-spec");
        assert_eq!(specs[1].name, "another-valid");
    }

    #[test]
    fn test_parse_specs_table_whitespace_variations() {
        let contents = "\
|  [spaced-spec](spaced-spec.md)  |  Ready  | Summary | — |
|[tight-spec](tight-spec.md)|Done|Summary|—|
| [tabbed-spec](tabbed-spec.md) |	In Progress	| Summary | — |";

        let specs = parse_specs_table(contents);

        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].name, "spaced-spec");
        assert_eq!(specs[0].status, SpecStatus::Ready);
        assert_eq!(specs[1].name, "tight-spec");
        assert_eq!(specs[1].status, SpecStatus::Done);
        assert_eq!(specs[2].name, "tabbed-spec");
        assert_eq!(specs[2].status, SpecStatus::InProgress);
    }

    #[test]
    fn test_parse_specs_table_empty_content() {
        let specs = parse_specs_table("");
        assert!(specs.is_empty());
    }

    #[test]
    fn test_parse_specs_table_no_table_rows() {
        let contents = "# Specs\n\nSome regular markdown content.";
        let specs = parse_specs_table(contents);
        assert!(specs.is_empty());
    }

    #[test]
    fn test_parse_specs_table_skips_non_table_lines() {
        let contents = "\
# Specs

Some preamble text.

| [my-spec](my-spec.md) | Ready | Summary | — |

More content after.";

        let specs = parse_specs_table(contents);

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "my-spec");
    }
}
