//! Epic selection, claiming, and iteration management.

use std::collections::HashMap;
use std::process::Command;

use tracing::{info, warn};

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
            let has_human_label = item
                .get("labels")
                .and_then(|l| l.as_array())
                .is_some_and(|ls| ls.iter().any(|l| l.as_str().is_some_and(|s| s == "human")));
            !has_human_label
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

    // Score epics by priority and ready children count, skipping stuck epics
    let scored: Vec<ScoredEpic> = groups
        .iter()
        .filter_map(
            |(parent_id, children): (&Option<String>, &Vec<&serde_json::Value>)| {
                let epic_id = parent_id.as_ref()?;
                if children.is_empty() {
                    return None;
                }
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
            // Release the epic so another agent can pick it up
            let _ = Command::new(bd_path)
                .args(["update", &best_epic_id, "--status=open", "--assignee="])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
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
    let epic_id = super::lifecycle::parse_bead_id(&stdout)?;

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
                if !super::lifecycle::assess_bead_specification(bd_path, id, agent_bead_id) {
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
    fn filter_claimable_beads_excludes_only_human_label() {
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
        assert_eq!(ids, vec!["b1", "b3", "b4", "b5", "b6"]);
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
            vec![
                "create",
                "--type=epic",
                "--title=Fix login bug",
                "--priority=1",
                "--json"
            ]
        );
    }

    #[test]
    fn wrap_reparent_args_produces_update_parent_command() {
        let args = wrap_reparent_args("beads-abc", "beads-xyz");
        assert_eq!(args, vec!["update", "beads-abc", "--parent=beads-xyz"]);
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
