//! Stale agent detection and recovery.

use std::process::Command;

use tracing::{info, warn};

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
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
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
            .output()
    });

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
                super::lifecycle::cleanup_agent_bead(bd_path, agent_id);
                let _ = crate::bd_lock::with_lock(|| {
                    Command::new(bd_path)
                        .args(["worktree", "remove", "--force", agent_id])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                });
                info!(agent_id = %agent_id, "hookless_stale_agent_cleaned_up");
                continue;
            }
        };

        // Get the hooked bead's title
        let hooked_title = get_bead_title(bd_path, &hook_bead_id);

        // Worktree name: epic ID if the agent had an epic, otherwise agent bead ID
        let epic_id = super::epic::get_epic_from_state(bd_path, agent_id);
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
    super::lifecycle::release_hook(bd_path, &stale.agent_bead_id);

    // Set hook on our agent for the stale bead
    let hook_arg = format!("hook={}", stale.hooked_bead_id);
    let result = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["set-state", new_agent_id, &hook_arg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
    });

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
    super::lifecycle::cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Only remove worktree for non-epic agents. Epic worktrees are reused
    // by the next worker who picks up the same epic.
    if !stale.has_epic {
        let _ = crate::bd_lock::with_lock(|| {
            Command::new(bd_path)
                .args(["worktree", "remove", &stale.worktree_name])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
        });
    }

    if hooked {
        // Mark retry:1 so next stale detection escalates to human
        let _ = crate::bd_lock::with_lock(|| {
            Command::new(bd_path)
                .args([
                    "set-state",
                    &stale.hooked_bead_id,
                    "retry=1",
                    "--reason=auto-reclaimed from stale agent",
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
        });

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
    super::lifecycle::release_bead(bd_path, &stale.agent_bead_id, &stale.hooked_bead_id);

    // Close stale agent bead
    super::lifecycle::cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Only remove worktree for non-epic agents
    if !stale.has_epic {
        let _ = crate::bd_lock::with_lock(|| {
            Command::new(bd_path)
                .args(["worktree", "remove", &stale.worktree_name])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
        });
    }

    info!(
        stale_agent = %stale.agent_bead_id,
        bead = %stale.hooked_bead_id,
        "stale_bead_released"
    );
}

/// Check if a bead has a specific label (e.g. "retry:1", "human").
fn has_label(bd_path: &str, bead_id: &str, target: &str) -> bool {
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["show", bead_id, "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
    });

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
    super::lifecycle::release_bead(bd_path, &stale.agent_bead_id, &stale.hooked_bead_id);

    // Close stale agent bead
    super::lifecycle::cleanup_agent_bead(bd_path, &stale.agent_bead_id);

    // Only remove worktree for non-epic agents
    if !stale.has_epic {
        let _ = crate::bd_lock::with_lock(|| {
            Command::new(bd_path)
                .args(["worktree", "remove", &stale.worktree_name])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
        });
    }

    // Flag for human review
    let _ = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["label", "add", &stale.hooked_bead_id, "human"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
    });

    // Add comment explaining why
    let comment = format!(
        "Auto-escalated: bead went stale twice (reclaimed once, went stale again). \
         Last stale agent: {}",
        stale.agent_bead_id,
    );
    let _ = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["comments", "add", &stale.hooked_bead_id, &comment])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
    });

    info!(
        stale_agent = %stale.agent_bead_id,
        bead = %stale.hooked_bead_id,
        "stale_bead_escalated_to_human"
    );
}

/// Get the hook value from an agent bead's labels.
/// Returns None if no hook is set or hook is "none".
fn get_hook_from_labels(bd_path: &str, agent_id: &str) -> Option<String> {
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["show", agent_id, "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
    })
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
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["show", bead_id, "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
    });

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
    Some(super::lifecycle::epoch_secs_to_iso8601(cutoff))
}
