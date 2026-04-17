use std::fmt;

use serde::{Deserialize, Serialize};

/// The role of a chat message participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl MessageRole {
    /// Return the role as a lowercase string slice.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Unified LLM request format used internally by the proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    /// Chat messages.
    pub messages: Vec<Message>,
    /// The actual model name (provider model ID, not deployment slug).
    pub model: String,
    /// Maximum tokens to generate.
    pub max_tokens: Option<i32>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Whether to stream the response (only non-streaming supported for now).
    pub stream: bool,
    /// Tool definitions for function calling.
    pub tools: Option<Vec<serde_json::Value>>,
    /// Any additional/pass-through parameters.
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// A single chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    /// Content can be a string or array of content blocks.
    pub content: serde_json::Value,
}

/// Unified LLM response format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub role: MessageRole,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: Option<i32>,
    pub cache_write_tokens: Option<i32>,
    pub tool_calls: Option<Vec<serde_json::Value>>,
    pub finish_reason: Option<String>,
    /// Model reasoning/thinking content (Anthropic thinking blocks, OpenAI reasoning_content).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// A single streaming event, normalized across providers.
///
/// Sent from the proxy layer through the engine to the SSE endpoint.
/// The `event` tag matches the SSE `event:` field name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Extended thinking / reasoning tokens.
    Thinking { delta: String },
    /// Content token(s) from the LLM.
    ContentDelta { delta: String },
    /// The LLM is requesting a tool call (emitted with full accumulated input).
    ToolCallStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Result of executing a tool.
    ToolResult {
        id: String,
        name: String,
        content: String,
        is_error: bool,
    },
    /// Stream finished. Carries conversation ID and final usage.
    Done {
        conversation_id: String,
        input_tokens: i32,
        output_tokens: i32,
        cache_read_tokens: Option<i32>,
        cache_write_tokens: Option<i32>,
    },
    /// Unrecoverable error.
    Error { message: String },
}
