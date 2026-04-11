//! Beads-based work source for ralph's core loop.

use std::process::Command;
use std::time::{Duration, SystemTime};

/// Result of checking if there's remaining work.
#[derive(Debug, PartialEq)]
pub enum WorkRemaining {
    /// There are work items with active status.
    Yes,
    /// All work items are done or blocked.
    No,
    /// All ready beads are for humans only (human label).
    HumanOnly(usize),
    /// Error reading the work source.
    ReadError(String),
}

/// Status of a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub enum WorkItemStatus {
    Blocked,
    Ready,
    InProgress,
    Done,
}

/// A single work item from a work source.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkItem {
    /// Name of the work item.
    pub name: String,
    /// Current status.
    pub status: WorkItemStatus,
    /// Timestamp for sorting.
    pub timestamp: Option<SystemTime>,
}

/// Work source backed by the `bd` CLI for bead-based workflows.
pub struct BeadsWorkSource {
    bd_path: String,
}

impl BeadsWorkSource {
    pub fn new(bd_path: String) -> Self {
        Self { bd_path }
    }

    /// Timeout for bd commands.
    const TIMEOUT: Duration = Duration::from_secs(5);

    /// Run a `bd` command with the given args, returning stdout on success.
    /// Kills the process if it exceeds the timeout.
    fn run_bd(&self, args: &[&str]) -> Result<String, String> {
        crate::perf::record_subprocess_spawn();
        // Hold the global bd lock across spawn + try_wait + kill so we serialize
        // against every other ralph-initiated bd call. The guard drops when this
        // function returns (success, timeout, or error).
        let _bd_guard = crate::bd_lock::acquire();
        let mut child = match Command::new(&self.bd_path)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!("{}: command not found", self.bd_path));
            }
            Err(e) => return Err(format!("failed to run bd: {}", e)),
        };

        // Poll with timeout
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stdout = child.stdout.take().map_or_else(Vec::new, |mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).unwrap_or(0);
                        buf
                    });
                    let stderr = child.stderr.take().map_or_else(Vec::new, |mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).unwrap_or(0);
                        buf
                    });
                    if status.success() {
                        return String::from_utf8(stdout)
                            .map_err(|e| format!("invalid utf8 from bd: {}", e));
                    } else {
                        let stderr_str = String::from_utf8_lossy(&stderr);
                        return Err(format!("bd exited with {}: {}", status, stderr_str.trim()));
                    }
                }
                Ok(None) => {
                    if start.elapsed() >= Self::TIMEOUT {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("bd command timed out".to_string());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(format!("failed to wait on bd: {}", e)),
            }
        }
    }

    /// Map a bd status string to WorkItemStatus.
    fn map_status(status: &str) -> WorkItemStatus {
        match status {
            "in_progress" => WorkItemStatus::InProgress,
            "open" => WorkItemStatus::Ready,
            "closed" => WorkItemStatus::Done,
            "deferred" => WorkItemStatus::Blocked,
            _ => WorkItemStatus::Ready,
        }
    }

    /// Check if there's remaining work (for auto-continue decisions).
    pub fn check_remaining(&self) -> WorkRemaining {
        match self.run_bd(&["ready", "--json"]) {
            Ok(stdout) => parse_ready_output(&stdout),
            Err(e) => WorkRemaining::ReadError(e),
        }
    }

    /// Detect the currently active work item name (for status bar display).
    pub fn detect_current(&self) -> Option<String> {
        let stdout = self
            .run_bd(&["list", "--json", "--status", "in_progress"])
            .ok()?;
        let items: serde_json::Value = serde_json::from_str(&stdout).ok()?;
        let arr = items.as_array()?;
        let first = arr.first()?;
        let id = first.get("id")?.as_str()?;
        let title = first.get("title").and_then(|t| t.as_str()).unwrap_or("");
        Some(format!("{} {}", id, title))
    }

    /// List all work items with status (for the work panel).
    pub fn list_items(&self) -> Result<Vec<WorkItem>, String> {
        let stdout = self.run_bd(&["list", "--json"])?;
        let items: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("failed to parse bd output: {}", e))?;
        let arr = items
            .as_array()
            .ok_or_else(|| "bd list output is not an array".to_string())?;
        Ok(arr
            .iter()
            .map(|item| {
                let name = item
                    .get("title")
                    .and_then(|t| t.as_str())
                    .or_else(|| item.get("id").and_then(|i| i.as_str()))
                    .unwrap_or("unknown")
                    .to_string();
                let status = item
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("open");
                let timestamp = None;
                WorkItem {
                    name,
                    status: Self::map_status(status),
                    timestamp,
                }
            })
            .collect())
    }

    /// Label for the "all complete" message.
    pub fn complete_message(&self) -> &'static str {
        "ALL BEADS COMPLETE"
    }
}

