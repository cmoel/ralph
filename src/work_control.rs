//! Auto-continue and iteration control logic.

use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Instant;

use tracing::{info, warn};

use crate::app::{App, AppStatus};
use crate::dolt::DoltServerState;
use crate::work_source::WorkRemaining;

impl App {
    /// Handle a worker's channel disconnecting (process exited).
    /// The worker_idx indicates which worker's process finished.
    pub fn handle_channel_disconnected(&mut self, worker_idx: usize, exit_code: Option<i32>) {
        self.dirty = true;
        self.workers[worker_idx].output_receiver = None;
        self.current_spec = None;
        self.workers[worker_idx].run_start_time = None;
        // Release wake lock when no workers are active
        if !self.any_worker_active() {
            self.wake_lock = None;
        }

        // Merge worktree to main on successful completion (before auto-continue check).
        // In beads mode, skip — worktree persists across children within an epic.
        if exit_code == Some(0) && self.config.behavior.mode != "beads" {
            let prev = self.selected_worker;
            self.selected_worker = worker_idx;
            self.merge_current_worktree();
            self.selected_worker = prev;
        }

        // Determine next state based on exit code and iteration control
        match exit_code {
            Some(0) if self.workers[worker_idx].should_auto_continue() => {
                // In beads mode, skip work check if dolt server is not running
                if self.config.behavior.mode == "beads" && self.dolt.state != DoltServerState::On {
                    self.workers[worker_idx].reset_iteration_state();
                } else {
                    // Kick off background check_remaining (non-blocking)
                    let complete_msg = self.work_source.complete_message();
                    let (tx, rx) = mpsc::channel();
                    let ws = Arc::clone(&self.work_source);
                    std::thread::spawn(move || {
                        let _ = tx.send((ws.check_remaining(), complete_msg));
                    });
                    self.workers[worker_idx].pending_work_check = Some(rx);
                }
            }
            Some(0) => {
                // Countdown exhausted or iterations = 0, just stop
                self.workers[worker_idx].reset_iteration_state();
            }
            Some(code) => {
                // Non-zero exit code → Error state — stop all workers
                self.workers[worker_idx].reset_iteration_state();
                self.add_text_line(format!("[Error: process exited with code {}]", code));
                for w in 0..self.workers.len() {
                    if w != worker_idx && self.workers[w].child_process.is_some() {
                        self.workers[w].kill_child();
                        self.release_worker_hooked_bead(w);
                    }
                }
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
                return;
            }
            None => {
                // Killed by signal (manual stop)
                self.workers[worker_idx].reset_iteration_state();
            }
        }

        self.update_derived_status();
    }

    /// Poll for background check_remaining results (auto-continue decision) for all workers.
    pub fn poll_work_check(&mut self) {
        for w in 0..self.workers.len() {
            let rx = match self.workers[w].pending_work_check.take() {
                Some(rx) => rx,
                None => continue,
            };

            match rx.try_recv() {
                Ok((result, complete_msg)) => {
                    // Discard stale results if we're in Error state
                    if self.status == AppStatus::Error {
                        continue;
                    }
                    self.dirty = true;
                    let prev = self.selected_worker;
                    self.selected_worker = w;
                    self.handle_work_remaining(result, complete_msg);
                    self.selected_worker = prev;
                }
                Err(TryRecvError::Empty) => {
                    self.workers[w].pending_work_check = Some(rx); // still running
                }
                Err(TryRecvError::Disconnected) => {
                    let prev = self.selected_worker;
                    self.selected_worker = w;
                    self.handle_work_remaining(
                        WorkRemaining::ReadError("background check failed".to_string()),
                        self.work_source.complete_message(),
                    );
                    self.selected_worker = prev;
                }
            }
        }
    }

    /// Process a check_remaining result for auto-continue decisions.
    fn handle_work_remaining(&mut self, result: WorkRemaining, complete_msg: &str) {
        let w = self.selected_worker;
        match result {
            WorkRemaining::Yes => {
                info!(
                    current = self.workers[w].current_iteration,
                    total = self.workers[w].total_iterations,
                    "auto_continue"
                );
                self.add_text_line(
                    "══════════════════ AUTO-CONTINUING ══════════════════".to_string(),
                );
                self.workers[w].auto_continue_pending = true;
            }
            WorkRemaining::No => {
                info!("all_work_complete");
                self.add_text_line(format!(
                    "══════════════════ {} ══════════════════",
                    complete_msg
                ));
                self.workers[w].reset_iteration_state();
                self.update_derived_status();
            }
            WorkRemaining::NeedsShaping(count) => {
                info!(count, "all_ready_beads_need_shaping");
                self.add_text_line(
                    "══════════════════ ALL READY BEADS NEED SHAPING ══════════════════"
                        .to_string(),
                );
                self.workers[w].reset_iteration_state();
                self.update_derived_status();
            }
            WorkRemaining::HumanOnly(count) => {
                info!(count, "all_ready_beads_human_only");
                self.add_text_line(format!(
                    "══════════════════ no work for Ralph — {} {} available for humans ══════════════════",
                    count,
                    if count == 1 { "bead" } else { "beads" }
                ));
                self.workers[w].reset_iteration_state();
                self.update_derived_status();
            }
            WorkRemaining::Missing => {
                warn!("work_source_missing");
                self.add_text_line("[Error: work source not found]".to_string());
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
            WorkRemaining::ReadError(e) => {
                warn!(error = %e, "work_source_read_error");
                self.add_text_line(format!("[Error reading work source: {}]", e));
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
        }
    }

    /// Start a new iteration run, reading config and setting up iteration tracking for all workers.
    /// Returns false if iterations = 0 (stopped mode).
    pub fn start_iteration_run(&mut self) -> bool {
        let iterations = self.config.behavior.iterations;
        if iterations == 0 {
            // Stopped mode - don't start
            info!("iterations_zero_no_start");
            return false;
        }

        for worker in &mut self.workers {
            worker.total_iterations = iterations;
            worker.current_iteration = 1;
        }
        true
    }
}
