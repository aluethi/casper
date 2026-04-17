//! `update_memory` tool: writes a new memory document for the agent.
//!
//! Moves the current content to `agent_memory_history`, increments the version,
//! and writes the new content. Enforces `max_document_tokens`.

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;
use uuid::Uuid;

use super::{Tool, ToolContext, ToolResult};
use crate::prompt::types::estimate_tokens;

/// Built-in tool that lets an agent update its own memory document.
pub struct UpdateMemoryTool {
    /// Maximum tokens allowed for the memory document.
    pub max_document_tokens: i32,
}

impl UpdateMemoryTool {
    pub fn new(max_document_tokens: i32) -> Self {
        Self { max_document_tokens }
    }

    /// Construct from a tools-config JSON entry.
    /// Expected key: `max_document_tokens` (int, default 4000).
    pub fn from_config(config: &serde_json::Value) -> Self {
        Self {
            max_document_tokens: config.get("max_document_tokens").and_then(|v| v.as_i64()).unwrap_or(4000) as i32,
        }
    }
}

#[async_trait]
impl Tool for UpdateMemoryTool {
    fn name(&self) -> &str {
        "update_memory"
    }

    fn description(&self) -> &str {
        "Update this agent's memory document. The old content is preserved in history. \
         Use this to remember important facts, decisions, or context across conversations."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The new memory document content (replaces the current memory)."
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CasperError::BadRequest("missing 'content' string".into()))?;

        // Enforce token limit
        let token_count = estimate_tokens(content);
        if token_count > self.max_document_tokens {
            return Ok(ToolResult::error(format!(
                "Memory document too large: {token_count} tokens exceeds limit of {}",
                self.max_document_tokens
            )));
        }

        // Begin transaction
        let mut tx = sqlx::pool::Pool::begin(&ctx.db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

        // Set tenant context for RLS
        sqlx::query("SELECT set_config('app.tenant_id', $1::text, true)")
            .bind(ctx.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error setting tenant: {e}")))?;

        // Get current memory (if any)
        let current: Option<(Uuid, String, i32)> = sqlx::query_as(
            "SELECT id, content, version FROM agent_memory
             WHERE tenant_id = $1 AND agent_name = $2
             FOR UPDATE",
        )
        .bind(ctx.tenant_id)
        .bind(&ctx.agent_name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error reading memory: {e}")))?;

        let new_version = match current {
            Some((memory_id, old_content, old_version)) => {
                // Archive old content to history
                sqlx::query(
                    "INSERT INTO agent_memory_history (id, memory_id, tenant_id, agent_name, content, version)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(Uuid::now_v7())
                .bind(memory_id)
                .bind(ctx.tenant_id)
                .bind(&ctx.agent_name)
                .bind(&old_content)
                .bind(old_version)
                .execute(&mut *tx)
                .await
                .map_err(|e| CasperError::Internal(format!("DB error archiving memory: {e}")))?;

                let new_ver = old_version + 1;

                // Update existing memory
                sqlx::query(
                    "UPDATE agent_memory SET content = $1, version = $2, updated_at = now()
                     WHERE id = $3",
                )
                .bind(content)
                .bind(new_ver)
                .bind(memory_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| CasperError::Internal(format!("DB error updating memory: {e}")))?;

                new_ver
            }
            None => {
                // Create new memory record
                sqlx::query(
                    "INSERT INTO agent_memory (id, tenant_id, agent_name, content, version)
                     VALUES ($1, $2, $3, $4, 1)",
                )
                .bind(Uuid::now_v7())
                .bind(ctx.tenant_id)
                .bind(&ctx.agent_name)
                .bind(content)
                .execute(&mut *tx)
                .await
                .map_err(|e| CasperError::Internal(format!("DB error creating memory: {e}")))?;

                1
            }
        };

        tx.commit()
            .await
            .map_err(|e| CasperError::Internal(format!("DB error committing: {e}")))?;

        tracing::info!(
            agent = %ctx.agent_name,
            version = new_version,
            tokens = token_count,
            "agent memory updated"
        );

        Ok(ToolResult::ok(json!({
            "status": "ok",
            "version": new_version,
            "token_count": token_count
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = UpdateMemoryTool::new(4000);
        assert_eq!(tool.name(), "update_memory");
        assert!(!tool.description().is_empty());
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["content"].is_object());
    }

    #[test]
    fn token_limit_check() {
        let _tool = UpdateMemoryTool::new(10); // very small limit
        // A string of 100 chars ~= 25 tokens, which exceeds limit of 10
        let long_content = "a".repeat(100);
        let tokens = estimate_tokens(&long_content);
        assert!(tokens > 10);
    }
}
