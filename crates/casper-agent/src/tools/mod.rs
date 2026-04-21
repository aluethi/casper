//! Tool trait, dispatcher, and built-in tools for the agent runtime.
//!
//! All tools implement the [`Tool`] trait. The [`ToolDispatcher`] holds a registry
//! of `Arc<dyn Tool>` keyed by name and routes `tool_use` blocks from the LLM
//! to the correct implementation.

pub mod ask_user;
pub mod delegate;
pub mod knowledge_search;
pub mod mcp;
pub mod update_memory;
pub mod web_fetch;
pub mod web_search;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

// ── Tool trait ────────────────────────────────────────────────────

/// Trait for resolving OAuth tokens at runtime. Implemented by the server layer
/// which has access to the vault and connection service.
#[async_trait]
pub trait TokenResolver: Send + Sync {
    /// Resolve a user's OAuth token for a manually-configured provider.
    async fn resolve_user_oauth(
        &self,
        tenant_id: Uuid,
        user_subject: &str,
        provider: &str,
    ) -> Result<String, CasperError>;

    /// Resolve a user's MCP OAuth 2.1 token (auto-discovered).
    async fn resolve_mcp_oauth(
        &self,
        tenant_id: Uuid,
        user_subject: &str,
        mcp_server_url: &str,
    ) -> Result<String, CasperError>;

    /// Start an MCP OAuth 2.1 flow. Returns the authorization URL.
    async fn start_mcp_oauth_flow(
        &self,
        tenant_id: Uuid,
        user_subject: &str,
        mcp_server_url: &str,
    ) -> Result<String, CasperError>;
}

/// Context passed to every tool invocation.
#[derive(Clone)]
pub struct ToolContext {
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub conversation_id: Uuid,
    pub correlation_id: Uuid,
    pub db: PgPool,
    /// The user who triggered this conversation (e.g. "user:jane@acme.com").
    /// Required for MCP servers with `user_oauth` or `mcp_oauth` auth.
    pub invoking_user: Option<String>,
    /// Token resolver for OAuth flows. Provided by the server layer.
    pub token_resolver: Option<Arc<dyn TokenResolver>>,
}

/// The result returned by a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// The content payload returned to the LLM.
    pub content: serde_json::Value,
    /// Whether this result represents an error.
    pub is_error: bool,
}

impl ToolResult {
    /// Create a successful result.
    pub fn ok(content: serde_json::Value) -> Self {
        Self {
            content,
            is_error: false,
        }
    }

    /// Create an error result.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: serde_json::Value::String(message.into()),
            is_error: true,
        }
    }
}

/// Trait that every tool (built-in or custom) must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The unique name of this tool (used in LLM tool definitions).
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema describing the tool's input parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given input and context.
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError>;
}

// ── ToolDispatcher ───────────────────────────────────────────────

/// Registry of available tools, keyed by name. Routes `tool_use` calls
/// from the LLM to the correct [`Tool`] implementation.
pub struct ToolDispatcher {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolDispatcher {
    /// Create a new empty dispatcher.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Overwrites any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    /// Execute a tool by name.
    pub async fn dispatch(
        &self,
        tool_name: &str,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        let tool = self
            .tools
            .get(tool_name)
            .ok_or_else(|| CasperError::BadRequest(format!("unknown tool: {tool_name}")))?;
        tool.execute(input, ctx).await
    }

    /// Return tool definitions in OpenAI function-calling format.
    pub fn tool_definitions(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": tool.parameters_schema(),
                    }
                })
            })
            .collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the dispatcher has no tools.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Return MCP tool summaries grouped by server name.
    ///
    /// Parses the `mcp__{server}__{tool}` prefix to group tools.
    /// Each entry is `(qualified_name, description)`.
    pub fn mcp_tool_summaries(&self) -> HashMap<String, Vec<(String, String)>> {
        let mut groups: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for tool in self.tools.values() {
            let name = tool.name();
            if let Some(rest) = name.strip_prefix("mcp__") {
                if let Some((server, _tool)) = rest.split_once("__") {
                    groups
                        .entry(server.to_string())
                        .or_default()
                        .push((name.to_string(), tool.description().to_string()));
                }
            }
        }
        // Sort tools within each server for deterministic output
        for tools in groups.values_mut() {
            tools.sort_by(|a, b| a.0.cmp(&b.0));
        }
        groups
    }
}

impl Default for ToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

// ── Dispatcher builder ──────────────────────────────────────────

