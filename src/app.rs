//! Application state and core logic.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ratatui::style::Color;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use tracing::{debug, info, warn};

use crate::config::reload_config;
use crate::config::{Config, LoadedConfig};
use crate::logging::ReloadHandle;
use crate::modals::{ConfigModalState, SpecsPanelState};
use crate::specs::{SpecsRemaining, check_specs_remaining, detect_current_spec};
use crate::ui::{TodoItem, TodoStatus};
use crate::wake_lock::WakeLock;
use crate::{OutputMessage, get_file_mtime, logging};

/// Application status states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Stopped,
    Running,
    Error,
}

/// Panels that can be selected/focused for scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectedPanel {
    /// Main output panel (default).
    #[default]
    Main,
    /// Tasks panel.
    Tasks,
}

impl SelectedPanel {
    /// Toggle between Main and Tasks panels.
    pub fn toggle(self) -> Self {
        match self {
            Self::Main => Self::Tasks,
            Self::Tasks => Self::Main,
        }
    }
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
    /// For tool_use blocks: accumulated JSON input string.
    pub input_json: String,
}

/// Main application state.
pub struct App {
    pub status: AppStatus,
    pub output_lines: Vec<String>,
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
    /// Path to the configuration file.
    pub config_path: PathBuf,
    /// Last known mtime of the config file for change detection.
    pub config_mtime: Option<SystemTime>,
    /// Last time we polled for config changes.
    pub last_config_poll: Instant,
    /// When config was last successfully reloaded (for "Reloaded" indicator fade).
    pub config_reloaded_at: Option<Instant>,
    /// Error message if config reload failed (invalid TOML, etc.).
    pub config_reload_error: Option<String>,
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
    /// Current task list (updated in-place by TodoWrite events).
    pub tasks: Vec<TodoItem>,
    /// Scroll offset for the tasks panel.
    pub tasks_scroll_offset: u16,
    /// Height of the tasks panel content area (excluding borders).
    pub tasks_pane_height: u16,
    /// Currently selected panel for scroll operations.
    pub selected_panel: SelectedPanel,
    /// Whether the tasks panel is manually collapsed.
    pub tasks_panel_collapsed: bool,
    /// Wake lock to prevent system idle sleep while running.
    pub wake_lock: Option<WakeLock>,
}

