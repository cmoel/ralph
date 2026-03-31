//! Application state and core logic.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use rusqlite::Connection;
use tracing::{debug, info, warn};

use crate::config::reload_config;
use crate::config::{Config, LoadedConfig, get_project_config_path};
use crate::doctor;
use crate::logging::ReloadHandle;
use crate::modals::{
    ConfigModalState, InitModalState, KanbanBoardState, SpecsPanelState, ToolAllowModalState,
};
use crate::output::OutputMessage;
use crate::wake_lock::WakeLock;
use crate::work_source::{WorkItem, WorkRemaining, WorkSource, create_work_source};
use crate::{get_file_mtime, logging};

/// Application status states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Stopped,
    Running,
    Error,
}

use crate::dolt::DoltManager;
pub use crate::dolt::DoltServerState;

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

use crate::tool_panel::{ContentBlockState, ToolPanel};

/// Main application state.
pub struct App {
    pub status: AppStatus,
    pub output_lines: Vec<Line<'static>>,
    pub scroll_offset: u16,
    pub is_auto_following: bool,
    pub show_already_running_popup: bool,
    pub show_config_modal: bool,
    pub main_pane_height: u16,
    pub main_pane_width: u16,
    pub child_process: Option<Child>,
    pub output_receiver: Option<Receiver<OutputMessage>>,
    /// Tracks content blocks by index during streaming.
    pub content_blocks: HashMap<usize, ContentBlockState>,
    /// Current line being accumulated (text that hasn't hit a newline yet).
    pub current_line: String,
    /// Session ID for this Ralph invocation (always populated).
    pub session_id: String,
    /// Loop counter for logging, incremented each time start_command() is called.
    pub loop_count: u64,
    /// Directory where logs are written.
    pub log_directory: Option<PathBuf>,
    /// Loaded configuration.
    pub config: Config,
    /// Path to the global configuration file.
    pub config_path: PathBuf,
    /// Path to the per-project configuration file, if it existed at startup.
    pub project_config_path: Option<PathBuf>,
    /// Last known mtime of the global config file for change detection.
    pub config_mtime: Option<SystemTime>,
    /// Last known mtime of the project config file for change detection.
    pub project_config_mtime: Option<SystemTime>,
    /// Last time we polled for config changes.
    pub last_config_poll: Instant,
    /// When config was last successfully reloaded (for "Reloaded" indicator fade).
    pub config_reloaded_at: Option<Instant>,
    /// Error message if global config reload failed.
    pub config_reload_error: Option<String>,
    /// Error message if per-project config reload failed.
    pub project_config_error: Option<String>,
    /// Name of the currently active spec (from specs/README.md).
    pub current_spec: Option<String>,
    /// Last time we polled for the current spec.
    pub last_spec_poll: Instant,
    /// Handle for dynamically reloading the log level.
    pub log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
    /// Current log level from config (to detect changes on reload).
    pub current_log_level: String,
    /// When the current run started (for elapsed time display).
    pub run_start_time: Option<Instant>,
    /// Whether the UI needs to be redrawn.
    pub dirty: bool,
    /// State for the config modal form (when open).
    pub config_modal_state: Option<ConfigModalState>,
    /// Flag indicating auto-continue should be triggered on next loop iteration.
    pub auto_continue_pending: bool,
    /// Whether the specs panel modal is visible.
    pub show_specs_panel: bool,
    /// State for the specs panel modal (when open).
    pub specs_panel_state: Option<SpecsPanelState>,
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
    /// Current iteration number within a run (1-indexed, 0 when stopped).
    pub current_iteration: u32,
    /// Total iterations configured for the current run:
    /// - Negative (-1): Infinite mode
    /// - Zero (0): Stopped mode (shouldn't start)
    /// - Positive (N): Countdown mode, runs N iterations
    pub total_iterations: i32,
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
    /// Pluggable work source (specs, beads, etc.).
    pub work_source: Arc<dyn WorkSource>,
    /// Receiver for background detect_current() result (poll_spec).
    pub spec_poll_rx: Option<Receiver<Option<String>>>,
    /// Receiver for background check_remaining() result (auto-continue).
    pub pending_work_check: Option<Receiver<(WorkRemaining, &'static str)>>,
    /// Receiver for background list_items() result (work panel modal).
    pub work_items_rx: Option<Receiver<Result<Vec<WorkItem>, String>>>,
    /// Dolt SQL server manager (beads mode only).
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
    /// Agent bead ID for this ralph instance (beads mode only).
    pub agent_bead_id: Option<String>,
    /// Git worktree name for this ralph instance (beads mode only).
    pub worktree_name: Option<String>,
    /// Git worktree path for this ralph instance (beads mode only).
    pub worktree_path: Option<PathBuf>,
    /// Handle to the heartbeat thread (so we can signal it to stop).
    pub heartbeat_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Currently hooked bead ID (the bead this agent is working on).
    pub hooked_bead_id: Option<String>,
    /// Resolved repository path for tool history tracking.
    pub repo_path: String,
    /// Whether the kanban board modal is visible.
    pub show_kanban_board: bool,
    /// State for the kanban board modal (when open).
    pub kanban_board_state: Option<KanbanBoardState>,
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
        let work_source = create_work_source(
            &loaded_config.config.behavior.mode,
            loaded_config.config.specs_path(),
            &loaded_config.config.behavior.bd_path,
        );
        Self {
            status: AppStatus::Stopped,
            output_lines: Vec::new(),
            scroll_offset: 0,
            is_auto_following: true,
            show_already_running_popup: false,
            show_config_modal: false,
            main_pane_height: 0,
            main_pane_width: 0,
            child_process: None,
            output_receiver: None,
            content_blocks: HashMap::new(),
            current_line: String::new(),
            session_id,
            loop_count: 0,
            log_directory,
            config: loaded_config.config,
            config_path: loaded_config.config_path.clone(),
            project_config_path: loaded_config.project_config_path.clone(),
            config_mtime: get_file_mtime(&loaded_config.config_path),
            project_config_mtime: loaded_config
                .project_config_path
                .as_ref()
                .and_then(|p| get_file_mtime(p)),
            // Initialize to "long ago" so we poll immediately on start
            last_config_poll: Instant::now() - Duration::from_secs(10),
            config_reloaded_at: None,
            config_reload_error: None,
            project_config_error: None,
            current_spec: None,
            // Initialize to "long ago" so we poll immediately on start
            last_spec_poll: Instant::now() - Duration::from_secs(10),
            log_level_handle,
            current_log_level,
            run_start_time: None,
            dirty: true,
            config_modal_state: None,
            auto_continue_pending: false,
            show_specs_panel: false,
            specs_panel_state: None,
            show_init_modal: false,
            init_modal_state: None,
            show_help_modal: false,
            show_quit_modal: false,
            hint: None,
            current_iteration: 0,
            total_iterations: 0,
            cumulative_tokens: 0,
            exchange_count: 0,
            last_tool_used: None,
            wake_lock: None,
            tool_panel: ToolPanel::new(),
            in_indented_text: false,
            work_source,
            spec_poll_rx: None,
            pending_work_check: None,
            work_items_rx: None,
            dolt: DoltManager::new(),
            error_at: None,
            doctor_rx: None,
            tool_history_db: None,
            tool_call_sequence: 0,
            show_tool_allow_modal: false,
            tool_allow_modal_state: None,
            agent_bead_id: None,
            worktree_name: None,
            worktree_path: None,
            heartbeat_stop: None,
            hooked_bead_id: None,
            repo_path: crate::db::detect_repo_path(),
            show_kanban_board: false,
            kanban_board_state: None,
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
        }
    }

