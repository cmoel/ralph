//! Ralph - TUI wrapper for claude CLI that displays formatted streaming output.

mod app;
mod config;
mod events;
mod logging;
mod modal_ui;
mod modals;
mod specs;
mod ui;
mod validators;

use crate::app::{App, AppStatus, ContentBlockState};
use crate::config::LoadedConfig;
use crate::events::{ClaudeEvent, ContentBlock, Delta, StreamInnerEvent};
use crate::modals::{
    ConfigModalState, SpecsPanelState, handle_config_modal_input, handle_specs_panel_input,
};
use crate::ui::{draw_ui, format_tool_summary, format_usage_summary};

use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};

use crate::logging::ReloadHandle;

use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::{DefaultTerminal, Terminal};
use tracing::{debug, info, trace, warn};

/// Message types for output processing.
pub enum OutputMessage {
    Line(String),
}

/// Get the modification time of a file, or None if it can't be determined.
pub fn get_file_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn main() -> Result<()> {
    use std::time::Instant;

    let start_time = Instant::now();

    // Generate session ID first (always available, even if logging fails)
    let session_id = logging::new_session_id();

    // Load configuration first (needed for log level)
    let loaded_config = config::load_config();

    // Initialize logging with config log level
    let (log_directory, _guard, reload_handle) =
        match logging::init(session_id.clone(), &loaded_config.config.logging.level) {
            Ok(ctx) => {
                // Clean up old log files after logging is initialized
                logging::cleanup_old_logs(&ctx.log_directory);
                (
                    Some(ctx.log_directory),
                    Some(ctx._guard),
                    Some(ctx.reload_handle),
                )
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize logging: {}", e);
                (None, None, None)
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

fn run_app(
    mut terminal: DefaultTerminal,
    session_id: String,
    log_directory: Option<PathBuf>,
    loaded_config: LoadedConfig,
    log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
) -> Result<()> {
    let mut app = App::new(session_id, log_directory, loaded_config, log_level_handle);

    loop {
        // Poll for output from child process
        poll_output(&mut app);

        // Handle auto-continue if pending
        if app.auto_continue_pending {
            app.auto_continue_pending = false;
            start_command(&mut app)?;
        }

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

            // Handle specs panel input
            if app.show_specs_panel {
                if let Event::Key(key) = event {
                    handle_specs_panel_input(&mut app, key.code);
                }
                continue;
            }

            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => {
                        app.kill_child();
                        return Ok(());
                    }
                    KeyCode::Char('s') => match app.status {
                        AppStatus::Stopped => {
                            start_command(&mut app)?;
                        }
                        AppStatus::Running => {
                            app.stop_command();
                        }
                        AppStatus::Error => {}
                    },
                    KeyCode::Char('c') => {
                        // Only allow config modal when not running
                        if app.status != AppStatus::Running {
                            app.show_config_modal = true;
                            app.config_modal_state =
                                Some(ConfigModalState::from_config(&app.config));
                        }
                    }
                    KeyCode::Char('l') => {
                        // Open specs panel (available in all states)
                        app.show_specs_panel = true;
                        app.specs_panel_state =
                            Some(SpecsPanelState::new(&app.config.specs_path()));
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

/// Start the command.
fn start_command(app: &mut App) -> Result<()> {
    if app.status == AppStatus::Running {
        app.show_already_running_popup = true;
        return Ok(());
    }

    // Check if prompt file exists (using config path)
    let prompt_path = app.config.prompt_path();
    if !prompt_path.exists() {
        app.status = AppStatus::Error;
        app.add_line(format!("Error: {} not found", prompt_path.display()));
        return Ok(());
    }

    // Increment loop counter and log loop_start
    app.loop_count += 1;
    info!(loop_number = app.loop_count, "loop_start");

    // Add divider if not first run
    if !app.output_lines.is_empty() {
        app.add_line("─".repeat(40));
    }

    // Reset streaming state for new command
    app.content_blocks.clear();
    app.current_line.clear();

    // Spawn the command using shell to handle the pipe
    // Use config values for claude path and prompt path
    // Args are hardcoded - Ralph depends on this specific format for streaming output
    let claude_path = app.config.claude_path();
    const CLAUDE_ARGS: &str =
        "--output-format=stream-json --verbose --print --include-partial-messages";
    let command = format!(
        "cat {} | {} {}",
        prompt_path.display(),
        claude_path.display(),
        CLAUDE_ARGS
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

            app.child_process = Some(child);
            app.output_receiver = Some(rx);
            app.status = AppStatus::Running;
            app.run_start_time = Some(std::time::Instant::now());
        }
        Err(e) => {
            app.status = AppStatus::Error;
            app.add_line(format!("Error starting command: {}", e));
        }
    }

    Ok(())
}

/// Poll for output from the child process.
fn poll_output(app: &mut App) {
    // First, collect all pending messages
    let mut messages = Vec::new();
    let mut channel_disconnected = false;

    if let Some(rx) = &app.output_receiver {
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
        process_line(app, &line);
    }

    // Check if the channel disconnected (all senders dropped = readers finished)
    if channel_disconnected {
        debug!("channel_disconnected");

        // Try to get exit status from child process
        let (exit_code, exit_status): (Option<i32>, Option<String>) =
            if let Some(mut child) = app.child_process.take() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if let Some(code) = status.code() {
                            if code != 0 {
                                warn!(exit_code = code, "process_exit_nonzero");
                                app.add_line(format!("[Process exited with code {}]", code));
                            }
                            (Some(code), Some(format!("exit_code={}", code)))
                        } else {
                            // Process was terminated by signal (Unix)
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                if let Some(signal) = status.signal() {
                                    info!(signal, "process_killed_by_signal");
                                    (None, Some(format!("signal={}", signal)))
                                } else {
                                    (None, Some("unknown".to_string()))
                                }
                            }
                            #[cfg(not(unix))]
                            {
                                (None, Some("unknown".to_string()))
                            }
                        }
                    }
                    Ok(None) => {
                        // Still running, put it back (shouldn't happen if channel disconnected)
                        app.child_process = Some(child);
                        return;
                    }
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };

        // Log loop_end with exit status
        let status_str = exit_status.unwrap_or_else(|| "unknown".to_string());
        info!(
            loop_number = app.loop_count,
            exit_status = %status_str,
            "loop_end"
        );

        app.handle_channel_disconnected(exit_code);
    }
}

/// Parse and process a single NDJSON line.
fn process_line(app: &mut App, line: &str) {
    // Skip empty lines
    if line.trim().is_empty() {
        return;
    }

    // Log raw JSON at TRACE level for protocol debugging
    trace!(json = line, "raw_json_line");

    // Handle stderr lines (pass through as-is)
    if line.starts_with("[stderr]") {
        app.add_line(line.to_string());
        return;
    }

    // Try to parse as JSON
    match serde_json::from_str::<ClaudeEvent>(line) {
        Ok(event) => process_event(app, event),
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

/// Process a parsed Claude event.
fn process_event(app: &mut App, event: ClaudeEvent) {
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
            process_stream_event(app, inner);
        }
        ClaudeEvent::Result(result) => {
            debug!(?result, "Result event");
            // Display usage summary
            let summary = format_usage_summary(&result);
            for line in summary.lines() {
                app.add_line(line.to_string());
            }
        }
    }
}

/// Process inner streaming events (unwrapped from stream_event).
fn process_stream_event(app: &mut App, event: StreamInnerEvent) {
    match event {
        StreamInnerEvent::MessageStart(msg) => {
            debug!(?msg, "Message start");
            // Clear content blocks for new message
            app.content_blocks.clear();
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

            app.content_blocks.insert(index, state);
            debug!(index, "Content block started");
        }
        StreamInnerEvent::ContentBlockDelta(delta_event) => {
            let index = delta_event.index;
            let state = app.content_blocks.entry(index).or_default();

            match delta_event.delta {
                Delta::TextDelta { text } => {
                    state.text.push_str(&text);
                    // Display text immediately as it streams
                    app.append_text(&text);
                }
                Delta::InputJsonDelta { partial_json } => {
                    state.input_json.push_str(&partial_json);
                }
            }
        }
        StreamInnerEvent::ContentBlockStop(stop) => {
            debug!(index = stop.index, "Content block stopped");
            // Always flush any pending text first to maintain order
            app.flush_current_line();
            // Then process tool_use blocks
            if let Some(state) = app.content_blocks.get(&stop.index)
                && let Some(tool_name) = &state.tool_name
            {
                let summary = format_tool_summary(tool_name, &state.input_json);
                app.add_line(summary);
            }
        }
        StreamInnerEvent::MessageDelta(delta) => {
            debug!(?delta, "Message delta");
        }
        StreamInnerEvent::MessageStop => {
            debug!("Message stopped");
            // Flush any remaining text
            app.flush_current_line();
        }
    }
}
