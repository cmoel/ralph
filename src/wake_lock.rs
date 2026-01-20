//! Wake lock management for Ralph.
//!
//! Prevents system idle sleep while claude is running.

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
    /// Attempts to acquire a wake lock that prevents system idle sleep.
    ///
    /// The display may still sleep. The lock is automatically released when
    /// the `WakeLock` is dropped.
    ///
    /// Returns `Ok(WakeLock)` on success, or `WakeLockError` if acquisition fails.
    pub fn new() -> Result<Self, WakeLockError> {
        let handle = keepawake::Builder::default()
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
