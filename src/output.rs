//! Output processing pipeline — drains the mpsc channel and processes Claude NDJSON events.

use crate::app::App;
use crate::db;
use crate::events::{
    ClaudeEvent, ContentBlock, Delta, StreamInnerEvent, ToolResultContent, UserContent,
};
use crate::tool_panel::{ContentBlockState, PendingToolCall, ToolCallEntry, ToolCallStatus};
use crate::ui::{
    ExchangeType, extract_text_from_task_result, extract_tool_summary,
    format_assistant_header_styled, format_no_result_warning_styled, format_tool_result_styled,
    format_tool_summary_styled, format_usage_summary,
};

use std::sync::mpsc::TryRecvError;

use ratatui::text::{Line, Span};
use tracing::{debug, info, trace, warn};

/// Message types for output processing.
pub enum OutputMessage {
    Line(String),
}

/// Adds indentation to a styled Line by prepending "  " to the first span.
fn indent_line(line: Line<'static>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
    spans.extend(line.spans);
    Line::from(spans)
}

/// Poll for output from all workers' child processes.
pub fn poll_output(app: &mut App) {
    let display_worker = app.selected_worker;
    for w_idx in 0..app.workers.len() {
        // Temporarily set selected_worker so process_line adds output to the right worker
        app.selected_worker = w_idx;
        poll_worker_output(app, w_idx);
    }
    app.selected_worker = display_worker;
}

/// Poll output for a single worker.
fn poll_worker_output(app: &mut App, w: usize) {
    // First, collect all pending messages
    let mut messages: Vec<OutputMessage> = Vec::new();
    let mut channel_disconnected = false;

    if let Some(rx) = &app.workers[w].output_receiver {
        loop {
            match rx.try_recv() {
                Ok(msg) => messages.push(msg),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    channel_disconnected = true;
                    break;
                }
            }
        }
    }

    // Process collected messages
    if !messages.is_empty() {
        app.dirty = true;
    }
    for msg in messages {
        let OutputMessage::Line(line) = msg;
        process_line(app, &line);
    }

    // Check if the channel disconnected (all senders dropped = readers finished)
    if channel_disconnected {
        debug!(worker = w, "channel_disconnected");

        // Try to get exit status from child process
        let (exit_code, exit_status): (Option<i32>, Option<String>) =
            if let Some(mut child) = app.workers[w].child_process.take() {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if let Some(code) = status.code() {
                            if code != 0 {
                                warn!(worker = w, exit_code = code, "process_exit_nonzero");
                            }
                            (Some(code), Some(format!("exit_code={}", code)))
                        } else {
                            // Process was terminated by signal (Unix)
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                if let Some(signal) = status.signal() {
                                    info!(worker = w, signal, "process_killed_by_signal");
                                    (None, Some(format!("signal={}", signal)))
                                } else {
                                    (None, Some("unknown".to_string()))
                                }
                            }
                            #[cfg(not(unix))]
                            {
                                (None, Some("unknown".to_string()))
                            }
                        }
                    }
                    Ok(None) => {
                        // Still running, put it back (shouldn't happen if channel disconnected)
                        app.workers[w].child_process = Some(child);
                        return;
                    }
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };

        // Log loop_end with exit status
        let status_str = exit_status.unwrap_or_else(|| "unknown".to_string());
        info!(
            worker = w,
            loop_number = app.loop_count,
            exit_status = %status_str,
            "loop_end"
        );

        app.handle_channel_disconnected(w, exit_code);
    }
}

/// Parse and process a single NDJSON line.
fn process_line(app: &mut App, line: &str) {
    // Skip empty lines
    if line.trim().is_empty() {
        return;
    }

    // Log raw JSON at TRACE level for protocol debugging.
    // SECURITY: TRACE may include full API responses. Acceptable at this level
    // since TRACE is never enabled in normal operation — only for local debugging.
    trace!(json = line, "raw_json_line");

    // Handle stderr lines (pass through as-is)
    if line.starts_with("[stderr]") {
        app.add_text_line(line.to_string());
        return;
    }

    // Try to parse as JSON
    match serde_json::from_str::<ClaudeEvent>(line) {
        Ok(event) => process_event(app, event),
        Err(e) => {
            // Check if this is an unknown event type by trying to parse as generic JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                    warn!(event_type, "Unknown event type, skipping");
                } else {
                    warn!(?e, "Failed to parse JSON line (no type field)");
                }
            } else {
                warn!(?e, "Malformed JSON line, skipping");
            }
        }
    }
}

