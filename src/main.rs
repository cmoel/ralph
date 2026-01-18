mod config;
mod events;
mod logging;

use crate::config::{
    Config, ConfigLoadStatus, LoadedConfig, ensure_config_exists, reload_config, save_config,
};

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::logging::ReloadHandle;

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    Wrap,
};
use ratatui::{DefaultTerminal, Terminal};
use tracing::{debug, info, trace, warn};

use crate::events::{ClaudeEvent, ContentBlock, Delta, StreamInnerEvent};

/// Contract a path by replacing the home directory with `~` for display.
#[allow(dead_code)] // Will be used in later UI slices
fn contract_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(suffix) = path.strip_prefix(&home)
    {
        return format!("~/{}", suffix.display());
    }
    path.display().to_string()
}

/// Get the modification time of a file, or None if it can't be determined.
fn get_file_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppStatus {
    Stopped,
    Running,
    Error,
}

impl AppStatus {
    #[allow(dead_code)] // May be used in later UI slices
    fn label(&self) -> &'static str {
        match self {
            AppStatus::Stopped => "IDLE",
            AppStatus::Running => "RUNNING",
            AppStatus::Error => "ERROR",
        }
    }

    fn border_type(&self) -> BorderType {
        match self {
            AppStatus::Stopped => BorderType::Rounded,
            AppStatus::Running | AppStatus::Error => BorderType::Double,
        }
    }

    /// Returns the color for this status, with pulsing effect for Error state.
    /// The pulse alternates between red and dark red at ~2Hz (every 15 frames at 30fps).
    fn pulsing_color(&self, frame_count: u64) -> Color {
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

/// Formats a duration as M:SS (under 1 hour) or H:MM:SS (1+ hours).
fn format_elapsed(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

enum OutputMessage {
    Line(String),
}

/// Tracks accumulated state for a content block being streamed.
#[derive(Debug, Default)]
struct ContentBlockState {
    /// For text blocks: accumulated text content.
    text: String,
    /// For tool_use blocks: the tool name.
    tool_name: Option<String>,
    /// For tool_use blocks: accumulated JSON input string.
    input_json: String,
}

/// Maximum length for truncated tool input display.
const TOOL_INPUT_MAX_LEN: usize = 60;

/// Log level options for the dropdown.
const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Which field is focused in the config modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigModalField {
    ClaudePath,
    ClaudeArgs,
    PromptFile,
    SpecsDirectory,
    LogLevel,
    SaveButton,
    CancelButton,
}

impl ConfigModalField {
    fn next(self) -> Self {
        match self {
            Self::ClaudePath => Self::ClaudeArgs,
            Self::ClaudeArgs => Self::PromptFile,
            Self::PromptFile => Self::SpecsDirectory,
            Self::SpecsDirectory => Self::LogLevel,
            Self::LogLevel => Self::SaveButton,
            Self::SaveButton => Self::CancelButton,
            Self::CancelButton => Self::ClaudePath,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::ClaudePath => Self::CancelButton,
            Self::ClaudeArgs => Self::ClaudePath,
            Self::PromptFile => Self::ClaudeArgs,
            Self::SpecsDirectory => Self::PromptFile,
            Self::LogLevel => Self::SpecsDirectory,
            Self::SaveButton => Self::LogLevel,
            Self::CancelButton => Self::SaveButton,
        }
    }
}

/// State for the config modal form.
#[derive(Debug, Clone)]
struct ConfigModalState {
    /// Current focused field.
    focus: ConfigModalField,
    /// Claude CLI path value.
    claude_path: String,
    /// Claude CLI args value.
    claude_args: String,
    /// Prompt file path value.
    prompt_file: String,
    /// Specs directory path value.
    specs_dir: String,
    /// Currently selected log level index in LOG_LEVELS.
    log_level_index: usize,
    /// Cursor position within the focused text field.
    cursor_pos: usize,
    /// Error message to display (e.g., save failed).
    error: Option<String>,
}

impl ConfigModalState {
    /// Create a new modal state initialized from the current config.
    fn from_config(config: &Config) -> Self {
        let log_level_index = LOG_LEVELS
            .iter()
            .position(|&l| l == config.logging.level)
            .unwrap_or(2); // Default to "info" (index 2)

        Self {
            focus: ConfigModalField::ClaudePath,
            claude_path: config.claude.path.clone(),
            claude_args: config.claude.args.clone(),
            prompt_file: config.paths.prompt.clone(),
            specs_dir: config.paths.specs.clone(),
            log_level_index,
            cursor_pos: config.claude.path.len(),
            error: None,
        }
    }

    /// Get a reference to the currently focused text field's value.
    fn current_field_value(&self) -> Option<&String> {
        match self.focus {
            ConfigModalField::ClaudePath => Some(&self.claude_path),
            ConfigModalField::ClaudeArgs => Some(&self.claude_args),
            ConfigModalField::PromptFile => Some(&self.prompt_file),
            ConfigModalField::SpecsDirectory => Some(&self.specs_dir),
            _ => None,
        }
    }

    /// Move focus to the next field, resetting cursor position.
    fn focus_next(&mut self) {
        self.focus = self.focus.next();
        self.update_cursor_for_new_focus();
    }

    /// Move focus to the previous field, resetting cursor position.
    fn focus_prev(&mut self) {
        self.focus = self.focus.prev();
        self.update_cursor_for_new_focus();
    }

    /// Update cursor position when focus changes to a new field.
    fn update_cursor_for_new_focus(&mut self) {
        if let Some(value) = self.current_field_value() {
            self.cursor_pos = value.len();
        } else {
            self.cursor_pos = 0;
        }
    }

    /// Insert a character at the current cursor position.
    fn insert_char(&mut self, c: char) {
        let cursor = self.cursor_pos;
        match self.focus {
            ConfigModalField::ClaudePath => {
                if cursor >= self.claude_path.len() {
                    self.claude_path.push(c);
                } else {
                    self.claude_path.insert(cursor, c);
                }
                self.cursor_pos += 1;
            }
            ConfigModalField::ClaudeArgs => {
                if cursor >= self.claude_args.len() {
                    self.claude_args.push(c);
                } else {
                    self.claude_args.insert(cursor, c);
                }
                self.cursor_pos += 1;
            }
            ConfigModalField::PromptFile => {
                if cursor >= self.prompt_file.len() {
                    self.prompt_file.push(c);
                } else {
                    self.prompt_file.insert(cursor, c);
                }
                self.cursor_pos += 1;
            }
            ConfigModalField::SpecsDirectory => {
                if cursor >= self.specs_dir.len() {
                    self.specs_dir.push(c);
                } else {
                    self.specs_dir.insert(cursor, c);
                }
                self.cursor_pos += 1;
            }
            _ => {}
        }
    }

    /// Delete the character before the cursor (backspace).
    fn delete_char_before(&mut self) {
        if self.cursor_pos == 0 {
            return;
        }
        let cursor = self.cursor_pos;
        match self.focus {
            ConfigModalField::ClaudePath => {
                self.claude_path.remove(cursor - 1);
                self.cursor_pos -= 1;
            }
            ConfigModalField::ClaudeArgs => {
                self.claude_args.remove(cursor - 1);
                self.cursor_pos -= 1;
            }
            ConfigModalField::PromptFile => {
                self.prompt_file.remove(cursor - 1);
                self.cursor_pos -= 1;
            }
            ConfigModalField::SpecsDirectory => {
                self.specs_dir.remove(cursor - 1);
                self.cursor_pos -= 1;
            }
            _ => {}
        }
    }

    /// Delete the character at the cursor position (delete key).
    fn delete_char_at(&mut self) {
        let cursor = self.cursor_pos;
        match self.focus {
            ConfigModalField::ClaudePath if cursor < self.claude_path.len() => {
                self.claude_path.remove(cursor);
            }
            ConfigModalField::ClaudeArgs if cursor < self.claude_args.len() => {
                self.claude_args.remove(cursor);
            }
            ConfigModalField::PromptFile if cursor < self.prompt_file.len() => {
                self.prompt_file.remove(cursor);
            }
            ConfigModalField::SpecsDirectory if cursor < self.specs_dir.len() => {
                self.specs_dir.remove(cursor);
            }
            _ => {}
        }
    }

    /// Move cursor left within the current field.
    fn cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
        }
    }

    /// Move cursor right within the current field.
    fn cursor_right(&mut self) {
        if let Some(value) = self.current_field_value()
            && self.cursor_pos < value.len()
        {
            self.cursor_pos += 1;
        }
    }

    /// Move to beginning of current field.
    fn cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    /// Move to end of current field.
    fn cursor_end(&mut self) {
        if let Some(value) = self.current_field_value() {
            self.cursor_pos = value.len();
        }
    }

    /// Cycle log level selection up.
    fn log_level_prev(&mut self) {
        if self.log_level_index > 0 {
            self.log_level_index -= 1;
        } else {
            self.log_level_index = LOG_LEVELS.len() - 1;
        }
    }

    /// Cycle log level selection down.
    fn log_level_next(&mut self) {
        if self.log_level_index < LOG_LEVELS.len() - 1 {
            self.log_level_index += 1;
        } else {
            self.log_level_index = 0;
        }
    }

    /// Get the currently selected log level.
    fn selected_log_level(&self) -> &'static str {
        LOG_LEVELS[self.log_level_index]
    }

    /// Build a Config from the current form values.
    fn to_config(&self) -> Config {
        Config {
            claude: crate::config::ClaudeConfig {
                path: self.claude_path.clone(),
                args: self.claude_args.clone(),
            },
            paths: crate::config::PathsConfig {
                prompt: self.prompt_file.clone(),
                specs: self.specs_dir.clone(),
            },
            logging: crate::config::LoggingConfig {
                level: self.selected_log_level().to_string(),
            },
        }
    }
}

