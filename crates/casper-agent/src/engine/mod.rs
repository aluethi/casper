//! Agent engine: drives the ReAct (Reason + Act) loop.
//!
//! The engine:
//! 1. Loads agent configuration from the database
//! 2. Assembles the prompt (system blocks + conversation history)
//! 3. Calls the LLM (via casper-catalog routing + casper-proxy dispatch)
//! 4. If the LLM returns tool_use blocks: dispatches tools, appends results, loops
//! 5. If the LLM returns end_turn: returns the final response
//! 6. Enforces a maximum number of turns to prevent infinite loops

mod helpers;
pub mod llm;

use std::sync::Arc;
use std::time::Instant;

use casper_base::CasperError;
use casper_observe::{AuditWriter, UsageRecorder};
use casper_proxy::{LlmRequest, Message};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::actor::{AgentResponse, AgentUsage};
use crate::tools::{ToolContext, ToolDispatcher};

pub use llm::{LlmCaller, RealLlmCaller};
#[cfg(test)]
pub use llm::MockLlmCaller;

/// Maximum number of ReAct loop iterations before we bail out.
const DEFAULT_MAX_TURNS: usize = 25;

// ── Agent configuration row from DB ──────────────────────────────

/// Agent configuration loaded from the database.
#[derive(Debug)]
struct AgentConfig {
    pub deployment_slug: String,
    pub prompt_stack: serde_json::Value,
    pub config: serde_json::Value,
    pub system_prompt: String, // assembled from prompt_stack
    pub max_turns: i32,
    pub max_tokens: i32,
    pub temperature: f64,
}

/// Row type for the agent config query.
type AgentConfigRow = (String, Option<String>, serde_json::Value, serde_json::Value, serde_json::Value);

// ── Agent engine ─────────────────────────────────────────────────

/// The agent engine drives the ReAct cycle.
pub struct AgentEngine {
    pub db: PgPool,
    pub http_client: reqwest::Client,
    pub tool_dispatcher: ToolDispatcher,
    pub llm_caller: Arc<dyn LlmCaller>,
    pub audit_writer: Option<AuditWriter>,
    pub usage_recorder: Option<UsageRecorder>,
}

impl AgentEngine {
    /// Create an engine with the real LLM caller.
    pub fn new(
        db: PgPool,
        http_client: reqwest::Client,
        tool_dispatcher: ToolDispatcher,
        audit_writer: Option<AuditWriter>,
        usage_recorder: Option<UsageRecorder>,
    ) -> Self {
        let llm_caller = Arc::new(RealLlmCaller {
            db: db.clone(),
            http_client: http_client.clone(),
        });
        Self {
            db,
            http_client,
            tool_dispatcher,
            llm_caller,
            audit_writer,
            usage_recorder,
        }
    }

    /// Create an engine with a custom LLM caller (for testing).
    #[cfg(test)]
    pub fn with_caller(
        db: PgPool,
        tool_dispatcher: ToolDispatcher,
        llm_caller: Arc<dyn LlmCaller>,
    ) -> Self {
        Self {
            db: db.clone(),
            http_client: reqwest::Client::new(),
            tool_dispatcher,
            llm_caller,
            audit_writer: None,
            usage_recorder: None,
        }
    }

