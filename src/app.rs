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
use crate::modals::{ConfigModalState, InitModalState, SpecsPanelState, ToolAllowModalState};
use crate::wake_lock::WakeLock;
use crate::work_source::{WorkItem, WorkRemaining, WorkSource, create_work_source};
use crate::{OutputMessage, get_file_mtime, logging};

/// Application status states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Stopped,
    Running,
    Error,
}

/// State of the Dolt SQL server (beads mode only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoltServerState {
    /// Haven't checked yet.
    Unknown,
    /// Server is not running.
    Off,
    /// Server is starting up (~5s).
    Starting,
    /// Server is running.
    On,
    /// Server is shutting down.
    Stopping,
}

impl AppStatus {
    pub fn border_type(&self) -> BorderType {
        match self {
            AppStatus::Stopped => BorderType::Rounded,
            AppStatus::Running | AppStatus::Error => BorderType::Double,
        }
    }

    /// Returns the color for this status, with pulsing effect for Error state.
    /// The pulse alternates between red and dark red at ~2Hz (every 15 frames at 30fps).
    pub fn pulsing_color(&self, frame_count: u64) -> Color {
        match self {
            AppStatus::Stopped => Color::Cyan,
            AppStatus::Running => Color::Green,
            AppStatus::Error => {
                if (frame_count / 15).is_multiple_of(2) {
                    Color::Red
                } else {
                    Color::Rgb(128, 0, 0)
                }
            }
        }
    }
}

/// Tracks accumulated state for a content block being streamed.
#[derive(Debug, Default)]
pub struct ContentBlockState {
    /// For text blocks: accumulated text content.
    pub text: String,
    /// For tool_use blocks: the tool name.
    pub tool_name: Option<String>,
    /// For tool_use blocks: the tool use ID (for correlating with results).
    pub tool_use_id: Option<String>,
    /// For tool_use blocks: accumulated JSON input string.
    pub input_json: String,
    /// Whether we've shown the assistant header for this text block.
    pub header_shown: bool,
}

/// A pending tool call waiting for its result.
#[derive(Debug, Clone)]
pub struct PendingToolCall {
    /// The tool name (e.g., "Read", "Bash").
    pub tool_name: String,
    /// The styled line to display.
    pub styled_line: Line<'static>,
}

/// Status of a tool call in the panel display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallStatus {
    /// Tool call sent, waiting for result.
    Pending,
    /// Tool call completed successfully.
    Success,
    /// Tool call returned an error.
    Error,
}

/// A tool call entry for the panel display.
#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    /// The tool name (e.g., "Read", "Bash").
    pub tool_name: String,
    /// Summary of the key argument (e.g., "git status", "/path/to/file.rs").
    pub summary: String,
    /// Current status.
    pub status: ToolCallStatus,
    /// Tool use ID for correlating with results.
    pub tool_use_id: Option<String>,
}

/// Which panel is currently focused for scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectedPanel {
    Main,
    Tools,
}