/// Formats a tool invocation for display.
///
/// Returns a formatted string like `[Tool: Bash] git status` for known tools,
/// or a truncated JSON representation for unknown tools.
fn format_tool_summary(tool_name: &str, input_json: &str) -> String {
    let prefix = format!("[Tool: {}]", tool_name);

    // Try to parse the accumulated JSON
    let input: serde_json::Value = match serde_json::from_str(input_json) {
        Ok(v) => v,
        Err(_) => return format!("{} (input parsing failed)", prefix),
    };

    // Format based on tool type
    let summary = match tool_name {
        "Bash" => format_bash_tool(&input),
        "Read" => format_read_tool(&input),
        "Edit" => format_edit_tool(&input),
        "Write" => format_write_tool(&input),
        "Grep" => format_grep_tool(&input),
        "Glob" => format_glob_tool(&input),
        _ => format_unknown_tool(&input),
    };

    format!("{} {}", prefix, summary)
}

fn format_bash_tool(input: &serde_json::Value) -> String {
    input
        .get("command")
        .and_then(|v| v.as_str())
        .map(|cmd| truncate_str(cmd, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no command)".to_string())
}

fn format_read_tool(input: &serde_json::Value) -> String {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no path)".to_string())
}

fn format_edit_tool(input: &serde_json::Value) -> String {
    let path = input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("(no path)");

    // Try to show context about what's being edited
    if let Some(old_str) = input.get("old_string").and_then(|v| v.as_str()) {
        let preview = truncate_str(old_str, 30);
        format!("{} \"{}\"", truncate_str(path, 40), preview)
    } else {
        truncate_str(path, TOOL_INPUT_MAX_LEN)
    }
}

fn format_write_tool(input: &serde_json::Value) -> String {
    input
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no path)".to_string())
}

fn format_grep_tool(input: &serde_json::Value) -> String {
    input
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no pattern)".to_string())
}

