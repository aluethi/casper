//! `delegate` tool: delegates a task to another agent.
//!
//! Returns a `__delegate__` sentinel that the engine intercepts. The engine
//! then spawns a child agent run in an ephemeral conversation with timeout
//! and depth-limit enforcement. See `engine::helpers::execute_delegation`.

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

/// Built-in tool that delegates a task to another agent.
/// Returns a sentinel that the engine intercepts to run the child agent.
pub struct DelegateTool;

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &str {
        "delegate"
    }

    fn description(&self) -> &str {
        "Delegate a task to another agent. The specified agent will receive your message \
         and respond. Use this when a task is better handled by a specialized agent."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "The name of the agent to delegate to."
                },
                "message": {
                    "type": "string",
                    "description": "The message or task to send to the other agent."
                }
            },
            "required": ["agent", "message"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        let agent = input
            .get("agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CasperError::BadRequest("missing 'agent' string".into()))?;

        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CasperError::BadRequest("missing 'message' string".into()))?;

        tracing::info!(
            from_agent = %ctx.agent_name,
            to_agent = %agent,
            "delegate tool invoked"
        );

        // Return a sentinel that the engine can detect and act on.
        Ok(ToolResult::ok(json!({
            "__delegate__": true,
            "agent": agent,
            "message": message,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = DelegateTool;
        assert_eq!(tool.name(), "delegate");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["agent"].is_object());
        assert!(schema["properties"]["message"].is_object());
    }

    #[tokio::test]
    async fn delegate_returns_sentinel() {
        let tool = DelegateTool;
        let ctx = super::super::tests_common::test_ctx();
        let result = tool
            .execute(json!({ "agent": "devops", "message": "deploy v1.2" }), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content["__delegate__"], true);
        assert_eq!(result.content["agent"], "devops");
        assert_eq!(result.content["message"], "deploy v1.2");
    }
}
