//! Ralph - TUI wrapper for claude CLI that displays formatted streaming output.

mod agent;
mod app;
mod config;
mod db;
mod doctor;
mod events;
mod logging;
mod modal_ui;
mod modals;
mod prompt_sniff;
mod specs;
mod templates;
mod tool_history;
mod tool_settings;
mod ui;
mod validators;
mod wake_lock;
mod work_source;

use clap::Parser;

use crate::app::{
    App, AppStatus, ContentBlockState, PendingToolCall, SelectedPanel, ToolCallEntry,
    ToolCallStatus,
};
use crate::config::{LoadedConfig, load_global_config, load_project_config};
use crate::events::{
    ClaudeEvent, ContentBlock, Delta, StreamInnerEvent, ToolResultContent, UserContent,
};
use crate::modals::{
    ConfigModalState, InitModalState, SpecsPanelState, ToolAllowModalState,
    handle_config_modal_input, handle_init_modal_input, handle_specs_panel_input,
    handle_tool_allow_modal_input,
};
use crate::ui::{
    ExchangeType, draw_ui, extract_text_from_task_result, extract_tool_summary,
    format_assistant_header_styled, format_no_result_warning_styled, format_tool_result_styled,
    format_tool_summary_styled, format_usage_summary,
};

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
use ratatui::text::{Line, Span};
use ratatui::{DefaultTerminal, Terminal};
use tracing::{debug, info, trace, warn};

/// CLI subcommands.
#[derive(Debug, Parser)]
enum Commands {
    /// Initialize project with Ralph scaffolding
    Init,
    /// Force-regenerate all managed files (preserves .ralph config)
    Reinit,
    /// Show status summary of work items
    Status {
        /// Print individual item statuses
        #[arg(long)]
        verbose: bool,
    },
    /// Check environment health and report pass/fail for each check
    Doctor,
    /// List implementable beads (beads mode only)
    Ready {
        /// Annotate each item with include/exclude reason
        #[arg(long)]
        verbose: bool,
    },
    /// Manage and inspect tool permissions and history
    #[command(subcommand)]
    Tool(ToolCommands),
}

/// Subcommands under `ralph tool`.
#[derive(Debug, Parser)]
enum ToolCommands {
    /// Query tool call history from the SQLite database
    History {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Filter by tool name (case-insensitive)
        #[arg(long)]
        tool: Option<String>,
        /// Show tool calls since this time (e.g., 6h, 1d, today, 2025-01-15)
        #[arg(long)]
        since: Option<String>,
        /// Show tool calls until this time (requires --since)
        #[arg(long)]
        until: Option<String>,
        /// Filter to rejected/errored tool calls only
        #[arg(long)]
        rejected: bool,
        /// Filter by repo (basename match if no slash, substring match if has slash)
        #[arg(long)]
        repo: Option<String>,
        /// Show tool calls from all repos (overrides default current-repo filter)
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Print the database file path and exit
        #[arg(long)]
        db_path: bool,
    },
    /// Allow a tool pattern in settings
    Allow {
        /// Tool pattern (e.g., Read, Bash(git:*), WebFetch(domain:docs.rs))
        pattern: String,
        /// Write to .claude/settings.json instead of .claude/settings.local.json
        #[arg(long)]
        project: bool,
    },
    /// Deny a tool pattern in settings
    Deny {
        /// Tool pattern (e.g., Bash(rm:*), Bash(sudo *))
        pattern: String,
        /// Write to .claude/settings.json instead of .claude/settings.local.json
        #[arg(long)]
        project: bool,
    },
    /// List tool permissions across all settings files
    List,
}

/// CLI argument parser.
#[derive(Debug, Parser)]
#[command(
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_SHA"), ")"),
    about = "TUI wrapper for claude CLI that displays formatted streaming output"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Message types for output processing.
pub enum OutputMessage {
    Line(String),
}

/// Adds indentation to a styled Line by prepending "  " to the first span.
fn indent_line(line: Line<'static>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
    spans.extend(line.spans);
    Line::from(spans)
}

/// Direction for scroll operations.
enum ScrollDirection {
    Up,
    Down,
}

/// Scroll the focused panel.
fn scroll_panel(app: &mut App, direction: ScrollDirection, amount: u16) {
    match app.selected_panel {
        SelectedPanel::Main => match direction {
            ScrollDirection::Up => app.scroll_up(amount),
            ScrollDirection::Down => app.scroll_down(amount),
        },
        SelectedPanel::Tools => match direction {
            ScrollDirection::Up => app.scroll_tools_up(amount),
            ScrollDirection::Down => app.scroll_tools_down(amount),
        },
    }
}

