//! Auto-continue and iteration control logic.

use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Instant;

use tracing::{info, warn};

use crate::app::{App, AppStatus};
use crate::dolt::DoltServerState;
use crate::work_source::WorkRemaining;

impl App {
    /// Handle poll_output logic, returning whether auto-continue should be pending.
    pub fn handle_channel_disconnected(&mut self, exit_code: Option<i32>) {
        self.dirty = true;
        let w = self.selected_worker;
        self.workers[w].output_receiver = None;
        self.current_spec = None;
        self.workers[w].run_start_time = None;
        // Release wake lock when process ends (drop releases it)
        self.wake_lock = None;

        // Merge worktree to main on successful completion (before auto-continue check).
        // In beads mode, skip — worktree persists across children within an epic.
        if exit_code == Some(0) && self.config.behavior.mode != "beads" {
            self.merge_current_worktree();
        }

        // Determine next state based on exit code and iteration control
        match exit_code {
            Some(0) if self.workers[self.selected_worker].should_auto_continue() => {
                // In beads mode, skip work check if dolt server is not running
                if self.config.behavior.mode == "beads" && self.dolt.state != DoltServerState::On {
                    self.workers[self.selected_worker].reset_iteration_state();
                    self.status = AppStatus::Stopped;
                } else {
                    // Kick off background check_remaining (non-blocking)
                    let complete_msg = self.work_source.complete_message();
                    let (tx, rx) = mpsc::channel();
                    let ws = Arc::clone(&self.work_source);
                    std::thread::spawn(move || {
                        let _ = tx.send((ws.check_remaining(), complete_msg));
                    });
                    self.workers[self.selected_worker].pending_work_check = Some(rx);
                    self.status = AppStatus::Stopped;
                }
            }
            Some(0) => {
                // Countdown exhausted or iterations = 0, just stop
                self.workers[self.selected_worker].reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
            Some(code) => {
                // Non-zero exit code → Error state
                self.workers[self.selected_worker].reset_iteration_state();
                self.add_text_line(format!("[Error: process exited with code {}]", code));
                self.status = AppStatus::Error;
                self.error_at = Some(Instant::now());
            }
            None => {
                // Killed by signal (manual stop) → Stopped state
                self.workers[self.selected_worker].reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
        }
    }

    /// Poll for background check_remaining result (auto-continue decision).
    pub fn poll_work_check(&mut self) {
        let w = self.selected_worker;
        let rx = match self.workers[w].pending_work_check.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok((result, complete_msg)) => {
                // Discard stale results if we're no longer in Stopped state
                if self.status != AppStatus::Stopped {
                    return;
                }
                self.dirty = true;
                self.handle_work_remaining(result, complete_msg);
            }
            Err(TryRecvError::Empty) => {
                self.workers[w].pending_work_check = Some(rx); // still running
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
                // Don't clear tasks during auto-continue
            }
            WorkRemaining::No => {
                info!("all_work_complete");
                self.add_text_line(format!(
                    "══════════════════ {} ══════════════════",
                    complete_msg
                ));
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
            WorkRemaining::NeedsShaping(count) => {
                info!(count, "all_ready_beads_need_shaping");
                self.add_text_line(
                    "══════════════════ ALL READY BEADS NEED SHAPING ══════════════════"
                        .to_string(),
                );
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Stopped;
            }
            WorkRemaining::HumanOnly(count) => {
                info!(count, "all_ready_beads_human_only");
                self.add_text_line(format!(
                    "══════════════════ no work for Ralph — {} {} available for humans ══════════════════",
                    count,
                    if count == 1 { "bead" } else { "beads" }
                ));
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Stopped;
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

    /// Start a new iteration run, reading config and setting up iteration tracking.
    /// Returns false if iterations = 0 (stopped mode).
    pub fn start_iteration_run(&mut self) -> bool {
        let iterations = self.config.behavior.iterations;
        if iterations == 0 {
            // Stopped mode - don't start
            info!("iterations_zero_no_start");
            return false;
        }

        let w = self.selected_worker;
        self.workers[w].total_iterations = iterations;
        self.workers[w].current_iteration = 1;
        true
    }
}
