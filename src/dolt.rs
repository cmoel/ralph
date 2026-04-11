//! Dolt SQL server state machine.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use tracing::{info, warn};

/// State of the Dolt SQL server.
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
    pub fn poll_status(&mut self, bd_path: &str) -> bool {
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
            crate::perf::record_subprocess_spawn();
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

    /// Toggle Dolt server on/off.
    pub fn toggle(&mut self, bd_path: &str) {
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
                    let success = run_dolt_command(&bd_path, "stop");
                    if success {
                        match truncate_server_log_if_large(DOLT_LOG_TRUNCATE_THRESHOLD) {
                            Ok(true) => {
                                info!("truncated .beads/dolt-server.log after dolt stop")
                            }
                            Ok(false) => {}
                            Err(e) => {
                                warn!(error = %e, "failed to truncate dolt-server.log after stop")
                            }
                        }
                    }
                    let _ = tx.send(success);
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
    crate::bd_lock::with_lock(|| {
        std::process::Command::new(bd_path)
            .args(["dolt", "status"])
            .output()
    })
    .map(|output| {
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains("server: running")
    })
    .unwrap_or(false)
}

/// Run a `bd dolt` subcommand (start/stop) and return whether it succeeded.
fn run_dolt_command(bd_path: &str, subcmd: &str) -> bool {
    crate::bd_lock::with_lock(|| {
        std::process::Command::new(bd_path)
            .args(["dolt", subcmd])
            .output()
    })
    .map(|output| output.status.success())
    .unwrap_or(false)
}

pub const DOLT_LOG_TRUNCATE_THRESHOLD: u64 = 50 * 1024 * 1024;

const DOLT_SERVER_LOG_PATH: &str = ".beads/dolt-server.log";
const DOLT_SERVER_PID_PATH: &str = ".beads/dolt-server.pid";

pub fn truncate_server_log_if_large(threshold_bytes: u64) -> std::io::Result<bool> {
    truncate_log_if_large(std::path::Path::new(DOLT_SERVER_LOG_PATH), threshold_bytes)
}

fn truncate_log_if_large(path: &std::path::Path, threshold_bytes: u64) -> std::io::Result<bool> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > threshold_bytes => {
            std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(path)?;
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn is_dolt_server_alive() -> bool {
    is_pid_alive(std::path::Path::new(DOLT_SERVER_PID_PATH))
}

fn is_pid_alive(pid_path: &std::path::Path) -> bool {
    let pid_str = match std::fs::read_to_string(pid_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pid: i32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn truncate_large_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.log");
        let mut f = std::fs::File::create(&path).unwrap();
        let buf = vec![0u8; 1024];
        for _ in 0..200 {
            f.write_all(&buf).unwrap();
        }
        drop(f);
        assert!(std::fs::metadata(&path).unwrap().len() > 100_000);

        let result = truncate_log_if_large(&path, 100_000).unwrap();
        assert!(result);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    }

    #[test]
    fn truncate_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.log");
        std::fs::write(&path, "hello").unwrap();
        let original_len = std::fs::metadata(&path).unwrap().len();

        let result = truncate_log_if_large(&path, 100_000).unwrap();
        assert!(!result);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), original_len);
    }

    #[test]
    fn truncate_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.log");

        let result = truncate_log_if_large(&path, 100_000).unwrap();
        assert!(!result);
    }

    #[test]
    fn is_pid_alive_no_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.pid");

        assert!(!is_pid_alive(&path));
    }

    #[test]
    fn is_pid_alive_stale_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stale.pid");
        std::fs::write(&path, "99999999").unwrap();

        assert!(!is_pid_alive(&path));
    }

    #[test]
    fn is_pid_alive_current_process() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current.pid");
        std::fs::write(&path, std::process::id().to_string()).unwrap();

        assert!(is_pid_alive(&path));
    }
}