    /// Validate board column TOML and store any error.
    /// Call after construction to set the initial hint if invalid.
    pub fn validate_board_config(&mut self) {
        if self.config.behavior.mode == "beads"
            && let Err(e) = crate::modals::load_board_config()
        {
            let msg = format!("Board TOML invalid: {e}");
            self.board_config_error = Some(msg.clone());
            self.set_hint(msg);
        }
    }

    pub fn visual_line_count(&mut self) -> u16 {
        if self.main_pane_width == 0 {
            return 0;
        }
        if let Some(cached) = self.cached_visual_line_count {
            return cached;
        }
        // Include both completed lines and the current partial line
        let mut content: Vec<Line> = self.output_lines.to_vec();
        if !self.current_line.is_empty() {
            content.push(Line::raw(&self.current_line));
        }
        let paragraph = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        let count = paragraph.line_count(self.main_pane_width) as u16;
        self.cached_visual_line_count = Some(count);
        count
    }

    pub fn max_scroll(&mut self) -> u16 {
        self.visual_line_count()
            .saturating_sub(self.main_pane_height)
    }

    pub fn scroll_up(&mut self, amount: u16) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(amount);
            self.is_auto_following = false;
        }
    }

    pub fn scroll_down(&mut self, amount: u16) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
        if self.scroll_offset >= max {
            self.is_auto_following = true;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll();
        self.is_auto_following = true;
    }

    /// Adds a styled line to the output.
    pub fn add_line(&mut self, line: Line<'static>) {
        self.output_lines.push(line);
        self.cached_visual_line_count = None;
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    /// Adds a plain text line to the output (convenience method).
    pub fn add_text_line(&mut self, text: String) {
        self.add_line(Line::raw(text));
    }

    /// Appends text with indentation to the current line, flushing complete lines to output.
    /// Used for assistant text which should be indented under the header.
    pub fn append_indented_text(&mut self, text: &str) {
        self.in_indented_text = true;
        for ch in text.chars() {
            if ch == '\n' {
                // Flush current line to output (with indentation prefix)
                let line = std::mem::take(&mut self.current_line);
                self.add_text_line(format!("  {}", line));
            } else {
                self.current_line.push(ch);
            }
        }
        // Update display with partial line if auto-following
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    /// Flushes any remaining text in current_line to output.
    /// Uses indentation if we're in an indented text block.
    pub fn flush_current_line(&mut self) {
        if !self.current_line.is_empty() {
            let line = std::mem::take(&mut self.current_line);
            self.cached_visual_line_count = None;
            if self.in_indented_text {
                self.add_text_line(format!("  {}", line));
            } else {
                self.add_text_line(line);
            }
        }
        self.in_indented_text = false;
    }

    pub fn kill_child(&mut self) {
        if let Some(mut child) = self.child_process.take() {
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            info!(pid, "process_killed");
        }
        self.output_receiver = None;
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
        self.kill_child();
        self.release_hooked_bead();
        self.status = AppStatus::Stopped;
        self.current_spec = None;
        self.run_start_time = None;
    }

    /// Release the currently hooked bead (clear hook, reset to open).
    /// Used during stop between iterations. Does not touch agent, worktree, or heartbeat.
    pub fn release_hooked_bead(&mut self) {
        if let (Some(agent_id), Some(bead_id)) = (&self.agent_bead_id, self.hooked_bead_id.take()) {
            crate::agent::release_bead(&self.config.behavior.bd_path, agent_id, &bead_id);
        }
    }

    pub fn poll_spec(&mut self) {
        // Check for completed background detect_current
        if let Some(rx) = self.spec_poll_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.current_spec = result;
                    self.dirty = true;
                }
                Err(TryRecvError::Empty) => {
                    self.spec_poll_rx = Some(rx); // still running
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    // Thread finished without sending (shouldn't happen)
                }
            }
        }

        // Only kick new poll when running
        if self.status != AppStatus::Running {
            return;
        }

        // In beads mode, skip if dolt server is not confirmed running
        if self.config.behavior.mode == "beads" && self.dolt.state != DoltServerState::On {
            return;
        }

        // Throttle: poll every 2 seconds
        if self.last_spec_poll.elapsed() < Duration::from_secs(2) {
            return;
        }

        self.last_spec_poll = Instant::now();

        // Kick off background detect_current
        let (tx, rx) = mpsc::channel();
        let ws = Arc::clone(&self.work_source);
        std::thread::spawn(move || {
            let _ = tx.send(ws.detect_current());
        });
        self.spec_poll_rx = Some(rx);
    }

    pub fn poll_config(&mut self) {
        // Throttle: poll every 2 seconds
        if self.last_config_poll.elapsed() < Duration::from_secs(2) {
            return;
        }

        self.last_config_poll = Instant::now();

        // Check global config mtime
        let global_mtime = get_file_mtime(&self.config_path);
        let global_changed = match (global_mtime, self.config_mtime) {
            (Some(current), Some(prev)) => current != prev,
            (Some(_), None) => true,
            _ => false,
        };

        // Check project config mtime (also detect new project config appearing)
        let project_path = self
            .project_config_path
            .clone()
            .or_else(get_project_config_path);
        let project_mtime = project_path.as_ref().and_then(|p| get_file_mtime(p));
        let project_changed = match (project_mtime, self.project_config_mtime) {
            (Some(current), Some(prev)) => current != prev,
            (Some(_), None) => true, // project config appeared
            (None, Some(_)) => true, // project config disappeared
            (None, None) => false,
        };

        if !global_changed && !project_changed {
            return;
        }

        self.dirty = true;

        // Update mtimes
        if let Some(mtime) = global_mtime {
            self.config_mtime = Some(mtime);
        }
        self.project_config_mtime = project_mtime;
        // Update project path (may have appeared or disappeared)
        self.project_config_path = project_path;

        let reloaded = reload_config(&self.config_path, self.project_config_path.as_ref());

        // Check if log level changed and update if we have a reload handle
        let new_log_level = &reloaded.config.logging.level;
        if new_log_level != &self.current_log_level
            && let Some(ref handle) = self.log_level_handle
        {
            match logging::update_log_level(handle, new_log_level) {
                Ok(()) => {
                    debug!(
                        old_level = %self.current_log_level,
                        new_level = %new_log_level,
                        "log_level_updated"
                    );
                    self.current_log_level = new_log_level.clone();
                }
                Err(e) => {
                    warn!(error = %e, "log_level_update_failed");
                }
            }
        }

        // Reconstruct work source if mode or specs path changed
        let new_mode = &reloaded.config.behavior.mode;
        let new_specs_path = reloaded.config.specs_path();
        let new_bd_path = &reloaded.config.behavior.bd_path;
        if new_mode != &self.config.behavior.mode
            || new_specs_path != self.config.specs_path()
            || new_bd_path != &self.config.behavior.bd_path
        {
            self.work_source = create_work_source(new_mode, new_specs_path, new_bd_path);
            self.clear_pending_work_ops();
        }

        self.config = reloaded.config;
        self.config_reload_error = reloaded.global_error;
        self.project_config_error = reloaded.project_error;

        if self.config_reload_error.is_none() && self.project_config_error.is_none() {
            self.config_reloaded_at = Some(Instant::now());
        }
    }

    /// Poll for background list_items result (work panel modal).
    pub fn poll_work_items(&mut self) {
        let rx = match self.work_items_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                self.dirty = true;
                if let Some(ref mut panel) = self.specs_panel_state {
                    panel.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.work_items_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                if let Some(ref mut panel) = self.specs_panel_state {
                    panel.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Poll for background kanban board data.
    pub fn poll_kanban_items(&mut self) {
        let rx = match self.kanban_items_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                self.dirty = true;
                if let Some(ref mut board) = self.kanban_board_state {
                    board.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.kanban_items_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                if let Some(ref mut board) = self.kanban_board_state {
                    board.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Poll for background bead detail data (drill-down from kanban board).
    pub fn poll_bead_detail(&mut self) {
        let rx = match self.bead_detail_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                self.dirty = true;
                if let Some(ref mut board) = self.kanban_board_state
                    && let Some(ref mut detail) = board.detail_view
                {
                    detail.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.bead_detail_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                if let Some(ref mut board) = self.kanban_board_state
                    && let Some(ref mut detail) = board.detail_view
                {
                    detail.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Poll for filesystem changes on .beads/ and trigger board re-fetch.
    pub fn poll_kanban_watcher(&mut self) {
        let rx = match self.kanban_fs_rx.as_ref() {
            Some(rx) => rx,
            None => return,
        };

        // Drain all pending events (we only care that something changed)
        let mut changed = false;
        while rx.try_recv().is_ok() {
            changed = true;
        }

        // If something changed and we're not already fetching, trigger re-fetch
        if changed
            && self.kanban_items_rx.is_none()
            && self.show_kanban_board
            && let Some(ref board) = self.kanban_board_state
        {
            let bd_path = self.config.behavior.bd_path.clone();
            let column_defs = board.column_defs.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = crate::modals::fetch_board_data(&bd_path, &column_defs);
                let _ = tx.send(result);
            });
            self.kanban_items_rx = Some(rx);
        }

        // Also refresh the detail modal if one is open
        if changed
            && self.bead_detail_rx.is_none()
            && let Some(ref board) = self.kanban_board_state
            && let Some(ref detail) = board.detail_view
            && !detail.is_loading
        {
            let bd_path = self.config.behavior.bd_path.clone();
            let bead_id = detail.id.clone();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let output = std::process::Command::new(&bd_path)
                    .args(["show", &bead_id, "--json"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output();
                let result = match output {
                    Ok(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        serde_json::from_str::<serde_json::Value>(&stdout)
                            .map(|val| {
                                if let Some(arr) = val.as_array() {
                                    arr.first().cloned().unwrap_or(val)
                                } else {
                                    val
                                }
                            })
                            .map_err(|e| e.to_string())
                    }
                    Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(result);
            });
            self.bead_detail_rx = Some(rx);
        }
    }

    /// Poll for background bead picker data.
    pub fn poll_bead_picker(&mut self) {
        let rx = match self.bead_picker_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                self.dirty = true;
                if let Some(ref mut state) = self.bead_picker_state {
                    state.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.bead_picker_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                if let Some(ref mut state) = self.bead_picker_state {
                    state.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Open the bead picker modal and start background data fetch.
    pub fn open_bead_picker(&mut self) {
        self.show_bead_picker = true;
        self.bead_picker_state = Some(crate::modals::BeadPickerState::new_loading());
        self.bead_picker_result = None;
        let bd_path = self.config.behavior.bd_path.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::modals::fetch_bead_picker_data(&bd_path);
            let _ = tx.send(result);
        });
        self.bead_picker_rx = Some(rx);
    }

    /// If a bead was picked and a dependency is pending, run `bd dep add`.
    pub fn poll_pending_dep(&mut self) {
        let picked_id = match self.bead_picker_result.take() {
            Some(id) => id,
            None => return,
        };
        let dep = match self.pending_dep.take() {
            Some(d) => d,
            None => return,
        };
        let bd_path = self.config.behavior.bd_path.clone();
        let (issue, depends_on) = match dep.direction {
            crate::modals::DepDirection::BlockedBy => (dep.bead_id, picked_id),
            crate::modals::DepDirection::Blocks => (picked_id, dep.bead_id),
        };
        if let Some(state) = &mut self.kanban_board_state {
            state.push_action(crate::modals::BoardAction::AddDependency {
                issue: issue.clone(),
                depends_on: depends_on.clone(),
            });
        }
        std::thread::spawn(move || {
            std::process::Command::new(&bd_path)
                .args(["dep", "add", &issue, &depends_on])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .ok();
        });
    }

    /// Stop the kanban filesystem watcher.
    pub fn stop_kanban_watcher(&mut self) {
        if let Some(stop) = self.kanban_watcher_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.kanban_fs_rx = None;
    }

    /// Clear all pending background work source operations.
    fn clear_pending_work_ops(&mut self) {
        self.spec_poll_rx = None;
        self.pending_work_check = None;
        self.work_items_rx = None;
        self.dolt.clear();
    }

    /// Poll for Dolt server status (beads mode only, throttled).
    pub fn poll_dolt_status(&mut self) {
        if self
            .dolt
            .poll_status(&self.config.behavior.bd_path, &self.config.behavior.mode)
        {
            self.dirty = true;
        }
    }

    /// Poll for Dolt toggle (start/stop) completion.
    pub fn poll_dolt_toggle(&mut self) {
        if self.dolt.poll_toggle() {
            self.dirty = true;
        }
    }

    /// Poll for background doctor check results. Displays only failures.
    pub fn poll_doctor(&mut self) {
        let rx = match self.doctor_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(checks) => {
                for check in &checks {
                    if !check.passed {
                        self.dirty = true;
                        self.add_text_line(format!("\u{2717} {}", check.message));
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                self.doctor_rx = Some(rx);
            }
            Err(TryRecvError::Disconnected) => {}
        }
    }

    /// Toggle Dolt server on/off (beads mode only).
    pub fn toggle_dolt_server(&mut self) {
        self.dolt
            .toggle(&self.config.behavior.bd_path, &self.config.behavior.mode);
    }

    /// Merge the current worktree branch to main and clean up.
    /// Returns true if merge succeeded (or no worktree to merge).
    /// Returns false if a merge conflict stopped the loop.
    pub fn merge_current_worktree(&mut self) -> bool {
        let Some(ref wt_name) = self.worktree_name else {
            return true;
        };

        if crate::agent::merge_worktree_to_main(wt_name) {
            let bd_path = self.config.behavior.bd_path.clone();
            let wt_name = wt_name.clone();
            crate::agent::remove_merged_worktree(&bd_path, &wt_name);
            self.worktree_name = None;
            self.worktree_path = None;
            true
        } else {
            let bd_path = self.config.behavior.bd_path.clone();
            let wt_name = wt_name.clone();

            if let Some(existing_bead_id) =
                crate::agent::find_merge_conflict_bead(&bd_path, &wt_name)
            {
                // Tier 2: Claude already tried — escalate to human
                crate::agent::escalate_merge_conflict(&bd_path, &wt_name, &existing_bead_id);
                self.add_text_line(
                    "[Merge conflict persists after Claude attempt — filed human bead, stopping]"
                        .into(),
                );
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
                false
            } else if let Some(bead_id) =
                crate::agent::file_merge_conflict_bead(&bd_path, &wt_name)
            {
                // Tier 1: First conflict — file bead, Claude resolves next iteration
                self.add_text_line(format!(
                    "[Merge conflict — filed {}, Claude will resolve next iteration]",
                    bead_id
                ));
                // Worktree preserved
                true
            } else {
                self.add_text_line(
                    "[Merge conflict — failed to file bead, stopping]".into(),
                );
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
                false
            }
        }
    }

    /// Clean up agent resources on quit (beads mode only).
    /// Full teardown: release bead, stop heartbeat, remove worktree, close agent.
    pub fn cleanup_agent(&mut self) {
        // Release hooked bead first (clear hook + reset to open)
        self.release_hooked_bead();

        // Signal heartbeat thread to stop
        if let Some(stop) = &self.heartbeat_stop {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        if let (Some(agent_id), Some(wt_name)) = (&self.agent_bead_id, &self.worktree_name) {
            crate::agent::cleanup(&self.config.behavior.bd_path, agent_id, wt_name);
        }

        self.agent_bead_id = None;
        self.worktree_name = None;
        self.worktree_path = None;
        self.heartbeat_stop = None;
    }
}
