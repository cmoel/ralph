//! Lightweight sniff test for PROMPT.md content.
//!
//! Checks for stale specs-mode keywords that suggest the user's PROMPT.md
//! needs regeneration.

/// Keywords that indicate stale specs-mode content.
const SPECS_KEYWORDS: &[&str] = &["specs/README.md", "specs/TEMPLATE.md"];

/// Check PROMPT.md content for stale specs-mode keywords.
/// Returns a list of warning messages (empty if no issues found).
pub fn sniff_prompt(content: &str) -> Vec<String> {
    let found: Vec<&&str> = SPECS_KEYWORDS
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
        "Warning: PROMPT.md contains stale specs-mode references ({}). Run `ralph reinit` to regenerate it.",
        keyword_list
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_warnings_when_content_is_clean() {
        let content = "# Agent Workflow\n\nComplete ONE vertical slice per session.";
        let warnings = sniff_prompt(content);
        assert!(warnings.is_empty());
    }

    #[test]
    fn warns_about_specs_keywords() {
        let content = "Read specs/README.md to find available work.";
        let warnings = sniff_prompt(content);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("specs/README.md"));
        assert!(warnings[0].contains("ralph reinit"));
    }

    #[test]
    fn detects_multiple_stale_keywords() {
        let content = "Read specs/README.md and specs/TEMPLATE.md for guidance.";
        let warnings = sniff_prompt(content);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("specs/README.md"));
        assert!(warnings[0].contains("specs/TEMPLATE.md"));
    }
}
