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
