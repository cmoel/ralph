use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{Event, KeyCode};
use ratatui::DefaultTerminal;

use crate::app::{App, AppStatus};
use crate::config::{compute_project_config_path, load_project_config};
use crate::execution;
use crate::modals::{
    ConfigModalState, InitModalState, WorkersStreamState, handle_bead_picker_input,
    handle_config_modal_input, handle_init_modal_input, handle_kanban_input,
    handle_tool_allow_modal_input, handle_workers_stream_input,
};
use crate::output;
use crate::perf::{self, PerfReporter};
use crate::startup::{ensure_worktree, merge_and_refresh_worktree};
use crate::ui::draw_ui;

pub(crate) fn run_event_loop(app: &mut App, terminal: &mut DefaultTerminal) -> Result<()> {
    let mut reporter = PerfReporter::new();
    loop {
        perf::record_loop_iter();
        reporter.maybe_flush();

        // Poll for output from child process
        output::poll_output(app);

        // Poll for background worker startup completion
        app.poll_worker_start();

        // Handle auto-continue for all workers
        for w_idx in 0..app.workers.len() {
            if app.workers[w_idx].auto_continue_pending {
                app.dirty = true;
                app.workers[w_idx].auto_continue_pending = false;
                app.selected_worker = w_idx;

                if !merge_and_refresh_worktree(app) {
                    continue;
                }

                app.workers[w_idx].increment_iteration();
                execution::claim_before_start(app);
                if !ensure_worktree(app) {
                    continue;
                }
                execution::start_command(app)?;
                app.update_derived_status();
            }
        }
        app.selected_worker = 0;

        // Poll for background work source operations
        app.poll_work_check();
        app.poll_kanban_items();
        app.poll_bead_detail();
        app.poll_preview_fetch();
        app.poll_board_mutations();
        app.poll_bead_picker();
        app.poll_pending_dep();

        // Poll for current bead (throttled to every 2 seconds)
        app.poll_bead();

        // Poll for config file changes (throttled to every 2 seconds)
        app.poll_config();

        // Poll for background doctor check results
        app.poll_doctor();

        // Auto-clear error flash after timeout
        app.check_error_timeout();

        // Auto-clear hint after timeout
        app.check_hint_timeout();

        // Draw UI only when state changed
        if app.dirty {
            let draw_start = Instant::now();
            terminal.draw(|f| draw_ui(f, app))?;
            perf::record_redraw(draw_start.elapsed());
            app.dirty = false;
        }

        // State-dependent poll timeout: fast when running/starting, slow when idle
        let poll_timeout = match app.status {
            AppStatus::Running | AppStatus::Starting => Duration::from_millis(50),
            _ => Duration::from_millis(250),
        };
        if crossterm::event::poll(poll_timeout)? {
            let event = crossterm::event::read()?;
            app.dirty = true;

            // Clear hint on any keypress
            if matches!(event, Event::Key(_)) {
                app.hint = None;
            }

            // Handle popup dismissal first
            if app.show_already_running_popup {
                if let Event::Key(key) = event
                    && (key.code == KeyCode::Enter || key.code == KeyCode::Esc)
                {
                    app.show_already_running_popup = false;
                }
                continue;
            }

            // Handle bead picker input
            if app.show_bead_picker {
                if let Event::Key(key) = event {
                    handle_bead_picker_input(app, key.code);
                }
                continue;
            }

            // Handle config modal input
            if app.show_config_modal {
                if let Event::Key(key) = event {
                    handle_config_modal_input(app, key.code, key.modifiers);
                }
                continue;
            }

            // Handle init modal input
            if app.show_init_modal {
                if let Event::Key(key) = event {
                    handle_init_modal_input(app, key.code);
                }
                continue;
            }

            // Handle quit confirmation modal input
            if app.show_quit_modal {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            return Ok(());
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.show_quit_modal = false;
                        }
                        _ => {}
                    }
                }
                continue;
            }

            // Handle help modal input — overlay on top of everything
            if app.help_context.is_some() {
                if let Event::Key(key) = event {
                    match key.code {
                        KeyCode::Char('?') | KeyCode::Esc => {
                            app.help_context = None;
                        }
                        KeyCode::Char('S') => match app.status {
                            AppStatus::Stopped | AppStatus::Error => {
                                app.help_context = None;
                                app.begin_starting_workers();
                            }
                            AppStatus::Running => {
                                app.help_context = None;
                                app.stop_command();
                            }
                            AppStatus::Starting => {}
                        },
                        KeyCode::Char('q') => {
                            app.help_context = None;
                            if app.status == AppStatus::Running {
                                app.set_hint("press s to stop the loop");
                            } else {
                                app.show_quit_modal = true;
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }

            // Handle tool allow modal input
            if app.show_tool_allow_modal {
                if let Event::Key(key) = event {
                    handle_tool_allow_modal_input(app, key.code, key.modifiers);
                }
                continue;
            }

            // Handle workers stream modal input
            if app.show_workers_stream {
                if let Event::Key(key) = event {
                    handle_workers_stream_input(app, key.code, key.modifiers);
                }
                continue;
            }

            // App-level keys handled before board, then fall through to board input
            if let Event::Key(key) = event {
                match key.code {
                    KeyCode::Char('q') => {
                        if app.status == AppStatus::Running {
                            app.set_hint("press s to stop the loop");
                        } else {
                            app.show_quit_modal = true;
                        }
                    }
                    KeyCode::Char('S') => match app.status {
                        AppStatus::Stopped | AppStatus::Error => {
                            app.begin_starting_workers();
                        }
                        AppStatus::Running => {
                            app.stop_command();
                        }
                        AppStatus::Starting => {}
                    },
                    KeyCode::Char('c') => {
                        app.show_config_modal = true;
                        let project_path = app
                            .project_config_path
                            .clone()
                            .or_else(compute_project_config_path);
                        let partial = project_path
                            .as_ref()
                            .filter(|p| p.exists())
                            .and_then(|p| load_project_config(p).ok())
                            .unwrap_or_default();
                        app.config_modal_state = Some(ConfigModalState::from_config(
                            &partial,
                            &app.config,
                            project_path,
                        ));
                    }
                    KeyCode::Char('i') => {
                        app.show_init_modal = true;
                        app.init_modal_state = Some(InitModalState::new(&app.config));
                    }
                    KeyCode::Char('w') if !app.workers.is_empty() => {
                        app.show_workers_stream = true;
                        app.workers_stream_state =
                            Some(WorkersStreamState::new(app.selected_worker));
                    }
                    _ => {
                        handle_kanban_input(app, key.code, key.modifiers);
                    }
                }
            }
        }
    }
}
