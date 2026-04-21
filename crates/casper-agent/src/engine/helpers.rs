//! Helper methods for AgentEngine: database operations, usage recording,
//! and audit logging.
//!
//! These are split from the main engine module to keep file sizes manageable.
//! They are all `impl AgentEngine` methods.

use std::time::Duration;

use casper_base::CasperError;
use casper_base::UsageEvent;
use casper_catalog::LlmResponse;
use serde_json::json;
use uuid::Uuid;

use crate::actor::AgentUsage;
use crate::prompt;
use crate::tools::{ToolDispatcher, ToolResult};

use super::{AgentConfig, AgentConfigRow, AgentEngine, DEFAULT_MAX_TURNS};

impl AgentEngine {
    /// Load the agent configuration from the database.
    pub(super) async fn load_agent_config(
        &self,
        tenant_id: Uuid,
        agent_name: &str,
    ) -> Result<AgentConfig, CasperError> {
        let row: Option<AgentConfigRow> = sqlx::query_as(
            "SELECT model_deployment, description, prompt_stack, tools, config
             FROM agents
             WHERE tenant_id = $1 AND name = $2 AND is_active = true",
        )
        .bind(tenant_id)
        .bind(agent_name)
        .fetch_optional(&self.db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error loading agent config: {e}")))?;

        let (deployment_slug, description, prompt_stack, tools, config) = row.ok_or_else(|| {
            CasperError::NotFound(format!("agent '{agent_name}' not found or inactive"))
        })?;

        let max_turns = config
            .get("max_turns")
            .and_then(|v| v.as_i64())
            .unwrap_or(DEFAULT_MAX_TURNS as i64) as i32;
        let max_tokens = config
            .get("max_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(8192) as i32;
        let temperature = config
            .get("temperature")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.7);

        // Look up tenant display name (never expose raw UUID in the prompt)
        let tenant_name: String =
            sqlx::query_scalar("SELECT display_name FROM tenants WHERE id = $1")
                .bind(tenant_id)
                .fetch_optional(&self.db)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "Unknown".to_string());

        Ok(AgentConfig {
            deployment_slug,
            description: description.unwrap_or_default(),
            prompt_stack,
            tools: tools.clone(),
            config,
            tenant_name,
            // Assembled later in run() after MCP tool discovery
            system_prompt: String::new(),
            max_turns,
            max_tokens,
            temperature,
        })
    }

