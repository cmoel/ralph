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
use crate::config::{LoadedConfig, compute_project_config_path, load_project_config};
use crate::modals::{
    ConfigModalState, InitModalState, KanbanBoardState, ToolAllowModalState, WorkersStreamState,
    handle_bead_picker_input, handle_config_modal_input, handle_init_modal_input,
    handle_kanban_input, handle_tool_allow_modal_input, handle_workers_stream_input,
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

/// Merge the current worktree branch to main, clean up, and create a fresh worktree.
/// Epic-aware: within an active epic, skips merge and reuses the worktree.
/// Returns false if the merge failed and the loop should stop.
fn merge_and_refresh_worktree(app: &mut App) -> bool {
    let bd_path = app.config.behavior.bd_path.clone();
    let w = app.selected_worker;

    let has_epic = app.workers[w].claimed_epic_id.is_some();
    let has_children = has_epic
        && app.workers[w]
            .claimed_epic_id
            .as_ref()
            .is_some_and(|eid| has_ready_children(&bd_path, eid));

    match agent::decide_iteration_action(has_epic, has_children) {
        agent::IterationAction::ContinueInEpic => return true,
        agent::IterationAction::CompleteEpicAndMerge => {
            let epic_id = app.workers[w].claimed_epic_id.clone().unwrap();
            let agent_id = app.workers[w].agent_bead_id.clone().unwrap_or_default();

            app.add_text_line(format!("[Completing epic: {}]", epic_id));
            agent::complete_epic(&bd_path, &epic_id);
            app.workers[w].claimed_epic_id = None;

            let _ = std::process::Command::new(&bd_path)
                .args(["set-state", &agent_id, "epic=none"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
        }
        agent::IterationAction::MergeOnly => {}
    }

    // Merge the current worktree to main
    if !app.merge_current_worktree() {
        return false;
    }

    // Clear stale worktree state if path no longer exists on disk
    let w = app.selected_worker;
    if let Some(ref path) = app.workers[w].worktree_path
        && !path.exists()
    {
        app.workers[w].worktree_name = None;
        app.workers[w].worktree_path = None;
    }

    // Worktree will be created after claim_before_start selects a new epic
    true
}

/// Ensure a worktree exists for the current epic.
/// Called after claim_before_start selects an epic, since worktree name = epic_id.
fn ensure_worktree(app: &mut App) -> bool {
    let w = app.selected_worker;
    if app.workers[w].worktree_path.is_some() {
        return true;
    }

    let worktree_name = if let Some(ref epic_id) = app.workers[w].claimed_epic_id {
        epic_id.clone()
    } else if let Some(ref agent_id) = app.workers[w].agent_bead_id {
        agent_id.clone()
    } else {
        return true;
    };

    let bd_path = app.config.behavior.bd_path.clone();
    if let Some((new_name, new_path)) = agent::create_or_reuse_worktree(&bd_path, &worktree_name) {
        app.workers[w].worktree_name = Some(new_name);
        app.workers[w].worktree_path = Some(new_path);
        true
    } else {
        app.add_text_line("[Failed to create worktree — stopping.]".into());
        app.workers[w].reset_iteration_state();
        app.status = AppStatus::Stopped;
        false
    }
}

/// Check if an epic has ready children.
fn has_ready_children(bd_path: &str, epic_id: &str) -> bool {
    let output = std::process::Command::new(bd_path)
        .args(["ready", "--parent", epic_id, "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
                .ok()
                .is_some_and(|items| !items.is_empty())
        }
        _ => false,
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
        Some(Commands::Doctor) => return cli::run_doctor(),
        Some(Commands::Ready { verbose }) => return cli::run_ready(verbose),
        Some(Commands::Logs { id, path }) => return cli::run_logs(id, path),
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
        project_config = ?loaded_config.project_config_path.as_ref().map(|p| p.display().to_string()),
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
    app.validate_board_config();

    // Hint when skill files haven't been scaffolded
    if !std::path::Path::new(".claude/skills/brain-dump/SKILL.md").exists() {
        app.set_hint("Run `ralph init` to get skills for shaping and brain-dumping beads.");
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
            checks.push(doctor::check_bd(cfg));
            let dolt_check = doctor::check_dolt_status(cfg);
            let dolt_running = dolt_check.passed;
            checks.push(dolt_check);
            if dolt_running {
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

    // Register agent beads for all workers (worktrees created on first loop start)
    {
        let bd_path = app.config.behavior.bd_path.clone();
        let heartbeat_interval = app.config.behavior.heartbeat_interval;
        for w in 0..app.workers.len() {
            let sid = if app.workers.len() > 1 {
                format!("{}-w{}", app.session_id, w)
            } else {
                app.session_id.clone()
            };
            if let Some(setup) = agent::register(&bd_path, &sid) {
                let stop = agent::start_heartbeat(
                    bd_path.clone(),
                    setup.agent_bead_id.clone(),
                    heartbeat_interval,
                );
                app.workers[w].agent_bead_id = Some(setup.agent_bead_id);
                app.workers[w].heartbeat_stop = Some(stop);
            } else {
                app.add_text_line(format!(
                    "[Worker {} agent registration failed — running without worktree]",
                    w
                ));
            }
        }
    }

    let result = run_event_loop(&mut app, &mut terminal);

    // Always clean up agent resources for all workers, regardless of how we exited
    for w in 0..app.workers.len() {
        app.workers[w].kill_child();
    }
    app.cleanup_agent();

    result
}

fn run_event_loop(app: &mut App, terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        // Poll for output from child process
        output::poll_output(app);

        // Handle auto-continue for all workers
        for w_idx in 0..app.workers.len() {
            if app.workers[w_idx].auto_continue_pending {
                app.dirty = true;
                app.workers[w_idx].auto_continue_pending = false;
                app.selected_worker = w_idx;

                if !merge_and_refresh_worktree(app) {
                    continue;
                }

                app.workers[w_idx].increment_iteration();
                execution::claim_before_start(app);
                if !ensure_worktree(app) {
                    continue;
                }
                execution::start_command(app)?;
                app.update_derived_status();
            }
        }
        app.selected_worker = 0;

        // Poll for background work source operations
        app.poll_work_check();
        app.poll_kanban_items();
        app.poll_bead_detail();
        app.poll_kanban_watcher();
        app.poll_bead_picker();
        app.poll_pending_dep();

        // Poll for current spec (throttled to every 2 seconds)
        app.poll_bead();

        // Poll for Dolt server state (beads mode only)
        // Process toggle results first so stale status polls don't override
        app.poll_dolt_toggle();
        app.poll_dolt_status();

        // Poll for config file changes (throttled to every 2 seconds)
        app.poll_config();

        // Poll for background doctor check results
        app.poll_doctor();

        // Auto-clear error flash after timeout
        app.check_error_timeout();

        // Auto-clear hint after timeout
        app.check_hint_timeout();

        // Draw UI only when state changed
        if app.dirty {
            terminal.draw(|f| draw_ui(f, app))?;
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

            // Handle bead picker input
            if app.show_bead_picker {
                if let Event::Key(key) = event {
                    handle_bead_picker_input(app, key.code);
                }
                continue;
            }

            // Handle config modal input
            if app.show_config_modal {
                if let Event::Key(key) = event {
                    handle_config_modal_input(app, key.code, key.modifiers);
                }
                continue;
            }

            // Handle kanban board input
            if app.show_kanban_board {
                if let Event::Key(key) = event {
                    handle_kanban_input(app, key.code, key.modifiers);
                }
                continue;
            }

            // Handle init modal input
            if app.show_init_modal {
                if let Event::Key(key) = event {
                    handle_init_modal_input(app, key.code);
                }
                continue;
            }

            // Handle quit confirmation modal input
            if app.show_quit_modal {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
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
                    handle_tool_allow_modal_input(app, key.code, key.modifiers);
                }
                continue;
            }

            // Handle workers stream modal input
            if app.show_workers_stream {
                if let Event::Key(key) = event {
                    handle_workers_stream_input(app, key.code, key.modifiers);
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
                    KeyCode::Char('S') => match app.status {
                        AppStatus::Stopped | AppStatus::Error => {
                            // Start new iteration run for all workers
                            if app.start_iteration_run() {
                                let worker_count = app.workers.len();
                                for w in 0..worker_count {
                                    app.selected_worker = w;
                                    if !merge_and_refresh_worktree(app) {
                                        continue;
                                    }
                                    execution::claim_before_start(app);
                                    if !ensure_worktree(app) {
                                        continue;
                                    }
                                    execution::start_command(app)?;
                                }
                                app.selected_worker = 0;
                                // Set status after starting all workers
                                if app.any_worker_active() {
                                    app.status = AppStatus::Running;
                                }
                            }
                        }
                        AppStatus::Running => {
                            app.stop_command();
                        }
                    },
                    KeyCode::Char('c') => {
                        // Open config modal
                        app.show_config_modal = true;
                        let project_path = app
                            .project_config_path
                            .clone()
                            .or_else(compute_project_config_path);
                        let partial = project_path
                            .as_ref()
                            .filter(|p| p.exists())
                            .and_then(|p| load_project_config(p).ok())
                            .unwrap_or_default();
                        app.config_modal_state = Some(ConfigModalState::from_config(
                            &partial,
                            &app.config,
                            project_path,
                        ));
                    }
                    KeyCode::Char('B') => {
                        if let Some(err) = &app.board_config_error {
                            app.set_hint(err.clone());
                            continue;
                        }
                        let board_config =
                            crate::modals::load_board_config().expect("already validated");
                        let column_defs = board_config.columns.clone();
                        app.show_kanban_board = true;
                        app.kanban_board_state =
                            Some(KanbanBoardState::new_loading(board_config.columns));
                        let bd_path = app.config.behavior.bd_path.clone();
                        let (tx, rx) = mpsc::channel();
                        std::thread::spawn(move || {
                            let result = crate::modals::fetch_board_data(&bd_path, &column_defs);
                            let _ = tx.send(result);
                        });
                        app.kanban_items_rx = Some(rx);

                        // Start filesystem watcher for live board updates
                        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                        let (fs_tx, fs_rx) = mpsc::channel();
                        let stop_clone = std::sync::Arc::clone(&stop);
                        std::thread::spawn(move || {
                            crate::modals::watch_beads_directory(fs_tx, stop_clone);
                        });
                        app.kanban_fs_rx = Some(fs_rx);
                        app.kanban_watcher_stop = Some(stop);
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
                        // Toggle Dolt server
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
                    KeyCode::Char('w') if !app.workers.is_empty() => {
                        app.show_workers_stream = true;
                        app.workers_stream_state =
                            Some(WorkersStreamState::new(app.selected_worker));
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
                        scroll_panel(app, ScrollDirection::Up, 1);
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        scroll_panel(app, ScrollDirection::Down, 1);
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        scroll_panel(app, ScrollDirection::Up, half_page);
                    }
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let half_page = app.main_pane_height / 2;
                        scroll_panel(app, ScrollDirection::Down, half_page);
                    }
                    KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let full_page = app.main_pane_height;
                        scroll_panel(app, ScrollDirection::Up, full_page);
                    }
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let full_page = app.main_pane_height;
                        scroll_panel(app, ScrollDirection::Down, full_page);
                    }
                    _ => {}
                },
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        scroll_panel(app, ScrollDirection::Up, 3);
                    }
                    MouseEventKind::ScrollDown => {
                        scroll_panel(app, ScrollDirection::Down, 3);
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
    fn cli_doctor_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "doctor"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Doctor)));
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
        let command = execution::assemble_prompt(&config, None, None).unwrap();

        // Should pipe prompt and mode content through Claude CLI
        assert!(command.contains("ralph-prompt.md") || command.contains("PROMPT.md"));
        assert!(command.contains("ralph-mode.md"));
        assert!(command.contains("--output-format=stream-json"));
        assert!(command.contains("--print"));
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

    #[test]
    fn cli_logs_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "logs"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Logs {
                id: None,
                path: false
            })
        ));
    }

    #[test]
    fn cli_logs_path_flag_parses() {
        let cli = Cli::try_parse_from(["ralph", "logs", "--path"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Logs {
                id: None,
                path: true
            })
        ));
    }

    #[test]
    fn cli_logs_id_parses() {
        let cli = Cli::try_parse_from(["ralph", "logs", "--id", "abc123"]).unwrap();
        match cli.command {
            Some(Commands::Logs { id, path }) => {
                assert_eq!(id.as_deref(), Some("abc123"));
                assert!(!path);
            }
            _ => panic!("Expected Logs"),
        }
    }
}
