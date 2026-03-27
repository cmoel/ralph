//! CLI argument parsing and subcommand implementations.

use std::io;
use std::process::{Command, Stdio};

use anyhow::Result;
use clap::Parser;

use crate::config;
use crate::doctor;
use crate::modals::InitModalState;
use crate::work_source;

/// CLI subcommands.
#[derive(Debug, Parser)]
pub enum Commands {
    /// Initialize project with Ralph scaffolding
    Init,
    /// Force-regenerate all managed files (preserves .ralph config)
    Reinit,
    /// Check environment health and report pass/fail for each check
    Doctor,
    /// List implementable beads (beads mode only)
    Ready {
        /// Annotate each item with include/exclude reason
        #[arg(long)]
        verbose: bool,
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

    if state.all_exist() {
        println!("All files already exist, nothing to create.");
        return Ok(());
    }

    let create_count = state.create_count();
    let skip_count = state.skip_count();

    match state.create_files() {
        Ok(()) => {
            println!(
                "Created {} file{}, skipped {} existing.",
                create_count,
                if create_count == 1 { "" } else { "s" },
                skip_count
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

/// Run the reinit subcommand: force-regenerate all managed files.
pub fn run_reinit() -> Result<()> {
    let loaded_config = config::load_config();
    let state = InitModalState::new_reinit(&loaded_config.config);

    let create_count = state.create_count();
    let regenerate_count = state.regenerate_count();

    if create_count == 0 && regenerate_count == 0 {
        println!("Nothing to do.");
        return Ok(());
    }

    match state.create_files() {
        Ok(()) => {
            let mut parts = Vec::new();
            if regenerate_count > 0 {
                parts.push(format!(
                    "regenerated {} file{}",
                    regenerate_count,
                    if regenerate_count == 1 { "" } else { "s" }
                ));
            }
            if create_count > 0 {
                parts.push(format!(
                    "created {} file{}",
                    create_count,
                    if create_count == 1 { "" } else { "s" }
                ));
            }
            println!("{} (old files backed up as .bak).", parts.join(", "));
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

    if cfg.behavior.mode != "beads" {
        eprintln!("Error: ralph ready requires beads mode");
        std::process::exit(1);
    }

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

        let needs_shaping = labels.is_some_and(|ls| {
            ls.iter().any(|l| {
                l.as_str()
                    .is_some_and(|s| work_source::is_shaping_label(s, &[]))
            })
        });

        if verbose {
            if needs_shaping {
                println!("{}: SKIPPED (needs-shaping) — {}", id, title);
            } else {
                println!("{}: READY — {}", id, title);
                has_implementable = true;
            }
        } else if !needs_shaping {
            println!("{}\t{}", id, title);
            has_implementable = true;
        }
    }

    if !has_implementable {
        if verbose {
            println!(
                "\nAll {} ready bead{} {} shaping.",
                items.len(),
                if items.len() == 1 { "" } else { "s" },
                if items.len() == 1 { "needs" } else { "need" }
            );
        }
        std::process::exit(1);
    }

    Ok(())
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

    if cfg.behavior.mode == "beads" {
        checks.push(doctor::check_bd(cfg));
        checks.push(doctor::check_bd_prime_hook());
        let dolt_check = doctor::check_dolt_status(cfg);
        let dolt_running = dolt_check.passed;
        checks.push(dolt_check);
        if dolt_running {
            checks.push(doctor::check_work_items(cfg));
            checks.push(doctor::check_unrecognized_labels(cfg));
        }
    } else {
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
