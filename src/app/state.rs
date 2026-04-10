use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::BorderType;
use rusqlite::Connection;
use tracing::info;

use crate::config::{Config, LoadedConfig};
use crate::dolt::DoltManager;
use crate::doctor;
use crate::logging::ReloadHandle;
use crate::modals::{ConfigModalState, InitModalState, KanbanBoardState, ToolAllowModalState};
use crate::output::OutputMessage;
use crate::startup::get_file_mtime;
use crate::tool_panel::{ContentBlockState, ToolPanel};
use crate::wake_lock::WakeLock;
use crate::work_source::{BeadsWorkSource, WorkRemaining};

/// Application status states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Stopped,
    Running,
    Error,
}

impl AppStatus {
    pub fn border_type(&self) -> BorderType {
        match self {
            AppStatus::Stopped => BorderType::Rounded,
            AppStatus::Running | AppStatus::Error => BorderType::Double,
        }
    }

    /// Returns the static color for this status.
    pub fn status_color(&self) -> Color {
        match self {
            AppStatus::Stopped => Color::Cyan,
            AppStatus::Running => Color::Green,
            AppStatus::Error => Color::Red,
        }
    }
}

/// Per-worker state extracted from App.
/// App holds a Vec<Worker> — currently always exactly one.
pub struct Worker {
    /// Worker index (0-based). Used when N-worker support is added.
    #[allow(dead_code)]
    pub id: usize,
    /// Handle to the running Claude CLI subprocess.
    pub child_process: Option<Child>,
    /// Channel receiving stdout/stderr from child process.
    pub output_receiver: Option<Receiver<OutputMessage>>,
    /// Git worktree name for this worker (beads mode only).
    pub worktree_name: Option<String>,
    /// Git worktree path for this worker (beads mode only).
    pub worktree_path: Option<PathBuf>,
    /// Agent bead ID for this worker (beads mode only).
    pub agent_bead_id: Option<String>,
    /// Currently hooked bead ID (the bead this worker is working on).
    pub hooked_bead_id: Option<String>,
    /// Handle to signal the heartbeat thread to stop.
    pub heartbeat_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// When the current run started (for elapsed time display).
    pub run_start_time: Option<Instant>,
    /// Current iteration number within a run (1-indexed, 0 when stopped).
    pub current_iteration: u32,
    /// Total iterations configured for the current run:
    /// - Negative (-1): Infinite mode
    /// - Zero (0): Stopped mode (shouldn't start)
    /// - Positive (N): Countdown mode, runs N iterations
    pub total_iterations: i32,
    /// Flag indicating auto-continue should be triggered on next loop iteration.
    pub auto_continue_pending: bool,
    /// Completed output lines to display.
    pub output_lines: Vec<Line<'static>>,
    /// Tracks content blocks by index during streaming.
    pub content_blocks: HashMap<usize, ContentBlockState>,
    /// Current line being accumulated (text that hasn't hit a newline yet).
    pub current_line: String,
    /// Receiver for background check_remaining() result (auto-continue).
    pub pending_work_check: Option<Receiver<(WorkRemaining, &'static str)>>,
    /// Currently claimed epic ID (the epic this worker is iterating through).
    pub claimed_epic_id: Option<String>,
    /// Human-readable error from the last result event (e.g. rate limit message).
    pub last_result_error: Option<String>,
}

impl Worker {
    /// Create a new worker with the given ID and default state.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            child_process: None,
            output_receiver: None,
            worktree_name: None,
            worktree_path: None,
            agent_bead_id: None,
            hooked_bead_id: None,
            heartbeat_stop: None,
            run_start_time: None,
            current_iteration: 0,
            total_iterations: 0,
            auto_continue_pending: false,
            output_lines: Vec::new(),
            content_blocks: HashMap::new(),
            current_line: String::new(),
            pending_work_check: None,
            claimed_epic_id: None,
            last_result_error: None,
        }
    }

    /// Terminate the child process if running.
    pub fn kill_child(&mut self) {
        if let Some(mut child) = self.child_process.take() {
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            info!(pid, "process_killed");
        }
        self.output_receiver = None;
    }

    /// Reset iteration state when stopping (error, manual stop, or run complete).
    pub fn reset_iteration_state(&mut self) {
        self.current_iteration = 0;
        self.total_iterations = 0;
    }

    /// Increment iteration counter for auto-continue.
    pub fn increment_iteration(&mut self) {
        self.current_iteration += 1;
    }

    /// Whether auto-continue should fire after a successful run.
    pub fn should_auto_continue(&self) -> bool {
        if self.total_iterations < 0 {
            true
        } else if self.total_iterations > 0 {
            self.current_iteration < self.total_iterations as u32
        } else {
            false
        }
    }
}

