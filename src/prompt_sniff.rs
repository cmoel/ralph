//! Lightweight sniff test for PROMPT.md content.
//!
//! Checks for mode-specific keywords that suggest the user's PROMPT.md
//! contains content intended for a different mode.

/// Keywords that belong in beads mode, not specs mode.
const BEADS_KEYWORDS: &[&str] = &["bd ready", "bd show", "bd create", "bd update", "bd close"];

/// Keywords that belong in specs mode, not beads mode.
const SPECS_KEYWORDS: &[&str] = &["specs/README.md", "specs/TEMPLATE.md"];

/// Check PROMPT.md content for keywords that don't match the current mode.
/// Returns a list of warning messages (empty if no issues found).
pub fn sniff_prompt(content: &str, mode: &str) -> Vec<String> {
    let keywords_to_check: &[&str] = match mode {
        "specs" => BEADS_KEYWORDS,
        "beads" => SPECS_KEYWORDS,
        _ => return vec![],
    };

    let found: Vec<&&str> = keywords_to_check
        .iter()
        .filter(|kw| content.contains(**kw))
        .collect();

    if found.is_empty() {
        return vec![];
    }

    let keyword_list = found
        .iter()
        .map(|kw| format!("\"{}\"", kw))
        .collect::<Vec<_>>()
        .join(", ");

    vec![format!(
        "Warning: PROMPT.md contains {} from a different mode (current: {}). Run `ralph reinit` to regenerate it.",
        keyword_list, mode
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_warnings_when_content_matches_mode() {
        let content = "Use specs/README.md to find work.\nMark specs as Ready.";
        let warnings = sniff_prompt(content, "specs");
        assert!(warnings.is_empty());
    }

    #[test]
    fn warns_about_specs_keywords_in_beads_mode() {
        let content = "Read specs/README.md to find available work.";
        let warnings = sniff_prompt(content, "beads");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("specs/README.md"));
        assert!(warnings[0].contains("current: beads"));
        assert!(warnings[0].contains("ralph reinit"));
    }

    #[test]
    fn warns_about_beads_keywords_in_specs_mode() {
        let content = "Run bd ready to find work. Use bd show to read details.";
        let warnings = sniff_prompt(content, "specs");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("bd ready"));
        assert!(warnings[0].contains("bd show"));
    }

    #[test]
    fn no_warnings_for_unknown_mode() {
        let content = "bd ready specs/README.md";
        let warnings = sniff_prompt(content, "custom");
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_warnings_when_content_is_clean() {
        let content = "# Agent Workflow\n\nComplete ONE vertical slice per session.";
        let warnings = sniff_prompt(content, "beads");
        assert!(warnings.is_empty());
    }

    #[test]
    fn detects_multiple_mismatched_keywords() {
        let content = "Read specs/README.md and specs/TEMPLATE.md for guidance.";
        let warnings = sniff_prompt(content, "beads");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("specs/README.md"));
        assert!(warnings[0].contains("specs/TEMPLATE.md"));
    }
}
