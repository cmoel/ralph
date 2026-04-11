use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

use tracing::{debug, info};

use crate::agent;
use crate::app::{App, AppStatus};
use crate::config::Config;
use crate::execution;
use crate::output::OutputMessage;
use crate::startup::has_ready_children;
use crate::wake_lock;

struct WorkerStartSnapshot {
    worker_index: usize,
    bd_path: String,
    stale_threshold: u64,
    agent_bead_id: Option<String>,
    claimed_epic_id: Option<String>,
    hooked_bead_id: Option<String>,
    worktree_name: Option<String>,
    worktree_path: Option<PathBuf>,
    has_output: bool,
    config: Config,
}

pub(crate) struct WorkerStartResult {
    pub worker_index: usize,
    pub claimed_epic_id: Option<String>,
    pub hooked_bead_id: Option<String>,
    pub worktree_name: Option<String>,
    pub worktree_path: Option<PathBuf>,
    pub child_process: Option<Child>,
    pub output_receiver: Option<Receiver<OutputMessage>>,
    pub output_lines: Vec<String>,
    pub error: Option<String>,
}

fn run_worker_startups(snapshots: Vec<WorkerStartSnapshot>) -> Vec<WorkerStartResult> {
    snapshots
        .into_iter()
        .map(run_single_worker_startup)
        .collect()
}

fn run_single_worker_startup(snapshot: WorkerStartSnapshot) -> WorkerStartResult {
    let mut result = WorkerStartResult {
        worker_index: snapshot.worker_index,
        claimed_epic_id: snapshot.claimed_epic_id.clone(),
        hooked_bead_id: None,
        worktree_name: snapshot.worktree_name.clone(),
        worktree_path: snapshot.worktree_path.clone(),
        child_process: None,
        output_receiver: None,
        output_lines: Vec::new(),
        error: None,
    };

    if let (Some(agent_id), Some(bead_id)) = (&snapshot.agent_bead_id, &snapshot.hooked_bead_id) {
        agent::release_bead(&snapshot.bd_path, agent_id, bead_id);
    }

    if !merge_and_refresh_bg(&snapshot, &mut result) {
        return result;
    }

    claim_before_start_bg(&snapshot, &mut result);

    if !ensure_worktree_bg(&snapshot, &mut result) {
        return result;
    }

    start_command_bg(&snapshot, &mut result);

    result
}

