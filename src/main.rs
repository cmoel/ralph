//! Ralph - TUI wrapper for claude CLI that displays formatted streaming output.

mod agent;
mod app;
mod cli;
mod config;
mod db;
mod doctor;
mod dolt;
mod events;
mod execution;
mod logging;
mod modals;
mod output;
mod prompt_sniff;
mod specs;
mod templates;
mod tool_history;
mod tool_panel;
mod tool_settings;
mod ui;
mod validators;
mod wake_lock;
mod work_control;
mod work_source;

use clap::Parser;

use crate::app::{App, AppStatus};
use crate::cli::{Cli, Commands, ToolCommands};
use crate::config::{LoadedConfig, load_global_config, load_project_config};
use crate::modals::{
    ConfigModalState, InitModalState, KanbanBoardState, SpecsPanelState, ToolAllowModalState,
    handle_config_modal_input, handle_init_modal_input, handle_kanban_input,
    handle_specs_panel_input, handle_tool_allow_modal_input,
};
use crate::tool_panel::SelectedPanel;
use crate::ui::draw_ui;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
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
use tracing::{debug, info, warn};

/// Direction for scroll operations.
enum ScrollDirection {
    Up,
    Down,
}

/// Scroll the focused panel.
fn scroll_panel(app: &mut App, direction: ScrollDirection, amount: u16) {
    match app.tool_panel.selected_panel {
        SelectedPanel::Main => match direction {
            ScrollDirection::Up => app.scroll_up(amount),
            ScrollDirection::Down => app.scroll_down(amount),
        },
        SelectedPanel::Tools => match direction {
            ScrollDirection::Up => app.tool_panel.scroll_up(amount),
            ScrollDirection::Down => app.tool_panel.scroll_down(amount),
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
        Some(Commands::Init) => return cli::run_init(),
        Some(Commands::Reinit) => return cli::run_reinit(),
        Some(Commands::Status { verbose }) => return cli::run_status(verbose),
        Some(Commands::Doctor) => return cli::run_doctor(),
        Some(Commands::Ready { verbose }) => return cli::run_ready(verbose),
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
        output::poll_output(&mut app);

        // Handle auto-continue if pending
        if app.auto_continue_pending {
            app.dirty = true;
            app.auto_continue_pending = false;
            app.increment_iteration();
            // Check for stale agents before claiming next bead
            app.start_stale_check();
            // In beads mode, claim next bead before continuing
            if execution::claim_before_start(&mut app) {
                execution::start_command(&mut app)?;
            } else {
                app.reset_iteration_state();
                app.status = AppStatus::Stopped;
            }
        }

        // Poll for background work source operations
        app.poll_work_check();
        app.poll_work_items();
        app.poll_kanban_items();
        app.poll_bead_detail();

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

        // Draw UI only when state changed
        if app.dirty {
            terminal.draw(|f| draw_ui(f, &mut app))?;
            app.dirty = false;
        }

        // State-dependent poll timeout: fast when running, slow when idle
        let poll_timeout = if app.status == AppStatus::Running {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(250)
        };
        if crossterm::event::poll(poll_timeout)? {
            let event = crossterm::event::read()?;
            app.dirty = true;

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

            // Handle kanban board input
            if app.show_kanban_board {
                if let Event::Key(key) = event {
                    handle_kanban_input(&mut app, key.code);
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
                    execution::handle_stale_modal_input(&mut app, key.code);
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
                            // Check for stale agents before claiming next bead
                            app.start_stale_check();
                            // Start new iteration run (reads config, sets up tracking)
                            if app.start_iteration_run() {
                                // In beads mode, claim a bead before starting
                                if !execution::claim_before_start(&mut app) {
                                    app.reset_iteration_state();
                                } else {
                                    execution::start_command(&mut app)?;
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
                    KeyCode::Char('l') if app.config.behavior.mode == "specs" => {
                        // Open specs panel (specs mode only)
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
                    KeyCode::Char('B') if app.config.behavior.mode == "beads" => {
                        app.show_kanban_board = true;
                        app.kanban_board_state = Some(KanbanBoardState::new_loading());
                        let bd_path = app.config.behavior.bd_path.clone();
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let output = std::process::Command::new(&bd_path)
                                .args(["list", "--json"])
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::piped())
                                .stderr(std::process::Stdio::piped())
                                .output();
                            let result = match output {
                                Ok(o) if o.status.success() => {
                                    let stdout = String::from_utf8_lossy(&o.stdout);
                                    serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
                                        .map_err(|e| format!("Failed to parse bd output: {e}"))
                                }
                                Ok(o) => {
                                    let stderr = String::from_utf8_lossy(&o.stderr);
                                    Err(format!("bd list failed: {stderr}"))
                                }
                                Err(e) => Err(format!("Failed to run bd: {e}")),
                            };
                            let _ = tx.send(result);
                        });
                        app.kanban_items_rx = Some(rx);
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
                        app.tool_panel.selected_panel.toggle();
                        if app.tool_panel.selected_panel == SelectedPanel::Tools
                            && app.tool_panel.selected.is_none()
                            && !app.tool_panel.entries.is_empty()
                        {
                            app.tool_panel.selected =
                                Some(app.tool_panel.entries.len().saturating_sub(1));
                        }
                    }
                    KeyCode::Char('t') => {
                        app.tool_panel.collapsed = !app.tool_panel.collapsed;
                    }
                    KeyCode::Char('A') if app.tool_panel.selected_panel == SelectedPanel::Tools => {
                        if let Some(idx) = app.tool_panel.selected
                            && let Some(entry) = app.tool_panel.entries.get(idx)
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
        let command = execution::assemble_prompt(&config).unwrap();

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
        let command = execution::assemble_prompt(&config).unwrap();

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
