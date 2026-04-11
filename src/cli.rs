//! CLI argument parsing and subcommand implementations.

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::Result;
use clap::Parser;

use crate::config;
use crate::doctor;
use crate::logging;
use crate::modals::InitModalState;
use crate::work_source;

/// CLI subcommands.
#[derive(Debug, Parser)]
pub enum Commands {
    /// Initialize project with Ralph scaffolding
    Init,
    /// Check environment health and report pass/fail for each check
    Doctor,
    /// List implementable beads
    Ready {
        /// Annotate each item with include/exclude reason
        #[arg(long)]
        verbose: bool,
    },
    /// Dump session logs to stdout
    Logs {
        /// Dump logs for a specific session ID
        #[arg(long)]
        id: Option<String>,
        /// Print the log directory path and exit
        #[arg(long)]
        path: bool,
    },
    /// Manage and inspect tool permissions and history
    #[command(subcommand)]
    Tool(ToolCommands),
}

/// Subcommands under `ralph tool`.
#[derive(Debug, Parser)]
pub enum ToolCommands {
    /// Query tool call history from the SQLite database
    History {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Filter by tool name (case-insensitive)
        #[arg(long)]
        tool: Option<String>,
        /// Show tool calls since this time (e.g., 6h, 1d, today, 2025-01-15)
        #[arg(long)]
        since: Option<String>,
        /// Show tool calls until this time (requires --since)
        #[arg(long)]
        until: Option<String>,
        /// Filter to rejected/errored tool calls only
        #[arg(long)]
        rejected: bool,
        /// Filter by repo (basename match if no slash, substring match if has slash)
        #[arg(long)]
        repo: Option<String>,
        /// Show tool calls from all repos (overrides default current-repo filter)
        #[arg(long)]
        all: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Print the database file path and exit
        #[arg(long)]
        db_path: bool,
    },
    /// Allow a tool pattern in settings
    Allow {
        /// Tool pattern (e.g., Read, Bash(git:*), WebFetch(domain:docs.rs))
        pattern: String,
        /// Write to .claude/settings.json instead of .claude/settings.local.json
        #[arg(long)]
        project: bool,
    },
    /// Deny a tool pattern in settings
    Deny {
        /// Tool pattern (e.g., Bash(rm:*), Bash(sudo *))
        pattern: String,
        /// Write to .claude/settings.json instead of .claude/settings.local.json
        #[arg(long)]
        project: bool,
    },
    /// List tool permissions across all settings files
    List,
}

/// CLI argument parser.
#[derive(Debug, Parser)]
#[command(
    version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_SHA"), ")"),
    about = "TUI wrapper for claude CLI that displays formatted streaming output"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Run the init subcommand: create project scaffolding files.