/// Get the modification time of a file, or None if it can't be determined.
pub fn get_file_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn main() -> Result<()> {
    use std::time::Instant;

    // Parse CLI args (handles --version, --help, subcommands)
    let cli = Cli::parse();

    // Handle subcommands that don't need the TUI
    match cli.command {
        Some(Commands::Init) => return run_init(),
        Some(Commands::Reinit) => return run_reinit(),
        Some(Commands::Status { verbose }) => return run_status(verbose),
        Some(Commands::Doctor) => return run_doctor(),
        Some(Commands::Ready { verbose }) => return run_ready(verbose),
        Some(Commands::Tool(tool_cmd)) => {
            return match tool_cmd {
                ToolCommands::History {
                    session,
                    tool,
                    since,
                    until,
                    rejected,
                    repo,
                    all,
                    json,
                    db_path,
                } => tool_history::run(tool_history::HistoryOptions {
                    session,
                    tool,
                    since,
                    until,
                    rejected,
                    json,
                    show_db_path: db_path,
                    repo,
                    all,
                }),
                ToolCommands::Allow { pattern, project } => {
                    tool_settings::allow_pattern(&pattern, project)
                }
                ToolCommands::Deny { pattern, project } => {
                    tool_settings::deny_pattern(&pattern, project)
                }
                ToolCommands::List => tool_settings::list_rules(),
            };
        }
        None => {}
    }

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

/// Run the init subcommand: create project scaffolding files.
fn run_init() -> Result<()> {
    let loaded_config = config::load_config();
    let state = InitModalState::new(&loaded_config.config);

    if state.all_exist() {
        println!("All files already exist, nothing to create.");
        return Ok(());
    }

    let create_count = state.create_count();
    let skip_count = state.skip_count();

    match state.create_files() {
        Ok(()) => {
            println!(
                "Created {} file{}, skipped {} existing.",
                create_count,
                if create_count == 1 { "" } else { "s" },
                skip_count
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Run the reinit subcommand: force-regenerate all managed files.
fn run_reinit() -> Result<()> {
    let loaded_config = config::load_config();
    let state = InitModalState::new_reinit(&loaded_config.config);

    let create_count = state.create_count();
    let regenerate_count = state.regenerate_count();

    if create_count == 0 && regenerate_count == 0 {
        println!("Nothing to do.");
        return Ok(());
    }

    match state.create_files() {
        Ok(()) => {
            let mut parts = Vec::new();
            if regenerate_count > 0 {
                parts.push(format!(
                    "regenerated {} file{}",
                    regenerate_count,
                    if regenerate_count == 1 { "" } else { "s" }
                ));
            }
            if create_count > 0 {
                parts.push(format!(
                    "created {} file{}",
                    create_count,
                    if create_count == 1 { "" } else { "s" }
                ));
            }
            println!("{} (old files backed up as .bak).", parts.join(", "));
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Run the status subcommand: print work item status summary.
fn run_status(verbose: bool) -> Result<()> {
    let loaded_config = config::load_config();
    let cfg = &loaded_config.config;
    let work_source = work_source::create_work_source(
        &cfg.behavior.mode,
        cfg.specs_path(),
        &cfg.behavior.bd_path,
    );

    let items = match work_source.list_items() {
        Ok(items) => items,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if items.is_empty() {
        println!("{}", work_source.complete_message());
        return Ok(());
    }

    let blocked = items
        .iter()
        .filter(|i| i.status == work_source::WorkItemStatus::Blocked)
        .count();
    let ready = items
        .iter()
        .filter(|i| i.status == work_source::WorkItemStatus::Ready)
        .count();
    let in_progress = items
        .iter()
        .filter(|i| i.status == work_source::WorkItemStatus::InProgress)
        .count();
    let done = items
        .iter()
        .filter(|i| i.status == work_source::WorkItemStatus::Done)
        .count();

    // Build summary parts, only including non-zero counts
    let mut parts = Vec::new();
    if in_progress > 0 {
        parts.push(format!("{} in progress", in_progress));
    }
    if ready > 0 {
        parts.push(format!("{} ready", ready));
    }
    if blocked > 0 {
        parts.push(format!("{} blocked", blocked));
    }
    if done > 0 {
        parts.push(format!("{} done", done));
    }

    println!("{}", parts.join(", "));

    if verbose {
        println!();
        for item in &items {
            println!("  [{}] {}", item.status.label(), item.name);
        }
    }

    Ok(())
}

/// Run the ready subcommand: list implementable beads.
fn run_ready(verbose: bool) -> Result<()> {
    let loaded_config = config::load_config();
    let cfg = &loaded_config.config;

    if cfg.behavior.mode != "beads" {
        eprintln!("Error: ralph ready requires beads mode");
        std::process::exit(1);
    }

    let output = Command::new(&cfg.behavior.bd_path)
        .args(["ready", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("Error: {}: command not found", cfg.behavior.bd_path);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: failed to run bd: {}", e);
            std::process::exit(1);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Error: bd ready failed: {}", stderr.trim());
        std::process::exit(1);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(serde_json::Value::Array(arr)) => arr,
        Ok(_) => {
            eprintln!("Error: unexpected bd ready output");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: failed to parse bd output: {}", e);
            std::process::exit(1);
        }
    };

    if items.is_empty() {
        if verbose {
            println!("No ready beads.");
        }
        std::process::exit(1);
    }

    let mut has_implementable = false;

    for item in &items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("???");
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let labels = item.get("labels").and_then(|v| v.as_array());

        let needs_shaping = labels.is_some_and(|ls| {
            ls.iter().any(|l| {
                l.as_str()
                    .is_some_and(|s| work_source::is_shaping_label(s, &[]))
            })
        });

        if verbose {
            if needs_shaping {
                println!("{}: SKIPPED (needs-shaping) — {}", id, title);
            } else {
                println!("{}: READY — {}", id, title);
                has_implementable = true;
            }
        } else if !needs_shaping {
            println!("{}\t{}", id, title);
            has_implementable = true;
        }
    }

    if !has_implementable {
        if verbose {
            println!(
                "\nAll {} ready bead{} {} shaping.",
                items.len(),
                if items.len() == 1 { "" } else { "s" },
                if items.len() == 1 { "needs" } else { "need" }
            );
        }
        std::process::exit(1);
    }

    Ok(())
}

/// Run the doctor subcommand: check environment health.
fn run_doctor() -> Result<()> {
    let loaded_config = config::load_config();
    let cfg = &loaded_config.config;

    let mut checks: Vec<doctor::CheckResult> = vec![
        doctor::check_config(&loaded_config),
        doctor::check_claude(cfg),
        doctor::check_prompt(cfg),
    ];

    if cfg.behavior.mode == "beads" {
        checks.push(doctor::check_bd(cfg));
        let dolt_check = doctor::check_dolt_status(cfg);
        let dolt_running = dolt_check.passed;
        checks.push(dolt_check);
        if dolt_running {
            checks.push(doctor::check_work_items(cfg));
            checks.push(doctor::check_unrecognized_labels(cfg));
        }
    } else {
        checks.push(doctor::check_work_items(cfg));
    }

    let mut all_passed = true;
    for check in &checks {
        if check.passed {
            println!("\u{2713} {}", check.message);
        } else {
            println!("\u{2717} {}", check.message);
            all_passed = false;
        }
    }

    if !all_passed {
        std::process::exit(1);
    }

    Ok(())
}

fn run_app(
    mut terminal: DefaultTerminal,
    session_id: String,
    log_directory: Option<PathBuf>,
    loaded_config: LoadedConfig,
    log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
) -> Result<()> {
    let loaded_for_doctor = loaded_config.clone();
    let mut app = App::new(session_id, log_directory, loaded_config, log_level_handle);

    // Sniff test: warn if PROMPT.md contains mode-mismatched content
    let prompt_path = app.config.prompt_path();
    if let Ok(content) = std::fs::read_to_string(&prompt_path) {
        for warning in prompt_sniff::sniff_prompt(&content, &app.config.behavior.mode) {
            app.add_text_line(warning);
        }
    }

    // Run doctor checks asynchronously — only surface failures
    {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let cfg = &loaded_for_doctor.config;
            let mut checks = vec![
                doctor::check_config(&loaded_for_doctor),
                doctor::check_claude(cfg),
                doctor::check_prompt(cfg),
            ];
            if cfg.behavior.mode == "beads" {
                checks.push(doctor::check_bd(cfg));
                let dolt_check = doctor::check_dolt_status(cfg);
                let dolt_running = dolt_check.passed;
                checks.push(dolt_check);
                if dolt_running {
                    checks.push(doctor::check_work_items(cfg));
                }
            } else {
                checks.push(doctor::check_work_items(cfg));
            }
            let _ = tx.send(checks);
        });
        app.doctor_rx = Some(rx);
    }

    // Initialize tool history database
    match db::open() {
        Ok(conn) => {
            app.tool_history_db = Some(conn);
        }
        Err(e) => {
            warn!(error = %e, "tool_history_db_open_failed");
            app.add_text_line(format!("[Tool history DB failed: {}]", e));
        }
    }

    // Register agent and create worktree (beads mode only)
    if app.config.behavior.mode == "beads" {
        let bd_path = app.config.behavior.bd_path.clone();
        let sid = app.session_id.clone();
        if let Some(setup) = agent::register(&bd_path, &sid) {
            let heartbeat_interval = app.config.behavior.heartbeat_interval;
            let stop =
                agent::start_heartbeat(bd_path, setup.agent_bead_id.clone(), heartbeat_interval);
            app.agent_bead_id = Some(setup.agent_bead_id);
            app.worktree_name = Some(setup.worktree_name);
            app.worktree_path = Some(setup.worktree_path);
            app.heartbeat_stop = Some(stop);
        } else {
            app.add_text_line("[Agent registration failed — running without worktree]".to_string());
        }

        // Check for stale agents (background, non-blocking)
        app.start_stale_check();
    }

    loop {
        // Poll for output from child process
        poll_output(&mut app);

        // Handle auto-continue if pending
        if app.auto_continue_pending {
            app.auto_continue_pending = false;
            app.increment_iteration();
            // In beads mode, claim next bead before continuing
            if claim_before_start(&mut app) {
                start_command(&mut app)?;
            } else {
                app.reset_iteration_state();
                app.status = AppStatus::Stopped;
            }
        }

        // Poll for background work source operations
        app.poll_work_check();
        app.poll_work_items();

        // Poll for current spec (throttled to every 2 seconds)
        app.poll_spec();

        // Poll for Dolt server state (beads mode only)
        // Process toggle results first so stale status polls don't override
        app.poll_dolt_toggle();
        app.poll_dolt_status();

        // Poll for config file changes (throttled to every 2 seconds)
        app.poll_config();

        // Poll for background doctor check results
        app.poll_doctor();

        // Poll for stale agent detection results
        app.poll_stale_check();

        // Auto-clear error flash after timeout
        app.check_error_timeout();

        // Auto-clear hint after timeout
        app.check_hint_timeout();

        // Draw UI
        terminal.draw(|f| draw_ui(f, &mut app))?;

        // Poll for events with a short timeout to allow process output polling
        if crossterm::event::poll(Duration::from_millis(50))? {
            let event = crossterm::event::read()?;

            // Clear hint on any keypress
            if matches!(event, Event::Key(_)) {
                app.hint = None;
            }

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

            // Handle init modal input
            if app.show_init_modal {
                if let Event::Key(key) = event {
                    handle_init_modal_input(&mut app, key.code);
                }
                continue;
            }

            // Handle stale recovery modal input
            if app.show_stale_modal {
                if let Event::Key(key) = event {
                    handle_stale_modal_input(&mut app, key.code);
                }
                continue;
            }

            // Handle quit confirmation modal input
            if app.show_quit_modal {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.kill_child();
                            app.cleanup_agent();
                            return Ok(());
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.show_quit_modal = false;
                        }
                        _ => {}
                    }
                }
                continue;
            }

            // Handle help modal input
            if app.show_help_modal {
                if let Event::Key(key) = event
                    && (key.code == KeyCode::Esc || key.code == KeyCode::Char('?'))
                {
                    app.show_help_modal = false;
                }
                continue;
            }

            // Handle tool allow modal input
            if app.show_tool_allow_modal {
                if let Event::Key(key) = event {
                    handle_tool_allow_modal_input(&mut app, key.code, key.modifiers);
                }
                continue;
            }

            match event {
                Event::Key(key) => match key.code {
                    KeyCode::Char('q') => {
                        if app.status == AppStatus::Running {
                            app.set_hint("press s to stop the loop");
                        } else {
                            app.show_quit_modal = true;
                        }
                    }
                    KeyCode::Char('s') => match app.status {
                        AppStatus::Stopped | AppStatus::Error => {
                            // Start new iteration run (reads config, sets up tracking)
                            if app.start_iteration_run() {
                                // In beads mode, claim a bead before starting
                                if !claim_before_start(&mut app) {
                                    app.reset_iteration_state();
                                } else {
                                    start_command(&mut app)?;
                                }
                            }
                            // If start_iteration_run returns false, iterations = 0, no-op
                        }
                        AppStatus::Running => {
                            app.stop_command();
                        }
                    },
                    KeyCode::Char('c') => {
                        // Open config modal with project tab if .ralph exists
                        app.show_config_modal = true;
                        app.config_modal_state = if let Some(ref project_path) =
                            app.project_config_path
                        {
                            let global_config = load_global_config(&app.config_path);
                            match load_project_config(project_path) {
                                Ok(partial) => Some(ConfigModalState::from_config_with_project(
                                    &global_config,
                                    &partial,
                                    &app.config,
                                    project_path.clone(),
                                )),
                                Err(_) => {
                                    // If .ralph can't be parsed, fall back to global-only
                                    Some(ConfigModalState::from_config(&app.config))
                                }
                            }
                        } else {
                            Some(ConfigModalState::from_config(&app.config))
                        };
                    }
                    KeyCode::Char('l') => {
                        // Open work panel (available in all states)
                        app.show_specs_panel = true;
                        app.specs_panel_state = Some(SpecsPanelState::new_loading(
                            app.work_source.label(),
                            &app.config.specs_path(),
                        ));
                        // Kick off background list_items
                        let (tx, rx) = mpsc::channel();
                        let ws = Arc::clone(&app.work_source);
                        std::thread::spawn(move || {
                            let _ = tx.send(ws.list_items());
                        });
                        app.work_items_rx = Some(rx);
                    }
                    KeyCode::Char('i') => {
                        // Open init modal
                        app.show_init_modal = true;
                        app.init_modal_state = Some(InitModalState::new(&app.config));
                    }
                    KeyCode::Char('?') => {
                        // Open help modal
                        app.show_help_modal = true;
                    }
                    KeyCode::Char('D') => {
                        // Toggle Dolt server (beads mode only)
                        app.toggle_dolt_server();
                    }
                    KeyCode::Tab => {
                        app.selected_panel.toggle();
                        if app.selected_panel == SelectedPanel::Tools
                            && app.tool_panel_selected.is_none()
                            && !app.tool_call_entries.is_empty()
                        {
                            app.tool_panel_selected =
                                Some(app.tool_call_entries.len().saturating_sub(1));
                        }
                    }
                    KeyCode::Char('t') => {
                        app.tool_panel_collapsed = !app.tool_panel_collapsed;
                    }
                    KeyCode::Char('A') if app.selected_panel == SelectedPanel::Tools => {
                        if let Some(idx) = app.tool_panel_selected
                            && let Some(entry) = app.tool_call_entries.get(idx)
                        {
                            app.show_tool_allow_modal = true;
                            app.tool_allow_modal_state =
                                Some(ToolAllowModalState::new(&entry.tool_name, &entry.summary));
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        scroll_panel(&mut app, ScrollDirection::Up, 1);
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        scroll_panel(&mut app, ScrollDirection::Down, 1);
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        scroll_panel(&mut app, ScrollDirection::Up, half_page);
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        scroll_panel(&mut app, ScrollDirection::Down, half_page);
                    }
                    KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let full_page = app.main_pane_height;
                        scroll_panel(&mut app, ScrollDirection::Up, full_page);
                    }
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let full_page = app.main_pane_height;
                        scroll_panel(&mut app, ScrollDirection::Down, full_page);
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        scroll_panel(&mut app, ScrollDirection::Up, 3);
                    }
                    MouseEventKind::ScrollDown => {
                        scroll_panel(&mut app, ScrollDirection::Down, 3);
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

/// Assemble the prompt content and build the shell command string for Claude CLI.
///
/// Reads PROMPT.md and optional mode-specific content, writes a temp file for mode
/// content if needed, and returns the full shell command to pipe into Claude.
fn assemble_prompt(config: &crate::config::Config) -> Result<String> {
    let prompt_path = config.prompt_path();
    let claude_path = config.claude_path();
    const CLAUDE_ARGS: &str =
        "--output-format=stream-json --verbose --print --include-partial-messages";

    let mode = &config.behavior.mode;
    let mode_temp_path = if let Some(content) = templates::mode_content(mode) {
        let path = std::env::temp_dir().join("ralph-mode.md");
        std::fs::write(&path, content)?;
        Some(path)
    } else {
        None
    };

    let command = if let Some(ref mode_path) = mode_temp_path {
        format!(
            "cat {} {} | {} {}",
            prompt_path.display(),
            mode_path.display(),
            claude_path.display(),
            CLAUDE_ARGS
        )
    } else {
        format!(
            "cat {} | {} {}",
            prompt_path.display(),
            claude_path.display(),
            CLAUDE_ARGS
        )
    };

    Ok(command)
}

/// In beads mode, claim the next available bead before starting claude.
/// Returns true if we should proceed with start_command (claimed or non-beads mode).
/// Returns false if claiming failed (no work available).
fn claim_before_start(app: &mut App) -> bool {
    if app.config.behavior.mode != "beads" {
        return true;
    }
    let agent_id = match &app.agent_bead_id {
        Some(id) => id.clone(),
        None => return true, // no agent registered, skip claiming
    };

    match agent::claim_next_bead(&app.config.behavior.bd_path, &agent_id) {
        Some((bead_id, title)) => {
            app.add_text_line(format!("[Claimed: {} {}]", bead_id, title));
            app.hooked_bead_id = Some(bead_id);
            true
        }
        None => {
            app.add_text_line("[No beads available to claim]".to_string());
            false
        }
    }
}

/// Handle input for the stale agent recovery modal.
fn handle_stale_modal_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('r') | KeyCode::Char('R') => {
            // Resume: claim the stale bead on our agent
            let stale = match app
                .stale_modal_state
                .as_ref()
                .and_then(|s| s.current().cloned())
            {
                Some(s) => s,
                None => return,
            };
            let agent_id = match &app.agent_bead_id {
                Some(id) => id.clone(),
                None => {
                    app.add_text_line("[Cannot resume — no agent registered]".to_string());
                    app.show_stale_modal = false;
                    app.stale_modal_state = None;
                    return;
                }
            };
            let bd_path = app.config.behavior.bd_path.clone();
            if agent::resume_stale_bead(&bd_path, &agent_id, &stale) {
                app.add_text_line(format!(
                    "[Resumed: {} \"{}\"]",
                    stale.hooked_bead_id, stale.hooked_bead_title
                ));
                app.hooked_bead_id = Some(stale.hooked_bead_id.clone());
            } else {
                app.add_text_line("[Resume failed]".to_string());
            }
            if let Some(ref mut state) = app.stale_modal_state {
                state.advance();
                if state.is_empty() {
                    app.show_stale_modal = false;
                    app.stale_modal_state = None;
                }
            }
        }
        KeyCode::Char('x') | KeyCode::Char('X') => {
            // Release: clear hook, reset bead to open
            let stale = match app
                .stale_modal_state
                .as_ref()
                .and_then(|s| s.current().cloned())
            {
                Some(s) => s,
                None => return,
            };
            let bd_path = app.config.behavior.bd_path.clone();
            agent::release_stale_bead(&bd_path, &stale);
            app.add_text_line(format!(
                "[Released: {} \"{}\"]",
                stale.hooked_bead_id, stale.hooked_bead_title
            ));
            if let Some(ref mut state) = app.stale_modal_state {
                state.advance();
                if state.is_empty() {
                    app.show_stale_modal = false;
                    app.stale_modal_state = None;
                }
            }
        }
        KeyCode::Char('n') | KeyCode::Char('N') => {
            // Skip this stale agent
            if let Some(ref mut state) = app.stale_modal_state {
                state.advance();
                if state.is_empty() {
                    app.show_stale_modal = false;
                    app.stale_modal_state = None;
                }
            }
        }
        KeyCode::Esc => {
            // Close modal entirely
            app.show_stale_modal = false;
            app.stale_modal_state = None;
        }
        _ => {}
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
        app.error_at = Some(std::time::Instant::now());
        app.add_text_line(format!("Error: {} not found", prompt_path.display()));
        return Ok(());
    }

    // Increment loop counter and log loop_start
    app.loop_count += 1;
    info!(loop_number = app.loop_count, "loop_start");

    // Add divider if not first run
    if !app.output_lines.is_empty() {
        app.add_text_line("─".repeat(40));
    }

    // Reset streaming state for new command
    app.content_blocks.clear();
    app.current_line.clear();

    let command = assemble_prompt(&app.config)?;
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // In beads mode, run claude in the worktree directory
    if let Some(ref wt_path) = app.worktree_path {
        cmd.current_dir(wt_path);
    }

    let child = cmd.spawn();

    match child {
        Ok(mut child) => {
            // Log command_spawned with PID
            debug!(pid = child.id(), "command_spawned");

            // Attempt to acquire wake lock (prevents system idle sleep)
            let wake_lock = if app.config.behavior.keep_awake {
                match wake_lock::acquire() {
                    Some(lock) => Some(lock),
                    None => {
                        // Wake lock failed - display warning in output panel
                        app.add_text_line(
                            "⚠ Warning: Could not acquire wake lock - system may sleep during execution"
                                .to_string(),
                        );
                        None
                    }
                }
            } else {
                None
            };
            app.wake_lock = wake_lock;

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
            app.error_at = Some(std::time::Instant::now());
            app.add_text_line(format!("Error starting command: {}", e));
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
        app.add_text_line(line.to_string());
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
        ClaudeEvent::User(user_event) => {
            debug!(?user_event, "User event");
            // Process tool results from user event
            if let Some(message) = user_event.message {
                for content in message.content {
                    match content {
                        UserContent::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            // Look up tool name from our mapping
                            let tool_name = app
                                .tool_id_to_name
                                .get(&tool_use_id)
                                .cloned()
                                .unwrap_or_else(|| {
                                    warn!(tool_use_id, "Orphan tool result (unknown ID)");
                                    "unknown".to_string()
                                });

                            let is_error = is_error.unwrap_or(false);

                            // Extract content string
                            let content_str = match content {
                                Some(ToolResultContent::Text(s)) => s,
                                Some(ToolResultContent::Structured(v)) => v.to_string(),
                                None => String::new(),
                            };

                            // For Task tool results, extract text from JSON array format
                            let content_str = if tool_name == "Task" {
                                extract_text_from_task_result(&content_str).unwrap_or(content_str)
                            } else {
                                content_str
                            };

                            // Update tool call record with result
                            if let Some(ref conn) = app.tool_history_db
                                && !db::update_tool_result(
                                    conn,
                                    &tool_use_id,
                                    &app.session_id,
                                    is_error,
                                    &content_str,
                                )
                            {
                                app.add_text_line(
                                    "[Warning: failed to update tool result]".to_string(),
                                );
                            }

                            // Update tool panel entry status
                            let panel_status = if is_error {
                                ToolCallStatus::Error
                            } else {
                                ToolCallStatus::Success
                            };
                            app.update_tool_call_status(&tool_use_id, panel_status);

                            // Check for pending tool call to correlate with
                            if let Some(pending) = app.pending_tool_calls.remove(&tool_use_id) {
                                // Display tool call first
                                app.add_line(pending.styled_line);
                                // Display result indented under call
                                let lines = format_tool_result_styled(
                                    &pending.tool_name,
                                    &content_str,
                                    is_error,
                                );
                                for line in lines {
                                    // Add indentation to styled line
                                    let indented = indent_line(line);
                                    app.add_line(indented);
                                }
                            } else {
                                // No pending call found - display result standalone
                                let lines =
                                    format_tool_result_styled(&tool_name, &content_str, is_error);
                                for line in lines {
                                    app.add_line(line);
                                }
                            }
                        }
                    }
                }
            }
        }
        ClaudeEvent::StreamEvent { event: inner } => {
            // Unwrap and process the inner streaming event
            process_stream_event(app, inner);
        }
        ClaudeEvent::Result(result) => {
            debug!(?result, "Result event");
            // Flush any pending tool calls that never received results
            let pending_calls: Vec<_> = app.pending_tool_calls.drain().collect();
            for (_id, pending) in pending_calls {
                app.add_line(pending.styled_line);
                app.add_line(indent_line(format_no_result_warning_styled()));
            }
            // Increment exchange counter
            app.exchange_count += 1;
            // Accumulate tokens for session total
            if let Some(usage) = &result.usage {
                let input = usage.input_tokens.unwrap_or(0);
                let output = usage.output_tokens.unwrap_or(0);
                app.cumulative_tokens += input + output;
            }
            // Determine exchange type
            let exchange_type = if app.exchange_count == 1 {
                ExchangeType::InitialPrompt
            } else if let Some(tool_name) = app.last_tool_used.take() {
                ExchangeType::AfterTool(tool_name)
            } else {
                ExchangeType::Continuation
            };
            // Display usage summary with exchange info
            let summary = format_usage_summary(&result, app.exchange_count, exchange_type);
            for line in summary.lines() {
                app.add_text_line(line.to_string());
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
            // Clear pending tool calls (new assistant turn)
            app.pending_tool_calls.clear();
        }
        StreamInnerEvent::ContentBlockStart(block_start) => {
            let index = block_start.index;
            let mut state = ContentBlockState::default();

            match block_start.content_block {
                ContentBlock::Text { text } => {
                    state.text = text;
                }
                ContentBlock::ToolUse { id, name, .. } => {
                    state.tool_name = Some(name);
                    state.tool_use_id = id;
                }
            }

            app.content_blocks.insert(index, state);
            debug!(index, "Content block started");
        }
        StreamInnerEvent::ContentBlockDelta(delta_event) => {
            let index = delta_event.index;

            match delta_event.delta {
                Delta::TextDelta { text } => {
                    // Check if we need to show the header (without holding mutable borrow)
                    let needs_header = app
                        .content_blocks
                        .get(&index)
                        .map(|s| !s.header_shown)
                        .unwrap_or(true);

                    if needs_header {
                        app.add_line(format_assistant_header_styled());
                    }

                    // Update state in a separate scope to release the borrow
                    {
                        let state = app.content_blocks.entry(index).or_default();
                        state.header_shown = true;
                        state.text.push_str(&text);
                    }

                    // Display text immediately as it streams (indented)
                    app.append_indented_text(&text);
                }
                Delta::InputJsonDelta { partial_json } => {
                    let state = app.content_blocks.entry(index).or_default();
                    state.input_json.push_str(&partial_json);
                }
            }
        }
        StreamInnerEvent::ContentBlockStop(stop) => {
            debug!(index = stop.index, "Content block stopped");
            // Flush any pending text (uses indentation flag automatically)
            app.flush_current_line();
            // Extract data from content block state before mutating app
            let block_data = app.content_blocks.get(&stop.index).and_then(|state| {
                state.tool_name.as_ref().map(|name| {
                    (
                        name.clone(),
                        state.tool_use_id.clone(),
                        state.input_json.clone(),
                    )
                })
            });
            // Then process tool_use blocks
            if let Some((tool_name, tool_use_id, input_json)) = block_data {
                // Register tool_use_id → tool_name mapping for result correlation
                if let Some(ref id) = tool_use_id {
                    app.tool_id_to_name.insert(id.clone(), tool_name.clone());
                }
                // Record tool call to history DB
                if let Some(ref conn) = app.tool_history_db {
                    app.tool_call_sequence += 1;
                    if db::insert_tool_call(
                        conn,
                        &app.session_id,
                        &tool_name,
                        tool_use_id.as_deref(),
                        &input_json,
                        app.tool_call_sequence,
                        &app.repo_path,
                    )
                    .is_none()
                    {
                        app.add_text_line("[Warning: failed to record tool call]".to_string());
                    }
                }
                // Track the last tool used for exchange categorization
                app.last_tool_used = Some(tool_name.clone());
                // Add entry to tool panel
                let summary = extract_tool_summary(&tool_name, &input_json);
                app.add_tool_call_entry(ToolCallEntry {
                    tool_name: tool_name.clone(),
                    summary,
                    status: ToolCallStatus::Pending,
                    tool_use_id: tool_use_id.clone(),
                });
                let styled_line = format_tool_summary_styled(&tool_name, &input_json);
                // Buffer tool call if it has an ID (for correlation with result)
                if let Some(ref id) = tool_use_id {
                    app.pending_tool_calls.insert(
                        id.clone(),
                        PendingToolCall {
                            tool_name: tool_name.clone(),
                            styled_line,
                        },
                    );
                } else {
                    // No ID - display immediately
                    app.add_line(styled_line);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_no_args_parses_successfully() {
        Cli::try_parse_from(["ralph"]).unwrap();
    }

    #[test]
    fn cli_version_flag_exits() {
        let result = Cli::try_parse_from(["ralph", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn cli_help_flag_exits() {
        let result = Cli::try_parse_from(["ralph", "--help"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_unknown_arg_fails() {
        let result = Cli::try_parse_from(["ralph", "--bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_init_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "init"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Init)));
    }

    #[test]
    fn cli_no_subcommand_gives_none() {
        let cli = Cli::try_parse_from(["ralph"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["ralph", "bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_reinit_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "reinit"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Reinit)));
    }

    #[test]
    fn cli_status_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Status { verbose: false })
        ));
    }

    #[test]
    fn cli_doctor_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "doctor"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Doctor)));
    }

    #[test]
    fn cli_status_verbose_parses() {
        let cli = Cli::try_parse_from(["ralph", "status", "--verbose"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Status { verbose: true })
        ));
    }

    #[test]
    fn cli_tool_history_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tool(ToolCommands::History { .. }))
        ));
    }

    #[test]
    fn cli_tool_history_session_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history", "--session", "abc123"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { session, .. })) => {
                assert_eq!(session.as_deref(), Some("abc123"));
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_tool_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history", "--tool", "Bash"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { tool, .. })) => {
                assert_eq!(tool.as_deref(), Some("Bash"));
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_since_parses() {
        let cli =
            Cli::try_parse_from(["ralph", "tool", "history", "--since", "6h", "--until", "1h"])
                .unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { since, until, .. })) => {
                assert_eq!(since.as_deref(), Some("6h"));
                assert_eq!(until.as_deref(), Some("1h"));
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_flags_parse() {
        let cli =
            Cli::try_parse_from(["ralph", "tool", "history", "--rejected", "--json"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { rejected, json, .. })) => {
                assert!(rejected);
                assert!(json);
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_db_path_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history", "--db-path"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { db_path, .. })) => {
                assert!(db_path);
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_allow_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "allow", "Read"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::Allow { pattern, project })) => {
                assert_eq!(pattern, "Read");
                assert!(!project);
            }
            _ => panic!("Expected Tool Allow"),
        }
    }

    #[test]
    fn cli_tool_allow_project_flag_parses() {
        let cli =
            Cli::try_parse_from(["ralph", "tool", "allow", "Bash(git:*)", "--project"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::Allow { pattern, project })) => {
                assert_eq!(pattern, "Bash(git:*)");
                assert!(project);
            }
            _ => panic!("Expected Tool Allow"),
        }
    }

    #[test]
    fn cli_tool_deny_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "deny", "Bash(rm:*)"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::Deny { pattern, project })) => {
                assert_eq!(pattern, "Bash(rm:*)");
                assert!(!project);
            }
            _ => panic!("Expected Tool Deny"),
        }
    }

    #[test]
    fn cli_tool_list_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tool(ToolCommands::List))
        ));
    }

    #[test]
    fn assemble_prompt_default_mode_includes_prompt_and_mode_file() {
        let config = crate::config::Config::default();
        let command = assemble_prompt(&config).unwrap();

        // Should pipe PROMPT.md and mode content through Claude CLI
        assert!(command.contains("PROMPT.md"));
        assert!(command.contains("ralph-mode.md"));
        assert!(command.contains("--output-format=stream-json"));
        assert!(command.contains("--print"));
    }

    #[test]
    fn assemble_prompt_unknown_mode_omits_mode_file() {
        let mut config = crate::config::Config::default();
        config.behavior.mode = "nonexistent-mode".to_string();
        let command = assemble_prompt(&config).unwrap();

        // Should only have PROMPT.md, no mode temp file
        assert!(command.contains("PROMPT.md"));
        assert!(!command.contains("ralph-mode.md"));
        assert!(command.contains("--output-format=stream-json"));
    }

    #[test]
    fn cli_ready_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "ready"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Ready { verbose: false })
        ));
    }

    #[test]
    fn cli_ready_verbose_parses() {
        let cli = Cli::try_parse_from(["ralph", "ready", "--verbose"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Ready { verbose: true })
        ));
    }
}