fn format_glob_tool(input: &serde_json::Value) -> String {
    input
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(|p| truncate_str(p, TOOL_INPUT_MAX_LEN))
        .unwrap_or_else(|| "(no pattern)".to_string())
}

fn format_unknown_tool(input: &serde_json::Value) -> String {
    let json_str = input.to_string();
    truncate_str(&json_str, TOOL_INPUT_MAX_LEN)
}

/// Formats a number with thousands separators (e.g., 7371 -> "7,371").
fn format_with_thousands(n: u64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

/// Formats the usage summary from a Result event.
///
/// Returns a two-line string: a separator line followed by the summary.
/// Example: "───────────────────────────────────\nCost: $0.05 | Tokens: 7,371 in / 9 out | Duration: 2.3s"
fn format_usage_summary(result: &crate::events::ResultEvent) -> String {
    let mut parts = Vec::new();

    // Format cost
    if let Some(cost) = result.total_cost_usd {
        parts.push(format!("Cost: ${:.2}", cost));
    }

    // Format tokens
    if let Some(usage) = &result.usage {
        let input = usage
            .input_tokens
            .map(format_with_thousands)
            .unwrap_or_else(|| "?".to_string());
        let output = usage
            .output_tokens
            .map(format_with_thousands)
            .unwrap_or_else(|| "?".to_string());
        parts.push(format!("Tokens: {} in / {} out", input, output));
    }

    // Format duration
    if let Some(duration_ms) = result.duration_ms {
        let seconds = duration_ms as f64 / 1000.0;
        parts.push(format!("Duration: {:.1}s", seconds));
    }

    // Build the summary line
    let separator = "─".repeat(35);
    let summary = parts.join(" | ");
    format!("{}\n{}", separator, summary)
}

/// Truncates a string to the given maximum length, appending "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    // Replace newlines with spaces for single-line display
    let single_line: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();

    if single_line.len() <= max_len {
        single_line
    } else {
        format!("{}...", &single_line[..max_len.saturating_sub(3)])
    }
}

struct App {
    status: AppStatus,
    output_lines: Vec<String>,
    scroll_offset: u16,
    is_auto_following: bool,
    show_already_running_popup: bool,
    show_config_modal: bool,
    main_pane_height: u16,
    main_pane_width: u16,
    child_process: Option<Child>,
    output_receiver: Option<Receiver<OutputMessage>>,
    /// Tracks content blocks by index during streaming.
    content_blocks: HashMap<usize, ContentBlockState>,
    /// Current line being accumulated (text that hasn't hit a newline yet).
    current_line: String,
    /// Session ID for this Ralph invocation (always populated).
    session_id: String,
    /// Loop counter for logging, incremented each time start_command() is called.
    loop_count: u64,
    /// Directory where logs are written.
    #[allow(dead_code)] // Will be used in later UI slices
    log_directory: Option<PathBuf>,
    /// Error that occurred during logging initialization.
    #[allow(dead_code)] // Will be used in later UI slices
    logging_error: Option<String>,
    /// Loaded configuration.
    config: Config,
    /// Path to the configuration file.
    config_path: PathBuf,
    /// Status of config loading.
    #[allow(dead_code)] // Will be used in later UI slices
    config_load_status: ConfigLoadStatus,
    /// Last known mtime of the config file for change detection.
    config_mtime: Option<SystemTime>,
    /// Last time we polled for config changes.
    last_config_poll: Instant,
    /// When config was last successfully reloaded (for "Reloaded" indicator fade).
    config_reloaded_at: Option<Instant>,
    /// Error message if config reload failed (invalid TOML, etc.).
    config_reload_error: Option<String>,
    /// Name of the currently active spec (from specs/README.md).
    current_spec: Option<String>,
    /// Last time we polled for the current spec.
    last_spec_poll: Instant,
    /// Transient error message to display in the status panel (e.g., editor spawn failure).
    // TODO: Remove in Slice 4 after config form is fully implemented (was for editor errors)
    #[allow(dead_code)]
    status_error: Option<String>,
    /// Handle for dynamically reloading the log level.
    log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
    /// Current log level from config (to detect changes on reload).
    current_log_level: String,
    /// When the current run started (for elapsed time display).
    run_start_time: Option<Instant>,
    /// Frame counter for animations (incremented each render cycle).
    frame_count: u64,
    /// State for the config modal form (when open).
    config_modal_state: Option<ConfigModalState>,
}