pub fn run_init() -> Result<()> {
    let loaded_config = config::load_config();
    let state = InitModalState::new(&loaded_config.config);

    if state.all_up_to_date() {
        println!("All skill files are up to date.");
        return Ok(());
    }

    state.print_diffs();

    let created = state.create_count();
    let updated = state.regenerate_count();
    let skipped = state.skip_count();

    match state.create_files() {
        Ok(()) => {
            let mut parts = Vec::new();
            if created > 0 {
                parts.push(format!(
                    "created {} file{}",
                    created,
                    if created == 1 { "" } else { "s" }
                ));
            }
            if updated > 0 {
                parts.push(format!(
                    "updated {} file{}",
                    updated,
                    if updated == 1 { "" } else { "s" }
                ));
            }
            if skipped > 0 {
                parts.push(format!("skipped {} unchanged", skipped));
            }
            println!("{}.", parts.join(", "));
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Run the ready subcommand: list implementable beads.
pub fn run_ready(verbose: bool) -> Result<()> {
    let loaded_config = config::load_config();
    let cfg = &loaded_config.config;

    let output = Command::new(&cfg.behavior.bd_path)
        .args(["ready", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("Error: {}: command not found", cfg.behavior.bd_path);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: failed to run bd: {}", e);
            std::process::exit(1);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("Error: bd ready failed: {}", stderr.trim());
        std::process::exit(1);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(serde_json::Value::Array(arr)) => arr,
        Ok(_) => {
            eprintln!("Error: unexpected bd ready output");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Error: failed to parse bd output: {}", e);
            std::process::exit(1);
        }
    };

    if items.is_empty() {
        if verbose {
            println!("No ready beads.");
        }
        std::process::exit(1);
    }

    let mut has_implementable = false;

    for item in &items {
        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("???");
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let labels = item.get("labels").and_then(|v| v.as_array());

        let is_human = labels.is_some_and(|ls| {
            ls.iter()
                .any(|l| l.as_str().is_some_and(work_source::is_human_label))
        });

        if verbose {
            if is_human {
                println!("{}: SKIPPED (human) — {}", id, title);
            } else {
                println!("{}: READY — {}", id, title);
                has_implementable = true;
            }
        } else if !is_human {
            println!("{}\t{}", id, title);
            has_implementable = true;
        }
    }

    if !has_implementable {
        if verbose {
            println!(
                "\nAll {} ready bead{} {} for humans.",
                items.len(),
                if items.len() == 1 { "" } else { "s" },
                if items.len() == 1 { "is" } else { "are" }
            );
        }
        std::process::exit(1);
    }

    Ok(())
}

/// Run the logs subcommand: dump session logs to stdout.
pub fn run_logs(id: Option<String>, path_only: bool) -> Result<()> {
    let log_dir = logging::log_directory()
        .ok_or_else(|| anyhow::anyhow!("Failed to determine log directory"))?;

    if path_only {
        println!("{}", log_dir.display());
        return Ok(());
    }

    if !log_dir.exists() {
        eprintln!("Error: log directory does not exist: {}", log_dir.display());
        std::process::exit(1);
    }

    // Collect and sort log files
    let mut log_files: Vec<PathBuf> = std::fs::read_dir(&log_dir)?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("ralph."))
        })
        .map(|e| e.path())
        .collect();
    log_files.sort();

    if log_files.is_empty() {
        eprintln!("Error: no log files found");
        std::process::exit(1);
    }

    let session_id = match id {
        Some(id) => id,
        None => find_most_recent_session(&log_files)?,
    };

    // Warning to stderr so it doesn't pollute piped output
    eprintln!("Review for secrets before sharing");

    // Dump all lines for this session (oldest first)
    let filter = format!("session_id={}", session_id);
    for file in &log_files {
        let content = std::fs::read_to_string(file)?;
        for line in content.lines() {
            if line.contains(&filter) {
                println!("{}", line);
            }
        }
    }

    Ok(())
}

/// Find the most recent session_start in log files (sorted oldest to newest).
fn find_most_recent_session(log_files: &[PathBuf]) -> Result<String> {
    for file in log_files.iter().rev() {
        let content = std::fs::read_to_string(file)?;
        for line in content.lines().rev() {
            if line.contains("session_start")
                && let Some(id) = extract_session_id(line)
            {
                return Ok(id);
            }
        }
    }
    eprintln!("Error: no sessions found in log files");
    std::process::exit(1);
}

/// Extract session_id value from a log line like `... session_id=abc123 ...`.
fn extract_session_id(line: &str) -> Option<String> {
    let marker = "session_id=";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Run the doctor subcommand: check environment health.
pub fn run_doctor() -> Result<()> {
    let loaded_config = config::load_config();
    let cfg = &loaded_config.config;

    let mut checks: Vec<doctor::CheckResult> = vec![
        doctor::check_config(&loaded_config),
        doctor::check_claude(cfg),
        doctor::check_prompt(cfg),
    ];

    checks.push(doctor::check_bd(cfg));
    checks.push(doctor::check_bd_prime_hook());
    checks.push(doctor::check_scaffolding_drift(cfg));
    checks.push(doctor::check_board_toml());
    let dolt_check = doctor::check_dolt_status(cfg);
    let dolt_running = dolt_check.passed;
    checks.push(dolt_check);
    if dolt_running {
        checks.push(doctor::check_work_items(cfg));
    }

    let mut all_passed = true;
    for check in &checks {
        if check.passed {
            println!("\u{2713} {}", check.message);
        } else {
            println!("\u{2717} {}", check.message);
            all_passed = false;
        }
    }

    if !all_passed {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_session_id_from_log_line() {
        let line = "2026-03-27T10:00:00.000Z  INFO ralph::logging: session_start session_id=abc123 log_level=info";
        assert_eq!(extract_session_id(line), Some("abc123".to_string()));
    }

    #[test]
    fn extract_session_id_at_end_of_line() {
        let line = "some prefix session_id=def456";
        assert_eq!(extract_session_id(line), Some("def456".to_string()));
    }

    #[test]
    fn extract_session_id_missing() {
        let line = "no session here";
        assert_eq!(extract_session_id(line), None);
    }
}
