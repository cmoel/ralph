//! Claude CLI NDJSON event types.
//!
//! The Claude CLI outputs newline-delimited JSON with various event types.
//! This module provides typed deserialization for these events.

use serde::Deserialize;

/// Top-level event wrapper that discriminates by "type" field.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeEvent {
    // Claude CLI wrapper events
    #[serde(rename = "system")]
    System(SystemEvent),
    #[serde(rename = "assistant")]
    Assistant(AssistantEvent),
    #[serde(rename = "result")]
    Result(ResultEvent),
    #[serde(rename = "stream_event")]
    StreamEvent { event: StreamInnerEvent },
    #[serde(rename = "user")]
    #[allow(dead_code)]
    User(UserEvent),

    // Heartbeat
    #[serde(rename = "ping")]
    Ping,
}

/// User event (tool results, etc.) - we skip these.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UserEvent {
    #[serde(default)]
    pub message: Option<serde_json::Value>,
}

/// Inner streaming events wrapped by stream_event.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum StreamInnerEvent {
    #[serde(rename = "message_start")]
    MessageStart(MessageStartEvent),
    #[serde(rename = "content_block_start")]
    ContentBlockStart(ContentBlockStartEvent),
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta(ContentBlockDeltaEvent),
    #[serde(rename = "content_block_stop")]
    ContentBlockStop(ContentBlockStopEvent),
    #[serde(rename = "message_delta")]
    MessageDelta(MessageDeltaEvent),
    #[serde(rename = "message_stop")]
    MessageStop,
}

/// System initialization event from Claude CLI.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SystemEvent {
    #[serde(default)]
    pub subtype: Option<String>,
}

/// Assistant turn marker from Claude CLI.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AssistantEvent {
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// Result event with usage statistics from Claude CLI.
/// Fields will be used in Slice 3 (Usage Summary).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ResultEvent {
    #[serde(default)]
    pub total_cost_usd: Option<f64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

/// Token usage information.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct UsageInfo {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
}

/// Message start event with message metadata.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MessageStartEvent {
    #[serde(default)]
    pub message: Option<MessageInfo>,
}

/// Message metadata.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MessageInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

/// Content block start event.
#[derive(Debug, Deserialize)]
pub struct ContentBlockStartEvent {
    pub index: usize,
    pub content_block: ContentBlock,
}

/// Content block types.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text {
        #[serde(default)]
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        #[serde(default)]
        #[allow(dead_code)]
        id: Option<String>,
        name: String,
        #[serde(default)]
        #[allow(dead_code)]
        input: serde_json::Value,
    },
}

/// Content block delta event.
#[derive(Debug, Deserialize)]
pub struct ContentBlockDeltaEvent {
    pub index: usize,
    pub delta: Delta,
}

/// Delta types for streaming content.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
}

/// Content block stop event.
#[derive(Debug, Deserialize)]
pub struct ContentBlockStopEvent {
    pub index: usize,
}

/// Message delta event (typically contains stop reason and usage).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MessageDeltaEvent {
    #[serde(default)]
    pub delta: Option<MessageDeltaInfo>,
    #[serde(default)]
    pub usage: Option<UsageInfo>,
}

/// Message delta information.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MessageDeltaInfo {
    #[serde(default)]
    pub stop_reason: Option<String>,
}
