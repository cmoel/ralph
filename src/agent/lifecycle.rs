//! Agent registration, heartbeat, bead specification checks, and cleanup.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tracing::{info, warn};

/// Result of agent registration.
pub struct AgentSetup {
    pub agent_bead_id: String,
}

/// Register an ephemeral agent bead and create a git worktree.
/// Returns None if any step fails (logs warnings).
pub fn register(bd_path: &str, session_id: &str) -> Option<AgentSetup> {
    // Create ephemeral agent bead with rig:ralph label
    let output = Command::new(bd_path)
        .args([
            "create",
            "--type=task",
            "--labels=rig:ralph",
            "--ephemeral",
            "--json",
            "--description=Ephemeral ralph agent bead",
            &format!("--title=ralph agent {}", session_id),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            warn!(error = %e, "agent_bead_create_failed");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "agent_bead_create_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let agent_bead_id = parse_bead_id(&stdout)?;
    info!(agent_bead_id = %agent_bead_id, "agent_bead_created");

    // Set agent to in_progress
    let _ = Command::new(bd_path)
        .args(["update", &agent_bead_id, "--status=in_progress"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    info!(agent_bead_id = %agent_bead_id, "agent_registered");

    Some(AgentSetup { agent_bead_id })
}

/// Start a background heartbeat thread that updates the agent bead periodically.
/// Returns a stop flag that can be set to true to stop the heartbeat.
pub fn start_heartbeat(
    bd_path: String,
    agent_bead_id: String,
    interval_secs: u64,
) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);

    std::thread::spawn(move || {
        let interval = Duration::from_secs(interval_secs);
        loop {
            std::thread::sleep(interval);
            if stop_clone.load(Ordering::Relaxed) {
                break;
            }
            let now = chrono_now_iso();
            let result = Command::new(&bd_path)
                .args([
                    "update",
                    &agent_bead_id,
                    &format!("--set-metadata=last_heartbeat={}", now),
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output();

            match result {
                Ok(o) if o.status.success() => {
                    info!(agent_bead_id = %agent_bead_id, "heartbeat_sent");
                }
                Ok(o) => {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    warn!(stderr = %stderr.trim(), "heartbeat_failed");
                }
                Err(e) => {
                    warn!(error = %e, "heartbeat_failed");
                }
            }
        }
    });

    stop
}

// claim_next_bead removed — replaced by select_and_claim_epic + claim_next_child

/// Check whether bead metadata indicates sufficient specification.
/// Returns None if the bead passes, or Some(reason) if it should be flagged.
pub fn check_bead_specification(bead: &serde_json::Value) -> Option<String> {
    let description = bead
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");

    if description.trim().is_empty() {
        return Some(
            "Under-specified: no description provided. Add a description explaining what done looks like.".to_string(),
        );
    }

    // Reject descriptions too brief for autonomous execution
    if description.trim().len() < 100 {
        return Some(
            "Under-specified: description too brief for autonomous execution. \
             Shape this bead with Approach, Edge Cases, and Acceptance sections."
                .to_string(),
        );
    }

    // Reject descriptions without structured sections (## headings).
    // Well-shaped beads have sections like ## Approach, ## Edge Cases, ## Acceptance (child tasks)
    // or ## Problem, ## Solution Shape, ## Boundaries (epics).
    let has_sections = description.contains("\n## ") || description.starts_with("## ");
    if !has_sections {
        return Some(
            "Under-specified: missing structured sections. \
             Shape this bead with Approach, Edge Cases, and Acceptance sections."
                .to_string(),
        );
    }

    None
}

/// Assess a claimed bead's specification quality.
/// If under-specified, flags for human review, resets to open, releases the hook, and returns false.
/// Returns true if the bead is ready for implementation.
pub(crate) fn assess_bead_specification(bd_path: &str, bead_id: &str, agent_bead_id: &str) -> bool {
    let output = Command::new(bd_path)
        .args(["show", bead_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => {
            warn!(bead_id = %bead_id, "assess_fetch_failed");
            return true; // If we can't fetch, let Claude handle it
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    // bd show --json returns an array; extract the first element
    let bead: serde_json::Value = match serde_json::from_str::<serde_json::Value>(stdout.as_ref()) {
        Ok(serde_json::Value::Array(arr)) => match arr.into_iter().next() {
            Some(v) => v,
            None => return true,
        },
        Ok(v) => v,
        Err(_) => return true,
    };

    if let Some(reason) = check_bead_specification(&bead) {
        let notes = format!("Flagged by Ralph: {}", reason);
        let _ = Command::new(bd_path)
            .args(["update", bead_id, "--notes", &notes])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();

        let _ = Command::new(bd_path)
            .args(["human", bead_id])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();

        reset_bead_to_open(bd_path, bead_id);
        release_hook(bd_path, agent_bead_id);

        info!(bead_id = %bead_id, reason = %reason, "bead_under_specified");
        return false;
    }

    true
}

/// Release the hook on this agent (clear the hook state dimension).
pub fn release_hook(bd_path: &str, agent_bead_id: &str) {
    let result = Command::new(bd_path)
        .args([
            "set-state",
            agent_bead_id,
            "hook=none",
            "--reason=work complete",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(agent_bead_id = %agent_bead_id, "hook_released");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "hook_release_failed");
        }
        Err(e) => {
            warn!(error = %e, "hook_release_failed");
        }
    }
}

/// Release the currently hooked bead: clear the hook and reset the bead to open.
/// Used during both stop (between iterations) and quit (full teardown).
pub fn release_bead(bd_path: &str, agent_bead_id: &str, bead_id: &str) {
    release_hook(bd_path, agent_bead_id);
    reset_bead_to_open(bd_path, bead_id);
}

/// Reset a bead's status to open so other agents can pick it up.
/// Skips the reset if the bead was already closed (e.g. by Claude during the iteration).
fn reset_bead_to_open(bd_path: &str, bead_id: &str) {
    // Check current status — don't reopen beads that Claude already closed
    if let Ok(o) = Command::new(bd_path)
        .args(["show", bead_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .and_then(|o| {
            if o.status.success() {
                Ok(o)
            } else {
                Err(std::io::ErrorKind::Other.into())
            }
        })
    {
        let stdout = String::from_utf8_lossy(&o.stdout);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&stdout) {
            let status = val
                .as_array()
                .and_then(|arr| arr.first())
                .unwrap_or(&val)
                .get("status")
                .and_then(|s| s.as_str());
            if matches!(status, Some("closed")) {
                info!(bead_id = %bead_id, "bead_already_closed_skipping_reset");
                return;
            }
        }
    }

    let result = Command::new(bd_path)
        .args(["update", bead_id, "--status=open", "--assignee="])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(bead_id = %bead_id, "bead_reset_to_open");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "bead_reset_failed");
        }
        Err(e) => {
            warn!(error = %e, "bead_reset_failed");
        }
    }
}

/// Clean up agent resources: release hook, close agent bead, merge + remove worktree.
pub fn cleanup(bd_path: &str, agent_bead_id: &str, worktree_name: &str) {
    info!(agent_bead_id = %agent_bead_id, "agent_cleanup_start");

    // Release any hooked bead
    release_hook(bd_path, agent_bead_id);

    // Close the agent bead
    cleanup_agent_bead(bd_path, agent_bead_id);

    // Try to merge worktree branch to main before removal
    if super::worktree::merge_worktree_to_main(worktree_name) {
        super::worktree::remove_merged_worktree(bd_path, worktree_name);
    } else {
        // Merge failed — leave worktree intact so user can resolve
        warn!(worktree_name = %worktree_name, "session_end_merge_failed_worktree_preserved");
    }
}

/// Close an agent bead (used during cleanup or when worktree creation fails).
pub(crate) fn cleanup_agent_bead(bd_path: &str, agent_bead_id: &str) {
    let result = Command::new(bd_path)
        .args(["close", agent_bead_id, "--reason=ralph session ended"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(agent_bead_id = %agent_bead_id, "agent_bead_closed");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "agent_bead_close_failed");
        }
        Err(e) => {
            warn!(error = %e, "agent_bead_close_failed");
        }
    }
}

/// Parse bead ID from bd create --json output.
pub(crate) fn parse_bead_id(json_output: &str) -> Option<String> {
    // bd create --json outputs multi-line JSON with an "id" field
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_output.trim())
        && let Some(id) = value.get("id").and_then(|v| v.as_str())
    {
        return Some(id.to_string());
    }
    let truncated: String = json_output.chars().take(200).collect();
    warn!(output = %truncated, "could_not_parse_bead_id");
    None
}

/// Get current time as ISO 8601 string using pure Rust.
fn chrono_now_iso() -> String {
    epoch_secs_to_iso8601(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
}

/// Convert epoch seconds to an ISO 8601 UTC string.
pub(crate) fn epoch_secs_to_iso8601(secs: u64) -> String {
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Converts days since Unix epoch to (year, month, day) — Hinnant algorithm.
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn check_bead_specification_passes_with_structured_description() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some feature",
            "description": "Implement the widget renderer.\n\n## Approach\nModify src/widget.rs to add render() method following the existing Panel pattern.\n\n## Edge Cases\nHandle empty widget list gracefully.\n\n## Acceptance\n- Widget renders in the TUI"
        });
        assert!(check_bead_specification(&bead).is_none());
    }

    #[test]
    fn check_bead_specification_passes_with_epic_structure() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some epic",
            "description": "## Problem\nUsers cannot see dependencies between beads.\n\n## Solution Shape\nAdd a graph view that renders bd graph output.\n\n## Boundaries\n- In scope: static render\n- No-go: interactive editing"
        });
        assert!(check_bead_specification(&bead).is_none());
    }

    #[test]
    fn check_bead_specification_rejects_empty_description() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some feature",
            "description": ""
        });
        assert!(check_bead_specification(&bead).is_some());
    }

    #[test]
    fn check_bead_specification_rejects_whitespace_only_description() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some feature",
            "description": "   \n  "
        });
        assert!(check_bead_specification(&bead).is_some());
    }

    #[test]
    fn check_bead_specification_rejects_missing_description() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some feature"
        });
        assert!(check_bead_specification(&bead).is_some());
    }

    #[test]
    fn check_bead_specification_rejects_brief_description() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some feature",
            "description": "Build a thing that does X. Done when Y works."
        });
        let reason = check_bead_specification(&bead);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("too brief"));
    }

    #[test]
    fn check_bead_specification_rejects_long_but_unstructured() {
        let bead = json!({
            "id": "ralph-abc",
            "title": "Some feature",
            "description": "This is a longer description that explains what needs to happen in some detail but does not include any structured sections with markdown headings so it should still be rejected by the specification check."
        });
        let reason = check_bead_specification(&bead);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("structured sections"));
    }

    #[test]
    fn parse_bead_id_extracts_id_from_json() {
        let json = r#"{"id": "beads-abc", "title": "Test"}"#;
        assert_eq!(parse_bead_id(json), Some("beads-abc".into()));
    }

    #[test]
    fn parse_bead_id_returns_none_for_invalid_json() {
        assert_eq!(parse_bead_id("not json"), None);
    }

    #[test]
    fn parse_bead_id_returns_none_for_missing_id() {
        let json = r#"{"title": "No id field"}"#;
        assert_eq!(parse_bead_id(json), None);
    }
}