impl App {
    fn new(
        session_id: String,
        log_directory: Option<PathBuf>,
        logging_error: Option<String>,
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
            logging_error,
            config: loaded_config.config,
            config_path: loaded_config.config_path.clone(),
            config_load_status: loaded_config.status,
            config_mtime: get_file_mtime(&loaded_config.config_path),
            // Initialize to "long ago" so we poll immediately on start
            last_config_poll: Instant::now() - Duration::from_secs(10),
            config_reloaded_at: None,
            config_reload_error: None,
            current_spec: None,
            // Initialize to "long ago" so we poll immediately on start
            last_spec_poll: Instant::now() - Duration::from_secs(10),
            status_error: None,
            log_level_handle,
            current_log_level,
            run_start_time: None,
            frame_count: 0,
            config_modal_state: None,
        }
    }

    fn visual_line_count(&self) -> u16 {
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

    fn max_scroll(&self) -> u16 {
        self.visual_line_count()
            .saturating_sub(self.main_pane_height)
    }

    fn scroll_up(&mut self, amount: u16) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(amount);
            self.is_auto_following = false;
        }
    }

    fn scroll_down(&mut self, amount: u16) {
        let max = self.max_scroll();
        self.scroll_offset = (self.scroll_offset + amount).min(max);
        if self.scroll_offset >= max {
            self.is_auto_following = true;
        }
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll();
        self.is_auto_following = true;
    }

    fn add_line(&mut self, line: String) {
        self.output_lines.push(line);
        if self.is_auto_following {
            self.scroll_to_bottom();
        }
    }

    /// Appends text to the current line, flushing complete lines to output.
    fn append_text(&mut self, text: &str) {
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
    fn flush_current_line(&mut self) {
        if !self.current_line.is_empty() {
            let line = std::mem::take(&mut self.current_line);
            self.add_line(line);
        }
    }

    /// Parses and processes a single NDJSON line.
    fn process_line(&mut self, line: &str) {
        // Skip empty lines
        if line.trim().is_empty() {
            return;
        }

        // Log raw JSON at TRACE level for protocol debugging
        trace!(json = line, "raw_json_line");

        // Handle stderr lines (pass through as-is)
        if line.starts_with("[stderr]") {
            self.add_line(line.to_string());
            return;
        }

        // Try to parse as JSON
        match serde_json::from_str::<ClaudeEvent>(line) {
            Ok(event) => self.process_event(event),
            Err(e) => {
                // Check if this is an unknown event type by trying to parse as generic JSON
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                        warn!(event_type, "Unknown event type, skipping");
                    } else {
                        warn!(?e, "Failed to parse JSON line (no type field)");
                    }
                } else {
                    warn!(?e, "Malformed JSON line, skipping");
                }
            }
        }
    }

    /// Processes a parsed Claude event.
    fn process_event(&mut self, event: ClaudeEvent) {
        match event {
            ClaudeEvent::Ping => {
                // Silently ignore ping events
                debug!("Received ping");
            }
            ClaudeEvent::System(sys) => {
                debug!(?sys, "System event");
            }
            ClaudeEvent::Assistant(asst) => {
                debug!(?asst, "Assistant event");
            }
            ClaudeEvent::User(_) => {
                // Skip user events (tool results, etc.)
                debug!("User event (skipped)");
            }
            ClaudeEvent::StreamEvent { event: inner } => {
                // Unwrap and process the inner streaming event
                self.process_stream_event(inner);
            }
            ClaudeEvent::Result(result) => {
                debug!(?result, "Result event");
                // Display usage summary
                let summary = format_usage_summary(&result);
                for line in summary.lines() {
                    self.add_line(line.to_string());
                }
            }
        }
    }

    /// Processes inner streaming events (unwrapped from stream_event).
    fn process_stream_event(&mut self, event: StreamInnerEvent) {
        match event {
            StreamInnerEvent::MessageStart(msg) => {
                debug!(?msg, "Message start");
                // Clear content blocks for new message
                self.content_blocks.clear();
            }
            StreamInnerEvent::ContentBlockStart(block_start) => {
                let index = block_start.index;
                let mut state = ContentBlockState::default();

                match block_start.content_block {
                    ContentBlock::Text { text } => {
                        state.text = text;
                    }
                    ContentBlock::ToolUse { name, .. } => {
                        state.tool_name = Some(name);
                    }
                }

                self.content_blocks.insert(index, state);
                debug!(index, "Content block started");
            }
            StreamInnerEvent::ContentBlockDelta(delta_event) => {
                let index = delta_event.index;
                let state = self.content_blocks.entry(index).or_default();

                match delta_event.delta {
                    Delta::TextDelta { text } => {
                        state.text.push_str(&text);
                        // Display text immediately as it streams
                        self.append_text(&text);
                    }
                    Delta::InputJsonDelta { partial_json } => {
                        state.input_json.push_str(&partial_json);
                    }
                }
            }
            StreamInnerEvent::ContentBlockStop(stop) => {
                debug!(index = stop.index, "Content block stopped");
                // Always flush any pending text first to maintain order
                self.flush_current_line();
                // Then process tool_use blocks
                if let Some(state) = self.content_blocks.get(&stop.index)
                    && let Some(tool_name) = &state.tool_name
                {
                    let summary = format_tool_summary(tool_name, &state.input_json);
                    self.add_line(summary);
                }
            }
            StreamInnerEvent::MessageDelta(delta) => {
                debug!(?delta, "Message delta");
            }
            StreamInnerEvent::MessageStop => {
                debug!("Message stopped");
                // Flush any remaining text
                self.flush_current_line();
            }
        }
    }

    fn start_command(&mut self) -> Result<()> {
        if self.status == AppStatus::Running {
            self.show_already_running_popup = true;
            return Ok(());
        }

        // Check if prompt file exists (using config path)
        let prompt_path = self.config.prompt_path();
        if !prompt_path.exists() {
            self.status = AppStatus::Error;
            self.add_line(format!("Error: {} not found", prompt_path.display()));
            return Ok(());
        }

        // Increment loop counter and log loop_start
        self.loop_count += 1;
        info!(loop_number = self.loop_count, "loop_start");

        // Add divider if not first run
        if !self.output_lines.is_empty() {
            self.add_line("─".repeat(40));
        }

        // Reset streaming state for new command
        self.content_blocks.clear();
        self.current_line.clear();

        // Spawn the command using shell to handle the pipe
        // Use config values for claude path, args, and prompt path
        let claude_path = self.config.claude_path();
        let command = format!(
            "cat {} | {} {}",
            prompt_path.display(),
            claude_path.display(),
            self.config.claude.args
        );
        let child = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(mut child) => {
                // Log command_spawned with PID
                debug!(pid = child.id(), "command_spawned");

                let (tx, rx) = mpsc::channel();

                // Read stdout in a background thread
                if let Some(stdout) = child.stdout.take() {
                    let tx_stdout = tx.clone();
                    thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines().map_while(Result::ok) {
                            if tx_stdout.send(OutputMessage::Line(line)).is_err() {
                                break;
                            }
                        }
                    });
                }

                // Read stderr in a background thread
                if let Some(stderr) = child.stderr.take() {
                    let tx_stderr = tx.clone();
                    thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines().map_while(Result::ok) {
                            if tx_stderr
                                .send(OutputMessage::Line(format!("[stderr] {}", line)))
                                .is_err()
                            {
                                break;
                            }
                        }
                    });
                }

                self.child_process = Some(child);
                self.output_receiver = Some(rx);
                self.status = AppStatus::Running;
                self.run_start_time = Some(Instant::now());
            }
            Err(e) => {
                self.status = AppStatus::Error;
                self.add_line(format!("Error starting command: {}", e));
            }
        }

        Ok(())
    }

    fn kill_child(&mut self) {
        if let Some(mut child) = self.child_process.take() {
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            info!(pid, "process_killed");
        }
        self.output_receiver = None;
    }

    fn poll_output(&mut self) {
        // First, collect all pending messages
        let mut messages = Vec::new();
        let mut channel_disconnected = false;

        if let Some(rx) = &self.output_receiver {
            loop {
                match rx.try_recv() {
                    Ok(msg) => messages.push(msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        channel_disconnected = true;
                        break;
                    }
                }
            }
        }

        // Process collected messages
        for msg in messages {
            let OutputMessage::Line(line) = msg;
            self.process_line(&line);
        }

        // Check if the channel disconnected (all senders dropped = readers finished)
        if channel_disconnected {
            debug!("channel_disconnected");

            // Try to get exit status from child process
            let exit_status: Option<String> = if let Some(mut child) = self.child_process.take() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if let Some(code) = status.code() {
                            if code != 0 {
                                warn!(exit_code = code, "process_exit_nonzero");
                                self.add_line(format!("[Process exited with code {}]", code));
                            }
                            Some(format!("exit_code={}", code))
                        } else {
                            // Process was terminated by signal (Unix)
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                if let Some(signal) = status.signal() {
                                    info!(signal, "process_killed_by_signal");
                                    Some(format!("signal={}", signal))
                                } else {
                                    Some("unknown".to_string())
                                }
                            }
                            #[cfg(not(unix))]
                            {
                                Some("unknown".to_string())
                            }
                        }
                    }
                    Ok(None) => {
                        // Still running, put it back (shouldn't happen if channel disconnected)
                        self.child_process = Some(child);
                        return;
                    }
                    Err(_) => None,
                }
            } else {
                None
            };

            // Log loop_end with exit status
            let status_str = exit_status.unwrap_or_else(|| "unknown".to_string());
            info!(
                loop_number = self.loop_count,
                exit_status = %status_str,
                "loop_end"
            );

            self.status = AppStatus::Stopped;
            self.output_receiver = None;
            // Clear current spec when stopped
            self.current_spec = None;
            // Clear elapsed timer when stopped
            self.run_start_time = None;
        }
    }

    fn poll_spec(&mut self) {
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

    fn poll_config(&mut self) {
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
}

/// Get the editor command to use for editing config.
/// Checks $VISUAL first, then $EDITOR, falls back to "vi".
// TODO: Remove in Slice 4 after config form is fully implemented
#[allow(dead_code)]
fn get_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
}