impl SelectedPanel {
    pub fn toggle(&mut self) {
        *self = match self {
            SelectedPanel::Main => SelectedPanel::Tools,
            SelectedPanel::Tools => SelectedPanel::Main,
        }
    }
}

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
    /// Path to the project configuration file (.ralph), if it existed at startup.
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
    /// Error message if project config (.ralph) reload failed.
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
    /// Frame counter for animations (incremented each render cycle).
    pub frame_count: u64,
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
    /// Maps tool_use_id to tool name for correlating results with calls.
    pub tool_id_to_name: HashMap<String, String>,
    /// Pending tool calls waiting for their results (keyed by tool_use_id).
    pub pending_tool_calls: HashMap<String, PendingToolCall>,
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
    /// Current Dolt server state (beads mode only).
    pub dolt_server_state: DoltServerState,
    /// Receiver for background dolt status check.
    pub dolt_status_rx: Option<Receiver<bool>>,
    /// Receiver for background dolt start/stop operation.
    pub dolt_toggle_rx: Option<Receiver<bool>>,
    /// Last time we polled dolt server status.
    pub last_dolt_poll: Instant,
    /// When the app entered Error state (for auto-clearing the pulsing flash).
    pub error_at: Option<Instant>,
    /// Receiver for background doctor checks (run once on TUI open).
    pub doctor_rx: Option<Receiver<Vec<doctor::CheckResult>>>,
    /// SQLite connection for tool call recording (None if DB open failed at startup).
    pub tool_history_db: Option<Connection>,
    /// Sequence counter for tool calls within this session.
    pub tool_call_sequence: u32,
    /// Tool call entries for the panel display.
    pub tool_call_entries: Vec<ToolCallEntry>,
    /// Scroll offset for the tool panel.
    pub tool_panel_scroll_offset: u16,
    /// Which panel is currently focused for scrolling.
    pub selected_panel: SelectedPanel,
    /// Whether the tool panel is collapsed.
    pub tool_panel_collapsed: bool,
    /// Cached height of the tool panel (set during draw).
    pub tool_panel_height: u16,
    /// Selected tool index when tools panel is focused (None = no selection).
    pub tool_panel_selected: Option<usize>,
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
            frame_count: 0,
            config_modal_state: None,
            auto_continue_pending: false,
            show_specs_panel: false,
            specs_panel_state: None,
            show_init_modal: false,
            init_modal_state: None,
            show_help_modal: false,
            show_quit_modal: false,
            current_iteration: 0,
            total_iterations: 0,
            cumulative_tokens: 0,
            exchange_count: 0,
            last_tool_used: None,
            wake_lock: None,
            tool_id_to_name: HashMap::new(),
            pending_tool_calls: HashMap::new(),
            in_indented_text: false,
            work_source,
            spec_poll_rx: None,
            pending_work_check: None,
            work_items_rx: None,
            dolt_server_state: DoltServerState::Unknown,
            dolt_status_rx: None,
            dolt_toggle_rx: None,
            // Initialize to "long ago" so we poll immediately on start
            last_dolt_poll: Instant::now() - Duration::from_secs(10),
            error_at: None,
            doctor_rx: None,
            tool_history_db: None,
            tool_call_sequence: 0,
            tool_call_entries: Vec::new(),
            tool_panel_scroll_offset: 0,
            selected_panel: SelectedPanel::Main,
            tool_panel_collapsed: false,
            tool_panel_height: 0,
            tool_panel_selected: None,
            show_tool_allow_modal: false,
            tool_allow_modal_state: None,
            agent_bead_id: None,
            worktree_name: None,
            worktree_path: None,
            heartbeat_stop: None,
            hooked_bead_id: None,
            repo_path: crate::db::detect_repo_path(),
        }
    }

    pub fn visual_line_count(&self) -> u16 {
        if self.main_pane_width == 0 {
            return 0;
        }
        // Include both completed lines and the current partial line
        let mut content: Vec<Line> = self.output_lines.to_vec();
        if !self.current_line.is_empty() {
            content.push(Line::raw(&self.current_line));
        }
        let paragraph = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL))
            .wrap(Wrap { trim: false });
        paragraph.line_count(self.main_pane_width) as u16
    }

    pub fn max_scroll(&self) -> u16 {
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

    /// Add a tool call entry to the panel.
    pub fn add_tool_call_entry(&mut self, entry: ToolCallEntry) {
        self.tool_call_entries.push(entry);
    }

    /// Update the status of a tool call entry by tool_use_id.
    pub fn update_tool_call_status(&mut self, tool_use_id: &str, status: ToolCallStatus) {
        if let Some(entry) = self
            .tool_call_entries
            .iter_mut()
            .rev()
            .find(|e| e.tool_use_id.as_deref() == Some(tool_use_id))
        {
            entry.status = status;
        }
    }

    pub fn scroll_tools_up(&mut self, amount: u16) {
        if amount == 1 {
            // Single-step: move selection
            self.select_tool_prev();
        } else {
            // Page scroll
            self.tool_panel_scroll_offset = self.tool_panel_scroll_offset.saturating_sub(amount);
        }
    }

    pub fn scroll_tools_down(&mut self, amount: u16) {
        if amount == 1 {
            // Single-step: move selection
            self.select_tool_next();
        } else {
            // Page scroll
            let max = self
                .tool_call_entries
                .len()
                .saturating_sub(self.tool_panel_height.saturating_sub(2) as usize)
                as u16;
            self.tool_panel_scroll_offset = (self.tool_panel_scroll_offset + amount).min(max);
        }
    }

    /// Move tool panel selection up.
    fn select_tool_prev(&mut self) {
        if self.tool_call_entries.is_empty() {
            return;
        }
        let current = self.tool_panel_selected.unwrap_or(0);
        let new = current.saturating_sub(1);
        self.tool_panel_selected = Some(new);
        self.ensure_tool_selection_visible();
    }

    /// Move tool panel selection down.
    fn select_tool_next(&mut self) {
        if self.tool_call_entries.is_empty() {
            return;
        }
        let max = self.tool_call_entries.len().saturating_sub(1);
        let current = self.tool_panel_selected.unwrap_or(0);
        let new = (current + 1).min(max);
        self.tool_panel_selected = Some(new);
        self.ensure_tool_selection_visible();
    }

    /// Ensure the selected tool is visible in the scroll viewport.
    fn ensure_tool_selection_visible(&mut self) {
        let Some(selected) = self.tool_panel_selected else {
            return;
        };
        let inner_height = self.tool_panel_height.saturating_sub(2) as usize;
        if inner_height == 0 {
            return;
        }
        let offset = self.tool_panel_scroll_offset as usize;
        if selected < offset {
            self.tool_panel_scroll_offset = selected as u16;
        } else if selected >= offset + inner_height {
            self.tool_panel_scroll_offset = (selected - inner_height + 1) as u16;
        }
    }

    /// Auto-revert from Error to Stopped after a timeout so pulsing doesn't last forever.
    pub fn check_error_timeout(&mut self) {
        if let Some(at) = self.error_at
            && at.elapsed() >= Duration::from_secs(5)
        {
            self.status = AppStatus::Stopped;
            self.error_at = None;
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
        if let (Some(agent_id), Some(bead_id)) = (&self.agent_bead_id, self.hooked_bead_id.take())
        {
            crate::agent::release_bead(&self.config.behavior.bd_path, agent_id, &bead_id);
        }
    }

    pub fn poll_spec(&mut self) {
        // Check for completed background detect_current
        if let Some(rx) = self.spec_poll_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.current_spec = result;
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
        if self.config.behavior.mode == "beads" && self.dolt_server_state != DoltServerState::On {
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

        // Check project config mtime (also detect new .ralph appearing)
        let project_path = self
            .project_config_path
            .clone()
            .or_else(get_project_config_path);
        let project_mtime = project_path.as_ref().and_then(|p| get_file_mtime(p));
        let project_changed = match (project_mtime, self.project_config_mtime) {
            (Some(current), Some(prev)) => current != prev,
            (Some(_), None) => true, // .ralph appeared
            (None, Some(_)) => true, // .ralph disappeared
            (None, None) => false,
        };

        if !global_changed && !project_changed {
            return;
        }

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

    /// Handle poll_output logic, returning whether auto-continue should be pending.
    pub fn handle_channel_disconnected(&mut self, exit_code: Option<i32>) {
        self.output_receiver = None;
        self.current_spec = None;
        self.run_start_time = None;
        // Release wake lock when process ends (drop releases it)
        self.wake_lock = None;

        // Determine next state based on exit code and iteration control
        match exit_code {
            Some(0) if self.should_auto_continue() => {
                // In beads mode, skip work check if dolt server is not running
                if self.config.behavior.mode == "beads"
                    && self.dolt_server_state != DoltServerState::On
                {
                    self.reset_iteration_state();
                    self.status = AppStatus::Stopped;
                } else {
                    // Kick off background check_remaining (non-blocking)
                    let complete_msg = self.work_source.complete_message();
                    let (tx, rx) = mpsc::channel();
                    let ws = Arc::clone(&self.work_source);
                    std::thread::spawn(move || {
                        let _ = tx.send((ws.check_remaining(), complete_msg));
                    });
                    self.pending_work_check = Some(rx);
                    self.status = AppStatus::Stopped;
                }
            }
            Some(0) => {
                // Countdown exhausted or iterations = 0, just stop
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
            Some(code) => {
                // Non-zero exit code → Error state
                self.reset_iteration_state();
                self.add_text_line(format!("[Error: process exited with code {}]", code));
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
            None => {
                // Killed by signal (manual stop) → Stopped state
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
        }
    }

    /// Poll for background check_remaining result (auto-continue decision).
    pub fn poll_work_check(&mut self) {
        let rx = match self.pending_work_check.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok((result, complete_msg)) => {
                // Discard stale results if we're no longer in Stopped state
                if self.status != AppStatus::Stopped {
                    return;
                }
                self.handle_work_remaining(result, complete_msg);
            }
            Err(TryRecvError::Empty) => {
                self.pending_work_check = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.handle_work_remaining(
                    WorkRemaining::ReadError("background check failed".to_string()),
                    self.work_source.complete_message(),
                );
            }
        }
    }

    /// Process a check_remaining result for auto-continue decisions.
    fn handle_work_remaining(&mut self, result: WorkRemaining, complete_msg: &str) {
        match result {
            WorkRemaining::Yes => {
                info!(
                    current = self.current_iteration,
                    total = self.total_iterations,
                    "auto_continue"
                );
                self.add_text_line(
                    "══════════════════ AUTO-CONTINUING ══════════════════".to_string(),
                );
                self.auto_continue_pending = true;
                // Don't clear tasks during auto-continue
            }
            WorkRemaining::No => {
                info!("all_work_complete");
                self.add_text_line(format!(
                    "══════════════════ {} ══════════════════",
                    complete_msg
                ));
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
            WorkRemaining::NeedsShaping(count) => {
                info!(count, "all_ready_beads_need_shaping");
                self.add_text_line(
                    "══════════════════ ALL READY BEADS NEED SHAPING ══════════════════"
                        .to_string(),
                );
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
            WorkRemaining::Missing => {
                warn!("work_source_missing");
                self.add_text_line("[Error: work source not found]".to_string());
                self.reset_iteration_state();
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
            WorkRemaining::ReadError(e) => {
                warn!(error = %e, "work_source_read_error");
                self.add_text_line(format!("[Error reading work source: {}]", e));
                self.reset_iteration_state();
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
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
                if let Some(ref mut panel) = self.specs_panel_state {
                    panel.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.work_items_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                if let Some(ref mut panel) = self.specs_panel_state {
                    panel.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Clear all pending background work source operations.
    fn clear_pending_work_ops(&mut self) {
        self.spec_poll_rx = None;
        self.pending_work_check = None;
        self.work_items_rx = None;
        self.dolt_status_rx = None;
        self.dolt_toggle_rx = None;
        self.dolt_server_state = DoltServerState::Unknown;
    }

    /// Determine if we should auto-continue based on iteration state.
    /// - Infinite mode (total_iterations < 0): always continue
    /// - Countdown mode (total_iterations > 0): continue if current < total
    /// - Stopped mode (total_iterations = 0): never continue
    fn should_auto_continue(&self) -> bool {
        if self.total_iterations < 0 {
            // Infinite mode
            true
        } else if self.total_iterations > 0 {
            // Countdown mode
            self.current_iteration < self.total_iterations as u32
        } else {
            // Stopped mode (shouldn't happen if we got here, but be safe)
            false
        }
    }

    /// Reset iteration state when stopping (error, manual stop, or run complete).
    pub fn reset_iteration_state(&mut self) {
        self.current_iteration = 0;
        self.total_iterations = 0;
    }

    /// Start a new iteration run, reading config and setting up iteration tracking.
    /// Returns false if iterations = 0 (stopped mode).
    pub fn start_iteration_run(&mut self) -> bool {
        let iterations = self.config.behavior.iterations;
        if iterations == 0 {
            // Stopped mode - don't start
            info!("iterations_zero_no_start");
            return false;
        }

        self.total_iterations = iterations;
        self.current_iteration = 1;
        true
    }

    /// Increment iteration counter for auto-continue.
    pub fn increment_iteration(&mut self) {
        self.current_iteration += 1;
    }

    /// Poll for Dolt server status (beads mode only, throttled).
    pub fn poll_dolt_status(&mut self) {
        if self.config.behavior.mode != "beads" {
            return;
        }

        // Check for completed background status check
        if let Some(rx) = self.dolt_status_rx.take() {
            match rx.try_recv() {
                Ok(running) => {
                    // Only update if not in a transitional state
                    if self.dolt_server_state != DoltServerState::Starting
                        && self.dolt_server_state != DoltServerState::Stopping
                    {
                        self.dolt_server_state = if running {
                            DoltServerState::On
                        } else {
                            DoltServerState::Off
                        };
                    }
                }
                Err(TryRecvError::Empty) => {
                    self.dolt_status_rx = Some(rx);
                    return;
                }
                Err(TryRecvError::Disconnected) => {}
            }
        }

        // Don't poll during transitional states
        if self.dolt_server_state == DoltServerState::Starting
            || self.dolt_server_state == DoltServerState::Stopping
        {
            return;
        }

        // Throttle: poll every 5 seconds
        if self.last_dolt_poll.elapsed() < Duration::from_secs(5) {
            return;
        }

        self.last_dolt_poll = Instant::now();

        // Kick off background status check
        let (tx, rx) = mpsc::channel();
        let bd_path = self.config.behavior.bd_path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(check_dolt_running(&bd_path));
        });
        self.dolt_status_rx = Some(rx);
    }

    /// Poll for Dolt toggle (start/stop) completion.
    pub fn poll_dolt_toggle(&mut self) {
        let rx = match self.dolt_toggle_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(success) => {
                match self.dolt_server_state {
                    DoltServerState::Starting => {
                        self.dolt_server_state = if success {
                            DoltServerState::On
                        } else {
                            DoltServerState::Off
                        };
                    }
                    DoltServerState::Stopping => {
                        self.dolt_server_state = if success {
                            DoltServerState::Off
                        } else {
                            DoltServerState::On
                        };
                    }
                    _ => {}
                }
                // Reset poll timer so we verify state soon
                self.last_dolt_poll = Instant::now() - Duration::from_secs(10);
            }
            Err(TryRecvError::Empty) => {
                self.dolt_toggle_rx = Some(rx);
            }
            Err(TryRecvError::Disconnected) => match self.dolt_server_state {
                DoltServerState::Starting => self.dolt_server_state = DoltServerState::Off,
                DoltServerState::Stopping => self.dolt_server_state = DoltServerState::On,
                _ => {}
            },
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
        if self.config.behavior.mode != "beads" {
            return;
        }

        match self.dolt_server_state {
            DoltServerState::Starting | DoltServerState::Stopping => (),
            DoltServerState::On => {
                info!("dolt_server_stopping");
                self.dolt_server_state = DoltServerState::Stopping;
                // Discard any in-flight status poll so stale results don't override
                self.dolt_status_rx = None;
                let (tx, rx) = mpsc::channel();
                let bd_path = self.config.behavior.bd_path.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(run_dolt_command(&bd_path, "stop"));
                });
                self.dolt_toggle_rx = Some(rx);
            }
            DoltServerState::Off | DoltServerState::Unknown => {
                info!("dolt_server_starting");
                self.dolt_server_state = DoltServerState::Starting;
                // Discard any in-flight status poll so stale results don't override
                self.dolt_status_rx = None;
                let (tx, rx) = mpsc::channel();
                let bd_path = self.config.behavior.bd_path.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(run_dolt_command(&bd_path, "start"));
                });
                self.dolt_toggle_rx = Some(rx);
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

/// Check if the Dolt server is running by calling `bd dolt status`.
fn check_dolt_running(bd_path: &str) -> bool {
    std::process::Command::new(bd_path)
        .args(["dolt", "status"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("server: running")
        })
        .unwrap_or(false)
}

/// Run a `bd dolt` subcommand (start/stop) and return whether it succeeded.
fn run_dolt_command(bd_path: &str, subcmd: &str) -> bool {
    std::process::Command::new(bd_path)
        .args(["dolt", subcmd])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
