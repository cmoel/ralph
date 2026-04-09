//! Agent lifecycle: registration, worktree creation, heartbeat, and cleanup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tracing::{info, warn};

/// Result of agent registration.
pub struct AgentSetup {
    pub agent_bead_id: String,
}

/// A stale agent detected during recovery.
#[derive(Clone)]
pub struct StaleAgent {
    pub agent_bead_id: String,
    pub hooked_bead_id: String,
    pub hooked_bead_title: String,
    pub worktree_name: String,
    /// Whether this agent was working within an epic (worktree should be preserved).
    pub has_epic: bool,
}

/// Outcome of attempting to resume a stale bead.
pub enum ResumeResult {
    /// Successfully reclaimed the bead for the new agent.
    Resumed,
    /// Bead was stuck a second time; escalated to human review.
    EscalatedToHuman,
    /// Failed to reclaim (bd command error).
    Failed,
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
fn assess_bead_specification(bd_path: &str, bead_id: &str, agent_bead_id: &str) -> bool {
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

/// Get list of files that differ between main and a worktree branch.
fn get_changed_files(worktree_name: &str) -> Vec<String> {
    let repo_root = repo_root();
    let diff_spec = format!("main...{}", worktree_name);
    let output = Command::new("git")
        .args(["diff", "--name-only", &diff_spec])
        .current_dir(&repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

/// Search for an existing open merge-conflict bead for this branch.
/// Returns the bead ID if found.
pub fn find_merge_conflict_bead(bd_path: &str, worktree_name: &str) -> Option<String> {
    let output = Command::new(bd_path)
        .args(["list", "--json", "--status=open", "--limit=0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;

    let prefix = format!("Merge conflict: {}", worktree_name);
    for item in &items {
        let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
        if title.starts_with(&prefix) {
            return item.get("id").and_then(|i| i.as_str()).map(String::from);
        }
    }

    None
}

/// File a P0 merge-conflict bead for Claude to resolve next iteration.
/// Returns the new bead ID on success.
pub fn file_merge_conflict_bead(bd_path: &str, worktree_name: &str) -> Option<String> {
    let files = get_changed_files(worktree_name);
    let files_display = if files.is_empty() {
        "Could not determine changed files".to_string()
    } else {
        files.join(", ")
    };

    let title = format!("Merge conflict: {} → main", worktree_name);
    let description = format!(
        "Merge main into this branch (git merge main), resolve all conflicts, \
         commit the resolution. Work in the existing worktree.\n\n\
         Branch: {}\n\
         Files with changes: {}",
        worktree_name, files_display
    );

    let output = Command::new(bd_path)
        .args([
            "create",
            &format!("--title={}", title),
            &format!("--description={}", description),
            "--type=task",
            "--priority=0",
            "--json",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "file_merge_conflict_bead_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let bead_id = parse_bead_id(&stdout)?;
    info!(bead_id = %bead_id, worktree_name = %worktree_name, "merge_conflict_bead_filed");
    Some(bead_id)
}

/// Escalate a merge conflict to human review.
/// Closes the existing merge-conflict bead and files a new human-labeled bead.
/// Returns the new human bead ID on success.
pub fn escalate_merge_conflict(
    bd_path: &str,
    worktree_name: &str,
    existing_bead_id: &str,
) -> Option<String> {
    // Close the existing merge-conflict bead
    let _ = Command::new(bd_path)
        .args([
            "close",
            existing_bead_id,
            "--reason=Claude could not resolve",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    let files = get_changed_files(worktree_name);
    let files_display = if files.is_empty() {
        "Could not determine changed files".to_string()
    } else {
        files.join(", ")
    };

    let title = format!("HUMAN: Merge conflict in {}", worktree_name);
    let description = format!(
        "Merge conflict persists after Claude's resolution attempt.\n\n\
         Branch: {}\n\
         Files with changes: {}\n\n\
         Previous bead {} was closed after Claude failed to resolve. \
         Manual intervention needed.",
        worktree_name, files_display, existing_bead_id
    );

    let output = Command::new(bd_path)
        .args([
            "create",
            &format!("--title={}", title),
            &format!("--description={}", description),
            "--type=bug",
            "--priority=0",
            "--labels=human",
            "--json",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "escalate_merge_conflict_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let bead_id = parse_bead_id(&stdout)?;
    info!(
        bead_id = %bead_id,
        existing_bead = %existing_bead_id,
        worktree_name = %worktree_name,
        "merge_conflict_escalated_to_human"
    );
    Some(bead_id)
}

/// Attempt to merge the worktree branch into main from the repo root.
/// Returns true if the merge succeeded, false if it failed (and aborts the merge).
pub fn merge_worktree_to_main(worktree_name: &str) -> bool {
    let repo_root = repo_root();
    info!(worktree_name = %worktree_name, "merge_worktree_start");

    let result = Command::new("git")
        .args(["merge", worktree_name])
        .current_dir(&repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(worktree_name = %worktree_name, "merge_worktree_success");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "merge_worktree_conflict");
            // Abort the failed merge
            let _ = Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(&repo_root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            false
        }
        Err(e) => {
            warn!(error = %e, "merge_worktree_failed");
            false
        }
    }
}

/// Remove worktree, revert .gitignore, and delete the merged branch.
pub fn remove_merged_worktree(bd_path: &str, worktree_name: &str) {
    let repo_root = repo_root();

    // Remove the worktree directory (--force handles untracked files like target/)
    match Command::new(bd_path)
        .args(["worktree", "remove", "--force", worktree_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "worktree_remove_failed");
        }
        Err(e) => {
            warn!(error = %e, "worktree_remove_failed");
        }
        _ => {}
    }

    // Revert .gitignore entry bd added
    let _ = Command::new("git")
        .args(["checkout", "--", ".gitignore"])
        .current_dir(&repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    // Delete the merged branch
    match Command::new("git")
        .args(["branch", "-d", worktree_name])
        .current_dir(&repo_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "branch_delete_failed");
        }
        Err(e) => {
            warn!(error = %e, "branch_delete_failed");
        }
        _ => {}
    }

    info!(worktree_name = %worktree_name, "merged_worktree_cleaned_up");
}

/// Create or reuse a worktree with the given name.
/// If a worktree directory already exists at `<repo-root>/<worktree_name>`,
/// reuses it (all previous commits are preserved).
/// Returns the worktree name and path, or None on failure.
pub fn create_or_reuse_worktree(bd_path: &str, worktree_name: &str) -> Option<(String, PathBuf)> {
    let worktree_path = resolve_worktree_path(worktree_name);

    // Check if worktree already exists — reuse it
    if worktree_path.exists() {
        symlink_settings_local(&worktree_path);
        info!(
            worktree_name = %worktree_name,
            worktree_path = %worktree_path.display(),
            "worktree_reused"
        );
        return Some((worktree_name.to_string(), worktree_path));
    }

    // Create new worktree
    let wt_output = Command::new(bd_path)
        .args(["worktree", "create", worktree_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let wt_output = match wt_output {
        Ok(o) => o,
        Err(e) => {
            warn!(error = %e, "worktree_create_failed");
            return None;
        }
    };

    if !wt_output.status.success() {
        let stderr = String::from_utf8_lossy(&wt_output.stderr);
        warn!(stderr = %stderr.trim(), "worktree_create_failed");
        return None;
    }

    symlink_settings_local(&worktree_path);

    info!(
        worktree_name = %worktree_name,
        worktree_path = %worktree_path.display(),
        "worktree_created"
    );

    Some((worktree_name.to_string(), worktree_path))
}

/// Get the repo root path.
fn repo_root() -> PathBuf {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| PathBuf::from(s.trim()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// Clean up agent resources: release hook, close agent bead, merge + remove worktree.
pub fn cleanup(bd_path: &str, agent_bead_id: &str, worktree_name: &str) {
    info!(agent_bead_id = %agent_bead_id, "agent_cleanup_start");

    // Release any hooked bead
    release_hook(bd_path, agent_bead_id);

    // Close the agent bead
    cleanup_agent_bead(bd_path, agent_bead_id);

    // Try to merge worktree branch to main before removal
    if merge_worktree_to_main(worktree_name) {
        remove_merged_worktree(bd_path, worktree_name);
    } else {
        // Merge failed — leave worktree intact so user can resolve
        warn!(worktree_name = %worktree_name, "session_end_merge_failed_worktree_preserved");
    }
}

/// Close an agent bead (used during cleanup or when worktree creation fails).
fn cleanup_agent_bead(bd_path: &str, agent_bead_id: &str) {
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
fn parse_bead_id(json_output: &str) -> Option<String> {
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
fn epoch_secs_to_iso8601(secs: u64) -> String {
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

/// Find stale agent beads that have hooked beads but stopped heartbeating.
///
/// Queries for in_progress agent beads with `rig:ralph` label that haven't been
/// updated within `stale_threshold_secs`. For each, checks if they have a hook
/// (a claimed bead). Returns only those with active hooks.
pub fn find_stale_agents(
    bd_path: &str,
    stale_threshold_secs: u64,
    exclude_agent_id: Option<&str>,
) -> Vec<StaleAgent> {
    // Calculate cutoff time (now - threshold) as ISO 8601
    let cutoff = cutoff_time_iso(stale_threshold_secs);
    let cutoff = match cutoff {
        Some(c) => c,
        None => {
            warn!("stale_check_cutoff_time_failed");
            return Vec::new();
        }
    };

    // Find in_progress agent beads updated before cutoff
    let output = Command::new(bd_path)
        .args([
            "list",
            "--json",
            "--label",
            "rig:ralph",
            "--status",
            "in_progress",
            "--include-infra",
            "--updated-before",
            &cutoff,
            "--limit",
            "0",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "stale_list_failed");
            return Vec::new();
        }
        Err(e) => {
            warn!(error = %e, "stale_list_failed");
            return Vec::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut stale_agents = Vec::new();

    for item in &items {
        let agent_id = match item.get("id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        // Skip our own agent
        if exclude_agent_id.is_some_and(|eid| eid == agent_id) {
            continue;
        }

        // Get labels via bd show to find the hook state
        let hook_bead_id = match get_hook_from_labels(bd_path, agent_id) {
            Some(id) => id,
            None => {
                // No hook — agent finished work but session ended without cleanup.
                // Close the agent bead and remove its worktree.
                cleanup_agent_bead(bd_path, agent_id);
                let _ = Command::new(bd_path)
                    .args(["worktree", "remove", "--force", agent_id])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output();
                info!(agent_id = %agent_id, "hookless_stale_agent_cleaned_up");
                continue;
            }
        };

        // Get the hooked bead's title
        let hooked_title = get_bead_title(bd_path, &hook_bead_id);

        // Worktree name: epic ID if the agent had an epic, otherwise agent bead ID
        let epic_id = get_epic_from_state(bd_path, agent_id);
        let has_epic = epic_id.is_some();
        let worktree_name = epic_id.unwrap_or_else(|| agent_id.to_string());

        stale_agents.push(StaleAgent {
            agent_bead_id: agent_id.to_string(),
            hooked_bead_id: hook_bead_id,
            hooked_bead_title: hooked_title,
            worktree_name,
            has_epic,
        });
    }

    if !stale_agents.is_empty() {
        info!(count = stale_agents.len(), "stale_agents_found");
    }

    stale_agents
}

/// Resume a stale bead: claim it on our agent, mark old agent dead.
/// If the bead was already retried once (has retry:1 label), escalate to human instead.
pub fn resume_stale_bead(bd_path: &str, new_agent_id: &str, stale: &StaleAgent) -> ResumeResult {
    // Check if this bead was already retried once
    if has_label(bd_path, &stale.hooked_bead_id, "retry:1") {
        escalate_to_human(bd_path, stale);
        return ResumeResult::EscalatedToHuman;
    }

    // Clear hook on stale agent
    release_hook(bd_path, &stale.agent_bead_id);

    // Set hook on our agent for the stale bead
    let hook_arg = format!("hook={}", stale.hooked_bead_id);
    let result = Command::new(bd_path)
        .args(["set-state", new_agent_id, &hook_arg])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    let hooked = match result {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "resume_hook_failed");
            false
        }
        Err(e) => {
            warn!(error = %e, "resume_hook_failed");
            false
        }
    };

    // Close stale agent bead
    cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Only remove worktree for non-epic agents. Epic worktrees are reused
    // by the next worker who picks up the same epic.
    if !stale.has_epic {
        let _ = Command::new(bd_path)
            .args(["worktree", "remove", &stale.worktree_name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
    }

    if hooked {
        // Mark retry:1 so next stale detection escalates to human
        let _ = Command::new(bd_path)
            .args([
                "set-state",
                &stale.hooked_bead_id,
                "retry=1",
                "--reason=auto-reclaimed from stale agent",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();

        info!(
            new_agent = %new_agent_id,
            stale_agent = %stale.agent_bead_id,
            bead = %stale.hooked_bead_id,
            "stale_bead_resumed"
        );
        ResumeResult::Resumed
    } else {
        ResumeResult::Failed
    }
}

/// Release a stale bead: clear hook, reset bead to open, clean up agent.
pub fn release_stale_bead(bd_path: &str, stale: &StaleAgent) {
    // Clear hook and reset bead to open
    release_bead(bd_path, &stale.agent_bead_id, &stale.hooked_bead_id);

    // Close stale agent bead
    cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Only remove worktree for non-epic agents
    if !stale.has_epic {
        let _ = Command::new(bd_path)
            .args(["worktree", "remove", &stale.worktree_name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
    }

    info!(
        stale_agent = %stale.agent_bead_id,
        bead = %stale.hooked_bead_id,
        "stale_bead_released"
    );
}

/// Check if a bead has a specific label (e.g. "retry:1", "human").
fn has_label(bd_path: &str, bead_id: &str, target: &str) -> bool {
    let output = Command::new(bd_path)
        .args(["show", bead_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    let Ok(o) = output else { return false };
    if !o.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&o.stdout);
    let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) else {
        return false;
    };
    let Some(item) = items.first() else {
        return false;
    };

    item.get("labels")
        .and_then(|l| l.as_array())
        .is_some_and(|labels| {
            labels
                .iter()
                .any(|l| l.as_str().is_some_and(|s| s == target))
        })
}

/// Escalate a stuck bead to human review: release it, flag it, add a comment.
fn escalate_to_human(bd_path: &str, stale: &StaleAgent) {
    // Release the hook and reset bead to open
    release_bead(bd_path, &stale.agent_bead_id, &stale.hooked_bead_id);

    // Close stale agent bead
    cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Only remove worktree for non-epic agents
    if !stale.has_epic {
        let _ = Command::new(bd_path)
            .args(["worktree", "remove", &stale.worktree_name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
    }

    // Flag for human review
    let _ = Command::new(bd_path)
        .args(["label", "add", &stale.hooked_bead_id, "human"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    // Add comment explaining why
    let comment = format!(
        "Auto-escalated: bead went stale twice (reclaimed once, went stale again). \
         Last stale agent: {}",
        stale.agent_bead_id,
    );
    let _ = Command::new(bd_path)
        .args(["comments", "add", &stale.hooked_bead_id, &comment])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    info!(
        stale_agent = %stale.agent_bead_id,
        bead = %stale.hooked_bead_id,
        "stale_bead_escalated_to_human"
    );
}

/// Get the hook value from an agent bead's labels.
/// Returns None if no hook is set or hook is "none".
fn get_hook_from_labels(bd_path: &str, agent_id: &str) -> Option<String> {
    let output = Command::new(bd_path)
        .args(["show", agent_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;
    let item = items.first()?;

    let labels = item.get("labels")?.as_array()?;
    for label in labels {
        if let Some(s) = label.as_str()
            && let Some(hook_val) = s.strip_prefix("hook:")
            && hook_val != "none"
        {
            return Some(hook_val.to_string());
        }
    }
    None
}

/// Get a bead's title via bd show.
fn get_bead_title(bd_path: &str, bead_id: &str) -> String {
    let output = Command::new(bd_path)
        .args(["show", bead_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
                .ok()
                .and_then(|items| items.first().cloned())
                .and_then(|item| item.get("title").and_then(|t| t.as_str()).map(String::from))
                .unwrap_or_else(|| bead_id.to_string())
        }
        _ => bead_id.to_string(),
    }
}

/// Calculate a cutoff time (now - threshold_secs) as ISO 8601 UTC string.
fn cutoff_time_iso(threshold_secs: u64) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let cutoff = now.as_secs().checked_sub(threshold_secs)?;
    Some(epoch_secs_to_iso8601(cutoff))
}

/// Resolve the worktree path by asking git.
fn resolve_worktree_path(worktree_name: &str) -> PathBuf {
    repo_root().join(worktree_name)
}

/// Symlink .claude/settings.local.json from the main repo into a worktree.
fn symlink_settings_local(worktree_path: &std::path::Path) {
    let main_root = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| PathBuf::from(s.trim()));

    let Some(main_root) = main_root else {
        return;
    };

    let source = main_root.join(".claude").join("settings.local.json");
    if !source.exists() {
        return;
    }

    let target_dir = worktree_path.join(".claude");
    if !target_dir.exists()
        && let Err(e) = std::fs::create_dir_all(&target_dir)
    {
        warn!(error = %e, "settings_local_symlink_mkdir_failed");
        return;
    }

    let target = target_dir.join("settings.local.json");
    if target.exists() {
        return;
    }

    match std::os::unix::fs::symlink(&source, &target) {
        Ok(()) => info!(
            source = %source.display(),
            target = %target.display(),
            "settings_local_symlinked"
        ),
        Err(e) => warn!(
            error = %e,
            source = %source.display(),
            target = %target.display(),
            "settings_local_symlink_failed"
        ),
    }
}

/// Result of selecting and claiming an epic.
pub struct EpicClaim {
    pub epic_id: String,
    pub child_bead_id: String,
    pub child_title: String,
}

/// Scored epic for selection ranking.
pub(crate) struct ScoredEpic {
    pub epic_id: String,
    pub priority: i64,
    pub ready_children: usize,
}

// --- Pure functions (testable, no I/O) ---

/// Filter beads to only those claimable by Ralph (exclude human/shaping labels).
pub fn filter_claimable_beads(items: &[serde_json::Value]) -> Vec<&serde_json::Value> {
    items
        .iter()
        .filter(|item| {
            let dominated_by_labels =
                item.get("labels")
                    .and_then(|l| l.as_array())
                    .is_some_and(|ls| {
                        ls.iter().any(|l| {
                            l.as_str().is_some_and(|s| {
                                matches!(
                                    s,
                                    "needs-shaping"
                                        | "shaping-required"
                                        | "human"
                                        | "needs-brain-dump"
                                )
                            })
                        })
                    });
            !dominated_by_labels
        })
        .collect()
}

/// Group claimable beads by their parent epic ID.
/// Returns a map of parent_id → Vec<bead>. Beads without a parent use None key.
pub fn group_beads_by_parent<'a>(
    beads: &[&'a serde_json::Value],
) -> HashMap<Option<String>, Vec<&'a serde_json::Value>> {
    let mut groups: HashMap<Option<String>, Vec<&'a serde_json::Value>> = HashMap::new();
    for bead in beads {
        let parent = bead
            .get("parent")
            .and_then(|p| p.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from);
        groups.entry(parent).or_default().push(bead);
    }
    groups
}

/// Identify standalone beads (those without a parent epic).
#[cfg(test)]
pub fn find_standalone_beads<'a>(beads: &[&'a serde_json::Value]) -> Vec<&'a serde_json::Value> {
    beads
        .iter()
        .filter(|b| {
            b.get("parent")
                .and_then(|p| p.as_str())
                .map_or(true, |s| s.is_empty())
        })
        .copied()
        .collect()
}

/// Score an epic for selection. Lower score = better (first pick).
/// Priority is the dominant factor (lower priority number = better).
/// Ready children count breaks ties (more children = more work available = better).
pub fn score_epic(priority: i64, ready_children: usize) -> i64 {
    priority * 100 - ready_children as i64 * 10
}

/// Select the best epic from scored candidates. Returns the epic ID with lowest score.
pub fn select_best_epic(scored: &[ScoredEpic]) -> Option<&str> {
    scored
        .iter()
        .min_by_key(|e| score_epic(e.priority, e.ready_children))
        .map(|e| e.epic_id.as_str())
}

/// What to do between iterations when an epic is active.
#[derive(Debug, PartialEq)]
pub enum IterationAction {
    /// Epic still has ready children — reuse worktree and claim next child.
    ContinueInEpic,
    /// Epic has no ready children — complete it, merge worktree, select new epic.
    CompleteEpicAndMerge,
    /// No active epic — just merge any existing worktree.
    MergeOnly,
}

/// Decide what action to take between iterations based on epic state.
pub fn decide_iteration_action(has_epic: bool, has_ready_children: bool) -> IterationAction {
    match (has_epic, has_ready_children) {
        (true, true) => IterationAction::ContinueInEpic,
        (true, false) => IterationAction::CompleteEpicAndMerge,
        (false, _) => IterationAction::MergeOnly,
    }
}

/// Build prompt context for a dirty worktree (uncommitted changes).
pub fn build_dirty_worktree_context(git_status: &str, git_diff: &str) -> String {
    format!(
        r#"## Dirty Worktree

This worktree has uncommitted changes from a previous worker session.
Analyze these changes. If you understand the intent and can continue, do so.
If the changes are unclear or problematic, escalate via `bd human <bead>` with an explanation.

### git status
```
{git_status}
```

### git diff
```
{git_diff}
```
"#
    )
}

// --- I/O functions for epic-scoped workflow ---

/// Select and claim the best epic, then claim its first child bead.
pub fn select_and_claim_epic(bd_path: &str, agent_bead_id: &str) -> Option<EpicClaim> {
    let output = Command::new(bd_path)
        .args(["ready", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "epic_ready_list_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;
    let claimable = filter_claimable_beads(&items);

    if claimable.is_empty() {
        info!("no_claimable_beads_for_epic_selection");
        return None;
    }

    let mut groups = group_beads_by_parent(&claimable);

    // Wrap standalone beads in auto-generated epics
    if let Some(standalone) = groups.remove(&None) {
        for bead in standalone {
            let bead_id = match bead.get("id").and_then(|v| v.as_str()) {
                Some(id) => id,
                None => continue,
            };
            let bead_title = bead.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let bead_priority = bead.get("priority").and_then(|v| v.as_i64()).unwrap_or(2);

            if let Some(epic_id) = wrap_standalone_bead(bd_path, bead_id, bead_title, bead_priority)
            {
                groups.entry(Some(epic_id)).or_default().push(bead);
            } else {
                warn!(bead_id = %bead_id, "standalone_wrap_failed_skipping");
            }
        }
    }

    // Score epics by priority and ready children count
    let scored: Vec<ScoredEpic> = groups
        .iter()
        .filter_map(
            |(parent_id, children): (&Option<String>, &Vec<&serde_json::Value>)| {
                let epic_id = parent_id.as_ref()?;
                let priority = get_bead_priority(bd_path, epic_id);
                Some(ScoredEpic {
                    epic_id: epic_id.clone(),
                    priority,
                    ready_children: children.len(),
                })
            },
        )
        .collect();

    if scored.is_empty() {
        info!("no_epics_to_score");
        return None;
    }

    let best_epic_id = select_best_epic(&scored)?.to_string();

    // Claim the epic
    let claim_result = Command::new(bd_path)
        .args(["update", &best_epic_id, "--claim"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match claim_result {
        Ok(o) if o.status.success() => {
            info!(epic_id = %best_epic_id, "epic_claimed");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            info!(epic_id = %best_epic_id, stderr = %stderr.trim(), "epic_claim_failed");
            return None;
        }
        Err(e) => {
            warn!(epic_id = %best_epic_id, error = %e, "epic_claim_command_failed");
            return None;
        }
    }

    // Record epic on agent state
    let epic_arg = format!("epic={}", best_epic_id);
    let _ = Command::new(bd_path)
        .args(["set-state", agent_bead_id, &epic_arg])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    // Claim first child within the epic
    match claim_next_child(bd_path, agent_bead_id, &best_epic_id) {
        Some((child_id, child_title)) => Some(EpicClaim {
            epic_id: best_epic_id,
            child_bead_id: child_id,
            child_title,
        }),
        None => {
            warn!(epic_id = %best_epic_id, "epic_claimed_but_no_children_ready");
            None
        }
    }
}

/// Build the `bd create` args for wrapping a standalone bead in an epic.
fn wrap_create_args(title: &str, priority: i64) -> Vec<String> {
    vec![
        "create".into(),
        "--type=epic".into(),
        format!("--title={}", title),
        format!("--priority={}", priority),
        "--json".into(),
    ]
}

/// Build the `bd update` args for reparenting a bead under an epic.
fn wrap_reparent_args(bead_id: &str, epic_id: &str) -> Vec<String> {
    vec![
        "update".into(),
        bead_id.into(),
        format!("--parent={}", epic_id),
    ]
}

/// Create a wrapper epic for a standalone bead, reparenting it.
fn wrap_standalone_bead(
    bd_path: &str,
    bead_id: &str,
    bead_title: &str,
    bead_priority: i64,
) -> Option<String> {
    let create_args = wrap_create_args(bead_title, bead_priority);
    let output = Command::new(bd_path)
        .args(&create_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(bead_id = %bead_id, stderr = %stderr.trim(), "wrap_epic_create_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let epic_id = parse_bead_id(&stdout)?;

    // Reparent the standalone bead under the new epic
    let reparent_args = wrap_reparent_args(bead_id, &epic_id);
    let result = Command::new(bd_path)
        .args(&reparent_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(bead_id = %bead_id, epic_id = %epic_id, "standalone_bead_wrapped");
            Some(epic_id)
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(bead_id = %bead_id, stderr = %stderr.trim(), "wrap_reparent_failed");
            None
        }
        Err(e) => {
            warn!(bead_id = %bead_id, error = %e, "wrap_reparent_command_failed");
            None
        }
    }
}

/// Claim the next ready child bead within an epic.
pub fn claim_next_child(
    bd_path: &str,
    agent_bead_id: &str,
    epic_id: &str,
) -> Option<(String, String)> {
    let output = Command::new(bd_path)
        .args(["ready", "--parent", epic_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(epic_id = %epic_id, stderr = %stderr.trim(), "child_ready_list_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;
    let claimable = filter_claimable_beads(&items);

    for bead in &claimable {
        let id = match bead.get("id").and_then(|i| i.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let title = bead.get("title").and_then(|t| t.as_str()).unwrap_or("");

        // Atomic claim
        let claim = Command::new(bd_path)
            .args(["update", id, "--claim"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output();

        match claim {
            Ok(o) if o.status.success() => {
                // Record hook on agent bead
                let hook_arg = format!("hook={}", id);
                let _ = Command::new(bd_path)
                    .args(["set-state", agent_bead_id, &hook_arg])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output();

                // Assess specification
                if !assess_bead_specification(bd_path, id, agent_bead_id) {
                    info!(bead_id = %id, "child_rejected_trying_next");
                    continue;
                }

                info!(bead_id = %id, title = %title, epic_id = %epic_id, "child_claimed");
                return Some((id.to_string(), title.to_string()));
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                info!(bead_id = %id, stderr = %stderr.trim(), "child_claim_failed_trying_next");
            }
            Err(e) => {
                warn!(bead_id = %id, error = %e, "child_claim_command_failed");
            }
        }
    }

    info!(epic_id = %epic_id, "no_claimable_children");
    None
}

/// Complete an epic: close it if eligible.
pub fn complete_epic(bd_path: &str, epic_id: &str) -> bool {
    let result = Command::new(bd_path)
        .args(["epic", "close-eligible"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(epic_id = %epic_id, "epic_close_eligible_run");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(epic_id = %epic_id, stderr = %stderr.trim(), "epic_close_eligible_failed");
            false
        }
        Err(e) => {
            warn!(epic_id = %epic_id, error = %e, "epic_close_eligible_command_failed");
            false
        }
    }
}

/// Get a bead's priority via bd show. Returns 2 (medium) if not found.
fn get_bead_priority(bd_path: &str, bead_id: &str) -> i64 {
    let output = Command::new(bd_path)
        .args(["show", bead_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
                .ok()
                .and_then(|items| items.first().cloned())
                .and_then(|item| item.get("priority").and_then(|p| p.as_i64()))
                .unwrap_or(2)
        }
        _ => 2,
    }
}

/// Check if a worktree has uncommitted changes.
/// Returns Some((git_status, git_diff)) if dirty, None if clean.
pub fn check_worktree_dirty(worktree_path: &std::path::Path) -> Option<(String, String)> {
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    let status = String::from_utf8_lossy(&status_output.stdout).to_string();
    if status.trim().is_empty() {
        return None;
    }

    let diff_output = Command::new("git")
        .args(["diff"])
        .current_dir(worktree_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
    Some((status, diff))
}

/// Get the epic ID from an agent bead's state labels.
pub fn get_epic_from_state(bd_path: &str, agent_id: &str) -> Option<String> {
    let output = Command::new(bd_path)
        .args(["show", agent_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;
    let item = items.first()?;

    let labels = item.get("labels")?.as_array()?;
    for label in labels {
        if let Some(s) = label.as_str()
            && let Some(epic_val) = s.strip_prefix("epic:")
            && epic_val != "none"
        {
            return Some(epic_val.to_string());
        }
    }
    None
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

    // --- Epic selection tests ---

    #[test]
    fn filter_claimable_beads_excludes_human_and_shaping_labels() {
        let items = vec![
            json!({"id": "b1", "title": "good", "labels": []}),
            json!({"id": "b2", "title": "human", "labels": ["human"]}),
            json!({"id": "b3", "title": "shaping", "labels": ["needs-shaping"]}),
            json!({"id": "b4", "title": "also good", "labels": ["feature"]}),
            json!({"id": "b5", "title": "brain dump", "labels": ["needs-brain-dump"]}),
            json!({"id": "b6", "title": "shaping req", "labels": ["shaping-required"]}),
        ];
        let result = filter_claimable_beads(&items);
        let ids: Vec<&str> = result
            .iter()
            .filter_map(|b| b.get("id").and_then(|i| i.as_str()))
            .collect();
        assert_eq!(ids, vec!["b1", "b4"]);
    }

    #[test]
    fn filter_claimable_beads_includes_all_when_no_skip_labels() {
        let items = vec![
            json!({"id": "b1", "title": "a", "labels": ["feature"]}),
            json!({"id": "b2", "title": "b"}),
        ];
        let result = filter_claimable_beads(&items);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn group_beads_by_parent_groups_correctly() {
        let items = vec![
            json!({"id": "b1", "parent": "epic-1"}),
            json!({"id": "b2", "parent": "epic-1"}),
            json!({"id": "b3", "parent": "epic-2"}),
            json!({"id": "b4"}),
            json!({"id": "b5", "parent": ""}),
        ];
        let refs: Vec<&serde_json::Value> = items.iter().collect();
        let groups = group_beads_by_parent(&refs);

        assert_eq!(groups.get(&Some("epic-1".into())).unwrap().len(), 2);
        assert_eq!(groups.get(&Some("epic-2".into())).unwrap().len(), 1);
        assert_eq!(groups.get(&None).unwrap().len(), 2);
    }

    #[test]
    fn find_standalone_beads_returns_beads_without_parent() {
        let items = vec![
            json!({"id": "b1", "parent": "epic-1"}),
            json!({"id": "b2"}),
            json!({"id": "b3", "parent": ""}),
            json!({"id": "b4", "parent": "epic-2"}),
        ];
        let refs: Vec<&serde_json::Value> = items.iter().collect();
        let standalone = find_standalone_beads(&refs);
        let ids: Vec<&str> = standalone
            .iter()
            .filter_map(|b| b.get("id").and_then(|i| i.as_str()))
            .collect();
        assert_eq!(ids, vec!["b2", "b3"]);
    }

    // --- Standalone bead wrapping command tests ---

    #[test]
    fn wrap_create_args_produces_epic_create_command() {
        let args = wrap_create_args("Fix login bug", 1);
        assert_eq!(
            args,
            vec!["create", "--type=epic", "--title=Fix login bug", "--priority=1", "--json"]
        );
    }

    #[test]
    fn wrap_reparent_args_produces_update_parent_command() {
        let args = wrap_reparent_args("beads-abc", "beads-xyz");
        assert_eq!(args, vec!["update", "beads-abc", "--parent=beads-xyz"]);
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

    #[test]
    fn score_epic_lower_priority_wins() {
        assert!(score_epic(0, 1) < score_epic(2, 1));
    }

    #[test]
    fn score_epic_more_children_breaks_tie() {
        assert!(score_epic(1, 5) < score_epic(1, 2));
    }

    #[test]
    fn select_best_epic_picks_lowest_score() {
        let scored = vec![
            ScoredEpic {
                epic_id: "epic-a".into(),
                priority: 2,
                ready_children: 3,
            },
            ScoredEpic {
                epic_id: "epic-b".into(),
                priority: 0,
                ready_children: 1,
            },
            ScoredEpic {
                epic_id: "epic-c".into(),
                priority: 1,
                ready_children: 2,
            },
        ];
        assert_eq!(select_best_epic(&scored), Some("epic-b"));
    }

    #[test]
    fn select_best_epic_returns_none_for_empty() {
        let scored: Vec<ScoredEpic> = vec![];
        assert_eq!(select_best_epic(&scored), None);
    }

    #[test]
    fn build_dirty_worktree_context_includes_status_and_diff() {
        let ctx = build_dirty_worktree_context("M src/main.rs", "+new line\n-old line");
        assert!(ctx.contains("M src/main.rs"));
        assert!(ctx.contains("+new line"));
        assert!(ctx.contains("previous worker session"));
    }

    // --- Iteration action tests ---

    #[test]
    fn decide_iteration_continues_when_children_ready() {
        assert_eq!(
            decide_iteration_action(true, true),
            IterationAction::ContinueInEpic,
        );
    }

    #[test]
    fn decide_iteration_completes_epic_when_children_exhausted() {
        assert_eq!(
            decide_iteration_action(true, false),
            IterationAction::CompleteEpicAndMerge,
        );
    }

    #[test]
    fn decide_iteration_merges_only_when_no_epic() {
        assert_eq!(
            decide_iteration_action(false, false),
            IterationAction::MergeOnly,
        );
    }

    #[test]
    fn decide_iteration_merges_when_no_epic_ignores_children_flag() {
        assert_eq!(
            decide_iteration_action(false, true),
            IterationAction::MergeOnly,
        );
    }
}