/// Main application state.
pub struct App {
    pub status: AppStatus,
    pub scroll_offset: u16,
    pub is_auto_following: bool,
    pub show_already_running_popup: bool,
    pub show_config_modal: bool,
    pub main_pane_height: u16,
    pub main_pane_width: u16,
    /// Session ID for this Ralph invocation (always populated).
    pub session_id: String,
    /// Loop counter for logging, incremented each time start_command() is called.
    pub loop_count: u64,
    /// Directory where logs are written.
    pub log_directory: Option<PathBuf>,
    /// Loaded configuration.
    pub config: Config,
    /// Path to the per-project configuration file, if it existed at startup.
    pub project_config_path: Option<PathBuf>,
    /// Last known mtime of the project config file for change detection.
    pub project_config_mtime: Option<SystemTime>,
    /// Last time we polled for config changes.
    pub last_config_poll: Instant,
    /// When config was last successfully reloaded (for "Reloaded" indicator fade).
    pub config_reloaded_at: Option<Instant>,
    /// Error message if per-project config reload failed.
    pub project_config_error: Option<String>,
    /// Name of the currently active bead (from bd list).
    pub current_bead: Option<String>,
    /// Last time we polled for the current bead.
    pub last_bead_poll: Instant,
    /// Handle for dynamically reloading the log level.
    pub log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
    /// Current log level from config (to detect changes on reload).
    pub current_log_level: String,
    /// Whether the UI needs to be redrawn.
    pub dirty: bool,
    /// State for the config modal form (when open).
    pub config_modal_state: Option<ConfigModalState>,
    /// Whether the init modal is visible.
    pub show_init_modal: bool,
    /// State for the init modal (when open).
    pub init_modal_state: Option<InitModalState>,
    /// Whether the help modal is visible.
    pub show_help_modal: bool,
    /// Whether the quit confirmation modal is visible.
    pub show_quit_modal: bool,
    /// Transient hint message displayed in the status bar (auto-clears after timeout).
    pub hint: Option<(String, Instant)>,
    /// Cumulative token count (input + output) across all exchanges in the session.
    pub cumulative_tokens: u64,
    /// Exchange counter within the current session (incremented on each Result event).
    pub exchange_count: u32,
    /// Name of the last tool used (for categorizing exchanges).
    pub last_tool_used: Option<String>,
    /// Wake lock to prevent system idle sleep while running.
    pub wake_lock: Option<WakeLock>,
    /// Tool call tracking and panel display state.
    pub tool_panel: ToolPanel,
    /// Whether we're currently in an indented text block (for flush).
    pub in_indented_text: bool,
    /// Work source for bead-based workflows.
    pub work_source: Arc<BeadsWorkSource>,
    /// Receiver for background detect_current() result (poll_bead).
    pub bead_poll_rx: Option<Receiver<Option<String>>>,
    /// Dolt SQL server manager.
    pub dolt: DoltManager,
    /// When the app entered Error state (for auto-clearing the pulsing flash).
    pub error_at: Option<Instant>,
    /// Receiver for background doctor checks (run once on TUI open).
    pub doctor_rx: Option<Receiver<Vec<doctor::CheckResult>>>,
    /// SQLite connection for tool call recording (None if DB open failed at startup).
    pub tool_history_db: Option<Connection>,
    /// Sequence counter for tool calls within this session.
    pub tool_call_sequence: u32,
    /// Whether the tool allow modal is visible.
    pub show_tool_allow_modal: bool,
    /// State for the tool allow modal (when open).
    pub tool_allow_modal_state: Option<ToolAllowModalState>,
    /// Resolved repository path for tool history tracking.
    pub repo_path: String,
    /// State for the kanban board (primary view, always present).
    pub kanban_board_state: KanbanBoardState,
    /// Receiver for background kanban board data (multiple bd commands).
    pub kanban_items_rx: Option<Receiver<Result<crate::modals::KanbanBoardData, String>>>,
    /// Receiver for background bd show --json result (bead detail drill-down).
    pub bead_detail_rx: Option<Receiver<Result<serde_json::Value, String>>>,
    /// Receiver for filesystem change events on .beads/ directory (kanban auto-refresh).
    pub kanban_fs_rx: Option<Receiver<()>>,
    /// Stop signal for the kanban filesystem watcher thread.
    pub kanban_watcher_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Cached visual line count (invalidated on content or width changes).
    pub cached_visual_line_count: Option<u16>,
    /// Error from parsing board_columns.toml (None = valid).
    pub board_config_error: Option<String>,
    /// Whether the bead picker modal is visible.
    pub show_bead_picker: bool,
    /// State for the bead picker modal (when open).
    pub bead_picker_state: Option<crate::modals::BeadPickerState>,
    /// Result from the bead picker — callers `.take()` this after the picker closes.
    pub bead_picker_result: Option<String>,
    /// Receiver for background bead picker data.
    pub bead_picker_rx: Option<Receiver<Result<Vec<crate::modals::BeadPickerItem>, String>>>,
    /// Pending dependency: bead ID + direction, waiting for bead picker result.
    pub pending_dep: Option<PendingDep>,
    /// Whether the workers stream modal is visible.
    pub show_workers_stream: bool,
    /// State for the workers stream modal (when open).
    pub workers_stream_state: Option<crate::modals::WorkersStreamState>,
    /// Per-worker state. Currently always exactly one worker.
    pub workers: Vec<Worker>,
    /// Index into `workers` for the currently selected/active worker.
    pub selected_worker: usize,
}