/// Process a parsed Claude event.
fn process_event(app: &mut App, event: ClaudeEvent) {
    match event {
        ClaudeEvent::Ping => {
            // Silently ignore ping events
            debug!("Received ping");
        }
        ClaudeEvent::RateLimit => {
            // Rate limit info is logged; the actual error message comes via
            // the subsequent Result event with is_error=true.
            debug!("Rate limit event received");
        }
        // SECURITY: DEBUG logs full event structures. Acceptable since DEBUG
        // is only enabled for local development, never in distributed logs.
        ClaudeEvent::System(sys) => {
            debug!(?sys, "System event");
        }
        ClaudeEvent::Assistant(asst) => {
            debug!(?asst, "Assistant event");
        }
        ClaudeEvent::User(user_event) => {
            debug!(?user_event, "User event");
            // Process tool results from user event
            if let Some(message) = user_event.message {
                for content in message.content {
                    match content {
                        UserContent::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            // Look up tool name from our mapping
                            let tool_name = app
                                .tool_panel
                                .id_to_name
                                .get(&tool_use_id)
                                .cloned()
                                .unwrap_or_else(|| {
                                    warn!(tool_use_id, "Orphan tool result (unknown ID)");
                                    "unknown".to_string()
                                });

                            let is_error = is_error.unwrap_or(false);

                            // Extract content string
                            let content_str = match content {
                                Some(ToolResultContent::Text(s)) => s,
                                Some(ToolResultContent::Structured(v)) => v.to_string(),
                                None => String::new(),
                            };

                            // For Task tool results, extract text from JSON array format
                            let content_str = if tool_name == "Task" {
                                extract_text_from_task_result(&content_str).unwrap_or(content_str)
                            } else {
                                content_str
                            };

                            // Update tool call record with result
                            if let Some(ref conn) = app.tool_history_db
                                && !db::update_tool_result(
                                    conn,
                                    &tool_use_id,
                                    &app.session_id,
                                    is_error,
                                    &content_str,
                                )
                            {
                                app.add_text_line(
                                    "[Warning: failed to update tool result]".to_string(),
                                );
                            }

                            // Update tool panel entry status
                            let panel_status = if is_error {
                                ToolCallStatus::Error
                            } else {
                                ToolCallStatus::Success
                            };
                            app.tool_panel.update_status(&tool_use_id, panel_status);

                            // Check for pending tool call to correlate with
                            if let Some(pending) = app.tool_panel.pending_calls.remove(&tool_use_id)
                            {
                                // Display tool call first
                                app.add_line(pending.styled_line);
                                // Display result indented under call
                                let lines = format_tool_result_styled(
                                    &pending.tool_name,
                                    &content_str,
                                    is_error,
                                );
                                for line in lines {
                                    // Add indentation to styled line
                                    let indented = indent_line(line);
                                    app.add_line(indented);
                                }
                            } else {
                                // No pending call found - display result standalone
                                let lines =
                                    format_tool_result_styled(&tool_name, &content_str, is_error);
                                for line in lines {
                                    app.add_line(line);
                                }
                            }
                        }
                    }
                }
            }
        }
        ClaudeEvent::StreamEvent { event: inner } => {
            // Unwrap and process the inner streaming event
            process_stream_event(app, inner);
        }
        ClaudeEvent::Result(result) => {
            debug!(?result, "Result event");
            // Store error message from result event (e.g. rate limit)
            if result.is_error.unwrap_or(false)
                && let Some(ref msg) = result.result
            {
                let w = app.selected_worker;
                app.workers[w].last_result_error = Some(msg.clone());
            }
            // Flush any pending tool calls that never received results
            let pending_calls: Vec<_> = app.tool_panel.pending_calls.drain().collect();
            for (_id, pending) in pending_calls {
                app.add_line(pending.styled_line);
                app.add_line(indent_line(format_no_result_warning_styled()));
            }
            // Increment exchange counter
            app.exchange_count += 1;
            // Accumulate tokens for session total
            if let Some(usage) = &result.usage {
                let input = usage.input_tokens.unwrap_or(0);
                let output = usage.output_tokens.unwrap_or(0);
                app.cumulative_tokens += input + output;
            }
            // Determine exchange type
            let exchange_type = if app.exchange_count == 1 {
                ExchangeType::InitialPrompt
            } else if let Some(tool_name) = app.last_tool_used.take() {
                ExchangeType::AfterTool(tool_name)
            } else {
                ExchangeType::Continuation
            };
            // Display usage summary with exchange info
            let summary = format_usage_summary(&result, app.exchange_count, exchange_type);
            for line in summary.lines() {
                app.add_text_line(line.to_string());
            }
        }
    }
}

