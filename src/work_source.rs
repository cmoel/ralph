//! Pluggable work source abstraction.
//!
//! Defines the `WorkSource` trait that decouples ralph's core loop from
//! the specific system that provides work items (specs, beads, etc.).

use std::path::PathBuf;
use std::time::SystemTime;

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
    Ready,
    InProgress,
    Done,
}

impl WorkItemStatus {
    /// Get the display color for this status.
    pub fn color(&self) -> Color {
        match self {
            Self::Blocked => Color::Red,
            Self::Ready => Color::Cyan,
            Self::InProgress => Color::Green,
            Self::Done => Color::DarkGray,
        }
    }

    /// Get the display label for this status.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Blocked => "Blocked",
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
/// Methods are synchronous (matching the current polling model).
/// Only used on the main thread — no Send + Sync required.
pub trait WorkSource {
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

/// Construct a work source from a mode string and config.
pub fn create_work_source(mode: &str, specs_dir: PathBuf) -> Box<dyn WorkSource> {
    match mode {
        "specs" => Box::new(SpecsWorkSource::new(specs_dir)),
        other => {
            warn!(mode = other, "unknown_mode_falling_back_to_specs");
            Box::new(SpecsWorkSource::new(specs_dir))
        }
    }
}
