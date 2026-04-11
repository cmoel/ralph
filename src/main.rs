//! Ralph - TUI wrapper for claude CLI that displays formatted streaming output.

mod agent;
mod app;
mod bd_lock;
mod cli;
mod config;
mod db;
mod doctor;
mod event_loop;
mod events;
mod execution;
mod logging;
mod modals;
mod output;
mod startup;
mod templates;
mod tool_history;
#[allow(dead_code)]
mod tool_panel;
mod tool_settings;
mod ui;
mod validators;
mod wake_lock;
mod work_control;
mod work_source;
mod work_start;

use std::io;

use anyhow::Result;
use clap::Parser;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use tracing::{debug, info};

use crate::cli::{Cli, Commands, ToolCommands};

fn main() -> Result<()> {
    use std::time::Instant;

    // Parse CLI args (handles --version, --help, subcommands)
    let cli = Cli::parse();

    // Handle subcommands that don't need the TUI
    match cli.command {
        Some(Commands::Init) => return cli::run_init(),
        Some(Commands::Doctor) => return cli::run_doctor(),
        Some(Commands::Ready { verbose }) => return cli::run_ready(verbose),
        Some(Commands::Logs { id, path }) => return cli::run_logs(id, path),
        Some(Commands::Tool(tool_cmd)) => {
            return match tool_cmd {
                ToolCommands::History {
                    session,
                    tool,
                    since,
                    until,
                    rejected,
                    repo,
                    all,
                    json,
                    db_path,
                } => tool_history::run(tool_history::HistoryOptions {
                    session,
                    tool,
                    since,
                    until,
                    rejected,
                    json,
                    show_db_path: db_path,
                    repo,
                    all,
                }),
                ToolCommands::Allow { pattern, project } => {
                    tool_settings::allow_pattern(&pattern, project)
                }
                ToolCommands::Deny { pattern, project } => {
                    tool_settings::deny_pattern(&pattern, project)
                }
                ToolCommands::List => tool_settings::list_rules(),
            };
        }
        None => {}
    }

    let start_time = Instant::now();

    // Generate session ID first (always available, even if logging fails)
    let session_id = logging::new_session_id();

    // Load configuration first (needed for log level)
    let loaded_config = config::load_config();

    // Initialize logging with config log level
    let (log_directory, _guard, reload_handle) =
        match logging::init(session_id.clone(), &loaded_config.config.logging.level) {
            Ok(ctx) => {
                // Clean up old log files after logging is initialized
                logging::cleanup_old_logs(&ctx.log_directory);
                (
                    Some(ctx.log_directory),
                    Some(ctx._guard),
                    Some(ctx.reload_handle),
                )
            }
            Err(e) => {
                eprintln!("Warning: Failed to initialize logging: {}", e);
                (None, None, None)
            }
        };

    // Log config status after logging is initialized
    debug!(
        project_config = ?loaded_config.project_config_path.as_ref().map(|p| p.display().to_string()),
        log_level = %loaded_config.config.logging.level,
        "config_loaded"
    );

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let terminal = Terminal::new(ratatui::backend::CrosstermBackend::new(stdout))?;

    let result = startup::run_app(
        terminal,
        session_id.clone(),
        log_directory,
        loaded_config,
        reload_handle,
    );

    // Restore terminal
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

    // Log session end
    let duration = start_time.elapsed();
    info!(
        session_id = %session_id,
        duration_secs = duration.as_secs_f64(),
        "session_end"
    );

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_no_args_parses_successfully() {
        Cli::try_parse_from(["ralph"]).unwrap();
    }

    #[test]
    fn cli_version_flag_exits() {
        let result = Cli::try_parse_from(["ralph", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn cli_help_flag_exits() {
        let result = Cli::try_parse_from(["ralph", "--help"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_unknown_arg_fails() {
        let result = Cli::try_parse_from(["ralph", "--bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_init_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "init"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Init)));
    }

    #[test]
    fn cli_no_subcommand_gives_none() {
        let cli = Cli::try_parse_from(["ralph"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_unknown_subcommand_fails() {
        let result = Cli::try_parse_from(["ralph", "bogus"]);
        assert!(result.is_err());
    }

    #[test]
    fn cli_reinit_subcommand_rejected() {
        assert!(Cli::try_parse_from(["ralph", "reinit"]).is_err());
    }

    #[test]
    fn cli_doctor_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "doctor"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Doctor)));
    }

    #[test]
    fn cli_tool_history_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tool(ToolCommands::History { .. }))
        ));
    }

    #[test]
    fn cli_tool_history_session_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history", "--session", "abc123"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { session, .. })) => {
                assert_eq!(session.as_deref(), Some("abc123"));
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_tool_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history", "--tool", "Bash"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { tool, .. })) => {
                assert_eq!(tool.as_deref(), Some("Bash"));
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_since_parses() {
        let cli =
            Cli::try_parse_from(["ralph", "tool", "history", "--since", "6h", "--until", "1h"])
                .unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { since, until, .. })) => {
                assert_eq!(since.as_deref(), Some("6h"));
                assert_eq!(until.as_deref(), Some("1h"));
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_flags_parse() {
        let cli =
            Cli::try_parse_from(["ralph", "tool", "history", "--rejected", "--json"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { rejected, json, .. })) => {
                assert!(rejected);
                assert!(json);
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_history_db_path_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "history", "--db-path"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::History { db_path, .. })) => {
                assert!(db_path);
            }
            _ => panic!("Expected Tool History"),
        }
    }

    #[test]
    fn cli_tool_allow_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "allow", "Read"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::Allow { pattern, project })) => {
                assert_eq!(pattern, "Read");
                assert!(!project);
            }
            _ => panic!("Expected Tool Allow"),
        }
    }

    #[test]
    fn cli_tool_allow_project_flag_parses() {
        let cli =
            Cli::try_parse_from(["ralph", "tool", "allow", "Bash(git:*)", "--project"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::Allow { pattern, project })) => {
                assert_eq!(pattern, "Bash(git:*)");
                assert!(project);
            }
            _ => panic!("Expected Tool Allow"),
        }
    }

    #[test]
    fn cli_tool_deny_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "deny", "Bash(rm:*)"]).unwrap();
        match cli.command {
            Some(Commands::Tool(ToolCommands::Deny { pattern, project })) => {
                assert_eq!(pattern, "Bash(rm:*)");
                assert!(!project);
            }
            _ => panic!("Expected Tool Deny"),
        }
    }

    #[test]
    fn cli_tool_list_parses() {
        let cli = Cli::try_parse_from(["ralph", "tool", "list"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Tool(ToolCommands::List))
        ));
    }

    #[test]
    fn assemble_prompt_includes_prompt_and_workflow_file() {
        let config = crate::config::Config::default();
        let command = execution::assemble_prompt(&config, None, None).unwrap();

        // Should pipe prompt and beads workflow content through Claude CLI
        assert!(command.contains("ralph-prompt.md") || command.contains("PROMPT.md"));
        assert!(command.contains("ralph-beads.md"));
        assert!(command.contains("--output-format=stream-json"));
        assert!(command.contains("--print"));
    }

    #[test]
    fn cli_ready_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "ready"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Ready { verbose: false })
        ));
    }

    #[test]
    fn cli_ready_verbose_parses() {
        let cli = Cli::try_parse_from(["ralph", "ready", "--verbose"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Ready { verbose: true })
        ));
    }

    #[test]
    fn cli_logs_subcommand_parses() {
        let cli = Cli::try_parse_from(["ralph", "logs"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Logs {
                id: None,
                path: false
            })
        ));
    }

    #[test]
    fn cli_logs_path_flag_parses() {
        let cli = Cli::try_parse_from(["ralph", "logs", "--path"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Logs {
                id: None,
                path: true
            })
        ));
    }

    #[test]
    fn cli_logs_id_parses() {
        let cli = Cli::try_parse_from(["ralph", "logs", "--id", "abc123"]).unwrap();
        match cli.command {
            Some(Commands::Logs { id, path }) => {
                assert_eq!(id.as_deref(), Some("abc123"));
                assert!(!path);
            }
            _ => panic!("Expected Logs"),
        }
    }
}
