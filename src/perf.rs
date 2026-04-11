//! Single-threaded perf counters flushed to the tracing log once per second.
//!
//! Temporary instrumentation for the 2.0 performance investigation (bead 2c7o).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing::info;

static LOOP_ITERS: AtomicU64 = AtomicU64::new(0);
static REDRAWS: AtomicU64 = AtomicU64::new(0);
static DRAW_MAX_US: AtomicU64 = AtomicU64::new(0);
static SUBPROCESS_SPAWNS: AtomicU64 = AtomicU64::new(0);

pub fn record_loop_iter() {
    LOOP_ITERS.fetch_add(1, Ordering::Relaxed);
}

pub fn record_redraw(duration: Duration) {
    REDRAWS.fetch_add(1, Ordering::Relaxed);
    let us = duration.as_micros() as u64;
    DRAW_MAX_US.fetch_max(us, Ordering::Relaxed);
}

pub fn record_subprocess_spawn() {
    SUBPROCESS_SPAWNS.fetch_add(1, Ordering::Relaxed);
}

pub struct PerfReporter {
    last_flush: Instant,
}

impl PerfReporter {
    pub fn new() -> Self {
        Self {
            last_flush: Instant::now(),
        }
    }

    pub fn maybe_flush(&mut self) {
        let elapsed = self.last_flush.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        let secs = elapsed.as_secs_f64();
        let loop_iters = LOOP_ITERS.swap(0, Ordering::Relaxed);
        let redraws = REDRAWS.swap(0, Ordering::Relaxed);
        let draw_max_us = DRAW_MAX_US.swap(0, Ordering::Relaxed);
        let spawns = SUBPROCESS_SPAWNS.swap(0, Ordering::Relaxed);
        info!(
            target: "ralph::perf",
            loop_iters_per_sec = loop_iters as f64 / secs,
            redraws_per_sec = redraws as f64 / secs,
            draw_max_ms = draw_max_us as f64 / 1000.0,
            subprocess_spawns_per_sec = spawns as f64 / secs,
            "perf_tick"
        );
        self.last_flush = Instant::now();
    }
}