fn merge_and_refresh_bg(snapshot: &WorkerStartSnapshot, result: &mut WorkerStartResult) -> bool {
    let bd_path = &snapshot.bd_path;

    let has_epic = result.claimed_epic_id.is_some();
    let has_children = has_epic
        && result
            .claimed_epic_id
            .as_ref()
            .is_some_and(|eid| has_ready_children(bd_path, eid));

    match agent::decide_iteration_action(has_epic, has_children) {
        agent::IterationAction::ContinueInEpic => return true,
        agent::IterationAction::CompleteEpicAndMerge => {
            let epic_id = result.claimed_epic_id.clone().unwrap();
            let agent_id = snapshot.agent_bead_id.clone().unwrap_or_default();
            result
                .output_lines
                .push(format!("[Completing epic: {}]", epic_id));
            agent::complete_epic(bd_path, &epic_id);
            result.claimed_epic_id = None;
            let _ = Command::new(bd_path)
                .args(["set-state", &agent_id, "epic=none"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .output();
        }
        agent::IterationAction::MergeOnly => {}
    }

    if let Some(wt_name) = result.worktree_name.clone() {
        if agent::merge_worktree_to_main(&wt_name) {
            agent::remove_merged_worktree(bd_path, &wt_name);
            result.worktree_name = None;
            result.worktree_path = None;
        } else if let Some(existing_bead_id) = agent::find_merge_conflict_bead(bd_path, &wt_name) {
            agent::escalate_merge_conflict(bd_path, &wt_name, &existing_bead_id);
            result.output_lines.push(
                "[Merge conflict persists after Claude attempt — filed human bead, stopping]"
                    .to_string(),
            );
            result.error = Some("Merge conflict".to_string());
            return false;
        } else if let Some(bead_id) = agent::file_merge_conflict_bead(bd_path, &wt_name) {
            result.output_lines.push(format!(
                "[Merge conflict — filed {}, Claude will resolve next iteration]",
                bead_id
            ));
        } else {
            result
                .output_lines
                .push("[Merge conflict — failed to file bead, stopping]".to_string());
            result.error = Some("Merge conflict".to_string());
            return false;
        }
    }

    if let Some(ref path) = result.worktree_path
        && !path.exists()
    {
        result.worktree_name = None;
        result.worktree_path = None;
    }

    true
}

fn claim_before_start_bg(snapshot: &WorkerStartSnapshot, result: &mut WorkerStartResult) {
    let agent_id = match &snapshot.agent_bead_id {
        Some(id) => id.clone(),
        None => return,
    };

    let bd_path = &snapshot.bd_path;

    let stale_agents = agent::find_stale_agents(bd_path, snapshot.stale_threshold, Some(&agent_id));
    if !stale_agents.is_empty() {
        let first = &stale_agents[0];
        match agent::resume_stale_bead(bd_path, &agent_id, first) {
            agent::ResumeResult::Resumed => {
                result.output_lines.push(format!(
                    "[Auto-reclaimed: {} \"{}\"]",
                    first.hooked_bead_id, first.hooked_bead_title
                ));
                result.hooked_bead_id = Some(first.hooked_bead_id.clone());
                for stale in stale_agents.iter().skip(1) {
                    agent::release_stale_bead(bd_path, stale);
                    result.output_lines.push(format!(
                        "[Released stale: {} \"{}\"]",
                        stale.hooked_bead_id, stale.hooked_bead_title
                    ));
                }
                return;
            }
            agent::ResumeResult::EscalatedToHuman => {
                result.output_lines.push(format!(
                    "[Escalated to human: {} \"{}\" — stuck twice]",
                    first.hooked_bead_id, first.hooked_bead_title
                ));
                for stale in stale_agents.iter().skip(1) {
                    agent::release_stale_bead(bd_path, stale);
                    result.output_lines.push(format!(
                        "[Released stale: {} \"{}\"]",
                        stale.hooked_bead_id, stale.hooked_bead_title
                    ));
                }
            }
            agent::ResumeResult::Failed => {}
        }
    }

    if let Some(ref epic_id) = result.claimed_epic_id {
        let epic_id = epic_id.clone();
        match agent::claim_next_child(bd_path, &agent_id, &epic_id) {
            Some((child_id, child_title)) => {
                result.output_lines.push(format!(
                    "[Claimed child: {} {} (epic: {})]",
                    child_id, child_title, epic_id
                ));
                result.hooked_bead_id = Some(child_id);
                return;
            }
            None => {
                result.output_lines.push(format!(
                    "[Epic {} has no more ready children — starting claimless]",
                    epic_id
                ));
                return;
            }
        }
    }

    match agent::select_and_claim_epic(bd_path, &agent_id) {
        Some(claim) => {
            result.output_lines.push(format!(
                "[Claimed epic: {} → child: {} {}]",
                claim.epic_id, claim.child_bead_id, claim.child_title
            ));
            result.claimed_epic_id = Some(claim.epic_id);
            result.hooked_bead_id = Some(claim.child_bead_id);
        }
        None => {
            result
                .output_lines
                .push("[No beads available to claim — starting claimless]".to_string());
        }
    }
}

fn ensure_worktree_bg(snapshot: &WorkerStartSnapshot, result: &mut WorkerStartResult) -> bool {
    if result.worktree_path.is_some() {
        return true;
    }

    let worktree_name = if let Some(ref epic_id) = result.claimed_epic_id {
        epic_id.clone()
    } else if let Some(ref agent_id) = snapshot.agent_bead_id {
        agent_id.clone()
    } else {
        return true;
    };

    let bd_path = &snapshot.bd_path;
    if let Some((new_name, new_path)) = agent::create_or_reuse_worktree(bd_path, &worktree_name) {
        result.worktree_name = Some(new_name);
        result.worktree_path = Some(new_path);
        true
    } else {
        result
            .output_lines
            .push("[Failed to create worktree — stopping.]".to_string());
        result.error = Some("Failed to create worktree".to_string());
        false
    }
}

fn start_command_bg(snapshot: &WorkerStartSnapshot, result: &mut WorkerStartResult) {
    if snapshot.has_output {
        result.output_lines.push("─".repeat(40));
    }

    let dirty_context = result
        .worktree_path
        .as_deref()
        .and_then(agent::check_worktree_dirty)
        .map(|(status, diff)| agent::build_dirty_worktree_context(&status, &diff));

    let command = match execution::assemble_prompt(
        &snapshot.config,
        result.hooked_bead_id.as_deref(),
        dirty_context,
    ) {
        Ok(cmd) => cmd,
        Err(e) => {
            result.error = Some(format!("Error assembling prompt: {}", e));
            return;
        }
    };

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ref wt_path) = result.worktree_path {
        cmd.current_dir(wt_path);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            debug!(pid = child.id(), "command_spawned");

            let (tx, rx) = mpsc::channel();

            if let Some(stdout) = child.stdout.take() {
                let tx_stdout = tx.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().map_while(Result::ok) {
                        if tx_stdout.send(OutputMessage::Line(line)).is_err() {
                            break;
                        }
                    }
                });
            }

            if let Some(stderr) = child.stderr.take() {
                let tx_stderr = tx.clone();
                std::thread::spawn(move || {
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

            result.child_process = Some(child);
            result.output_receiver = Some(rx);
        }
        Err(e) => {
            result.error = Some(format!("Error starting command: {}", e));
        }
    }
}

impl App {
    pub fn begin_starting_workers(&mut self) {
        if self.status == AppStatus::Starting || self.status == AppStatus::Running {
            return;
        }

        if !self.start_iteration_run() {
            return;
        }

        let snapshots: Vec<_> = (0..self.workers.len())
            .map(|w| WorkerStartSnapshot {
                worker_index: w,
                bd_path: self.config.behavior.bd_path.clone(),
                stale_threshold: self.config.behavior.stale_threshold,
                agent_bead_id: self.workers[w].agent_bead_id.clone(),
                claimed_epic_id: self.workers[w].claimed_epic_id.clone(),
                hooked_bead_id: self.workers[w].hooked_bead_id.clone(),
                worktree_name: self.workers[w].worktree_name.clone(),
                worktree_path: self.workers[w].worktree_path.clone(),
                has_output: !self.workers[w].output_lines.is_empty(),
                config: self.config.clone(),
            })
            .collect();

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let results = run_worker_startups(snapshots);
            let _ = tx.send(results);
        });

        self.start_workers_rx = Some(rx);
        self.status = AppStatus::Starting;
    }

    pub fn poll_worker_start(&mut self) {
        let rx = match self.start_workers_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(results) => {
                self.dirty = true;
                let mut any_started = false;

                for result in results {
                    let w = result.worker_index;
                    self.selected_worker = w;

                    for line in result.output_lines {
                        self.add_text_line(line);
                    }

                    self.workers[w].claimed_epic_id = result.claimed_epic_id;
                    self.workers[w].hooked_bead_id = result.hooked_bead_id;
                    self.workers[w].worktree_name = result.worktree_name;
                    self.workers[w].worktree_path = result.worktree_path;

                    if result.error.is_some() {
                        self.workers[w].reset_iteration_state();
                    } else if result.child_process.is_some() {
                        self.workers[w].content_blocks.clear();
                        self.workers[w].current_line.clear();
                        self.workers[w].child_process = result.child_process;
                        self.workers[w].output_receiver = result.output_receiver;
                        self.workers[w].run_start_time = Some(Instant::now());
                        self.loop_count += 1;
                        info!(loop_number = self.loop_count, "loop_start");
                        any_started = true;
                    } else {
                        self.workers[w].reset_iteration_state();
                    }
                }
                self.selected_worker = 0;

                if any_started {
                    if self.config.behavior.keep_awake {
                        self.wake_lock = wake_lock::acquire();
                        if self.wake_lock.is_none() {
                            self.add_text_line(
                                "⚠ Warning: Could not acquire wake lock - system may sleep during execution"
                                    .to_string(),
                            );
                        }
                    }
                    self.status = AppStatus::Running;
                } else {
                    self.status = AppStatus::Stopped;
                }
            }
            Err(TryRecvError::Empty) => {
                self.start_workers_rx = Some(rx);
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                self.add_text_line(
                    "[Worker startup failed — background thread crashed]".to_string(),
                );
                for worker in &mut self.workers {
                    worker.reset_iteration_state();
                }
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_workers(n: u32) -> App {
        use crate::config::LoadedConfig;
        let mut loaded = LoadedConfig::default_for_test();
        loaded.config.behavior.workers = n.max(1);
        App::new("test".into(), None, loaded, None)
    }

    #[test]
    fn starting_to_running_on_successful_start() {
        let mut app = app_with_workers(1);

        let child = Command::new("sleep").arg("60").spawn().unwrap();
        let (_output_tx, output_rx) = mpsc::channel::<OutputMessage>();

        let result = WorkerStartResult {
            worker_index: 0,
            claimed_epic_id: Some("epic-1".to_string()),
            hooked_bead_id: Some("bead-1".to_string()),
            worktree_name: Some("wt-1".to_string()),
            worktree_path: Some(PathBuf::from("/tmp/wt-1")),
            child_process: Some(child),
            output_receiver: Some(output_rx),
            output_lines: vec!["[Claimed epic: epic-1]".to_string()],
            error: None,
        };

        let (tx, rx) = mpsc::channel();
        tx.send(vec![result]).unwrap();

        app.status = AppStatus::Starting;
        app.start_workers_rx = Some(rx);

        app.poll_worker_start();

        assert_eq!(app.status, AppStatus::Running);
        assert!(app.workers[0].run_start_time.is_some());
        assert_eq!(app.workers[0].claimed_epic_id.as_deref(), Some("epic-1"));
        assert_eq!(app.workers[0].hooked_bead_id.as_deref(), Some("bead-1"));
        assert!(app.workers[0].child_process.is_some());

        app.workers[0].kill_child();
    }

    #[test]
    fn starting_to_stopped_on_error() {
        let mut app = app_with_workers(1);
        app.workers[0].total_iterations = 5;
        app.workers[0].current_iteration = 1;

        let result = WorkerStartResult {
            worker_index: 0,
            claimed_epic_id: None,
            hooked_bead_id: None,
            worktree_name: None,
            worktree_path: None,
            child_process: None,
            output_receiver: None,
            output_lines: vec!["[Merge conflict]".to_string()],
            error: Some("Merge conflict".to_string()),
        };

        let (tx, rx) = mpsc::channel();
        tx.send(vec![result]).unwrap();

        app.status = AppStatus::Starting;
        app.start_workers_rx = Some(rx);

        app.poll_worker_start();

        assert_eq!(app.status, AppStatus::Stopped);
        assert_eq!(app.workers[0].current_iteration, 0);
        assert_eq!(app.workers[0].total_iterations, 0);
    }

    #[test]
    fn starting_to_error_on_thread_crash() {
        let mut app = app_with_workers(1);

        let (tx, rx) = mpsc::channel::<Vec<WorkerStartResult>>();
        drop(tx);

        app.status = AppStatus::Starting;
        app.start_workers_rx = Some(rx);

        app.poll_worker_start();

        assert_eq!(app.status, AppStatus::Error);
    }

    #[test]
    fn stopwatch_not_set_while_starting() {
        let mut app = app_with_workers(1);
        app.status = AppStatus::Starting;

        assert!(app.workers[0].run_start_time.is_none());
    }

    #[test]
    fn s_keybinding_ignored_during_starting() {
        let mut app = app_with_workers(1);
        app.status = AppStatus::Starting;
        app.config.behavior.iterations = 5;

        app.begin_starting_workers();

        assert_eq!(app.status, AppStatus::Starting);
        assert!(app.start_workers_rx.is_none());
    }
}
