use std::collections::{HashMap, HashSet};

use super::state::{ColumnDef, KanbanBoardData, KanbanCard, short_id};

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

    let output = cmd
        .output()
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
            Ok(Ok(_)) => {
                last_event = Some(Instant::now());
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

/// Fetch board data from pipeline sources (called from background thread).
pub fn fetch_board_data(
    bd_path: &str,
    column_defs: &[ColumnDef],
) -> Result<KanbanBoardData, String> {
    use std::thread;

    // Collect all (col_idx, emoji, command) tuples
    let mut tasks: Vec<(usize, String, String)> = Vec::new();
    for (col_idx, col_def) in column_defs.iter().enumerate() {
        for source in &col_def.sources {
            tasks.push((col_idx, source.emoji.clone(), source.command.clone()));
        }
    }

    // Spawn all source commands in parallel
    type PipelineHandle = (
        usize,
        String,
        thread::JoinHandle<Result<Vec<serde_json::Value>, String>>,
    );
    let bd = bd_path.to_string();
    let handles: Vec<PipelineHandle> = tasks
        .into_iter()
        .map(|(col_idx, emoji, command)| {
            let bd_clone = bd.clone();
            let handle = thread::spawn(move || run_shell_pipeline(&command, &bd_clone));
            (col_idx, emoji, handle)
        })
        .collect();

    // Also fetch stats in parallel
    let p = bd_path.to_string();
    let h_stats = thread::spawn(move || {
        let output = std::process::Command::new(&p)
            .args(["stats", "--json"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                serde_json::from_str::<serde_json::Value>(&stdout).ok()
            }
            _ => None,
        }
    });

    // Collect results into columns
    let col_count = column_defs.len();
    let mut columns: Vec<Vec<KanbanCard>> = vec![Vec::new(); col_count];
    let mut all_items: Vec<serde_json::Value> = Vec::new();

    for (col_idx, emoji, handle) in handles {
        match handle.join().map_err(|_| "thread panic".to_string())? {
            Ok(items) => {
                all_items.extend(items.iter().cloned());
                for item in &items {
                    if let Some(card) = parse_card(item, &emoji) {
                        columns[col_idx].push(card);
                    }
                }
            }
            Err(err) => {
                // Render an error card in the column; other sources still render
                columns[col_idx].push(KanbanCard {
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

    // Dedup within each column by ID
    for column in &mut columns {
        let mut seen = HashSet::new();
        column.retain(|card| card.is_error || card.id.is_empty() || seen.insert(card.id.clone()));
    }

    // Detect epics (beads that are parents of other beads)
    let parent_ids: HashSet<String> = all_items
        .iter()
        .filter_map(|item| {
            item.get("parent")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    for column in &mut columns {
        for card in column.iter_mut() {
            if parent_ids.contains(&card.id) {
                card.is_epic = true;
            }
        }
    }

    // Sort each column by priority (error cards sort to the end)
    for column in &mut columns {
        column.sort_by_key(|c| if c.is_error { u64::MAX } else { c.priority });
    }

    // Build bidirectional dependency neighbor map
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

    // Detect manual-blocked beads: status=blocked but no actual blocking dependencies
    let manual_blocked_ids: HashSet<String> = columns
        .iter()
        .flat_map(|col| col.iter())
        .filter(|card| card.status == "blocked" && card.blockers.is_empty())
        .map(|card| card.id.clone())
        .collect();

    // Stats
    let stats = h_stats.join().map_err(|_| "thread panic")?;
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

    Ok(KanbanBoardData {
        columns,
        open_count,
        closed_count,
        dep_neighbors,
        manual_blocked_ids,
    })
}
