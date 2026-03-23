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
        // Skip beads that need shaping
        let has_shaping = item
            .get("labels")
            .and_then(|l| l.as_array())
            .is_some_and(|ls| {
                ls.iter().any(|l| {
                    l.as_str()
                        .is_some_and(|s| matches!(s, "needs-shaping" | "shaping-required"))
                })
            });
        if has_shaping {
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
    warn!(output = %json_output, "could_not_parse_bead_id");
    None
}

/// Get current time as ISO 8601 string (without pulling in chrono crate).
fn chrono_now_iso() -> String {
    // Use system command for simplicity since we don't have chrono
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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
