//! Logging infrastructure for Ralph.
//!
//! Provides structured file logging with daily rotation to platform-standard directories.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use directories::ProjectDirs;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Handle for dynamically changing the log level at runtime.
pub type ReloadHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

/// Result of initializing the logging system.
pub struct LoggingContext {
    /// Guard that must be held for the application lifetime to ensure logs are flushed.
    pub _guard: WorkerGuard,
    /// The session ID for this Ralph invocation.
    pub session_id: String,
    /// The directory where logs are written.
    pub log_directory: PathBuf,
    /// Handle for dynamically reloading the log level filter.
    pub reload_handle: Arc<Mutex<ReloadHandle>>,
}

/// Error that occurred during logging initialization.
#[derive(Debug)]
pub struct LoggingError {
    pub message: String,
}

impl std::fmt::Display for LoggingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Generates a 6-character random hex session ID.
fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: [u8; 3] = rng.random();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Initializes the logging system.
///
/// The `log_level` parameter specifies the initial log level (e.g., "info", "debug").
/// If `RALPH_LOG` environment variable is set, it overrides the provided level.
///
/// Returns a `LoggingContext` on success, or a `LoggingError` on failure.
/// The returned `WorkerGuard` must be held for the application lifetime.
/// The `reload_handle` can be used to dynamically change the log level.
pub fn init(log_level: &str) -> Result<LoggingContext, LoggingError> {
    let session_id = generate_session_id();

    // Get platform-appropriate log directory
    let project_dirs = ProjectDirs::from("dev", "cmoel", "ralph").ok_or_else(|| LoggingError {
        message: "Failed to determine platform directories".to_string(),
    })?;

    // Use cache_dir as base, but we want state/logs directory
    // macOS: ~/Library/Logs/ralph/
    // Linux: ~/.local/state/ralph/
    // Windows: %LocalAppData%\ralph\
    let log_dir = if cfg!(target_os = "macos") {
        dirs_home_log_dir()
    } else {
        project_dirs.state_dir().map(PathBuf::from)
    }
    .ok_or_else(|| LoggingError {
        message: "Failed to determine log directory".to_string(),
    })?;

    // Create log directory if it doesn't exist
    fs::create_dir_all(&log_dir).map_err(|e| LoggingError {
        message: format!("Failed to create log directory: {}", e),
    })?;

    // Create rolling daily file appender
    let file_appender = tracing_appender::rolling::daily(&log_dir, "ralph");

    // Use non-blocking writes
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Build the env filter: RALPH_LOG env var takes precedence, then provided level
    let env_filter = std::env::var("RALPH_LOG")
        .ok()
        .and_then(|level| EnvFilter::try_new(&level).ok())
        .unwrap_or_else(|| {
            EnvFilter::try_new(log_level).unwrap_or_else(|_| {
                tracing::warn!(
                    invalid_level = %log_level,
                    "Invalid log level in config, defaulting to info"
                );
                EnvFilter::new("info")
            })
        });

    // Wrap the filter in a reload layer for dynamic updates
    let (filter_layer, reload_handle) = reload::Layer::new(env_filter);

    // Create a formatting layer
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_span_events(FmtSpan::NONE)
        .with_target(true);

    // Build and set the subscriber
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    // Log session start
    info!(session_id = %session_id, log_level = %log_level, "session_start");

    Ok(LoggingContext {
        _guard: guard,
        session_id,
        log_directory: log_dir,
        reload_handle: Arc::new(Mutex::new(reload_handle)),
    })
}

/// Updates the log level filter at runtime.
///
/// Returns `Ok(())` if the level was successfully updated, or an error message if the level is invalid.
pub fn update_log_level(handle: &Arc<Mutex<ReloadHandle>>, new_level: &str) -> Result<(), String> {
    // Check for RALPH_LOG env override - if set, don't allow config to override it
    if std::env::var("RALPH_LOG").is_ok() {
        return Ok(()); // Silently ignore, env var takes precedence
    }

    let new_filter = EnvFilter::try_new(new_level)
        .map_err(|e| format!("Invalid log level '{}': {}", new_level, e))?;

    let guard = handle
        .lock()
        .map_err(|e| format!("Failed to acquire log level lock: {}", e))?;

    guard
        .reload(new_filter)
        .map_err(|e| format!("Failed to reload log level: {}", e))?;

    info!(new_level = %new_level, "log_level_changed");
    Ok(())
}

/// Gets the macOS ~/Library/Logs/ralph/ directory.
fn dirs_home_log_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library").join("Logs").join("ralph"))
}

/// Cleans up log files older than the retention period.
///
/// Scans the log directory for `ralph.*` files and deletes those older than 7 days.
/// Errors are logged at WARN level but don't prevent app startup.
pub fn cleanup_old_logs(log_dir: &PathBuf) {
    use std::time::{Duration, SystemTime};
    use tracing::{debug, warn};

    const RETENTION_DAYS: u64 = 7;
    let retention_duration = Duration::from_secs(RETENTION_DAYS * 24 * 60 * 60);

    let entries = match fs::read_dir(log_dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!(error = %e, "Failed to read log directory for cleanup");
            return;
        }
    };

    let now = SystemTime::now();
    let mut deleted_count = 0u32;

    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();

        // Only process ralph.* log files
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) if name.starts_with("ralph.") && name != "ralph" => name,
            _ => continue,
        };

        // Get file modification time
        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                warn!(file = %file_name, error = %e, "Failed to read metadata for log file");
                continue;
            }
        };

        let modified = match metadata.modified() {
            Ok(t) => t,
            Err(e) => {
                warn!(file = %file_name, error = %e, "Failed to get modification time for log file");
                continue;
            }
        };

        // Check if file is older than retention period
        let age = match now.duration_since(modified) {
            Ok(d) => d,
            Err(_) => continue, // File is in the future, skip
        };

        if age > retention_duration {
            match fs::remove_file(&path) {
                Ok(()) => {
                    debug!(file = %file_name, age_days = age.as_secs() / 86400, "Deleted old log file");
                    deleted_count += 1;
                }
                Err(e) => {
                    warn!(file = %file_name, error = %e, "Failed to delete old log file");
                }
            }
        }
    }

    if deleted_count > 0 {
        debug!(count = deleted_count, "Log cleanup completed");
    }
}
