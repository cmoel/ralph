use crossterm::event::{KeyCode, KeyModifiers};

use super::overlays::{CloseConfirmState, DeferState, DepDirectionState};
use super::state::{BoardAction, BoardFocus, DepDirection, spawn_bd};
use crate::app::App;

/// Handle keyboard input for the kanban board (primary view).
pub fn handle_kanban_input(app: &mut App, key_code: KeyCode, modifiers: KeyModifiers) {
    let state = &mut app.kanban_board_state;

    // If close confirmation is open, handle its input
    if let Some(confirm) = &mut state.close_confirm {
        match key_code {
            KeyCode::Esc => {
                state.close_confirm = None;
            }
            KeyCode::Enter => {
                let bead_id = confirm.bead_id.clone();
                let reason = confirm.reason.trim().to_string();
                let bd_path = app.config.behavior.bd_path.clone();
                let previous_status = state
                    .find_card(&bead_id)
                    .map(|c| c.status.clone())
                    .unwrap_or_else(|| "open".to_string());
                state.close_confirm = None;
                state.push_action(BoardAction::Close {
                    bead_id: bead_id.clone(),
                    previous_status,
                });
                std::thread::spawn(move || {
                    let mut cmd = std::process::Command::new(&bd_path);
                    cmd.arg("close").arg(&bead_id);
                    if !reason.is_empty() {
                        cmd.arg("--reason").arg(&reason);
                    }
                    cmd.stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .ok();
                });
            }
            KeyCode::Backspace => {
                confirm.delete_char_before();
            }
            KeyCode::Left => {
                confirm.cursor_left();
            }
            KeyCode::Right => {
                confirm.cursor_right();
            }
            KeyCode::Char(c) => {
                confirm.insert_char(c);
            }
            _ => {}
        }
        return;
    }

    // If dep direction picker is open, handle its input
    if let Some(dep_dir) = &state.dep_direction {
        match key_code {
            KeyCode::Esc => {
                state.dep_direction = None;
            }
            KeyCode::Char('1') => {
                app.pending_dep = Some(crate::app::PendingDep {
                    bead_id: dep_dir.bead_id.clone(),
                    direction: DepDirection::BlockedBy,
                });
                state.dep_direction = None;
                app.open_bead_picker();
            }
            KeyCode::Char('2') => {
                app.pending_dep = Some(crate::app::PendingDep {
                    bead_id: dep_dir.bead_id.clone(),
                    direction: DepDirection::Blocks,
                });
                state.dep_direction = None;
                app.open_bead_picker();
            }
            _ => {}
        }
        return;
    }

    // If defer input is open, handle its input
    if let Some(defer) = &mut state.defer_input {
        match key_code {
            KeyCode::Esc => {
                state.defer_input = None;
            }
            KeyCode::Enter => {
                let bead_id = defer.bead_id.clone();
                let until = defer.until.trim().to_string();
                let bd_path = app.config.behavior.bd_path.clone();
                let previous_status = state
                    .find_card(&bead_id)
                    .map(|c| c.status.clone())
                    .unwrap_or_else(|| "open".to_string());
                state.defer_input = None;
                state.push_action(BoardAction::Defer {
                    bead_id: bead_id.clone(),
                    previous_status,
                });
                std::thread::spawn(move || {
                    let mut cmd = std::process::Command::new(&bd_path);
                    if until.is_empty() {
                        cmd.args(["update", &bead_id, "--status=deferred"]);
                    } else {
                        cmd.args(["defer", &bead_id, "--until"]).arg(&until);
                    }
                    cmd.stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .ok();
                });
            }
            KeyCode::Backspace => {
                defer.delete_char_before();
            }
            KeyCode::Left => {
                defer.cursor_left();
            }
            KeyCode::Right => {
                defer.cursor_right();
            }
            KeyCode::Char(c) => {
                defer.insert_char(c);
            }
            _ => {}
        }
        return;
    }

    // If preview pane has focus, handle preview input
    if state.focus == BoardFocus::Preview {
        match key_code {
            KeyCode::Esc | KeyCode::Enter => {
                state.focus = BoardFocus::Board;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(ref mut detail) = state.preview_detail {
                    detail.scroll_offset = detail.scroll_offset.saturating_add(1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(ref mut detail) = state.preview_detail {
                    detail.scroll_offset = detail.scroll_offset.saturating_sub(1);
                }
            }
            KeyCode::Char('?') => {
                app.help_context = Some(crate::modals::HelpContext::Preview);
            }
            _ => {}
        }
        return;
    }

    match key_code {
        KeyCode::Esc => {
            // Board is the primary view — Esc is a no-op
        }
        KeyCode::Enter => {
            // Move focus to preview pane if there's a selected card
            if state.selected_card().is_some() && state.preview_detail.is_some() {
                state.focus = BoardFocus::Preview;
            }
        }
        KeyCode::Char('X') => {
            if let Some(card) = state.selected_card() {
                state.close_confirm = Some(CloseConfirmState {
                    bead_id: card.id.clone(),
                    reason: String::new(),
                    cursor_pos: 0,
                });
            }
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if let Some(card) = state.selected_card()
                && card.priority > 0
            {
                let bead_id = card.id.clone();
                let old_priority = card.priority;
                let new_priority = old_priority - 1;
                let bd_path = app.config.behavior.bd_path.clone();
                state.push_action(BoardAction::ChangePriority {
                    bead_id: bead_id.clone(),
                    old_priority,
                    new_priority,
                });
                let p = new_priority.to_string();
                spawn_bd(&bd_path, &["update", &bead_id, "--priority", &p]);
            }
        }
        KeyCode::Char('-') => {
            if let Some(card) = state.selected_card()
                && card.priority < 4
            {
                let bead_id = card.id.clone();
                let old_priority = card.priority;
                let new_priority = old_priority + 1;
                let bd_path = app.config.behavior.bd_path.clone();
                state.push_action(BoardAction::ChangePriority {
                    bead_id: bead_id.clone(),
                    old_priority,
                    new_priority,
                });
                let p = new_priority.to_string();
                spawn_bd(&bd_path, &["update", &bead_id, "--priority", &p]);
            }
        }
        KeyCode::Char('H') => {
            if let Some(card) = state.selected_card() {
                let bead_id = card.id.clone();
                let has_human = card.labels.contains(&"human".to_string());
                let bd_path = app.config.behavior.bd_path.clone();
                state.push_action(BoardAction::ToggleHumanLabel {
                    bead_id: bead_id.clone(),
                    was_present: has_human,
                });
                let flag = if has_human {
                    "--remove-label=human"
                } else {
                    "--add-label=human"
                };
                spawn_bd(&bd_path, &["update", &bead_id, flag]);
            }
        }
        KeyCode::Char('d') => {
            if let Some(card) = state.selected_card() {
                state.defer_input = Some(DeferState {
                    bead_id: card.id.clone(),
                    until: String::new(),
                    cursor_pos: 0,
                });
            }
        }
        KeyCode::Char('b') => {
            if let Some(card) = state.selected_card() {
                state.dep_direction = Some(DepDirectionState {
                    bead_id: card.id.clone(),
                });
            }
        }
        KeyCode::Char('u') => {
            if let Some(action) = state.undo_stack.pop() {
                let bd_path = app.config.behavior.bd_path.clone();
                action.execute_reverse(&bd_path);
                state.set_status(format!("Undid: {}", action.describe()));
                state.redo_stack.push(action);
            }
        }
        KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(action) = state.redo_stack.pop() {
                let bd_path = app.config.behavior.bd_path.clone();
                action.execute_forward(&bd_path);
                state.set_status(format!("Redid: {}", action.describe()));
                state.undo_stack.push(action);
            }
        }
        KeyCode::Char('?') => {
            app.help_context = Some(crate::modals::HelpContext::Board);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.move_left();
            state.schedule_preview_fetch();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            state.move_right();
            state.schedule_preview_fetch();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.move_up();
            state.schedule_preview_fetch();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            state.move_down();
            state.schedule_preview_fetch();
        }
        _ => {}
    }
}