/// Result of attempting to open the config in an editor.
// TODO: Remove in Slice 4 after config form is fully implemented
#[allow(dead_code)]
#[derive(Debug)]
enum EditConfigResult {
    /// Successfully opened and closed the editor
    Success,
    /// Could not determine config path
    NoConfigPath,
    /// Editor failed to spawn
    SpawnFailed(String),
    /// Editor exited with non-zero status
    EditorError(i32),
}

/// Opens the config file in the user's preferred editor.
/// Suspends the terminal, runs the editor, then restores the terminal.
// TODO: Remove in Slice 4 after config form is fully implemented
#[allow(dead_code)]
fn open_config_in_editor(terminal: &mut DefaultTerminal) -> EditConfigResult {
    let config_path = match ensure_config_exists() {
        Some(path) => path,
        None => return EditConfigResult::NoConfigPath,
    };

    let editor = get_editor();
    debug!(editor = %editor, config_path = %config_path.display(), "opening_config_editor");

    // Suspend the TUI
    if let Err(e) = disable_raw_mode() {
        warn!(error = %e, "failed to disable raw mode");
    }
    if let Err(e) = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture) {
        warn!(error = %e, "failed to leave alternate screen");
    }

    // Spawn the editor and wait for it to complete
    let result = Command::new(&editor).arg(&config_path).status();

    // Restore the TUI
    if let Err(e) = enable_raw_mode() {
        warn!(error = %e, "failed to re-enable raw mode");
    }
    if let Err(e) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture) {
        warn!(error = %e, "failed to re-enter alternate screen");
    }
    // Force a full redraw
    if let Err(e) = terminal.clear() {
        warn!(error = %e, "failed to clear terminal after editor");
    }

    match result {
        Ok(status) => {
            if status.success() {
                info!(editor = %editor, "config_editor_closed");
                EditConfigResult::Success
            } else {
                let code = status.code().unwrap_or(-1);
                warn!(editor = %editor, exit_code = code, "config_editor_exited_with_error");
                EditConfigResult::EditorError(code)
            }
        }
        Err(e) => {
            warn!(editor = %editor, error = %e, "config_editor_spawn_failed");
            EditConfigResult::SpawnFailed(format!("{}: {}", editor, e))
        }
    }
}

/// Detect the currently in-progress spec from specs/README.md
fn detect_current_spec(specs_dir: &std::path::Path) -> Option<String> {
    let readme_path = specs_dir.join("README.md");

    let contents = match std::fs::read_to_string(&readme_path) {
        Ok(c) => c,
        Err(e) => {
            debug!(path = ?readme_path, error = %e, "spec_readme_read_failed");
            return None;
        }
    };

    let mut found_specs: Vec<String> = Vec::new();

    for line in contents.lines() {
        // Look for table rows with "In Progress" status
        // Pattern: | [spec-name](...)  | In Progress | ... |
        if line.contains("| In Progress |") || line.contains("| In Progress|") {
            // Extract spec name from the link: | [spec-name](spec-name.md) |
            if let Some(start) = line.find("| [") {
                let after_bracket = &line[start + 3..];
                if let Some(end) = after_bracket.find(']') {
                    let spec_name = after_bracket[..end].to_string();
                    found_specs.push(spec_name);
                }
            }
        }
    }

    if found_specs.len() > 1 {
        warn!(
            specs = ?found_specs,
            "multiple_specs_in_progress"
        );
    }

    found_specs.into_iter().next()
}

