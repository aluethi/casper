//! `ask_user` tool: requests input from the human user.
//!
//! Returns an `__ask_user__` sentinel that the engine intercepts. In the
//! streaming path, the engine sends a `StreamEvent::AskUser` to the client,
//! then awaits the user's response via the `ask_rx` channel. The response
//! endpoint `POST /api/v1/agents/respond` delivers the answer back.

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

/// Built-in tool that asks the user a question and pauses for a response.
/// The engine checks for `__ask_user__: true` in the result and pauses accordingly.
pub struct AskUserTool;

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a question when you need clarification or a decision. \
         You may provide a list of options for the user to choose from."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user."
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of choices for the user to pick from."
                }
            },
            "required": ["question"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        let question = input
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CasperError::BadRequest("missing 'question' string".into()))?;

        let options = input
            .get("options")
            .cloned()
            .unwrap_or(json!([]));

        tracing::info!(
            agent = %ctx.agent_name,
            question = %question,
            "ask_user tool invoked"
        );

        // Return a sentinel that the engine can detect and act on.
        Ok(ToolResult::ok(json!({
            "__ask_user__": true,
            "question": question,
            "options": options,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = AskUserTool;
        assert_eq!(tool.name(), "ask_user");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["question"].is_object());
        assert!(schema["properties"]["options"].is_object());
    }

    #[tokio::test]
    async fn ask_user_returns_sentinel() {
        let tool = AskUserTool;
        let ctx = super::super::tests_common::test_ctx();
        let result = tool
            .execute(
                json!({ "question": "Which environment?", "options": ["staging", "prod"] }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content["__ask_user__"], true);
        assert_eq!(result.content["question"], "Which environment?");
    }

    #[tokio::test]
    async fn ask_user_no_options() {
        let tool = AskUserTool;
        let ctx = super::super::tests_common::test_ctx();
        let result = tool
            .execute(json!({ "question": "What should I do?" }), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content["__ask_user__"], true);
    }
}
