use std::collections::{HashMap, HashSet};

use super::state::{
    ColumnDef, KanbanCard, KanbanColumnUpdate, KanbanFetchMsg, KanbanFinalized, short_id,
};

fn parse_card(item: &serde_json::Value, emoji: &str) -> Option<KanbanCard> {
    let id = item.get("id").and_then(|v| v.as_str())?.to_string();
    let title = item
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let priority = item.get("priority").and_then(|v| v.as_u64()).unwrap_or(4);
    let blockers = item
        .get("blocked_by")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| short_id(s).to_string()))
                .collect()
        })
        .unwrap_or_default();
    let labels = item
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let status = item
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(KanbanCard {
        id,
        title,
        priority,
        blockers,
        emoji: emoji.to_string(),
        is_epic: false,
        is_error: false,
        labels,
        status,
    })
}

fn run_shell_pipeline(command: &str, bd_path: &str) -> Result<Vec<serde_json::Value>, String> {
    crate::perf::record_subprocess_spawn();
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", command])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Ensure the directory containing bd is on PATH so pipeline commands
    // can find the bd binary even when bd_path is an absolute path.
    let bd_abs = std::path::Path::new(bd_path);
    if let Some(parent) = bd_abs.parent().filter(|p| !p.as_os_str().is_empty())
        && let Ok(current_path) = std::env::var("PATH")
    {
        cmd.env("PATH", format!("{}:{current_path}", parent.display()));
    }

    let output = crate::bd_lock::with_lock(|| cmd.output())
        .map_err(|e| format!("Failed to run pipeline: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Pipeline failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
        .map_err(|e| format!("Failed to parse pipeline output: {e}"))
}

fn should_ignore_event(paths: &[std::path::PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    paths.iter().all(|path| {
        let components: Vec<_> = path.components().collect();
        let beads_idx = components.iter().position(
            |c| matches!(c, std::path::Component::Normal(s) if s.to_str() == Some(".beads")),
        );
        let Some(beads_idx) = beads_idx else {
            return false;
        };

        // Heartbeat marker file — bd touches it on every command; not a mutation signal.
        if path.file_name().and_then(|s| s.to_str()) == Some("last-touched") {
            return true;
        }

        // JSONL backup directory — large, periodic, not a mutation signal.
        for component in &components[beads_idx + 1..] {
            if let std::path::Component::Normal(s) = component
                && s.to_str() == Some("backup")
            {
                return true;
            }
        }
        false
    })
}

/// Watch .beads/ directory for changes and send notifications (called from background thread).
/// Debounces events — waits 200ms after the last change before notifying.
pub fn watch_beads_directory(
    tx: std::sync::mpsc::Sender<()>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use notify::{Config, RecursiveMode, Watcher};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let beads_dir = match std::env::current_dir() {
        Ok(dir) => dir.join(".beads"),
        Err(_) => return,
    };

    if !beads_dir.exists() {
        return;
    }

    let (event_tx, event_rx) = mpsc::channel();
    let mut watcher = match notify::RecommendedWatcher::new(event_tx, Config::default()) {
        Ok(w) => w,
        Err(_) => return,
    };

    if watcher.watch(&beads_dir, RecursiveMode::Recursive).is_err() {
        return;
    }

    let debounce_duration = Duration::from_millis(200);
    let mut last_event: Option<Instant> = None;

    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        match event_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(event)) => {
                if !should_ignore_event(&event.paths) {
                    last_event = Some(Instant::now());
                }
            }
            Ok(Err(_)) => {} // notify error, ignore
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(last) = last_event
            && last.elapsed() >= debounce_duration
        {
            let _ = tx.send(());
            last_event = None;
        }
    }
    // watcher is dropped here, stopping the OS-level watch
}

/// Fetch board data from pipeline sources, streaming one column at a time.
///
/// bd v1.0's default embedded Dolt backend is single-writer — parallel `bd` invocations
/// race for the DB lock and all but one fail. We serialize every bd call and publish
/// incremental updates over `tx` so the UI can show per-column progress while the
/// full refresh is in flight.
///
/// Message order: one `Column { col_idx, update }` per column in declaration order,
/// followed by exactly one `Finalized(..)`. The receiver may drop at any point; sends
/// short-circuit the function so it exits cleanly.
pub fn stream_board_data(
    bd_path: &str,
    column_defs: &[ColumnDef],
    tx: std::sync::mpsc::Sender<KanbanFetchMsg>,
) {
    let mut all_items: Vec<serde_json::Value> = Vec::new();

    for (col_idx, col_def) in column_defs.iter().enumerate() {
        let mut col_cards: Vec<KanbanCard> = Vec::new();
        let mut col_items: Vec<serde_json::Value> = Vec::new();

        for source in &col_def.sources {
            match run_shell_pipeline(&source.command, bd_path) {
                Ok(items) => {
                    for item in &items {
                        if let Some(card) = parse_card(item, &source.emoji) {
                            col_cards.push(card);
                        }
                    }
                    col_items.extend(items);
                }
                Err(err) => {
                    col_cards.push(KanbanCard {
                        id: String::new(),
                        title: format!("Error: {err}"),
                        priority: 999,
                        blockers: Vec::new(),
                        emoji: "\u{26a0}\u{fe0f}".to_string(), // ⚠️
                        is_epic: false,
                        is_error: true,
                        labels: Vec::new(),
                        status: String::new(),
                    });
                }
            }
        }

        // Dedup within this column by ID (errors and empty-id cards pass through)
        let mut seen: HashSet<String> = HashSet::new();
        col_cards
            .retain(|card| card.is_error || card.id.is_empty() || seen.insert(card.id.clone()));

        // Sort by priority (error cards sort to the end)
        col_cards.sort_by_key(|c| if c.is_error { u64::MAX } else { c.priority });

        all_items.extend(col_items.iter().cloned());

        let update = KanbanColumnUpdate { cards: col_cards };
        if tx.send(KanbanFetchMsg::Column { col_idx, update }).is_err() {
            return; // receiver gone, abandon
        }
    }

    // Fetch stats serially — last so the earlier columns render first
    crate::perf::record_subprocess_spawn();
    let stats_output = crate::bd_lock::with_lock(|| {
        std::process::Command::new(bd_path)
            .args(["stats", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    });
    let stats: Option<serde_json::Value> = match stats_output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            serde_json::from_str::<serde_json::Value>(&stdout).ok()
        }
        _ => None,
    };

    let (open_count, closed_count) = stats
        .and_then(|s| s.get("summary").cloned())
        .map(|s| {
            let open = s.get("open_issues").and_then(|v| v.as_u64()).unwrap_or(0)
                + s.get("in_progress_issues")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                + s.get("blocked_issues")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                + s.get("deferred_issues")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
            let closed = s.get("closed_issues").and_then(|v| v.as_u64()).unwrap_or(0);
            (open, closed)
        })
        .unwrap_or((0, 0));

    // Bidirectional dependency neighbor map over all fetched items
    let mut dep_neighbors: HashMap<String, HashSet<String>> = HashMap::new();
    for item in &all_items {
        if let Some(id) = item.get("id").and_then(|v| v.as_str())
            && let Some(blockers) = item.get("blocked_by").and_then(|v| v.as_array())
        {
            for b in blockers {
                if let Some(bid) = b.as_str() {
                    dep_neighbors
                        .entry(id.to_string())
                        .or_default()
                        .insert(bid.to_string());
                    dep_neighbors
                        .entry(bid.to_string())
                        .or_default()
                        .insert(id.to_string());
                }
            }
        }
    }

    // Epic = any bead that appears as a parent of another
    let epic_ids: HashSet<String> = all_items
        .iter()
        .filter_map(|item| {
            item.get("parent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    // Manual-blocked = status=blocked with empty/missing blocked_by
    let manual_blocked_ids: HashSet<String> = all_items
        .iter()
        .filter_map(|item| {
            let status = item.get("status").and_then(|v| v.as_str())?;
            if status != "blocked" {
                return None;
            }
            let has_deps = item
                .get("blocked_by")
                .and_then(|v| v.as_array())
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            if has_deps {
                None
            } else {
                item.get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
        })
        .collect();

    let _ = tx.send(KanbanFetchMsg::Finalized(KanbanFinalized {
        open_count,
        closed_count,
        dep_neighbors,
        manual_blocked_ids,
        epic_ids,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn should_ignore_event_backup() {
        assert!(should_ignore_event(&[PathBuf::from(
            "/proj/.beads/backup/2026-04-10.db"
        )]));
    }

    #[test]
    fn should_ignore_event_last_touched() {
        assert!(should_ignore_event(&[PathBuf::from(
            "/proj/.beads/last-touched"
        )]));
    }

    #[test]
    fn should_ignore_event_mixed_not_all_ignored() {
        assert!(!should_ignore_event(&[
            PathBuf::from("/proj/.beads/last-touched"),
            PathBuf::from("/proj/.beads/some-real-data.json"),
        ]));
    }

    #[test]
    fn should_ignore_event_data_file() {
        assert!(!should_ignore_event(&[PathBuf::from(
            "/proj/.beads/some-real-data.json"
        )]));
    }

    #[test]
    fn should_ignore_event_embeddeddolt_changes_are_signal() {
        assert!(!should_ignore_event(&[PathBuf::from(
            "/proj/.beads/embeddeddolt/ralph/.dolt/noms/abc.bin"
        )]));
    }

    #[test]
    fn should_ignore_event_empty() {
        assert!(!should_ignore_event(&[]));
    }
}
