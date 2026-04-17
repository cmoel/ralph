//! Epic selection, claiming, and iteration management.

use std::collections::HashMap;
use std::process::Command;

use tracing::{info, warn};

/// Result of selecting and claiming work — either a standalone bead or a child of an epic.
pub struct Claim {
    pub bead_id: String,
    pub bead_title: String,
    /// None when this is a standalone bead; Some(epic_id) when it's a child of an epic.
    pub epic_id: Option<String>,
}

/// Scored candidate for selection ranking. Can be a standalone bead or an epic.
pub(crate) struct ScoredCandidate {
    pub kind: CandidateKind,
    pub priority: i64,
    pub ready_count: usize,
}

#[derive(Clone)]
pub(crate) enum CandidateKind {
    Standalone { bead_id: String, bead_title: String },
    Epic { epic_id: String },
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

/// Select the best work candidate. Returns the candidate with lowest score.
pub(crate) fn select_best_candidate(candidates: Vec<ScoredCandidate>) -> Option<ScoredCandidate> {
    candidates
        .into_iter()
        .min_by_key(|c| score_epic(c.priority, c.ready_count))
}

/// Resolve the worktree name for a running worker.
/// Prefers epic_id (for children of epics), falls back to bead_id (for standalone beads),
/// then to agent_id (for claimless admin work).
pub fn resolve_worktree_name(
    epic_id: Option<&str>,
    bead_id: Option<&str>,
    agent_id: Option<&str>,
) -> Option<String> {
    epic_id.or(bead_id).or(agent_id).map(String::from)
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

// --- I/O functions for work selection ---

/// Select and claim the best available work — either a standalone bead or an epic's first child.
pub fn select_and_claim_work(bd_path: &str, agent_bead_id: &str) -> Option<Claim> {
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["ready", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "work_ready_list_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = serde_json::from_str(&stdout).ok()?;
    let claimable = filter_claimable_beads(&items);

    if claimable.is_empty() {
        info!("no_claimable_beads_for_work_selection");
        return None;
    }

    let groups = group_beads_by_parent(&claimable);

    // Build candidates: each standalone bead is its own candidate; each parent epic is one candidate.
    let mut candidates: Vec<ScoredCandidate> = Vec::new();
    for (parent_id, children) in &groups {
        match parent_id {
            None => {
                for bead in children {
                    let Some(bead_id) = bead.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let bead_title = bead.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let priority = bead.get("priority").and_then(|v| v.as_i64()).unwrap_or(2);
                    candidates.push(ScoredCandidate {
                        kind: CandidateKind::Standalone {
                            bead_id: bead_id.to_string(),
                            bead_title: bead_title.to_string(),
                        },
                        priority,
                        ready_count: 1,
                    });
                }
            }
            Some(epic_id) => {
                if children.is_empty() {
                    continue;
                }
                let priority = get_bead_priority(bd_path, epic_id);
                candidates.push(ScoredCandidate {
                    kind: CandidateKind::Epic {
                        epic_id: epic_id.clone(),
                    },
                    priority,
                    ready_count: children.len(),
                });
            }
        }
    }

    if candidates.is_empty() {
        info!("no_work_candidates_to_score");
        return None;
    }

    let best = select_best_candidate(candidates)?;

    match best.kind {
        CandidateKind::Standalone {
            bead_id,
            bead_title,
        } => claim_standalone_bead(bd_path, agent_bead_id, bead_id, bead_title),
        CandidateKind::Epic { epic_id } => {
            claim_epic_and_first_child(bd_path, agent_bead_id, epic_id)
        }
    }
}

/// Atomically claim a standalone bead, record the hook, and assess its specification.
fn claim_standalone_bead(
    bd_path: &str,
    agent_bead_id: &str,
    bead_id: String,
    bead_title: String,
) -> Option<Claim> {
    let claim_result = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["update", &bead_id, "--claim"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
    });

    match claim_result {
        Ok(o) if o.status.success() => {
            let hook_arg = format!("hook={}", bead_id);
            let _ = crate::bd_lock::with_lock(|| {
                Command::new(bd_path)
                    .args(["set-state", agent_bead_id, &hook_arg])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
            });

            if !super::lifecycle::assess_bead_specification(bd_path, &bead_id, agent_bead_id) {
                info!(bead_id = %bead_id, "standalone_rejected_assessment");
                return None;
            }

            info!(bead_id = %bead_id, title = %bead_title, "standalone_claimed");
            Some(Claim {
                bead_id,
                bead_title,
                epic_id: None,
            })
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            info!(bead_id = %bead_id, stderr = %stderr.trim(), "standalone_claim_failed");
            None
        }
        Err(e) => {
            warn!(bead_id = %bead_id, error = %e, "standalone_claim_command_failed");
            None
        }
    }
}

/// Claim an epic, record it on agent state, and claim its first ready child.
fn claim_epic_and_first_child(
    bd_path: &str,
    agent_bead_id: &str,
    epic_id: String,
) -> Option<Claim> {
    let claim_result = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["update", &epic_id, "--claim"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
    });

    match claim_result {
        Ok(o) if o.status.success() => {
            info!(epic_id = %epic_id, "epic_claimed");
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            info!(epic_id = %epic_id, stderr = %stderr.trim(), "epic_claim_failed");
            return None;
        }
        Err(e) => {
            warn!(epic_id = %epic_id, error = %e, "epic_claim_command_failed");
            return None;
        }
    }

    let epic_arg = format!("epic={}", epic_id);
    let _ = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["set-state", agent_bead_id, &epic_arg])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
    });

    match claim_next_child(bd_path, agent_bead_id, &epic_id) {
        Some((child_id, child_title)) => Some(Claim {
            bead_id: child_id,
            bead_title: child_title,
            epic_id: Some(epic_id),
        }),
        None => {
            warn!(epic_id = %epic_id, "epic_claimed_but_no_children_ready");
            let _ = crate::bd_lock::with_lock(|| {
                Command::new(bd_path)
                    .args(["update", &epic_id, "--status=open", "--assignee="])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output()
            });
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
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["ready", "--parent", epic_id, "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
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
        let claim = crate::bd_lock::with_lock(|| {
            Command::new(bd_path)
                .args(["update", id, "--claim"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .output()
        });

        match claim {
            Ok(o) if o.status.success() => {
                // Record hook on agent bead
                let hook_arg = format!("hook={}", id);
                let _ = crate::bd_lock::with_lock(|| {
                    Command::new(bd_path)
                        .args(["set-state", agent_bead_id, &hook_arg])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .output()
                });

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
    let result = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["epic", "close-eligible"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    });

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

    #[test]
    fn score_epic_lower_priority_wins() {
        assert!(score_epic(0, 1) < score_epic(2, 1));
    }

    #[test]
    fn score_epic_more_children_breaks_tie() {
        assert!(score_epic(1, 5) < score_epic(1, 2));
    }

    #[test]
    fn select_best_candidate_picks_lowest_score() {
        let candidates = vec![
            ScoredCandidate {
                kind: CandidateKind::Epic {
                    epic_id: "epic-a".into(),
                },
                priority: 2,
                ready_count: 3,
            },
            ScoredCandidate {
                kind: CandidateKind::Epic {
                    epic_id: "epic-b".into(),
                },
                priority: 0,
                ready_count: 1,
            },
            ScoredCandidate {
                kind: CandidateKind::Standalone {
                    bead_id: "bead-c".into(),
                    bead_title: "c".into(),
                },
                priority: 1,
                ready_count: 1,
            },
        ];
        let best = select_best_candidate(candidates).unwrap();
        match best.kind {
            CandidateKind::Epic { epic_id } => assert_eq!(epic_id, "epic-b"),
            CandidateKind::Standalone { .. } => panic!("expected Epic"),
        }
    }

    #[test]
    fn select_best_candidate_prefers_standalone_over_epic_when_higher_priority() {
        let candidates = vec![
            ScoredCandidate {
                kind: CandidateKind::Epic {
                    epic_id: "epic-a".into(),
                },
                priority: 2,
                ready_count: 3,
            },
            ScoredCandidate {
                kind: CandidateKind::Standalone {
                    bead_id: "bead-b".into(),
                    bead_title: "b".into(),
                },
                priority: 0,
                ready_count: 1,
            },
        ];
        let best = select_best_candidate(candidates).unwrap();
        match best.kind {
            CandidateKind::Standalone { bead_id, .. } => assert_eq!(bead_id, "bead-b"),
            CandidateKind::Epic { .. } => panic!("expected Standalone"),
        }
    }

    #[test]
    fn select_best_candidate_returns_none_for_empty() {
        assert!(select_best_candidate(Vec::new()).is_none());
    }

    #[test]
    fn resolve_worktree_name_prefers_epic() {
        assert_eq!(
            resolve_worktree_name(Some("epic-1"), Some("bead-1"), Some("agent-1")),
            Some("epic-1".into())
        );
    }

    #[test]
    fn resolve_worktree_name_falls_back_to_bead_id_for_standalone() {
        assert_eq!(
            resolve_worktree_name(None, Some("bead-1"), Some("agent-1")),
            Some("bead-1".into())
        );
    }

    #[test]
    fn resolve_worktree_name_falls_back_to_agent_id_for_claimless() {
        assert_eq!(
            resolve_worktree_name(None, None, Some("agent-1")),
            Some("agent-1".into())
        );
    }

    #[test]
    fn resolve_worktree_name_returns_none_when_nothing_known() {
        assert_eq!(resolve_worktree_name(None, None, None), None);
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