/// Build a [`ToolDispatcher`] from an agent's `tools` JSON config.
///
/// The config shape is:
/// ```json
/// {
///   "builtin": [{ "name": "delegate", ... }, { "name": "web_search", ... }],
///   "mcp": [{ "name": "jira", "url": "https://...", "api_key": "..." }]
/// }
/// ```
///
/// Built-in tools are registered by name. MCP tools are discovered from each
/// configured server and registered with a `mcp__{server}__{tool}` prefix.
/// If an MCP server is unreachable, its tools are silently skipped so the
/// agent can still start.
pub async fn build_dispatcher(
    tools_config: &serde_json::Value,
    http_client: &reqwest::Client,
) -> ToolDispatcher {
    let mut dispatcher = ToolDispatcher::new();

    // ── Built-in tools ──
    if let Some(builtin) = tools_config.get("builtin").and_then(|v| v.as_array()) {
        for entry in builtin {
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
            match name {
                "delegate" => dispatcher.register(Arc::new(delegate::DelegateTool)),
                "ask_user" => dispatcher.register(Arc::new(ask_user::AskUserTool)),
                "knowledge_search" => dispatcher.register(Arc::new(
                    knowledge_search::KnowledgeSearchTool::from_config(entry),
                )),
                "update_memory" => dispatcher.register(Arc::new(
                    update_memory::UpdateMemoryTool::from_config(entry),
                )),
                "web_search" => dispatcher.register(Arc::new(
                    web_search::WebSearchTool::from_config(entry, http_client.clone()),
                )),
                "web_fetch" => dispatcher.register(Arc::new(
                    web_fetch::WebFetchTool::from_config_with_client(entry, http_client.clone()),
                )),
                other => {
                    tracing::warn!(tool = other, "unknown built-in tool in config — skipping");
                }
            }
        }
    }

    // ── MCP tools ──
    if let Some(servers) = tools_config.get("mcp").and_then(|v| v.as_array()) {
        for server_cfg in servers {
            let server_name = server_cfg
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let url = match server_cfg.get("url").and_then(|v| v.as_str()) {
                Some(u) => u,
                None => {
                    tracing::warn!(
                        server = server_name,
                        "MCP server config missing 'url' — skipping"
                    );
                    continue;
                }
            };
            // Parse auth config: { type: "bearer"|"user_oauth"|"none", ... }
            let auth_cfg = server_cfg.get("auth");
            let auth_type = auth_cfg
                .and_then(|a| a.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("bearer"); // default to bearer for backward compat

            let (api_key, mcp_auth) = match auth_type {
                "mcp_oauth" => (None, mcp::McpAuth::McpOAuth),
                "user_oauth" => {
                    let provider = auth_cfg
                        .and_then(|a| a.get("provider"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(server_name)
                        .to_string();
                    (None, mcp::McpAuth::UserOAuth { provider })
                }
                "none" => (None, mcp::McpAuth::None),
                _ => {
                    // Bearer: read api_key from config (backward compat) or auth.token_ref
                    let key = server_cfg
                        .get("api_key")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    (key, mcp::McpAuth::Bearer)
                }
            };

            let client = Arc::new(crate::McpClient::new(url, api_key, http_client.clone()));

            let tools = mcp::discover_and_wrap(server_name, client, mcp_auth).await;
            for tool in tools {
                dispatcher.register(Arc::new(tool));
            }
        }
    }

    tracing::debug!(
        builtin = tools_config
            .get("builtin")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0),
        mcp = dispatcher.len().saturating_sub(
            tools_config
                .get("builtin")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        ),
        total = dispatcher.len(),
        "tool dispatcher built"
    );

    dispatcher
}

// ── Shared test utilities ────────────────────────────────────────

#[cfg(test)]
pub(crate) mod tests_common {
    use super::*;

    pub fn test_ctx() -> ToolContext {
        ToolContext {
            tenant_id: Uuid::nil(),
            agent_name: "test-agent".to_string(),
            conversation_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            invoking_user: None,
            token_resolver: None,
            db: sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .connect_lazy("postgres://localhost/casper_test_nonexistent")
                .unwrap(),
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A trivial tool for testing.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echoes the input back."
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            })
        }
        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: &ToolContext,
        ) -> Result<ToolResult, CasperError> {
            Ok(ToolResult::ok(input))
        }
    }

    use super::tests_common::test_ctx;

    #[test]
    fn dispatcher_register_and_len() {
        let mut d = ToolDispatcher::new();
        assert!(d.is_empty());
        d.register(Arc::new(EchoTool));
        assert_eq!(d.len(), 1);
        assert!(!d.is_empty());
    }

    #[test]
    fn dispatcher_get() {
        let mut d = ToolDispatcher::new();
        d.register(Arc::new(EchoTool));
        assert!(d.get("echo").is_some());
        assert!(d.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn dispatcher_dispatch_ok() {
        let mut d = ToolDispatcher::new();
        d.register(Arc::new(EchoTool));
        let ctx = test_ctx();
        let result = d
            .dispatch("echo", json!({"message": "hello"}), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content["message"], "hello");
    }

    #[tokio::test]
    async fn dispatcher_dispatch_unknown_tool() {
        let d = ToolDispatcher::new();
        let ctx = test_ctx();
        let err = d
            .dispatch("nonexistent", json!({}), &ctx)
            .await
            .unwrap_err();
        assert!(matches!(err, CasperError::BadRequest(_)));
    }

    #[test]
    fn dispatcher_tool_definitions_openai_format() {
        let mut d = ToolDispatcher::new();
        d.register(Arc::new(EchoTool));
        let defs = d.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["type"], "function");
        assert_eq!(defs[0]["function"]["name"], "echo");
        assert!(defs[0]["function"]["parameters"].is_object());
    }

    #[test]
    fn tool_result_ok() {
        let r = ToolResult::ok(json!("success"));
        assert!(!r.is_error);
        assert_eq!(r.content, json!("success"));
    }

    #[test]
    fn tool_result_error() {
        let r = ToolResult::error("something went wrong");
        assert!(r.is_error);
        assert_eq!(r.content, json!("something went wrong"));
    }
}