    /// Run the ReAct loop for a single user message.
    ///
    /// This is the main entry point called by the actor task.
    pub async fn run(
        &self,
        tenant_id: Uuid,
        agent_name: &str,
        conversation_id: Uuid,
        user_message: &str,
        author: &str,
        _metadata: &serde_json::Value,
    ) -> Result<AgentResponse, CasperError> {
        let correlation_id = Uuid::now_v7();
        let start = Instant::now();

        tracing::info!(
            agent = %agent_name,
            conversation_id = %conversation_id,
            correlation_id = %correlation_id,
            "starting ReAct loop"
        );

        // 1. Load agent config
        let config = self.load_agent_config(tenant_id, agent_name).await?;
        let max_turns = (config.max_turns as usize).min(DEFAULT_MAX_TURNS);

        // 2. Build initial messages
        let system_prompt = config.system_prompt.clone();
        let tool_defs = self.tool_dispatcher.tool_definitions();

        // Load conversation history
        let history = crate::prompt::load_conversation_history(
            &self.db,
            tenant_id,
            conversation_id,
            8000, // token budget for history
        )
        .await
        .map_err(|e| CasperError::Internal(format!("Failed to load history: {e}")))?;

        // Convert history to messages
        let mut messages: Vec<Message> = history
            .into_iter()
            .map(|h| Message {
                role: h.role,
                content: h.content,
            })
            .collect();

        // Append the new user message
        messages.push(Message {
            role: "user".to_string(),
            content: serde_json::Value::String(user_message.to_string()),
        });

        // Store the user message in the DB
        self.store_message(
            tenant_id,
            conversation_id,
            "user",
            &serde_json::Value::String(user_message.to_string()),
            author,
        )
        .await?;

        // 3. ReAct loop
        let tool_ctx = ToolContext {
            tenant_id,
            agent_name: agent_name.to_string(),
            conversation_id,
            correlation_id,
            db: self.db.clone(),
        };

        let mut usage = AgentUsage::default();

        for turn in 0..max_turns {
            tracing::debug!(
                agent = %agent_name,
                turn,
                messages = messages.len(),
                "ReAct turn"
            );

            // Build the LLM request
            let request = LlmRequest {
                messages: messages.clone(),
                model: config.deployment_slug.clone(),
                max_tokens: Some(config.max_tokens),
                temperature: Some(config.temperature),
                stream: false,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs.clone())
                },
                extra: json!({
                    "system": system_prompt,
                }),
            };

            // Call LLM
            let (response, backend_id) = self.llm_caller.call(tenant_id, &request).await?;

            // Accumulate usage
            usage.input_tokens += response.input_tokens;
            usage.output_tokens += response.output_tokens;
            usage.cache_read_tokens += response.cache_read_tokens.unwrap_or(0);
            usage.cache_write_tokens += response.cache_write_tokens.unwrap_or(0);
            usage.llm_calls += 1;

            // Record usage event
            self.record_usage(
                tenant_id,
                agent_name,
                &config.deployment_slug,
                &response,
                backend_id,
                correlation_id,
            )
            .await;

            // Check for tool calls
            let has_tools = response
                .tool_calls
                .as_ref()
                .is_some_and(|tc| !tc.is_empty());

            if has_tools {
                let tool_calls = response.tool_calls.as_ref().unwrap();

                // Build assistant message with tool_use blocks
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();
                if !response.content.is_empty() {
                    content_blocks.push(json!({
                        "type": "text",
                        "text": response.content,
                    }));
                }
                for tc in tool_calls {
                    content_blocks.push(tc.clone());
                }

                let assistant_msg = Message {
                    role: "assistant".to_string(),
                    content: json!(content_blocks),
                };
                messages.push(assistant_msg.clone());

                // Store assistant message with tool_use blocks
                self.store_message(
                    tenant_id,
                    conversation_id,
                    "assistant",
                    &assistant_msg.content,
                    agent_name,
                )
                .await?;

                // Execute each tool call and collect results
                for tc in tool_calls {
                    let tool_name = tc
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let tool_id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let tool_input = tc
                        .get("input")
                        .cloned()
                        .unwrap_or(json!({}));

                    tracing::debug!(
                        tool = %tool_name,
                        tool_id = %tool_id,
                        "executing tool"
                    );

                    let tool_result = self
                        .tool_dispatcher
                        .dispatch(tool_name, tool_input, &tool_ctx)
                        .await;

                    usage.tool_calls += 1;

                    let result_content = match tool_result {
                        Ok(result) => {
                            if result.is_error {
                                json!({
                                    "type": "tool_result",
                                    "tool_use_id": tool_id,
                                    "is_error": true,
                                    "content": result.content,
                                })
                            } else {
                                json!({
                                    "type": "tool_result",
                                    "tool_use_id": tool_id,
                                    "content": result.content,
                                })
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                tool = %tool_name,
                                error = %e,
                                "tool execution failed"
                            );
                            json!({
                                "type": "tool_result",
                                "tool_use_id": tool_id,
                                "is_error": true,
                                "content": format!("Tool error: {e}"),
                            })
                        }
                    };

                    // Check for sentinel results (delegate, ask_user)
                    if let Some(content) = result_content.get("content") {
                        if content.get("__delegate__").is_some()
                            || content.get("__ask_user__").is_some()
                        {
                            // For now, just include the result — actual handling
                            // will be added when wiring to the actor system.
                            tracing::info!(
                                tool = %tool_name,
                                "sentinel tool result detected"
                            );
                        }
                    }

                    // Add tool result as a message
                    let tool_msg = Message {
                        role: "tool".to_string(),
                        content: result_content.clone(),
                    };
                    messages.push(tool_msg);

                    // Store tool result message
                    self.store_message(
                        tenant_id,
                        conversation_id,
                        "tool",
                        &result_content,
                        agent_name,
                    )
                    .await?;
                }

                // Continue the loop — next iteration will send results to LLM
                continue;
            }

