use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::Result;
use ratatui::DefaultTerminal;
use tracing::warn;

use crate::agent;
use crate::app::{App, AppStatus};
use crate::config::LoadedConfig;
use crate::db;
use crate::doctor;
use crate::event_loop::run_event_loop;
use crate::logging::ReloadHandle;
use crate::modals;

/// Merge the current worktree branch to main, clean up, and create a fresh worktree.
/// Epic-aware: within an active epic, skips merge and reuses the worktree.
/// Returns false if the merge failed and the loop should stop.
pub(crate) fn merge_and_refresh_worktree(app: &mut App) -> bool {
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
pub(crate) fn ensure_worktree(app: &mut App) -> bool {
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

pub(crate) fn run_app(
    mut terminal: DefaultTerminal,
    session_id: String,
    log_directory: Option<PathBuf>,
    loaded_config: LoadedConfig,
    log_level_handle: Option<Arc<Mutex<ReloadHandle>>>,
) -> Result<()> {
    let loaded_for_doctor = loaded_config.clone();
    let mut app = App::new(session_id, log_directory, loaded_config, log_level_handle);
    app.validate_board_config();

    // Hint when skill files are missing or drifted from compiled-in templates
    let init_state = modals::InitModalState::new(&loaded_for_doctor.config);
    if let Some(msg) = init_state.hint_message() {
        app.set_hint(msg);
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
            checks.push(doctor::check_scaffolding_drift(cfg));
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

    // Start board data fetch and filesystem watcher
    if app.board_config_error.is_none() {
        let bd_path = app.config.behavior.bd_path.clone();
        let column_defs = app.kanban_board_state.column_defs.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = modals::fetch_board_data(&bd_path, &column_defs);
            let _ = tx.send(result);
        });
        app.kanban_items_rx = Some(rx);

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (fs_tx, fs_rx) = mpsc::channel();
        let stop_clone = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || {
            modals::watch_beads_directory(fs_tx, stop_clone);
        });
        app.kanban_fs_rx = Some(fs_rx);
        app.kanban_watcher_stop = Some(stop);
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

    // Always clean up resources, regardless of how we exited
    app.stop_kanban_watcher();
    for w in 0..app.workers.len() {
        app.workers[w].kill_child();
    }
    app.cleanup_agent();

    result
}
