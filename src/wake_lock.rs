//! Wake lock management for Ralph.
//!
//! Prevents display and system idle sleep while claude is running.

use tracing::{info, warn};

/// A system wake lock that prevents idle sleep.
///
/// The lock is released when this struct is dropped.
pub struct WakeLock {
    _inner: keepawake::KeepAwake,
}

/// Error that occurred during wake lock acquisition.
#[derive(Debug)]
pub struct WakeLockError {
    pub message: String,
}

impl std::fmt::Display for WakeLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl WakeLock {
    /// Attempts to acquire a wake lock that prevents display and system idle sleep.
    ///
    /// The lock is automatically released when the `WakeLock` is dropped.
    ///
    /// Returns `Ok(WakeLock)` on success, or `WakeLockError` if acquisition fails.
    pub fn new() -> Result<Self, WakeLockError> {
        let handle = keepawake::Builder::default()
            .display(true)
            .idle(true)
            .reason("Running claude CLI")
            .app_name("ralph")
            .create()
            .map_err(|e| WakeLockError {
                message: format!("Failed to acquire wake lock: {}", e),
            })?;

        info!("wake_lock_acquired");
        Ok(WakeLock { _inner: handle })
    }
}

impl Drop for WakeLock {
    fn drop(&mut self) {
        info!("wake_lock_released");
    }
}

/// Attempts to acquire a wake lock, logging any failures.
///
/// Returns `Some(WakeLock)` on success, or `None` if acquisition fails.
/// Failures are logged at WARN level.
pub fn acquire() -> Option<WakeLock> {
    match WakeLock::new() {
        Ok(lock) => Some(lock),
        Err(e) => {
            warn!(error = %e, "wake_lock_failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_lock_new_succeeds_on_current_platform() {
        let lock = WakeLock::new();
        assert!(
            lock.is_ok(),
            "WakeLock::new() should succeed: {:?}",
            lock.err()
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn wake_lock_holds_display_and_idle_assertions_on_macos() {
        use std::process::Command;

        let pid = std::process::id();
        let pid_prefix = format!("pid {}(", pid);
        let reason = "Running claude CLI";
        let lock = WakeLock::new().expect("WakeLock::new() should succeed");

        let output = Command::new("pmset")
            .args(["-g", "assertions"])
            .output()
            .expect("pmset should be available on macOS");
        let stdout = String::from_utf8_lossy(&output.stdout);

        let our_lines: Vec<&str> = stdout
            .lines()
            .filter(|line| line.contains(&pid_prefix) && line.contains(reason))
            .collect();

        assert!(
            our_lines
                .iter()
                .any(|l| l.contains("PreventUserIdleDisplaySleep")),
            "Expected PreventUserIdleDisplaySleep for pid {pid}:\n{stdout}"
        );
        assert!(
            our_lines
                .iter()
                .any(|l| l.contains("PreventUserIdleSystemSleep")),
            "Expected PreventUserIdleSystemSleep for pid {pid}:\n{stdout}"
        );

        drop(lock);

        let output = Command::new("pmset")
            .args(["-g", "assertions"])
            .output()
            .expect("pmset should be available on macOS");
        let stdout = String::from_utf8_lossy(&output.stdout);

        let has_our_assertion = stdout
            .lines()
            .any(|line| line.contains(&pid_prefix) && line.contains(reason));
        assert!(
            !has_our_assertion,
            "Expected assertions for pid {pid} to be released after drop:\n{stdout}"
        );
    }
}