            // No tool calls — this is the final response
            let final_message = response.content.clone();

            // Store assistant response
            self.store_message(
                tenant_id,
                conversation_id,
                "assistant",
                &serde_json::Value::String(final_message.clone()),
                agent_name,
            )
            .await?;

            // Record audit
            self.record_audit(
                tenant_id,
                author,
                agent_name,
                conversation_id,
                correlation_id,
                &usage,
            );

            usage.duration_ms = start.elapsed().as_millis() as u64;

            tracing::info!(
                agent = %agent_name,
                turns = turn + 1,
                llm_calls = usage.llm_calls,
                tool_calls = usage.tool_calls,
                duration_ms = usage.duration_ms,
                "ReAct loop completed"
            );

            return Ok(AgentResponse {
                message: final_message,
                conversation_id,
                usage,
                correlation_id,
            });
        }

        // Max turns exceeded
        let _duration_ms = start.elapsed().as_millis() as u64;
        Err(CasperError::Internal(format!(
            "Agent '{agent_name}' exceeded maximum turns ({max_turns})"
        )))
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolResult};
    use std::sync::Arc;

    /// A trivial echo tool for testing.
    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "Echoes input back." }
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
            _ctx: &crate::tools::ToolContext,
        ) -> Result<ToolResult, CasperError> {
            let msg = input.get("message").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolResult::ok(json!({ "echoed": msg })))
        }
    }

    fn test_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://localhost/casper_test_nonexistent")
            .unwrap()
    }

    #[tokio::test]
    async fn mock_simple_response() {
        let pool = test_pool();
        let dispatcher = ToolDispatcher::new();
        let caller = Arc::new(MockLlmCaller::simple("Hello, I'm the agent!"));

        let engine = AgentEngine::with_caller(pool, dispatcher, caller);

        // We can't actually run the full engine because it tries to load from DB,
        // but we can verify the mock caller works.
        let request = LlmRequest {
            messages: vec![Message {
                role: "user".to_string(),
                content: json!("Hi"),
            }],
            model: "test".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stream: false,
            tools: None,
            extra: json!({}),
        };

        let (response, backend_id) = engine.llm_caller.call(Uuid::nil(), &request).await.unwrap();
        assert_eq!(response.content, "Hello, I'm the agent!");
        assert_eq!(response.finish_reason.as_deref(), Some("end_turn"));
        assert!(backend_id.is_none());
    }

    #[tokio::test]
    async fn mock_with_tool_call() {
        let pool = test_pool();
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Arc::new(EchoTool));

        let caller = Arc::new(MockLlmCaller::with_tool_call(
            "echo",
            json!({"message": "test"}),
            "Done echoing!",
        ));

        let engine = AgentEngine::with_caller(pool, dispatcher, caller);

        // Verify the mock produces tool calls then a final response
        let request = LlmRequest {
            messages: vec![],
            model: "test".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stream: false,
            tools: None,
            extra: json!({}),
        };

        let (r1, _) = engine.llm_caller.call(Uuid::nil(), &request).await.unwrap();
        assert!(r1.tool_calls.is_some());
        assert_eq!(r1.finish_reason.as_deref(), Some("tool_use"));

        let (r2, _) = engine.llm_caller.call(Uuid::nil(), &request).await.unwrap();
        assert!(r2.tool_calls.is_none());
        assert_eq!(r2.content, "Done echoing!");
    }

    #[test]
    fn usage_accumulation() {
        let mut usage = AgentUsage::default();
        usage.input_tokens += 100;
        usage.output_tokens += 50;
        usage.llm_calls += 1;
        usage.tool_calls += 2;

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.llm_calls, 1);
        assert_eq!(usage.tool_calls, 2);
    }
}
