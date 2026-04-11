use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::config::{get_project_config_path, reload_config};
use crate::logging;
use crate::startup::get_file_mtime;
use crate::work_source::BeadsWorkSource;

use super::state::{App, AppStatus};

/// How often to probe the beads DB for mutations.
const BOARD_MUTATION_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Compute a compact byte signature of the current board state that changes
/// on every mutation that bumps `updated_at`, plus creates and deletes.
///
/// Signal = bytes(`bd count --json`) ++ bytes(`bd list --all --sort updated -n 1 --json --flat`).
/// Total wall time ~0.9s on a warm cache, <1KB payload. Returns `None` if
/// either bd call fails — caller should skip the tick and retry next interval.
fn compute_board_signature(bd_path: &str) -> Option<Vec<u8>> {
    crate::perf::record_subprocess_spawn();
    let count_out = crate::bd_lock::with_lock(|| {
        std::process::Command::new(bd_path)
            .args(["count", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .ok()?;
    if !count_out.status.success() {
        return None;
    }

    crate::perf::record_subprocess_spawn();
    let newest_out = crate::bd_lock::with_lock(|| {
        std::process::Command::new(bd_path)
            .args([
                "list", "--all", "--sort", "updated", "-n", "1", "--json", "--flat",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .ok()?;
    if !newest_out.status.success() {
        return None;
    }

    let mut sig = count_out.stdout;
    sig.extend_from_slice(&newest_out.stdout);
    Some(sig)
}

impl App {
    pub fn poll_bead(&mut self) {
        // Check for completed background detect_current
        if let Some(rx) = self.bead_poll_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.current_bead = result;
                    self.dirty = true;
                }
                Err(TryRecvError::Empty) => {
                    self.bead_poll_rx = Some(rx); // still running
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    // Thread finished without sending (shouldn't happen)
                }
            }
        }

        // Only kick new poll when running
        if self.status != AppStatus::Running {
            return;
        }

        // Throttle: poll every 2 seconds
        if self.last_bead_poll.elapsed() < Duration::from_secs(2) {
            return;
        }

        self.last_bead_poll = Instant::now();

        // Kick off background detect_current
        let (tx, rx) = mpsc::channel();
        let ws = Arc::clone(&self.work_source);
        std::thread::spawn(move || {
            let _ = tx.send(ws.detect_current());
        });
        self.bead_poll_rx = Some(rx);
    }

    pub fn poll_config(&mut self) {
        // Throttle: poll every 2 seconds
        if self.last_config_poll.elapsed() < Duration::from_secs(2) {
            return;
        }

        self.last_config_poll = Instant::now();

        // Check project config mtime (also detect new project config appearing)
        let project_path = self
            .project_config_path
            .clone()
            .or_else(get_project_config_path);
        let project_mtime = project_path.as_ref().and_then(|p| get_file_mtime(p));
        let project_changed = match (project_mtime, self.project_config_mtime) {
            (Some(current), Some(prev)) => current != prev,
            (Some(_), None) => true, // project config appeared
            (None, Some(_)) => true, // project config disappeared
            (None, None) => false,
        };

        if !project_changed {
            return;
        }

        self.dirty = true;

        self.project_config_mtime = project_mtime;
        // Update project path (may have appeared or disappeared)
        self.project_config_path = project_path;

        let reloaded = reload_config(self.project_config_path.as_ref());

        // Check if log level changed and update if we have a reload handle
        let new_log_level = &reloaded.config.logging.level;
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
                }
            }
        }

        // Reconstruct work source if bd_path changed
        let new_bd_path = &reloaded.config.behavior.bd_path;
        if new_bd_path != &self.config.behavior.bd_path {
            self.work_source = Arc::new(BeadsWorkSource::new(new_bd_path.clone()));
            self.clear_pending_work_ops();
        }

        // Reshape worker pool if workers changed
        let new_workers = reloaded.config.behavior.workers;
        let old_workers = self.config.behavior.workers;
        if new_workers != old_workers {
            if self.status != AppStatus::Running {
                self.reshape_workers_to(new_workers as usize);
            } else {
                info!(
                    old = old_workers,
                    new = new_workers,
                    "workers_change_deferred_running"
                );
            }
        }

        self.config = reloaded.config;
        self.project_config_error = reloaded.project_error;

        if self.project_config_error.is_none() {
            self.config_reloaded_at = Some(Instant::now());
        }
    }

    /// Poll for background kanban board data — drains all pending streamed
    /// messages in one tick, applying column updates and the finalize message
    /// as they arrive.
    pub fn poll_kanban_items(&mut self) {
        use crate::modals::KanbanFetchMsg;

        let rx = match self.kanban_items_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        let mut received_any = false;
        let mut disconnected = false;

        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    received_any = true;
                    match msg {
                        KanbanFetchMsg::Column { col_idx, update } => {
                            self.kanban_board_state.populate_column(col_idx, update);
                        }
                        KanbanFetchMsg::Finalized(finalized) => {
                            self.kanban_board_state.populate_finalized(finalized);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }

        if received_any {
            self.dirty = true;
        }

        // Keep the receiver alive while the fetcher might still send more.
        if !disconnected {
            self.kanban_items_rx = Some(rx);
        }
    }

    /// Poll for background bead detail data (preview pane).
    pub fn poll_bead_detail(&mut self) {
        let rx = match self.bead_detail_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                // Absorb transient external-lock contention: an external bd
                // process (e.g. `bd list` from another shell) held the
                // embedded-Dolt file lock while we tried to fetch the bead
                // detail. Keep the existing preview visible and let the next
                // cursor movement or refresh retry — much better UX than
                // flashing bd's JSON error blob into the preview pane.
                if let Err(ref e) = result
                    && crate::bd_lock::is_transient_lock_error(e.as_bytes())
                {
                    return;
                }
                self.dirty = true;
                if let Some(ref mut detail) = self.kanban_board_state.preview_detail {
                    detail.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.bead_detail_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                if let Some(ref mut detail) = self.kanban_board_state.preview_detail {
                    detail.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Poll for debounced preview fetch — fires after cursor stops moving for 150ms.
    pub fn poll_preview_fetch(&mut self) {
        let state = &mut self.kanban_board_state;
        let debounce = std::time::Duration::from_millis(150);

        if let Some(moved_at) = state.preview_cursor_moved
            && moved_at.elapsed() >= debounce
            && let Some(pending_id) = state.preview_pending_id.take()
        {
            state.preview_cursor_moved = None;
            state.preview_bead_id = Some(pending_id.clone());
            match state.preview_detail.as_mut() {
                Some(detail) if detail.id == pending_id => {
                    detail.is_loading = true;
                }
                _ => {
                    state.preview_detail = Some(crate::modals::BeadDetailState::new_loading(
                        pending_id.clone(),
                    ));
                }
            }

            let bd_path = self.config.behavior.bd_path.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                crate::perf::record_subprocess_spawn();
                let output = crate::bd_lock::with_lock(|| {
                    std::process::Command::new(&bd_path)
                        .args(["show", &pending_id, "--json"])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .output()
                });
                let result = match output {
                    Ok(out) if out.status.success() => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        serde_json::from_str::<serde_json::Value>(&stdout)
                            .map(|val| {
                                if let Some(arr) = val.as_array() {
                                    arr.first().cloned().unwrap_or(val)
                                } else {
                                    val
                                }
                            })
                            .map_err(|e| e.to_string())
                    }
                    Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
                    Err(e) => Err(e.to_string()),
                };
                let _ = tx.send(result);
            });
            self.bead_detail_rx = Some(rx);
            self.dirty = true;
        }
    }

    /// Kick off a streaming board refresh if one isn't already in flight.
    /// Called on startup, on the `r` keybinding, and indirectly by
    /// [`App::mutate_and_refresh_kanban`] after every user mutation.
    pub fn trigger_kanban_refresh(&mut self) {
        if self.kanban_items_rx.is_some() {
            return;
        }
        let bd_path = self.config.behavior.bd_path.clone();
        let column_defs = self.kanban_board_state.column_defs.clone();
        self.kanban_board_state.begin_refresh();
        self.dirty = true;
        // Invalidate the mutation signature so the next poll re-establishes a
        // fresh baseline against the newly-fetched state.
        self.last_board_signature = None;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            crate::modals::stream_board_data(&bd_path, &column_defs, tx);
        });
        self.kanban_items_rx = Some(rx);

        // If the preview pane is showing a bead, refetch its detail in parallel.
        self.refresh_preview_detail();
    }

    /// Run a bd mutation (with the global bd_lock) then immediately kick off a
    /// board refresh on the SAME background thread. Chaining mutation → refresh
    /// in one thread guarantees the refresh observes post-mutation state —
    /// there's no race where a concurrent refresh could start before the
    /// mutation lands.
    ///
    /// If a refresh is already in flight, the old receiver is replaced; the
    /// old stream thread will notice on its next send and abort.
    pub fn mutate_and_refresh_kanban(&mut self, args: Vec<String>) {
        let bd_path = self.config.behavior.bd_path.clone();
        let column_defs = self.kanban_board_state.column_defs.clone();
        self.kanban_board_state.begin_refresh();
        self.dirty = true;
        // Invalidate the mutation signature — the mutation we're about to run
        // will shift it, and we want the next poll to re-baseline from the
        // post-mutation state instead of triggering a redundant refresh.
        self.last_board_signature = None;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            crate::perf::record_subprocess_spawn();
            let _ = crate::bd_lock::with_lock(|| {
                std::process::Command::new(&bd_path)
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
            });
            crate::modals::stream_board_data(&bd_path, &column_defs, tx);
        });
        self.kanban_items_rx = Some(rx);
        self.refresh_preview_detail();
    }

    /// Refetch the currently-previewed bead's detail JSON, if a preview pane
    /// is showing one and isn't already loading.
    fn refresh_preview_detail(&mut self) {
        if self.bead_detail_rx.is_some() {
            return;
        }
        let Some(ref detail) = self.kanban_board_state.preview_detail else {
            return;
        };
        if detail.is_loading {
            return;
        }
        let bd_path = self.config.behavior.bd_path.clone();
        let bead_id = detail.id.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            crate::perf::record_subprocess_spawn();
            let output = crate::bd_lock::with_lock(|| {
                std::process::Command::new(&bd_path)
                    .args(["show", &bead_id, "--json"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .output()
            });
            let result = match output {
                Ok(out) if out.status.success() => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    serde_json::from_str::<serde_json::Value>(&stdout)
                        .map(|val| {
                            if let Some(arr) = val.as_array() {
                                arr.first().cloned().unwrap_or(val)
                            } else {
                                val
                            }
                        })
                        .map_err(|e| e.to_string())
                }
                Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(result);
        });
        self.bead_detail_rx = Some(rx);
    }

    /// Check the beads DB for mutations on a slow timer and trigger a full
    /// refresh if anything changed. Drains the pending signature probe (if
    /// any), then kicks off a new probe once per [`BOARD_MUTATION_POLL_INTERVAL`].
    ///
    /// Signal: bytes of `bd count --json` concatenated with
    /// `bd list --all --sort updated -n 1 --json --flat`. Catches every field
    /// mutation that bumps `updated_at` plus creates, deletes, and dep
    /// add/remove (bd rewrites the newest bead's JSON when its dep graph
    /// changes, even though `updated_at` itself isn't touched — verified
    /// empirically against a pair of real beads).
    pub fn poll_board_mutations(&mut self) {
        if let Some(rx) = self.board_signature_rx.take() {
            match rx.try_recv() {
                Ok(Some(new_sig)) => {
                    let changed = self
                        .last_board_signature
                        .as_ref()
                        .is_some_and(|prev| *prev != new_sig);
                    self.last_board_signature = Some(new_sig);
                    if changed {
                        self.trigger_kanban_refresh();
                    }
                }
                Ok(None) => {
                    // Probe failed (lock contention, bd error, etc.); skip
                    // this tick and retry on the next interval.
                }
                Err(TryRecvError::Empty) => {
                    self.board_signature_rx = Some(rx);
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    // Thread died without sending — treat as a failed probe.
                }
            }
        }

        let ready_to_poll = self
            .last_board_signature_check_at
            .is_none_or(|t| t.elapsed() >= BOARD_MUTATION_POLL_INTERVAL);
        if !ready_to_poll {
            return;
        }

        self.last_board_signature_check_at = Some(Instant::now());
        let bd_path = self.config.behavior.bd_path.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(compute_board_signature(&bd_path));
        });
        self.board_signature_rx = Some(rx);
    }

    /// Poll for background bead picker data.
    pub fn poll_bead_picker(&mut self) {
        let rx = match self.bead_picker_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                self.dirty = true;
                if let Some(ref mut state) = self.bead_picker_state {
                    state.populate(result);
                }
            }
            Err(TryRecvError::Empty) => {
                self.bead_picker_rx = Some(rx); // still running
            }
            Err(TryRecvError::Disconnected) => {
                self.dirty = true;
                if let Some(ref mut state) = self.bead_picker_state {
                    state.populate(Err("Background fetch failed".to_string()));
                }
            }
        }
    }

    /// Open the bead picker modal and start background data fetch.
    pub fn open_bead_picker(&mut self) {
        self.show_bead_picker = true;
        self.bead_picker_state = Some(crate::modals::BeadPickerState::new_loading());
        self.bead_picker_result = None;
        let bd_path = self.config.behavior.bd_path.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::modals::fetch_bead_picker_data(&bd_path);
            let _ = tx.send(result);
        });
        self.bead_picker_rx = Some(rx);
    }

    /// If a bead was picked and a dependency is pending, run `bd dep add`.
    pub fn poll_pending_dep(&mut self) {
        let picked_id = match self.bead_picker_result.take() {
            Some(id) => id,
            None => return,
        };
        let dep = match self.pending_dep.take() {
            Some(d) => d,
            None => return,
        };
        let (issue, depends_on) = match dep.direction {
            crate::modals::DepDirection::BlockedBy => (dep.bead_id, picked_id),
            crate::modals::DepDirection::Blocks => (picked_id, dep.bead_id),
        };
        self.kanban_board_state
            .push_action(crate::modals::BoardAction::AddDependency {
                issue: issue.clone(),
                depends_on: depends_on.clone(),
            });
        self.mutate_and_refresh_kanban(vec!["dep".into(), "add".into(), issue, depends_on]);
    }

    /// Clear all pending background work source operations.
    fn clear_pending_work_ops(&mut self) {
        self.bead_poll_rx = None;
        let w = self.selected_worker;
        self.workers[w].pending_work_check = None;
    }

    /// Poll for background doctor check results. Displays only failures.
    pub fn poll_doctor(&mut self) {
        let rx = match self.doctor_rx.take() {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(checks) => {
                for check in &checks {
                    if !check.passed {
                        self.dirty = true;
                        self.add_text_line(format!("\u{2717} {}", check.message));
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                self.doctor_rx = Some(rx);
            }
            Err(TryRecvError::Disconnected) => {}
        }
    }

    /// Merge the current worktree branch to main and clean up.
    /// Returns true if merge succeeded (or no worktree to merge).
    /// Returns false if a merge conflict stopped the loop.
    pub fn merge_current_worktree(&mut self) -> bool {
        let w = self.selected_worker;
        let Some(ref wt_name) = self.workers[w].worktree_name else {
            return true;
        };

        if crate::agent::merge_worktree_to_main(wt_name) {
            let bd_path = self.config.behavior.bd_path.clone();
            let wt_name = wt_name.clone();
            crate::agent::remove_merged_worktree(&bd_path, &wt_name);
            self.workers[w].worktree_name = None;
            self.workers[w].worktree_path = None;
            true
        } else {
            let bd_path = self.config.behavior.bd_path.clone();
            let wt_name = wt_name.clone();

            if let Some(existing_bead_id) =
                crate::agent::find_merge_conflict_bead(&bd_path, &wt_name)
            {
                // Tier 2: Claude already tried — escalate to human
                crate::agent::escalate_merge_conflict(&bd_path, &wt_name, &existing_bead_id);
                self.add_text_line(
                    "[Merge conflict persists after Claude attempt — filed human bead, stopping]"
                        .into(),
                );
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Stopped;
                false
            } else if let Some(bead_id) = crate::agent::file_merge_conflict_bead(&bd_path, &wt_name)
            {
                // Tier 1: First conflict — file bead, Claude resolves next iteration
                self.add_text_line(format!(
                    "[Merge conflict — filed {}, Claude will resolve next iteration]",
                    bead_id
                ));
                // Worktree preserved
                true
            } else {
                self.add_text_line("[Merge conflict — failed to file bead, stopping]".into());
                self.workers[w].reset_iteration_state();
                self.status = AppStatus::Stopped;
                false
            }
        }
    }

    /// Clean up agent resources on quit.
    /// Full teardown: release bead, stop heartbeat, remove worktree, close agent.
    pub fn cleanup_agent(&mut self) {
        for w in 0..self.workers.len() {
            // Release hooked bead first (clear hook + reset to open)
            self.release_worker_hooked_bead(w);

            // Signal heartbeat thread to stop
            if let Some(stop) = &self.workers[w].heartbeat_stop {
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }

            if let (Some(agent_id), Some(wt_name)) = (
                &self.workers[w].agent_bead_id,
                &self.workers[w].worktree_name,
            ) {
                crate::agent::cleanup(&self.config.behavior.bd_path, agent_id, wt_name);
            }

            self.workers[w].agent_bead_id = None;
            self.workers[w].worktree_name = None;
            self.workers[w].worktree_path = None;
            self.workers[w].heartbeat_stop = None;
            self.workers[w].claimed_epic_id = None;
        }
    }

    /// Returns true if any worker has a running process or pending output.
    pub fn any_worker_active(&self) -> bool {
        self.workers
            .iter()
            .any(|w| w.child_process.is_some() || w.output_receiver.is_some())
    }

    /// Update app status based on aggregate worker state.
    /// Does not override Error state (auto-clears via timeout).
    pub fn update_derived_status(&mut self) {
        if self.status == AppStatus::Error || self.status == AppStatus::Starting {
            return;
        }
        self.status = if self.any_worker_active() {
            AppStatus::Running
        } else {
            AppStatus::Stopped
        };
    }
}