fn main() -> Result<()> {
    use std::time::Instant;

    let start_time = Instant::now();

    // Generate session ID first (always available, even if logging fails)
    let session_id = logging::new_session_id();

    // Load configuration first (needed for log level)
    let loaded_config = config::load_config();

    // Initialize logging with config log level
    let (log_directory, logging_error, _guard, reload_handle) =
        match logging::init(session_id.clone(), &loaded_config.config.logging.level) {
            Ok(ctx) => {
                // Clean up old log files after logging is initialized
                logging::cleanup_old_logs(&ctx.log_directory);
                (
                    Some(ctx.log_directory),
                    None,
                    Some(ctx._guard),
                    Some(ctx.reload_handle),
                )
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize logging: {}", e);
                (None, Some(e.message), None, None)
            }
        };

    // Log config status after logging is initialized
    debug!(
        config_path = %loaded_config.config_path.display(),
        status = ?loaded_config.status,
        log_level = %loaded_config.config.logging.level,
        "config_loaded"
    );

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;

    let result = run_app(
        terminal,
        session_id.clone(),
        log_directory,
        logging_error,
        loaded_config,
        reload_handle,
    );

    // Restore terminal
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    // Log session end
    let duration = start_time.elapsed();
    info!(
        session_id = %session_id,
        duration_secs = duration.as_secs_f64(),
        "session_end"
    );

    result
}

/// Handle keyboard input for the config modal.
fn handle_config_modal_input(app: &mut App, key_code: KeyCode, modifiers: KeyModifiers) {
    let Some(state) = &mut app.config_modal_state else {
        return;
    };

    // Clear any previous error when user takes action
    if state.error.is_some() && key_code != KeyCode::Esc {
        state.error = None;
    }

    match key_code {
        // Navigation between fields
        KeyCode::Tab => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                state.focus_prev();
            } else {
                state.focus_next();
            }
        }
        KeyCode::BackTab => {
            state.focus_prev();
        }

        // Cancel / close
        KeyCode::Esc => {
            app.show_config_modal = false;
            app.config_modal_state = None;
        }

        // Enter - context-dependent
        KeyCode::Enter => match state.focus {
            ConfigModalField::SaveButton => {
                // Save config to file
                let new_config = state.to_config();
                match save_config(&new_config, &app.config_path) {
                    Ok(()) => {
                        // Update app config and close modal
                        app.config = new_config;
                        // Update mtime so we don't trigger a reload
                        app.config_mtime = get_file_mtime(&app.config_path);
                        app.show_config_modal = false;
                        app.config_modal_state = None;
                        debug!("Config saved successfully via modal");
                    }
                    Err(e) => {
                        // Show error in modal, don't close
                        state.error = Some(e);
                    }
                }
            }
            ConfigModalField::CancelButton => {
                app.show_config_modal = false;
                app.config_modal_state = None;
            }
            _ => {
                // Enter in text fields moves to next field
                state.focus_next();
            }
        },

        // Text input handling
        KeyCode::Char(c) => {
            if matches!(
                state.focus,
                ConfigModalField::ClaudePath
                    | ConfigModalField::ClaudeArgs
                    | ConfigModalField::PromptFile
                    | ConfigModalField::SpecsDirectory
            ) {
                state.insert_char(c);
            }
        }

        KeyCode::Backspace => {
            state.delete_char_before();
        }

        KeyCode::Delete => {
            state.delete_char_at();
        }

        // Cursor movement within text fields
        KeyCode::Left => {
            if matches!(state.focus, ConfigModalField::LogLevel) {
                state.log_level_prev();
            } else {
                state.cursor_left();
            }
        }

        KeyCode::Right => {
            if matches!(state.focus, ConfigModalField::LogLevel) {
                state.log_level_next();
            } else {
                state.cursor_right();
            }
        }

        KeyCode::Home => {
            state.cursor_home();
        }

        KeyCode::End => {
            state.cursor_end();
        }

        // Up/Down for log level dropdown and button navigation
        KeyCode::Up => match state.focus {
            ConfigModalField::LogLevel => state.log_level_prev(),
            ConfigModalField::SaveButton | ConfigModalField::CancelButton => state.focus_prev(),
            _ => {}
        },

        KeyCode::Down => match state.focus {
            ConfigModalField::LogLevel => state.log_level_next(),
            ConfigModalField::SaveButton | ConfigModalField::CancelButton => state.focus_next(),
            _ => {}
        },

        _ => {}
    }
}