    /// Store a message in the conversation.
    pub(super) async fn store_message(
        &self,
        tenant_id: Uuid,
        conversation_id: Uuid,
        role: &str,
        content: &serde_json::Value,
        author: &str,
    ) -> Result<(), CasperError> {
        let token_count = prompt::estimate_tokens_json(content);
        sqlx::query(
            "INSERT INTO messages (id, tenant_id, conversation_id, role, content, token_count, author)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(conversation_id)
        .bind(role)
        .bind(content)
        .bind(token_count)
        .bind(author)
        .execute(&self.db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error storing message: {e}")))?;

        Ok(())
    }

    /// Record a usage event for an LLM call.
    pub(super) async fn record_usage(
        &self,
        tenant_id: Uuid,
        agent_name: &str,
        deployment_slug: &str,
        response: &LlmResponse,
        backend_id: Option<Uuid>,
        correlation_id: Uuid,
    ) {
        if let Some(recorder) = &self.usage_recorder {
            let event = UsageEvent {
                tenant_id,
                source: "agent".to_string(),
                model: response.model.clone(),
                deployment_slug: Some(deployment_slug.to_string()),
                agent_name: Some(agent_name.to_string()),
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                cache_read_tokens: response.cache_read_tokens,
                cache_write_tokens: response.cache_write_tokens,
                backend_id,
                correlation_id,
            };
            if let Err(e) = recorder.record(event).await {
                tracing::warn!(error = %e, "failed to record usage event");
            }
        }
    }

    /// Record an audit entry for an agent invocation.
    pub(super) fn record_audit(
        &self,
        tenant_id: Uuid,
        actor: &str,
        agent_name: &str,
        conversation_id: Uuid,
        correlation_id: Uuid,
        usage: &AgentUsage,
    ) {
        if let Some(writer) = &self.audit_writer {
            writer.log_action(
                tenant_id,
                actor,
                "agent.invoke",
                Some(agent_name),
                json!({
                    "conversation_id": conversation_id.to_string(),
                    "llm_calls": usage.llm_calls,
                    "tool_calls": usage.tool_calls,
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                }),
                "success",
                correlation_id,
                "agent-system",
            );
        }
    }

    /// Execute a delegation to a child agent.
    ///
    /// Creates an ephemeral conversation for the child, runs the child agent's
    /// full ReAct loop, and returns its final response as a `ToolResult`.
    /// Respects timeout and max_depth from the delegate tool config.
    pub(super) async fn execute_delegation(
        &self,
        target_agent: &str,
        message: &str,
        tenant_id: Uuid,
        parent_agent: &str,
        _correlation_id: Uuid,
        timeout_secs: u64,
        max_depth: u32,
    ) -> ToolResult {
        // Depth check
        if self.delegation_depth >= max_depth {
            return ToolResult::error(format!(
                "Maximum delegation depth ({max_depth}) exceeded. \
                 Cannot delegate from '{parent_agent}' to '{target_agent}'."
            ));
        }

        tracing::info!(
            from = %parent_agent,
            to = %target_agent,
            depth = self.delegation_depth + 1,
            timeout_secs,
            "delegating to child agent"
        );

        // Create an ephemeral conversation for the child
        let child_conv_id = Uuid::now_v7();
        if let Err(e) = sqlx::query(
            "INSERT INTO conversations (id, tenant_id, agent_name, status, title)
             VALUES ($1, $2, $3, 'active', $4)",
        )
        .bind(child_conv_id)
        .bind(tenant_id)
        .bind(target_agent)
        .bind(format!("delegation from {parent_agent}"))
        .execute(&self.db)
        .await
        {
            return ToolResult::error(format!("Failed to create delegation conversation: {e}"));
        }

        // Build a child engine at depth + 1
        let child_engine = AgentEngine {
            db: self.db.clone(),
            http_client: self.http_client.clone(),
            tool_dispatcher: ToolDispatcher::new(),
            llm_caller: self.llm_caller.clone(),
            audit_writer: self.audit_writer.clone(),
            usage_recorder: self.usage_recorder.clone(),
            delegation_depth: self.delegation_depth + 1,
        };

        let target_owned = target_agent.to_string();
        let message_owned = message.to_string();
        let parent_owned = parent_agent.to_string();

        // Box::pin to break async recursion (run → execute_delegation → run)
        let result = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            Box::pin(child_engine.run(
                tenant_id,
                &target_owned,
                child_conv_id,
                &message_owned,
                &parent_owned,
                &json!({"delegation_depth": self.delegation_depth + 1}),
            )),
        )
        .await;

        // Mark the ephemeral conversation as completed
        let _ = sqlx::query("UPDATE conversations SET status = 'completed' WHERE id = $1")
            .bind(child_conv_id)
            .execute(&self.db)
            .await;

        match result {
            Ok(Ok(response)) => {
                tracing::info!(
                    from = %parent_agent,
                    to = %target_owned,
                    child_llm_calls = response.usage.llm_calls,
                    child_tool_calls = response.usage.tool_calls,
                    "delegation completed"
                );
                ToolResult::ok(serde_json::Value::String(response.message))
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    from = %parent_agent,
                    to = %target_owned,
                    error = %e,
                    "delegation failed"
                );
                ToolResult::error(format!("Agent '{target_owned}' failed: {e}"))
            }
            Err(_) => {
                tracing::warn!(
                    from = %parent_agent,
                    to = %target_owned,
                    timeout_secs,
                    "delegation timed out"
                );
                ToolResult::error(format!(
                    "Delegation to '{target_owned}' timed out after {timeout_secs}s"
                ))
            }
        }
    }
}
