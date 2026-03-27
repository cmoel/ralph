//! Agent lifecycle: registration, worktree creation, heartbeat, and cleanup.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tracing::{info, warn};

/// Result of agent registration and worktree setup.
pub struct AgentSetup {
    pub agent_bead_id: String,
    pub worktree_name: String,
    pub worktree_path: PathBuf,
}

/// A stale agent detected during recovery.
#[derive(Clone)]
pub struct StaleAgent {
    pub agent_bead_id: String,
    pub hooked_bead_id: String,
    pub hooked_bead_title: String,
    pub worktree_name: String,
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
            "--no-history",
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

    // Create worktree named after agent bead ID
    let worktree_name = agent_bead_id.clone();
    let wt_output = Command::new(bd_path)
        .args(["worktree", "create", &worktree_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();

    let wt_output = match wt_output {
        Ok(o) => o,
        Err(e) => {
            warn!(error = %e, "worktree_create_failed");
            cleanup_agent_bead(bd_path, &agent_bead_id);
            return None;
        }
    };

    if !wt_output.status.success() {
        let stderr = String::from_utf8_lossy(&wt_output.stderr);
        warn!(stderr = %stderr.trim(), "worktree_create_failed");
        cleanup_agent_bead(bd_path, &agent_bead_id);
        return None;
    }

    // Resolve worktree path (it's created at ./<name> relative to repo root)
    let worktree_path = resolve_worktree_path(&worktree_name);
    info!(
        agent_bead_id = %agent_bead_id,
        worktree_name = %worktree_name,
        worktree_path = %worktree_path.display(),
        "agent_registered"
    );

    Some(AgentSetup {
        agent_bead_id,
        worktree_name,
        worktree_path,
    })
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

/// Pick the next ready bead and claim it for this agent.
///
/// Iterates through `bd ready --json` results, skipping beads with shaping labels.
/// Uses `bd update --claim` for atomic claiming (fails if already claimed).
/// On success, records the hook on the agent bead via `bd set-state`.
///
/// Returns `(bead_id, title)` on success, `None` if no claimable work.
pub fn claim_next_bead(bd_path: &str, agent_bead_id: &str) -> Option<(String, String)> {
    let output = Command::new(bd_path)
        .args(["ready", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "claim_ready_list_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: serde_json::Value = serde_json::from_str(stdout.as_ref()).ok()?;
    let arr = items.as_array()?;

    for item in arr {
        // Skip beads that need shaping or are human-only
        let should_skip = item
            .get("labels")
            .and_then(|l| l.as_array())
            .is_some_and(|ls| {
                ls.iter().any(|l| {
                    l.as_str().is_some_and(|s| {
                        matches!(s, "needs-shaping" | "shaping-required" | "no-ralph")
                    })
                })
            });
        if should_skip {
            continue;
        }

        let id = match item.get("id").and_then(|i| i.as_str()) {
            Some(id) => id,
            None => continue,
        };
        let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");

        // Atomic claim — fails if already claimed by another agent
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

                info!(bead_id = %id, title = %title, "bead_claimed");
                return Some((id.to_string(), title.to_string()));
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                info!(bead_id = %id, stderr = %stderr.trim(), "claim_failed_trying_next");
            }
            Err(e) => {
                warn!(bead_id = %id, error = %e, "claim_command_failed");
            }
        }
    }

    info!("no_claimable_beads");
    None
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
fn reset_bead_to_open(bd_path: &str, bead_id: &str) {
    let result = Command::new(bd_path)
        .args(["update", bead_id, "--status=open"])
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

/// Clean up agent resources: release hook, close agent bead, remove worktree.
pub fn cleanup(bd_path: &str, agent_bead_id: &str, worktree_name: &str) {
    info!(agent_bead_id = %agent_bead_id, "agent_cleanup_start");

    // Release any hooked bead
    release_hook(bd_path, agent_bead_id);

    // Close the agent bead
    cleanup_agent_bead(bd_path, agent_bead_id);

    // Remove the worktree
    let result = Command::new(bd_path)
        .args(["worktree", "remove", worktree_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output();

    match result {
        Ok(o) if o.status.success() => {
            info!(worktree_name = %worktree_name, "worktree_removed");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            warn!(stderr = %stderr.trim(), "worktree_remove_failed");
        }
        Err(e) => {
            warn!(error = %e, "worktree_remove_failed");
        }
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
    // bd create --json outputs JSON with an "id" field
    for line in json_output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line)
            && let Some(id) = value.get("id").and_then(|v| v.as_str())
        {
            return Some(id.to_string());
        }
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
            None => continue, // No hook or hook:none
        };

        // Get the hooked bead's title
        let hooked_title = get_bead_title(bd_path, &hook_bead_id);

        // Worktree name matches agent bead ID (set during registration)
        let worktree_name = agent_id.to_string();

        stale_agents.push(StaleAgent {
            agent_bead_id: agent_id.to_string(),
            hooked_bead_id: hook_bead_id,
            hooked_bead_title: hooked_title,
            worktree_name,
        });
    }

    if !stale_agents.is_empty() {
        info!(count = stale_agents.len(), "stale_agents_found");
    }

    stale_agents
}

/// Resume a stale bead: claim it on our agent, mark old agent dead.
/// Returns true on success.
pub fn resume_stale_bead(bd_path: &str, new_agent_id: &str, stale: &StaleAgent) -> bool {
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

    // Try to remove stale worktree (non-fatal if it fails)
    let _ = Command::new(bd_path)
        .args(["worktree", "remove", &stale.worktree_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    if hooked {
        info!(
            new_agent = %new_agent_id,
            stale_agent = %stale.agent_bead_id,
            bead = %stale.hooked_bead_id,
            "stale_bead_resumed"
        );
    }

    hooked
}

/// Release a stale bead: clear hook, reset bead to open, clean up agent.
pub fn release_stale_bead(bd_path: &str, stale: &StaleAgent) {
    // Clear hook and reset bead to open
    release_bead(bd_path, &stale.agent_bead_id, &stale.hooked_bead_id);

    // Close stale agent bead
    cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Try to remove stale worktree (non-fatal if it fails)
    let _ = Command::new(bd_path)
        .args(["worktree", "remove", &stale.worktree_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();

    info!(
        stale_agent = %stale.agent_bead_id,
        bead = %stale.hooked_bead_id,
        "stale_bead_released"
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
    // Get the repo root, worktree is created relative to it
    let root = Command::new("git")
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
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    root.join(worktree_name)
}
