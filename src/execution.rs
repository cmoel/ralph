use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use anyhow::Result;
use tracing::{debug, info};

use crate::agent;
use crate::app::{App, AppStatus};
use crate::output;
use crate::templates;
use crate::wake_lock;

/// Assemble the prompt content and build the shell command string for Claude CLI.
///
/// Reads PROMPT.md and optional mode-specific content, writes a temp file for mode
/// content if needed, and returns the full shell command to pipe into Claude.
pub fn assemble_prompt(config: &crate::config::Config) -> Result<String> {
    let prompt_path = config.prompt_path();
    let claude_path = config.claude_path();
    const CLAUDE_ARGS: &str =
        "--output-format=stream-json --verbose --print --include-partial-messages";

    let mode = &config.behavior.mode;
    let mode_temp_path = if let Some(content) = templates::mode_content(mode) {
        let path = std::env::temp_dir().join("ralph-mode.md");
        std::fs::write(&path, content)?;
        Some(path)
    } else {
        None
    };

    let command = if let Some(ref mode_path) = mode_temp_path {
        format!(
            "cat {} {} | {} {}",
            prompt_path.display(),
            mode_path.display(),
            claude_path.display(),
            CLAUDE_ARGS
        )
    } else {
        format!(
            "cat {} | {} {}",
            prompt_path.display(),
            claude_path.display(),
            CLAUDE_ARGS
        )
    };

    Ok(command)
}

/// In beads mode, claim the next available bead before starting claude.
/// Reclaims stale beads first (priority over new claims), then falls through
/// to claiming a new bead if nothing was reclaimed.
/// Returns true if we should proceed with start_command (claimed or non-beads mode).
/// Returns false if claiming failed (no work available).
pub fn claim_before_start(app: &mut App) -> bool {
    if app.config.behavior.mode != "beads" {
        return true;
    }
    // Release any previously hooked bead before claiming a new one.
    // Without this, auto-continue overwrites hooked_bead_id and orphans
    // the old bead in in_progress status forever.
    app.release_hooked_bead();

    let agent_id = match &app.agent_bead_id {
        Some(id) => id.clone(),
        None => return true, // no agent registered, skip claiming
    };

    // Auto-reclaim stale beads (priority over new claims)
    let bd_path = app.config.behavior.bd_path.clone();
    let stale_agents = agent::find_stale_agents(
        &bd_path,
        app.config.behavior.stale_threshold,
        Some(&agent_id),
    );
    if !stale_agents.is_empty() {
        let first = &stale_agents[0];
        match agent::resume_stale_bead(&bd_path, &agent_id, first) {
            agent::ResumeResult::Resumed => {
                app.add_text_line(format!(
                    "[Auto-reclaimed: {} \"{}\"]",
                    first.hooked_bead_id, first.hooked_bead_title
                ));
                app.hooked_bead_id = Some(first.hooked_bead_id.clone());
                // Release remaining stale agents back to open
                for stale in stale_agents.iter().skip(1) {
                    agent::release_stale_bead(&bd_path, stale);
                    app.add_text_line(format!(
                        "[Released stale: {} \"{}\"]",
                        stale.hooked_bead_id, stale.hooked_bead_title
                    ));
                }
                return true;
            }
            agent::ResumeResult::EscalatedToHuman => {
                app.add_text_line(format!(
                    "[Escalated to human: {} \"{}\" — stuck twice]",
                    first.hooked_bead_id, first.hooked_bead_title
                ));
                // Release remaining stale agents back to open
                for stale in stale_agents.iter().skip(1) {
                    agent::release_stale_bead(&bd_path, stale);
                    app.add_text_line(format!(
                        "[Released stale: {} \"{}\"]",
                        stale.hooked_bead_id, stale.hooked_bead_title
                    ));
                }
                // Fall through to claim new work
            }
            agent::ResumeResult::Failed => {
                // Fall through to claim new work
            }
        }
    }

    match agent::claim_next_bead(&app.config.behavior.bd_path, &agent_id) {
        Some((bead_id, title)) => {
            app.add_text_line(format!("[Claimed: {} {}]", bead_id, title));
            app.hooked_bead_id = Some(bead_id);
            true
        }
        None => {
            app.add_text_line("[No beads available to claim]".to_string());
            false
        }
    }
}

/// Start the command.
pub fn start_command(app: &mut App) -> Result<()> {
    if app.status == AppStatus::Running {
        app.show_already_running_popup = true;
        return Ok(());
    }

    // Check if prompt file exists (using config path)
    let prompt_path = app.config.prompt_path();
    if !prompt_path.exists() {
        app.status = AppStatus::Error;
        app.error_at = Some(std::time::Instant::now());
        app.add_text_line(format!("Error: {} not found", prompt_path.display()));
        return Ok(());
    }

    // Increment loop counter and log loop_start
    app.loop_count += 1;
    info!(loop_number = app.loop_count, "loop_start");

    // Add divider if not first run
    if !app.output_lines.is_empty() {
        app.add_text_line("─".repeat(40));
    }

    // Reset streaming state for new command
    app.content_blocks.clear();
    app.current_line.clear();

    let command = assemble_prompt(&app.config)?;
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // In beads mode, run claude in the worktree directory
    if let Some(ref wt_path) = app.worktree_path {
        cmd.current_dir(wt_path);
    }

    let child = cmd.spawn();

    match child {
        Ok(mut child) => {
            // Log command_spawned with PID
            debug!(pid = child.id(), "command_spawned");

            // Attempt to acquire wake lock (prevents system idle sleep)
            let wake_lock = if app.config.behavior.keep_awake {
                match wake_lock::acquire() {
                    Some(lock) => Some(lock),
                    None => {
                        // Wake lock failed - display warning in output panel
                        app.add_text_line(
                            "⚠ Warning: Could not acquire wake lock - system may sleep during execution"
                                .to_string(),
                        );
                        None
                    }
                }
            } else {
                None
            };
            app.wake_lock = wake_lock;

            let (tx, rx) = mpsc::channel();

            // Read stdout in a background thread
            if let Some(stdout) = child.stdout.take() {
                let tx_stdout = tx.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().map_while(Result::ok) {
                        if tx_stdout.send(output::OutputMessage::Line(line)).is_err() {
                            break;
                        }
                    }
                });
            }

            // Read stderr in a background thread
            if let Some(stderr) = child.stderr.take() {
                let tx_stderr = tx.clone();
                thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().map_while(Result::ok) {
                        if tx_stderr
                            .send(output::OutputMessage::Line(format!("[stderr] {}", line)))
                            .is_err()
                        {
                            break;
                        }
                    }
                });
            }

            app.child_process = Some(child);
            app.output_receiver = Some(rx);
            app.status = AppStatus::Running;
            app.run_start_time = Some(std::time::Instant::now());
        }
        Err(e) => {
            app.status = AppStatus::Error;
            app.error_at = Some(std::time::Instant::now());
            app.add_text_line(format!("Error starting command: {}", e));
        }
    }

    Ok(())
}