fn run_app(
    mut terminal: DefaultTerminal,
    session_id: String,
    log_directory: Option<PathBuf>,
    logging_error: Option<String>,
    loaded_config: LoadedConfig,
    log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
) -> Result<()> {
    let mut app = App::new(
        session_id,
        log_directory,
        logging_error,
        loaded_config,
        log_level_handle,
    );

    loop {
        // Poll for output from child process
        app.poll_output();

        // Poll for current spec (throttled to every 2 seconds)
        app.poll_spec();

        // Poll for config file changes (throttled to every 2 seconds)
        app.poll_config();

        // Draw UI
        terminal.draw(|f| draw_ui(f, &mut app))?;

        // Poll for events with a short timeout to allow process output polling
        if crossterm::event::poll(Duration::from_millis(50))? {
            let event = crossterm::event::read()?;

            // Handle popup dismissal first
            if app.show_already_running_popup {
                if let Event::Key(key) = event
                    && (key.code == KeyCode::Enter || key.code == KeyCode::Esc)
                {
                    app.show_already_running_popup = false;
                }
                continue;
            }

            // Handle config modal input
            if app.show_config_modal {
                if let Event::Key(key) = event {
                    handle_config_modal_input(&mut app, key.code, key.modifiers);
                }
                continue;
            }

            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => {
                        app.kill_child();
                        return Ok(());
                    }
                    KeyCode::Char('s') => {
                        if app.status != AppStatus::Error {
                            app.start_command()?;
                        }
                    }
                    KeyCode::Char('c') => {
                        // Only allow config modal when not running
                        if app.status != AppStatus::Running {
                            app.show_config_modal = true;
                            app.config_modal_state =
                                Some(ConfigModalState::from_config(&app.config));
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.scroll_up(1);
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.scroll_down(1);
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        app.scroll_up(half_page);
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        app.scroll_down(half_page);
                    }
                    KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.scroll_up(app.main_pane_height);
                    }
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.scroll_down(app.main_pane_height);
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        app.scroll_up(3);
                    }
                    MouseEventKind::ScrollDown => {
                        app.scroll_down(3);
                    }
                    _ => {}
                },
                Event::Resize(_, _) => {
                    // Terminal resized, will be handled in next draw
                }
                _ => {}
            }
        }
    }
}

fn draw_ui(f: &mut Frame, app: &mut App) {
    // Increment frame counter for animations
    app.frame_count = app.frame_count.wrapping_add(1);

    // Two-panel layout: output (flexible) + command (fixed height 3)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // Output panel (flexible)
            Constraint::Length(3), // Command panel (border + 1 content row + border)
        ])
        .split(f.area());

    // Update main pane dimensions for scroll calculations
    app.main_pane_height = chunks[0].height.saturating_sub(2); // Account for borders
    app.main_pane_width = chunks[0].width;

    // Output panel with session ID as title
    let mut content: Vec<Line> = app.output_lines.iter().map(Line::raw).collect();
    if !app.current_line.is_empty() {
        content.push(Line::raw(&app.current_line));
    }

    let mut output_block = Block::default()
        .borders(Borders::ALL)
        .border_type(app.status.border_type())
        .border_style(Style::default().fg(app.status.pulsing_color(app.frame_count)))
        .title(Line::from(format!(" {} ", app.session_id)).left_aligned());

    if let Some(spec) = &app.current_spec {
        output_block = output_block.title(Line::from(format!(" {} ", spec)).right_aligned());
    }

    let output_panel = Paragraph::new(content)
        .block(output_block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll_offset, 0));

    f.render_widget(output_panel, chunks[0]);

    // Scrollbar - only visible when content exceeds viewport
    let visual_lines = app.visual_line_count();
    if visual_lines > app.main_pane_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));

        let mut scrollbar_state = ScrollbarState::default()
            .content_length(visual_lines as usize)
            .position(app.scroll_offset as usize)
            .viewport_content_length(app.main_pane_height as usize);

        f.render_stateful_widget(scrollbar, chunks[0], &mut scrollbar_state);
    }

    // Command panel with keyboard shortcuts (left) and status indicator (right)
    let shortcuts = match app.status {
        AppStatus::Error => "[q] Quit",
        AppStatus::Stopped => "[s] Start  [c] Config  [q] Quit",
        AppStatus::Running => "[s] Start  [q] Quit",
    };

    // Status indicator: colored dot + text (elapsed time when running)
    let status_dot = "● ";
    let status_text = match app.status {
        AppStatus::Stopped => "IDLE".to_string(),
        AppStatus::Running => {
            if let Some(start_time) = app.run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "RUNNING".to_string()
            }
        }
        AppStatus::Error => {
            // Show frozen elapsed time if available, otherwise just ERROR
            if let Some(start_time) = app.run_start_time {
                format_elapsed(start_time.elapsed())
            } else {
                "ERROR".to_string()
            }
        }
    };
    let status_color = app.status.pulsing_color(app.frame_count);

    // Calculate spacing to right-align the status indicator
    // Total width minus borders (2), shortcuts length, status indicator length
    let inner_width = chunks[1].width.saturating_sub(2) as usize;
    let status_len = status_dot.len() + status_text.len();
    let shortcuts_len = shortcuts.len();
    let spacing = inner_width.saturating_sub(shortcuts_len + status_len);

    let command_line = Line::from(vec![
        Span::styled(shortcuts, Style::default().fg(Color::DarkGray)),
        Span::raw(" ".repeat(spacing)),
        Span::styled(status_dot, Style::default().fg(status_color)),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]);

    let command_panel = Paragraph::new(command_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(app.status.border_type())
            .border_style(Style::default().fg(app.status.pulsing_color(app.frame_count))),
    );

    f.render_widget(command_panel, chunks[1]);

    // Popup dialog if needed
    if app.show_already_running_popup {
        let popup_area = centered_rect(40, 5, f.area());
        f.render_widget(Clear, popup_area);
        let popup = Paragraph::new("Command already running")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Notice")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default());
        f.render_widget(popup, popup_area);
    }

    // Config modal
    if app.show_config_modal {
        draw_config_modal(f, app);
    }
}

