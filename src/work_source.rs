//! Pluggable work source abstraction.
//!
//! Defines the `WorkSource` trait that decouples ralph's core loop from
//! the specific system that provides work items (specs, beads, etc.).

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ratatui::style::Color;
use tracing::warn;

use crate::specs::{self, SpecStatus, SpecsRemaining};

/// Result of checking if there's remaining work.
pub enum WorkRemaining {
    /// There are work items with active status.
    Yes,
    /// All work items are done or blocked.
    No,
    /// Work source is missing (e.g., README not found).
    Missing,
    /// Error reading the work source.
    ReadError(String),
}

/// Status of a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WorkItemStatus {
    Blocked,
    NeedsShaping,
    Ready,
    InProgress,
    Done,
}

impl WorkItemStatus {
    /// Get the display color for this status.
    pub fn color(&self) -> Color {
        match self {
            Self::Blocked => Color::Red,
            Self::NeedsShaping => Color::Yellow,
            Self::Ready => Color::Cyan,
            Self::InProgress => Color::Green,
            Self::Done => Color::DarkGray,
        }
    }

    /// Get the display label for this status.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Blocked => "Blocked",
            Self::NeedsShaping => "Needs Shaping",
            Self::Ready => "Ready",
            Self::InProgress => "In Progress",
            Self::Done => "Done",
        }
    }
}

impl From<SpecStatus> for WorkItemStatus {
    fn from(s: SpecStatus) -> Self {
        match s {
            SpecStatus::Blocked => Self::Blocked,
            SpecStatus::NeedsShaping => Self::NeedsShaping,
            SpecStatus::Ready => Self::Ready,
            SpecStatus::InProgress => Self::InProgress,
            SpecStatus::Done => Self::Done,
        }
    }
}

/// A single work item from a work source.
#[derive(Debug, Clone)]
pub struct WorkItem {
    /// Name of the work item.
    pub name: String,
    /// Current status.
    pub status: WorkItemStatus,
    /// Timestamp for sorting.
    pub timestamp: Option<SystemTime>,
}

/// Trait for pluggable work sources.
///
/// Implementations provide work items to ralph's core loop.
/// Methods are synchronous but callers run them on background threads
/// to avoid blocking the TUI event loop.
pub trait WorkSource: Send + Sync {
    /// Check if there's remaining work (for auto-continue decisions).
    fn check_remaining(&self) -> WorkRemaining;

    /// Detect the currently active work item name (for status bar display).
    fn detect_current(&self) -> Option<String>;

    /// List all work items with status (for the work panel).
    fn list_items(&self) -> Result<Vec<WorkItem>, String>;

    /// Label for this work source (e.g., "Specs", "Beads").
    fn label(&self) -> &'static str;

    /// Label for the "all complete" message.
    fn complete_message(&self) -> &'static str;
}

/// Work source backed by spec files in a specs directory.
pub struct SpecsWorkSource {
    specs_dir: PathBuf,
}

impl SpecsWorkSource {
    pub fn new(specs_dir: PathBuf) -> Self {
        Self { specs_dir }
    }
}

impl WorkSource for SpecsWorkSource {
    fn check_remaining(&self) -> WorkRemaining {
        match specs::check_specs_remaining(&self.specs_dir) {
            SpecsRemaining::Yes => WorkRemaining::Yes,
            SpecsRemaining::No => WorkRemaining::No,
            SpecsRemaining::Missing => WorkRemaining::Missing,
            SpecsRemaining::ReadError(e) => WorkRemaining::ReadError(e),
        }
    }

    fn detect_current(&self) -> Option<String> {
        specs::detect_current_spec(&self.specs_dir)
    }

    fn list_items(&self) -> Result<Vec<WorkItem>, String> {
        specs::parse_specs_readme(&self.specs_dir).map(|entries| {
            entries
                .into_iter()
                .map(|e| WorkItem {
                    name: e.name,
                    status: e.status.into(),
                    timestamp: e.timestamp,
                })
                .collect()
        })
    }

    fn label(&self) -> &'static str {
        "Specs"
    }

    fn complete_message(&self) -> &'static str {
        "ALL SPECS COMPLETE"
    }
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
}

impl WorkSource for BeadsWorkSource {
    fn check_remaining(&self) -> WorkRemaining {
        match self.run_bd(&["ready", "--json"]) {
            Ok(stdout) => match serde_json::from_str::<serde_json::Value>(&stdout) {
                Ok(serde_json::Value::Array(arr)) => {
                    if arr.is_empty() {
                        WorkRemaining::No
                    } else {
                        WorkRemaining::Yes
                    }
                }
                Ok(_) => WorkRemaining::ReadError("unexpected bd ready output".to_string()),
                Err(e) => WorkRemaining::ReadError(format!("failed to parse bd output: {}", e)),
            },
            Err(e) => WorkRemaining::ReadError(e),
        }
    }

    fn detect_current(&self) -> Option<String> {
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

    fn list_items(&self) -> Result<Vec<WorkItem>, String> {
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

    fn label(&self) -> &'static str {
        "Beads"
    }

    fn complete_message(&self) -> &'static str {
        "ALL BEADS COMPLETE"
    }
}

/// Construct a work source from a mode string and config.
pub fn create_work_source(mode: &str, specs_dir: PathBuf, bd_path: &str) -> Arc<dyn WorkSource> {
    match mode {
        "specs" => Arc::new(SpecsWorkSource::new(specs_dir)),
        "beads" => Arc::new(BeadsWorkSource::new(bd_path.to_string())),
        other => {
            warn!(mode = other, "unknown_mode_falling_back_to_specs");
            Arc::new(SpecsWorkSource::new(specs_dir))
        }
    }
}
