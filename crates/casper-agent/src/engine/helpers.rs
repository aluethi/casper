//! Helper methods for AgentEngine: database operations, usage recording,
//! and audit logging.
//!
//! These are split from the main engine module to keep file sizes manageable.
//! They are all `impl AgentEngine` methods.

use casper_base::CasperError;
use casper_observe::UsageEvent;
use casper_proxy::LlmResponse;
use serde_json::json;
use uuid::Uuid;

use crate::actor::AgentUsage;
use crate::prompt;

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

        let (deployment_slug, description, prompt_stack, tools, config) =
            row.ok_or_else(|| {
                CasperError::NotFound(format!("agent '{agent_name}' not found or inactive"))
            })?;

        let max_turns = config.get("max_turns").and_then(|v| v.as_i64()).unwrap_or(DEFAULT_MAX_TURNS as i64) as i32;
        let max_tokens = config.get("max_tokens").and_then(|v| v.as_i64()).unwrap_or(8192) as i32;
        let temperature = config.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7);

        // Look up tenant display name (never expose raw UUID in the prompt)
        let tenant_name: String = sqlx::query_scalar(
            "SELECT display_name FROM tenants WHERE id = $1"
        )
        .bind(tenant_id)
        .fetch_optional(&self.db)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "Unknown".to_string());

        // Assemble system prompt from prompt_stack blocks
        let system_prompt = prompt::assemble_system_prompt(
            &prompt_stack,
            &tools,
            agent_name,
            description.as_deref().unwrap_or(""),
            tenant_id,
            &tenant_name,
            &self.db,
        ).await;

        Ok(AgentConfig {
            deployment_slug,
            prompt_stack,
            config,
            system_prompt,
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
}