/// Check if a label is the human label (the only label that affects pickup).
pub fn is_human_label(label: &str) -> bool {
    label == "human"
}

fn has_label(item: &serde_json::Value, check: fn(&str) -> bool) -> bool {
    item.get("labels")
        .and_then(|l| l.as_array())
        .is_some_and(|ls| ls.iter().any(|l| l.as_str().is_some_and(check)))
}

/// Parse the JSON output of `bd ready --json` into a `WorkRemaining` value.
///
/// Only the `human` label affects filtering. All other labels are ignored.
fn parse_ready_output(stdout: &str) -> WorkRemaining {
    match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(serde_json::Value::Array(arr)) => {
            if arr.is_empty() {
                WorkRemaining::No
            } else {
                let implementable: Vec<_> = arr
                    .iter()
                    .filter(|item| !has_label(item, is_human_label))
                    .collect();
                if implementable.is_empty() {
                    WorkRemaining::HumanOnly(arr.len())
                } else {
                    WorkRemaining::Yes
                }
            }
        }
        Ok(_) => WorkRemaining::ReadError("unexpected bd ready output".to_string()),
        Err(e) => WorkRemaining::ReadError(format!("failed to parse bd output: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_array_returns_no() {
        assert_eq!(parse_ready_output("[]"), WorkRemaining::No);
    }

    #[test]
    fn non_human_labels_are_implementable() {
        let input = r#"[
            {"id": "ralph-1", "labels": ["needs-brain-dump"]},
            {"id": "ralph-2", "labels": ["needs-shaping", "other"]}
        ]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::Yes);
    }

    #[test]
    fn no_labels_field_returns_yes() {
        let input = r#"[{"id": "ralph-1", "title": "Do something"}]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::Yes);
    }

    #[test]
    fn empty_labels_array_returns_yes() {
        let input = r#"[{"id": "ralph-1", "labels": []}]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::Yes);
    }

    #[test]
    fn invalid_json_returns_read_error() {
        let result = parse_ready_output("not json at all");
        assert!(matches!(result, WorkRemaining::ReadError(_)));
    }

    #[test]
    fn non_array_json_returns_read_error() {
        assert_eq!(
            parse_ready_output("{}"),
            WorkRemaining::ReadError("unexpected bd ready output".to_string()),
        );
    }

    #[test]
    fn is_human_label_matches() {
        assert!(is_human_label("human"));
        assert!(!is_human_label("ready"));
        assert!(!is_human_label("needs-shaping"));
    }

    #[test]
    fn all_human_returns_human_only() {
        let input = r#"[
            {"id": "ralph-1", "labels": ["human"]},
            {"id": "ralph-2", "labels": ["human", "other"]}
        ]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::HumanOnly(2));
    }

    #[test]
    fn mix_of_human_and_implementable_returns_yes() {
        let input = r#"[
            {"id": "ralph-1", "labels": ["human"]},
            {"id": "ralph-2", "labels": ["ready"]}
        ]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::Yes);
    }

    #[test]
    fn human_label_filters_bead() {
        let input = r#"[
            {"id": "ralph-1", "labels": ["human"]}
        ]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::HumanOnly(1));
    }

    #[test]
    fn non_human_label_does_not_filter() {
        let input = r#"[
            {"id": "ralph-1", "labels": ["needs-brain-dump"]}
        ]"#;
        assert_eq!(parse_ready_output(input), WorkRemaining::Yes);
    }
}
