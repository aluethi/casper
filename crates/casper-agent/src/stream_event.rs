use serde::{Deserialize, Serialize};

/// A single streaming event sent from the agent engine to the SSE endpoint.
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
    /// An MCP tool requires a user OAuth connection that doesn't exist yet.
    ConnectRequired {
        provider: String,
        display_name: String,
    },
    /// MCP server requires OAuth 2.1 authorization.
    McpOAuthRequired {
        mcp_server_url: String,
        authorization_url: String,
    },
    /// Agent is asking the user a question.
    AskUser {
        question_id: String,
        question: String,
        options: Vec<String>,
    },
    /// Unrecoverable error.
    Error { message: String },
}