impl App {
    pub fn new(
        session_id: String,
        log_directory: Option<PathBuf>,
        loaded_config: LoadedConfig,
        log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
    ) -> Self {
        let current_log_level = loaded_config.config.logging.level.clone();
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
            config_mtime: get_file_mtime(&loaded_config.config_path),
            // Initialize to "long ago" so we poll immediately on start
            last_config_poll: Instant::now() - Duration::from_secs(10),
            config_reloaded_at: None,
            config_reload_error: None,
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
            current_iteration: 0,
            total_iterations: 0,
            cumulative_tokens: 0,
            exchange_count: 0,
            last_tool_used: None,
            tasks: Vec::new(),
            tasks_scroll_offset: 0,
            tasks_pane_height: 0,
            selected_panel: SelectedPanel::default(),
            tasks_panel_collapsed: false,
            wake_lock: None,
        }
    }

    pub fn visual_line_count(&self) -> u16 {
        if self.main_pane_width == 0 {
            return 0;
        }
        // Include both completed lines and the current partial line
        let mut content: Vec<Line> = self.output_lines.iter().map(Line::raw).collect();
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

    pub fn add_line(&mut self, line: String) {
        self.output_lines.push(line);
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    /// Appends text to the current line, flushing complete lines to output.
    pub fn append_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                // Flush current line to output
                let line = std::mem::take(&mut self.current_line);
                self.add_line(line);
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
    pub fn flush_current_line(&mut self) {
        if !self.current_line.is_empty() {
            let line = std::mem::take(&mut self.current_line);
            self.add_line(line);
        }
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

    /// Stop the running command (user-initiated)
    pub fn stop_command(&mut self) {
        if self.status != AppStatus::Running {
            return;
        }
        info!("manual_stop");
        self.kill_child();
        self.status = AppStatus::Stopped;
        self.current_spec = None;
        self.run_start_time = None;
        self.clear_tasks();
    }

    pub fn poll_spec(&mut self) {
        // Only poll when running
        if self.status != AppStatus::Running {
            return;
        }

        // Throttle: poll every 2 seconds
        if self.last_spec_poll.elapsed() < Duration::from_secs(2) {
            return;
        }

        self.last_spec_poll = Instant::now();
        self.current_spec = detect_current_spec(&self.config.specs_path());
    }

    pub fn poll_config(&mut self) {
        // Throttle: poll every 2 seconds
        if self.last_config_poll.elapsed() < Duration::from_secs(2) {
            return;
        }

        self.last_config_poll = Instant::now();

        // Get current mtime
        let current_mtime = match get_file_mtime(&self.config_path) {
            Some(mtime) => mtime,
            None => {
                debug!(path = ?self.config_path, "config_mtime_check_failed");
                return;
            }
        };

        // Check if mtime changed
        let mtime_changed = match self.config_mtime {
            Some(prev_mtime) => current_mtime != prev_mtime,
            None => true, // No previous mtime, treat as changed
        };

        if !mtime_changed {
            return;
        }

        // Mtime changed, attempt to reload config
        self.config_mtime = Some(current_mtime);

        match reload_config(&self.config_path) {
            Ok(new_config) => {
                // Check if log level changed and update if we have a reload handle
                let new_log_level = &new_config.logging.level;
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
                            // Continue with config reload, just don't update log level
                        }
                    }
                }

                self.config = new_config;
                self.config_reload_error = None;
                self.config_reloaded_at = Some(Instant::now());
            }
            Err(error) => {
                // Keep previous config, show error
                self.config_reload_error = Some(error);
            }
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
                // Check if there are specs remaining
                match check_specs_remaining(&self.config.specs_path()) {
                    SpecsRemaining::Yes => {
                        info!(
                            current = self.current_iteration,
                            total = self.total_iterations,
                            "auto_continue"
                        );
                        self.add_line(
                            "══════════════════ AUTO-CONTINUING ══════════════════".to_string(),
                        );
                        self.auto_continue_pending = true;
                        self.status = AppStatus::Stopped;
                        // Don't clear tasks during auto-continue
                    }
                    SpecsRemaining::No => {
                        info!("all_specs_complete");
                        self.add_line(
                            "══════════════════ ALL SPECS COMPLETE ══════════════════".to_string(),
                        );
                        self.reset_iteration_state();
                        self.status = AppStatus::Stopped;
                        self.clear_tasks();
                    }
                    SpecsRemaining::Missing => {
                        warn!("specs_readme_missing");
                        self.add_line("[Error: specs/README.md not found]".to_string());
                        self.reset_iteration_state();
                        self.status = AppStatus::Error;
                        self.clear_tasks();
                    }
                    SpecsRemaining::ReadError(e) => {
                        warn!(error = %e, "specs_readme_read_error");
                        self.add_line(format!("[Error reading specs/README.md: {}]", e));
                        self.reset_iteration_state();
                        self.status = AppStatus::Error;
                        self.clear_tasks();
                    }
                }
            }
            Some(0) => {
                // Countdown exhausted or iterations = 0, just stop
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
                self.clear_tasks();
            }
            Some(_code) => {
                // Non-zero exit code → Error state
                self.reset_iteration_state();
                self.status = AppStatus::Error;
                self.clear_tasks();
            }
            None => {
                // Killed by signal (manual stop) → Stopped state
                self.reset_iteration_state();
                self.status = AppStatus::Stopped;
                self.clear_tasks();
            }
        }
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
    fn reset_iteration_state(&mut self) {
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

    /// Update the task list from a TodoWrite event.
    /// Replaces all existing tasks and logs the change.
    /// If `auto_expand_tasks_panel` config is enabled, expands the panel.
    pub fn update_tasks(&mut self, tasks: Vec<TodoItem>) {
        let task_count = tasks.len();
        let completed = tasks
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count();
        let in_progress = tasks
            .iter()
            .filter(|t| t.status == TodoStatus::InProgress)
            .count();
        let pending = tasks
            .iter()
            .filter(|t| t.status == TodoStatus::Pending)
            .count();

        info!(task_count, completed, in_progress, pending, "tasks_updated");

        self.tasks = tasks;

        // Auto-expand if configured (only affects collapse state when tasks arrive)
        if self.config.behavior.auto_expand_tasks_panel && !self.tasks.is_empty() {
            self.tasks_panel_collapsed = false;
        }

        // Reset scroll to bottom to show latest state
        self.scroll_tasks_to_bottom();
    }

    /// Clear all tasks and reset scroll and collapse state.
    pub fn clear_tasks(&mut self) {
        if !self.tasks.is_empty() {
            info!(previous_count = self.tasks.len(), "tasks_cleared");
            self.tasks.clear();
            self.tasks_scroll_offset = 0;
            self.tasks_panel_collapsed = false;
        }
    }

    /// Toggle the tasks panel collapsed state.
    /// Only works when there are tasks (no-op when empty).
    pub fn toggle_tasks_panel_collapsed(&mut self) {
        if !self.tasks.is_empty() {
            self.tasks_panel_collapsed = !self.tasks_panel_collapsed;
        }
    }

    /// Get the count of completed tasks.
    pub fn completed_task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == TodoStatus::Completed)
            .count()
    }

    /// Calculate the maximum scroll offset for the tasks panel.
    pub fn max_tasks_scroll(&self) -> u16 {
        let task_lines = self.tasks.len() as u16;
        task_lines.saturating_sub(self.tasks_pane_height)
    }

    /// Scroll the tasks panel up.
    pub fn scroll_tasks_up(&mut self, amount: u16) {
        self.tasks_scroll_offset = self.tasks_scroll_offset.saturating_sub(amount);
    }

    /// Scroll the tasks panel down.
    pub fn scroll_tasks_down(&mut self, amount: u16) {
        let max = self.max_tasks_scroll();
        self.tasks_scroll_offset = (self.tasks_scroll_offset + amount).min(max);
    }

    /// Scroll tasks panel to bottom.
    pub fn scroll_tasks_to_bottom(&mut self) {
        self.tasks_scroll_offset = self.max_tasks_scroll();
    }
}