/// Tracks a pending dependency addition while the bead picker is open.
#[derive(Debug)]
pub struct PendingDep {
    /// The bead we pressed 'b' on.
    pub bead_id: String,
    /// The direction chosen by the user.
    pub direction: crate::modals::DepDirection,
}

impl App {
    pub fn new(
        session_id: String,
        log_directory: Option<PathBuf>,
        loaded_config: LoadedConfig,
        log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
    ) -> Self {
        let current_log_level = loaded_config.config.logging.level.clone();
        let worker_count = loaded_config.config.behavior.workers.max(1) as usize;
        let work_source = Arc::new(BeadsWorkSource::new(
            loaded_config.config.behavior.bd_path.clone(),
        ));
        let board_columns = crate::modals::load_board_config()
            .map(|c| c.columns)
            .unwrap_or_default();
        let kanban_board_state = KanbanBoardState::new_loading(board_columns);
        Self {
            status: AppStatus::Stopped,
            scroll_offset: 0,
            is_auto_following: true,
            show_already_running_popup: false,
            show_config_modal: false,
            main_pane_height: 0,
            main_pane_width: 0,
            session_id,
            loop_count: 0,
            log_directory,
            config: loaded_config.config,
            project_config_path: loaded_config.project_config_path.clone(),
            project_config_mtime: loaded_config
                .project_config_path
                .as_ref()
                .and_then(|p| get_file_mtime(p)),
            // Initialize to "long ago" so we poll immediately on start
            last_config_poll: Instant::now() - Duration::from_secs(10),
            config_reloaded_at: None,
            project_config_error: None,
            current_bead: None,
            // Initialize to "long ago" so we poll immediately on start
            last_bead_poll: Instant::now() - Duration::from_secs(10),
            log_level_handle,
            current_log_level,
            dirty: true,
            config_modal_state: None,
            show_init_modal: false,
            init_modal_state: None,
            show_help_modal: false,
            show_quit_modal: false,
            hint: None,
            cumulative_tokens: 0,
            exchange_count: 0,
            last_tool_used: None,
            wake_lock: None,
            tool_panel: ToolPanel::new(),
            in_indented_text: false,
            work_source,
            bead_poll_rx: None,
            dolt: DoltManager::new(),
            error_at: None,
            doctor_rx: None,
            tool_history_db: None,
            tool_call_sequence: 0,
            show_tool_allow_modal: false,
            tool_allow_modal_state: None,
            repo_path: crate::db::detect_repo_path(),
            kanban_board_state,
            kanban_items_rx: None,
            bead_detail_rx: None,
            kanban_fs_rx: None,
            kanban_watcher_stop: None,
            cached_visual_line_count: None,
            board_config_error: None,
            show_bead_picker: false,
            bead_picker_state: None,
            bead_picker_result: None,
            bead_picker_rx: None,
            pending_dep: None,
            show_workers_stream: false,
            workers_stream_state: None,
            workers: (0..worker_count).map(Worker::new).collect(),
            selected_worker: 0,
        }
    }

    /// Returns a reference to the currently selected worker.
    #[allow(dead_code)]
    pub fn worker(&self) -> &Worker {
        &self.workers[self.selected_worker]
    }

    /// Returns a mutable reference to the currently selected worker.
    #[allow(dead_code)]
    pub fn worker_mut(&mut self) -> &mut Worker {
        &mut self.workers[self.selected_worker]
    }

