//! Process-wide serialization lock for `bd` CLI invocations.
//!
//! bd v1.0's default storage backend is embedded Dolt, which is single-writer:
//! concurrent `bd` processes racing for the DB lock all but one fail with
//! `failed to open database: embeddeddolt: another process holds the exclusive lock`.
//! Ralph spawns bd from many places (board fetch, heartbeats, work polling, agent
//! lifecycle, …) and any two of them firing at once hit this error.
//!
//! This module exposes a single static [`Mutex`] that every bd-spawning call site
//! must hold from spawn to wait-for-exit. Holding the guard across `.output()` /
//! `.status()` is sufficient because both block until the child exits.
//!
//! Usage:
//! ```ignore
//! let output = {
//!     let _guard = crate::bd_lock::acquire();
//!     std::process::Command::new(bd_path).args(&["list", "--json"]).output()
//! };
//! ```
//!
//! For long-running spawns (e.g. `spawn() + try_wait()` loops), keep the guard
//! alive until the child has fully exited.

use std::sync::{Mutex, MutexGuard, OnceLock};

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Acquire the global bd mutex. Drops (releases) on guard scope exit.
///
/// Recovers from poisoning by taking the inner value — a panicked holder leaves
/// the mutex in a usable state from our perspective (we hold no invariant state
/// behind the lock, just the right to run bd).
pub fn acquire() -> MutexGuard<'static, ()> {
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run a closure while holding the global bd lock. Use this to wrap a
/// `Command::new(bd_path)...output()` / `.status()` call: the guard is held
/// across the blocking spawn-and-wait, serializing it against every other
/// ralph-initiated bd invocation.
///
/// Keep the closure's body tight — hold the lock only across the spawn, not
/// across downstream result parsing.
pub fn with_lock<F, T>(f: F) -> T
where
    F: FnOnce() -> T,
{
    let _guard = acquire();
    f()
}

/// Returns true if a bd subprocess's stderr payload indicates the embedded
/// Dolt backend's single-writer lock was held by an *external* process (e.g.
/// `bd list` fired from another shell while ralph was mid-fetch).
///
/// The internal [`acquire`]/[`with_lock`] mutex serializes ralph's own bd
/// calls, but it can't arbitrate against external processes — the OS-level
/// embedded-Dolt file lock does that, and it arbitrates by failing one side
/// outright. Call sites that would otherwise surface this error to the user
/// (the preview pane, board pipeline error cards) should check this predicate
/// and treat a match as transient: keep the previous state visible, skip
/// this update, and let the next tick retry.
pub fn is_transient_lock_error(stderr: &[u8]) -> bool {
    // bd emits this phrase in a JSON error blob on stderr when the
    // embedded-Dolt file lock is contended. The wording has been stable
    // across the bd v1.0.x line.
    const NEEDLE: &[u8] = b"another process holds the exclusive lock";
    stderr.windows(NEEDLE.len()).any(|w| w == NEEDLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_lock_runs_closure_and_returns_value() {
        assert_eq!(with_lock(|| 42), 42);
        assert_eq!(with_lock(|| "hello".to_string()), "hello");
    }

    #[test]
    fn is_transient_lock_error_matches_bd_v1_blob() {
        let stderr = br#"{  "error": "failed to open database: embeddeddolt: another process holds the exclusive lock on /Users/cmoel/Code/personal/ralph/.beads/embeddeddolt; the embedded backend supports only one writer at a time - use the dolt server backend for concurrent access"}"#;
        assert!(is_transient_lock_error(stderr));
    }

    #[test]
    fn is_transient_lock_error_rejects_unrelated_errors() {
        assert!(!is_transient_lock_error(b""));
        assert!(!is_transient_lock_error(b"Error: bead not found"));
        assert!(!is_transient_lock_error(b"permission denied"));
        assert!(!is_transient_lock_error(
            b"failed to parse: unexpected token"
        ));
    }

    #[test]
    fn is_transient_lock_error_matches_substring_anywhere() {
        // The phrase might be embedded in a larger log or wrapped error.
        assert!(is_transient_lock_error(
            b"WARN: another process holds the exclusive lock (retrying...)"
        ));
    }

    /// Grep-based guardrail: every `Command::new(...)` site under `src/` must
    /// either spawn a known non-bd binary (git / sh / sleep / pmset) OR be
    /// preceded by a `bd_lock::with_lock` / `bd_lock::acquire` call within the
    /// same enclosing scope (approximated as the 25 lines above the spawn).
    ///
    /// This catches the "forgot to take the mutex" class of regression. A
    /// future contributor adding a new bd spawn site without the guard would
    /// reintroduce bd v1.0 embedded-Dolt lock contention (the exact bug the
    /// `bd_lock` module exists to prevent).
    ///
    /// False positives are tolerable — add the binary to
    /// [`NON_BD_SPAWN_LITERALS`] or wrap the call in `with_lock`. False
    /// negatives (real bd spawns slipping through) defeat the point, so the
    /// test is deliberately strict: anything that isn't a whitelisted literal
    /// must hold the lock.
    #[test]
    fn every_command_new_site_holds_bd_lock() {
        use std::fs;
        use std::path::{Path, PathBuf};

        /// Literal Command::new arguments that are known NOT to spawn bd.
        /// Everything else must be wrapped in bd_lock.
        const NON_BD_SPAWN_LITERALS: &[&str] = &["\"git\"", "\"sh\"", "\"sleep\"", "\"pmset\""];

        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                    out.push(p);
                }
            }
        }

        let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        walk(&src_root, &mut files);
        assert!(!files.is_empty(), "no .rs files found under src/");

        let mut violations: Vec<String> = Vec::new();

        for file in &files {
            // bd_lock.rs itself contains `Command::new` inside the module
            // docstring and test fixtures; it would false-positive.
            if file.file_name().and_then(|s| s.to_str()) == Some("bd_lock.rs") {
                continue;
            }

            let contents = fs::read_to_string(file).expect("read src file");
            let lines: Vec<&str> = contents.lines().collect();

            for (idx, line) in lines.iter().enumerate() {
                // Skip comments and doc lines.
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") || trimmed.starts_with('*') {
                    continue;
                }
                if !line.contains("Command::new(") {
                    continue;
                }

                // Is this a whitelisted non-bd spawn?
                let is_non_bd = NON_BD_SPAWN_LITERALS
                    .iter()
                    .any(|literal| line.contains(&format!("Command::new({literal}")));
                if is_non_bd {
                    continue;
                }

                // Require a bd_lock reference within the 25 lines above
                // (enough to cover long `run_bd`-style functions where the
                // guard is declared once at the top).
                let window_start = idx.saturating_sub(25);
                let window = &lines[window_start..=idx];
                let has_lock = window
                    .iter()
                    .any(|l| l.contains("bd_lock::with_lock") || l.contains("bd_lock::acquire"));

                if !has_lock {
                    violations.push(format!(
                        "{}:{}: `{}` has no bd_lock guard in the 25 lines above — wrap in `bd_lock::with_lock(|| ...)` or add the binary to NON_BD_SPAWN_LITERALS if it's not a bd spawn",
                        file.display(),
                        idx + 1,
                        line.trim()
                    ));
                }
            }
        }

        assert!(
            violations.is_empty(),
            "bd_lock guard missing from {} Command::new site(s):\n{}",
            violations.len(),
            violations.join("\n")
        );
    }
}
