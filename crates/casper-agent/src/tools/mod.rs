//! Tool trait, dispatcher, and built-in tools for the agent runtime.
//!
//! All tools implement the [`Tool`] trait. The [`ToolDispatcher`] holds a registry
//! of `Arc<dyn Tool>` keyed by name and routes `tool_use` blocks from the LLM
//! to the correct implementation.

pub mod ask_user;
pub mod delegate;
pub mod knowledge_search;
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

/// Context passed to every tool invocation.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub conversation_id: Uuid,
    pub correlation_id: Uuid,
    pub db: PgPool,
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
        let tool = self.tools.get(tool_name).ok_or_else(|| {
            CasperError::BadRequest(format!("unknown tool: {tool_name}"))
        })?;
        tool.execute(input, ctx).await
    }

    /// Return tool definitions formatted for the LLM API (Anthropic format).
    pub fn tool_definitions(&self) -> Vec<serde_json::Value> {
        self.tools
            .values()
            .map(|tool| {
                serde_json::json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "input_schema": tool.parameters_schema(),
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
}

impl Default for ToolDispatcher {
    fn default() -> Self {
        Self::new()
    }
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
    fn dispatcher_tool_definitions() {
        let mut d = ToolDispatcher::new();
        d.register(Arc::new(EchoTool));
        let defs = d.tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["name"], "echo");
        assert!(defs[0]["input_schema"].is_object());
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
