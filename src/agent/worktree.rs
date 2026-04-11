//! Git worktree creation, merging, and cleanup.

use std::path::PathBuf;
use std::process::Command;

use tracing::{info, warn};

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
    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["list", "--json", "--status=open", "--limit=0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
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

    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
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
    })
    .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "file_merge_conflict_bead_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let bead_id = super::lifecycle::parse_bead_id(&stdout)?;
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
    let _ = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args([
                "close",
                existing_bead_id,
                "--reason=Claude could not resolve",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
    });

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

    let output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
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
    })
    .ok()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr.trim(), "escalate_merge_conflict_failed");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let bead_id = super::lifecycle::parse_bead_id(&stdout)?;
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
    let remove_result = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["worktree", "remove", "--force", worktree_name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
    });
    match remove_result {
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
    let wt_output = crate::bd_lock::with_lock(|| {
        Command::new(bd_path)
            .args(["worktree", "create", worktree_name])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    });

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

    #[cfg(unix)]
    {
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
    #[cfg(not(unix))]
    {
        match std::fs::copy(&source, &target) {
            Ok(_) => info!(
                source = %source.display(),
                target = %target.display(),
                "settings_local_copied"
            ),
            Err(e) => warn!(
                error = %e,
                source = %source.display(),
                target = %target.display(),
                "settings_local_copy_failed"
            ),
        }
    }
}