/// Process inner streaming events (unwrapped from stream_event).
fn process_stream_event(app: &mut App, event: StreamInnerEvent) {
    let w = app.selected_worker;
    match event {
        StreamInnerEvent::MessageStart(msg) => {
            debug!(?msg, "Message start");
            // Clear content blocks for new message
            app.workers[w].content_blocks.clear();
            // Clear pending tool calls (new assistant turn)
            app.tool_panel.pending_calls.clear();
        }
        StreamInnerEvent::ContentBlockStart(block_start) => {
            let index = block_start.index;
            let mut state = ContentBlockState::default();

            match block_start.content_block {
                ContentBlock::Text { text } => {
                    state.text = text;
                }
                ContentBlock::ToolUse { id, name, .. } => {
                    state.tool_name = Some(name);
                    state.tool_use_id = id;
                }
            }

            app.workers[w].content_blocks.insert(index, state);
            debug!(index, "Content block started");
        }
        StreamInnerEvent::ContentBlockDelta(delta_event) => {
            let index = delta_event.index;

            match delta_event.delta {
                Delta::TextDelta { text } => {
                    // Check if we need to show the header (without holding mutable borrow)
                    let needs_header = app.workers[w]
                        .content_blocks
                        .get(&index)
                        .map(|s| !s.header_shown)
                        .unwrap_or(true);

                    if needs_header {
                        app.add_line(format_assistant_header_styled());
                    }

                    // Update state in a separate scope to release the borrow
                    {
                        let state = app.workers[w].content_blocks.entry(index).or_default();
                        state.header_shown = true;
                        state.text.push_str(&text);
                    }

                    // Display text immediately as it streams (indented)
                    app.append_indented_text(&text);
                }
                Delta::InputJsonDelta { partial_json } => {
                    let state = app.workers[w].content_blocks.entry(index).or_default();
                    state.input_json.push_str(&partial_json);
                }
            }
        }
        StreamInnerEvent::ContentBlockStop(stop) => {
            debug!(index = stop.index, "Content block stopped");
            // Flush any pending text (uses indentation flag automatically)
            app.flush_current_line();
            // Extract data from content block state before mutating app
            let block_data = app.workers[w]
                .content_blocks
                .get(&stop.index)
                .and_then(|state| {
                    state.tool_name.as_ref().map(|name| {
                        (
                            name.clone(),
                            state.tool_use_id.clone(),
                            state.input_json.clone(),
                        )
                    })
                });
            // Then process tool_use blocks
            if let Some((tool_name, tool_use_id, input_json)) = block_data {
                // Register tool_use_id → tool_name mapping for result correlation
                if let Some(ref id) = tool_use_id {
                    app.tool_panel
                        .id_to_name
                        .insert(id.clone(), tool_name.clone());
                }
                // Record tool call to history DB
                if let Some(ref conn) = app.tool_history_db {
                    app.tool_call_sequence += 1;
                    if db::insert_tool_call(
                        conn,
                        &app.session_id,
                        &tool_name,
                        tool_use_id.as_deref(),
                        &input_json,
                        app.tool_call_sequence,
                        &app.repo_path,
                    )
                    .is_none()
                    {
                        app.add_text_line("[Warning: failed to record tool call]".to_string());
                    }
                }
                // Track the last tool used for exchange categorization
                app.last_tool_used = Some(tool_name.clone());
                // Add entry to tool panel
                let summary = extract_tool_summary(&tool_name, &input_json);
                app.tool_panel.add_entry(ToolCallEntry {
                    tool_name: tool_name.clone(),
                    summary,
                    status: ToolCallStatus::Pending,
                    tool_use_id: tool_use_id.clone(),
                });
                let styled_line = format_tool_summary_styled(&tool_name, &input_json);
                // Buffer tool call if it has an ID (for correlation with result)
                if let Some(ref id) = tool_use_id {
                    app.tool_panel.pending_calls.insert(
                        id.clone(),
                        PendingToolCall {
                            tool_name: tool_name.clone(),
                            styled_line,
                        },
                    );
                } else {
                    // No ID - display immediately
                    app.add_line(styled_line);
                }
            }
        }
        StreamInnerEvent::MessageDelta(delta) => {
            debug!(?delta, "Message delta");
        }
        StreamInnerEvent::MessageStop => {
            debug!("Message stopped");
            // Flush any remaining text
            app.flush_current_line();
        }
    }
}
