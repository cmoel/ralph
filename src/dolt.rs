//! Dolt SQL server state machine (beads mode only).

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use tracing::info;

/// State of the Dolt SQL server (beads mode only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoltServerState {
    /// Haven't checked yet.
    Unknown,
    /// Server is not running.
    Off,
    /// Server is starting up (~5s).
    Starting,
    /// Server is running.
    On,
    /// Server is shutting down.
    Stopping,
}

/// Manages the Dolt SQL server lifecycle and background polling.
pub struct DoltManager {
    pub state: DoltServerState,
    status_rx: Option<Receiver<bool>>,
    toggle_rx: Option<Receiver<bool>>,
    last_poll: Instant,
}

impl DoltManager {
    pub fn new() -> Self {
        Self {
            state: DoltServerState::Unknown,
            status_rx: None,
            toggle_rx: None,
            // Initialize to "long ago" so we poll immediately on start
            last_poll: Instant::now() - Duration::from_secs(10),
        }
    }

    /// Poll for Dolt server status (throttled). Returns true if UI should redraw.
    pub fn poll_status(&mut self, bd_path: &str, mode: &str) -> bool {
        if mode != "beads" {
            return false;
        }

        let mut dirty = false;

        // Check for completed background status check
        if let Some(rx) = self.status_rx.take() {
            match rx.try_recv() {
                Ok(running) => {
                    // Only update if not in a transitional state
                    if self.state != DoltServerState::Starting
                        && self.state != DoltServerState::Stopping
                    {
                        let new_state = if running {
                            DoltServerState::On
                        } else {
                            DoltServerState::Off
                        };
                        if self.state != new_state {
                            self.state = new_state;
                            dirty = true;
                        }
                    }
                }
                Err(TryRecvError::Empty) => {
                    self.status_rx = Some(rx);
                    return false;
                }
                Err(TryRecvError::Disconnected) => {}
            }
        }

        // Don't poll during transitional states
        if self.state == DoltServerState::Starting || self.state == DoltServerState::Stopping {
            return dirty;
        }

        // Throttle: poll every 5 seconds
        if self.last_poll.elapsed() < Duration::from_secs(5) {
            return dirty;
        }

        self.last_poll = Instant::now();

        // Kick off background status check
        let (tx, rx) = mpsc::channel();
        let bd_path = bd_path.to_string();
        std::thread::spawn(move || {
            let _ = tx.send(check_dolt_running(&bd_path));
        });
        self.status_rx = Some(rx);

        dirty
    }

    /// Poll for Dolt toggle (start/stop) completion. Returns true if UI should redraw.
    pub fn poll_toggle(&mut self) -> bool {
        let rx = match self.toggle_rx.take() {
            Some(rx) => rx,
            None => return false,
        };

        match rx.try_recv() {
            Ok(success) => {
                match self.state {
                    DoltServerState::Starting => {
                        self.state = if success {
                            DoltServerState::On
                        } else {
                            DoltServerState::Off
                        };
                    }
                    DoltServerState::Stopping => {
                        self.state = if success {
                            DoltServerState::Off
                        } else {
                            DoltServerState::On
                        };
                    }
                    _ => {}
                }
                // Reset poll timer so we verify state soon
                self.last_poll = Instant::now() - Duration::from_secs(10);
                true
            }
            Err(TryRecvError::Empty) => {
                self.toggle_rx = Some(rx);
                false
            }
            Err(TryRecvError::Disconnected) => {
                match self.state {
                    DoltServerState::Starting => self.state = DoltServerState::Off,
                    DoltServerState::Stopping => self.state = DoltServerState::On,
                    _ => {}
                }
                true
            }
        }
    }

    /// Toggle Dolt server on/off (beads mode only).
    pub fn toggle(&mut self, bd_path: &str, mode: &str) {
        if mode != "beads" {
            return;
        }

        match self.state {
            DoltServerState::Starting | DoltServerState::Stopping => (),
            DoltServerState::On => {
                info!("dolt_server_stopping");
                self.state = DoltServerState::Stopping;
                // Discard any in-flight status poll so stale results don't override
                self.status_rx = None;
                let (tx, rx) = mpsc::channel();
                let bd_path = bd_path.to_string();
                std::thread::spawn(move || {
                    let _ = tx.send(run_dolt_command(&bd_path, "stop"));
                });
                self.toggle_rx = Some(rx);
            }
            DoltServerState::Off | DoltServerState::Unknown => {
                info!("dolt_server_starting");
                self.state = DoltServerState::Starting;
                // Discard any in-flight status poll so stale results don't override
                self.status_rx = None;
                let (tx, rx) = mpsc::channel();
                let bd_path = bd_path.to_string();
                std::thread::spawn(move || {
                    let _ = tx.send(run_dolt_command(&bd_path, "start"));
                });
                self.toggle_rx = Some(rx);
            }
        }
    }

    /// Clear all pending background operations and reset state.
    pub fn clear(&mut self) {
        self.status_rx = None;
        self.toggle_rx = None;
        self.state = DoltServerState::Unknown;
    }
}

/// Check if the Dolt server is running by calling `bd dolt status`.
fn check_dolt_running(bd_path: &str) -> bool {
    std::process::Command::new(bd_path)
        .args(["dolt", "status"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("server: running")
        })
        .unwrap_or(false)
}

/// Run a `bd dolt` subcommand (start/stop) and return whether it succeeded.
fn run_dolt_command(bd_path: &str, subcmd: &str) -> bool {
    std::process::Command::new(bd_path)
        .args(["dolt", subcmd])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
