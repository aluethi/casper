use serde::{Deserialize, Serialize};

/// A single block in the agent's prompt stack configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptBlock {
    Text {
        label: String,
        content: String,
    },
    Environment {
        label: String,
    },
    Variable {
        label: String,
        key: String,
        value: String,
    },
    Snippet {
        label: String,
        snippet_name: String,
    },
    AgentMemory {
        label: String,
    },
    TenantMemory {
        label: String,
    },
    Knowledge {
        label: String,
        #[serde(default = "default_budget")]
        budget_tokens: i32,
    },
    Delegates {
        label: String,
        #[serde(default)]
        agents: Vec<DelegateAgent>,
    },
    Datasource {
        label: String,
        source: DatasourceConfig,
        #[serde(default = "default_budget")]
        budget_tokens: i32,
        #[serde(default = "default_on_missing")]
        on_missing: OnMissing,
    },
}

/// A delegate agent entry in the Delegates prompt block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateAgent {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub when: String,
}

fn default_budget() -> i32 { 500 }
fn default_on_missing() -> OnMissing { OnMissing::Skip }

/// Datasource block configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DatasourceConfig {
    Mcp {
        server: String,
        tool: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    Http {
        url: String,
        #[serde(default = "default_http_method")]
        method: String,
        #[serde(default)]
        headers: serde_json::Value,
    },
}

fn default_http_method() -> String { "GET".to_string() }

/// What to do when a datasource's metadata variables are missing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OnMissing {
    Skip,
    Fail,
}

/// A message in the conversation history, ready for prompt assembly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: String,
    pub content: serde_json::Value,
    pub token_count: i32,
}

/// Result of prompt assembly: the system prompt sections and conversation history.
#[derive(Debug, Clone)]
pub struct AssembledPrompt {
    /// System prompt sections (concatenated to form the full system prompt).
    pub system_sections: Vec<PromptSection>,
    /// Conversation history messages (most recent, within budget).
    pub history: Vec<HistoryMessage>,
    /// Total tokens used by system prompt sections.
    pub system_tokens: i32,
    /// Total tokens used by conversation history.
    pub history_tokens: i32,
}

/// A named section of the system prompt.
#[derive(Debug, Clone)]
pub struct PromptSection {
    pub label: String,
    pub content: String,
    pub token_count: i32,
}

/// Estimate token count: ~4 characters per token.
/// This is a rough estimate; tasks 5B-5D will integrate tiktoken-rs.
pub fn estimate_tokens(text: &str) -> i32 {
    (text.len() / 4).max(1) as i32
}

/// Estimate tokens for a JSON value by serializing it.
pub fn estimate_tokens_json(value: &serde_json::Value) -> i32 {
    let s = serde_json::to_string(value).unwrap_or_default();
    estimate_tokens(&s)
}