fn draw_config_modal(f: &mut Frame, app: &App) {
    let modal_width = 70;
    let modal_height = 20;
    let modal_area = centered_rect(modal_width, modal_height, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, modal_area);

    // Get form state (fall back to read-only view if no state)
    let state = app.config_modal_state.as_ref();

    // Build the modal content
    let config_path_display = app.config_path.display().to_string();
    let log_dir_display = app
        .log_directory
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not configured)".to_string());

    let separator = "─".repeat(modal_width.saturating_sub(4) as usize);
    let field_width = 40;

    // Helper to render a text input field - returns owned Spans
    let render_field = |value: &str, focused: bool, cursor_pos: usize| -> Vec<Span<'static>> {
        let display_value: String = if value.len() > field_width {
            // Show the portion around the cursor
            let start = cursor_pos.saturating_sub(field_width / 2);
            let end = (start + field_width).min(value.len());
            let start = end.saturating_sub(field_width);
            value[start..end].to_string()
        } else {
            value.to_string()
        };

        // Calculate cursor position within displayed text
        let visible_cursor = if value.len() > field_width {
            let start = cursor_pos.saturating_sub(field_width / 2);
            let end = (start + field_width).min(value.len());
            let start = end.saturating_sub(field_width);
            cursor_pos - start
        } else {
            cursor_pos
        };

        if focused {
            // Split at cursor for visual indication
            let char_indices: Vec<_> = display_value.char_indices().collect();
            let (before, cursor_char, rest) = if visible_cursor < char_indices.len() {
                let (idx, _) = char_indices[visible_cursor];
                let before = display_value[..idx].to_string();
                let cursor_char = display_value[idx..]
                    .chars()
                    .next()
                    .unwrap_or(' ')
                    .to_string();
                let rest_start = idx + cursor_char.len();
                let rest = if rest_start < display_value.len() {
                    display_value[rest_start..].to_string()
                } else {
                    String::new()
                };
                (before, cursor_char, rest)
            } else {
                (display_value.clone(), " ".to_string(), String::new())
            };

            vec![
                Span::styled(before, Style::default().fg(Color::White)),
                Span::styled(
                    cursor_char,
                    Style::default().fg(Color::Black).bg(Color::White),
                ),
                Span::styled(rest, Style::default().fg(Color::White)),
            ]
        } else {
            vec![Span::styled(
                display_value,
                Style::default().fg(Color::White),
            )]
        }
    };

    // Helper for label styling
    let label_style = Style::default().fg(Color::DarkGray);
    let focused_label_style = Style::default().fg(Color::Cyan);

    // Get values from state or config
    let (claude_path, claude_args, prompt_file, specs_dir, log_level, cursor_pos, focus) =
        if let Some(s) = state {
            (
                s.claude_path.as_str(),
                s.claude_args.as_str(),
                s.prompt_file.as_str(),
                s.specs_dir.as_str(),
                s.selected_log_level(),
                s.cursor_pos,
                Some(s.focus),
            )
        } else {
            (
                app.config.claude.path.as_str(),
                app.config.claude.args.as_str(),
                app.config.paths.prompt.as_str(),
                app.config.paths.specs.as_str(),
                app.config.logging.level.as_str(),
                0,
                None,
            )
        };

    // Build content lines
    let mut content = vec![
        Line::from(vec![
            Span::styled("  Config file: ", label_style),
            Span::raw(&config_path_display),
        ]),
        Line::from(vec![
            Span::styled("  Log directory: ", label_style),
            Span::raw(&log_dir_display),
        ]),
        Line::from(format!("  {separator}")),
    ];

    // Claude CLI path field
    let path_focused = focus == Some(ConfigModalField::ClaudePath);
    let path_label_style = if path_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut path_line = vec![Span::styled("  Claude CLI path: ", path_label_style)];
    path_line.extend(render_field(claude_path, path_focused, cursor_pos));
    content.push(Line::from(path_line));

    // Claude CLI args field
    let args_focused = focus == Some(ConfigModalField::ClaudeArgs);
    let args_label_style = if args_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut args_line = vec![Span::styled("  Claude CLI args: ", args_label_style)];
    args_line.extend(render_field(claude_args, args_focused, cursor_pos));
    content.push(Line::from(args_line));

    // Prompt file field
    let prompt_focused = focus == Some(ConfigModalField::PromptFile);
    let prompt_label_style = if prompt_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut prompt_line = vec![Span::styled("  Prompt file:     ", prompt_label_style)];
    prompt_line.extend(render_field(prompt_file, prompt_focused, cursor_pos));
    content.push(Line::from(prompt_line));

    // Specs directory field
    let specs_focused = focus == Some(ConfigModalField::SpecsDirectory);
    let specs_label_style = if specs_focused {
        focused_label_style
    } else {
        label_style
    };
    let mut specs_line = vec![Span::styled("  Specs directory: ", specs_label_style)];
    specs_line.extend(render_field(specs_dir, specs_focused, cursor_pos));
    content.push(Line::from(specs_line));

    // Log level dropdown
    let level_focused = focus == Some(ConfigModalField::LogLevel);
    let level_label_style = if level_focused {
        focused_label_style
    } else {
        label_style
    };
    let level_display = if level_focused {
        format!("< {} >", log_level)
    } else {
        log_level.to_string()
    };
    let level_value_style = if level_focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::White)
    };
    content.push(Line::from(vec![
        Span::styled("  Log level:       ", level_label_style),
        Span::styled(level_display, level_value_style),
    ]));

    content.push(Line::from(""));

    // Error message if any
    if let Some(s) = state {
        if let Some(error) = &s.error {
            content.push(Line::from(Span::styled(
                format!("  Error: {}", error),
                Style::default().fg(Color::Red),
            )));
        } else {
            content.push(Line::from(""));
        }
    } else {
        content.push(Line::from(""));
    }

    // Buttons
    let save_focused = focus == Some(ConfigModalField::SaveButton);
    let cancel_focused = focus == Some(ConfigModalField::CancelButton);

    let save_style = if save_focused {
        Style::default().fg(Color::Black).bg(Color::Cyan)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let cancel_style = if cancel_focused {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };

    content.push(Line::from(vec![
        Span::raw("                    "),
        Span::styled(" Save ", save_style),
        Span::raw("    "),
        Span::styled(" Cancel ", cancel_style),
    ]));

    content.push(Line::from(""));

    let modal = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Configuration ")
            .title_alignment(ratatui::layout::Alignment::Center)
            .style(Style::default().fg(Color::White)),
    );

    f.render_widget(modal, modal_area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