    /// Validate board column TOML and store any error.
    /// Call after construction to set the initial hint if invalid.
    pub fn validate_board_config(&mut self) {
        if let Err(e) = crate::modals::load_board_config() {
            let msg = format!("Board TOML invalid: {e}");
            self.board_config_error = Some(msg.clone());
            self.set_hint(msg);
        }
    }

    /// Auto-revert from Error to Stopped after a timeout.
    pub fn check_error_timeout(&mut self) {
        if let Some(at) = self.error_at
            && at.elapsed() >= Duration::from_secs(5)
        {
            self.status = AppStatus::Stopped;
            self.error_at = None;
            self.dirty = true;
        }
    }

    /// Set a transient hint message in the status bar.
    pub fn set_hint(&mut self, message: impl Into<String>) {
        self.hint = Some((message.into(), Instant::now()));
    }

    /// Auto-clear hint after timeout.
    pub fn check_hint_timeout(&mut self) {
        if let Some((_, at)) = &self.hint
            && at.elapsed() >= Duration::from_secs(3)
        {
            self.hint = None;
            self.dirty = true;
        }
    }

    /// Stop the running command (user-initiated)
    pub fn stop_command(&mut self) {
        if self.status != AppStatus::Running {
            return;
        }
        info!("manual_stop");
        for w in 0..self.workers.len() {
            self.workers[w].kill_child();
            self.workers[w].run_start_time = None;
            self.release_worker_hooked_bead(w);
        }
        self.status = AppStatus::Stopped;
        self.current_bead = None;
    }

    /// Release the currently hooked bead for the selected worker.
    /// Used during stop between iterations. Does not touch agent, worktree, or heartbeat.
    pub fn release_hooked_bead(&mut self) {
        let w = self.selected_worker;
        self.release_worker_hooked_bead(w);
    }

    /// Release the hooked bead for a specific worker by index.
    pub fn release_worker_hooked_bead(&mut self, w: usize) {
        let bead_id = self.workers[w].hooked_bead_id.take();
        if let (Some(agent_id), Some(bead_id)) = (&self.workers[w].agent_bead_id, bead_id) {
            crate::agent::release_bead(&self.config.behavior.bd_path, agent_id, &bead_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a minimal App with N workers (no side effects).
    fn app_with_workers(n: u32) -> App {
        use crate::config::LoadedConfig;
        let mut loaded = LoadedConfig::default_for_test();
        loaded.config.behavior.workers = n.max(1);
        App::new("test".into(), None, loaded, None)
    }

    #[test]
    fn worker_count_matches_config() {
        for n in [1, 2, 4] {
            let app = app_with_workers(n);
            assert_eq!(app.workers.len(), n as usize);
        }
    }

    #[test]
    fn worker_ids_are_sequential() {
        let app = app_with_workers(3);
        for (i, w) in app.workers.iter().enumerate() {
            assert_eq!(w.id, i);
        }
    }

    #[test]
    fn default_single_worker() {
        let app = app_with_workers(1);
        assert_eq!(app.workers.len(), 1);
        assert_eq!(app.selected_worker, 0);
    }

    #[test]
    fn status_derivation_all_stopped() {
        let mut app = app_with_workers(3);
        app.status = AppStatus::Stopped;
        app.update_derived_status();
        assert_eq!(app.status, AppStatus::Stopped);
    }

    #[test]
    fn status_derivation_one_running() {
        let mut app = app_with_workers(3);
        // Simulate one worker having a receiver (active)
        let (_tx, rx) = std::sync::mpsc::channel::<crate::output::OutputMessage>();
        app.workers[1].output_receiver = Some(rx);
        app.status = AppStatus::Stopped;
        app.update_derived_status();
        assert_eq!(app.status, AppStatus::Running);
    }

    #[test]
    fn status_derivation_does_not_override_error() {
        let mut app = app_with_workers(2);
        app.status = AppStatus::Error;
        app.update_derived_status();
        assert_eq!(app.status, AppStatus::Error);
    }

    #[test]
    fn any_worker_active_false_when_all_idle() {
        let app = app_with_workers(3);
        assert!(!app.any_worker_active());
    }

    #[test]
    fn any_worker_active_true_with_receiver() {
        let mut app = app_with_workers(3);
        let (_tx, rx) = std::sync::mpsc::channel::<crate::output::OutputMessage>();
        app.workers[2].output_receiver = Some(rx);
        assert!(app.any_worker_active());
    }
}
