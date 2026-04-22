use casper_base::TenantDb;
use casper_base::{CasperError, TenantId};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::helpers::to_rfc3339;

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

// ── Request / Response types ────────────────────────────────────

fn default_source() -> String {
    "api".to_string()
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct RunRequest {
    pub message: String,
    pub conversation_id: Option<Uuid>,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
    #[serde(default)]
    pub r#async: bool,
    pub draft: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
pub struct MessageResponse {
    pub id: Uuid,
    pub role: String,
    pub content: serde_json::Value,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize, Clone)]
pub struct UsageResponse {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: i32,
    pub cache_write_tokens: i32,
    pub llm_calls: i32,
    pub tool_calls: i32,
    pub duration_ms: u64,
}

#[derive(Serialize, Clone)]
pub struct RunResponse {
    pub conversation_id: Uuid,
    pub message: MessageResponse,
    pub usage: UsageResponse,
    pub correlation_id: Uuid,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<StepResponse>,
}

#[derive(Serialize, Clone)]
pub struct StepResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallStepResponse>>,
}

#[derive(Serialize, Clone)]
pub struct ToolCallStepResponse {
    pub name: String,
    pub input: serde_json::Value,
    pub result: String,
    pub is_error: bool,
}

#[derive(Serialize)]
pub struct AsyncAccepted {
    pub task_id: Uuid,
    pub status: &'static str,
    pub poll_url: String,
}

#[derive(Serialize)]
pub struct TaskStatusResponse {
    pub task_id: Uuid,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
}

// ── Service functions ────────────────────────────────────────────

/// Prepare the conversation: verify the agent, create/validate conversation_id.
/// Returns the conversation_id to use.
pub async fn prepare_conversation(
    db: &PgPool,
    tenant_id: TenantId,
    agent_name: &str,
    conversation_id: Option<Uuid>,
    message: &str,
) -> Result<Uuid, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Verify agent exists and is active
    let agent_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM agents
            WHERE tenant_id = $1 AND name = $2 AND is_active = true
        )",
    )
    .bind(tenant_id.0)
    .bind(agent_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !agent_exists {
        return Err(CasperError::NotFound(format!(
            "agent '{agent_name}' not found or inactive"
        )));
    }

    let conv_id = match conversation_id {
        Some(id) => {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM conversations
                    WHERE id = $1 AND tenant_id = $2
                )",
            )
            .bind(id)
            .bind(tenant_id.0)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

            if !exists {
                return Err(CasperError::NotFound(format!("conversation {id}")));
            }
            id
        }
        None => {
            let new_id = Uuid::now_v7();
            let title: String = if message.len() > 50 {
                format!("{}...", &message[..50])
            } else {
                message.to_string()
            };

            sqlx::query(
                "INSERT INTO conversations (id, tenant_id, agent_name, status, title)
                 VALUES ($1, $2, $3, 'active', $4)",
            )
            .bind(new_id)
            .bind(tenant_id.0)
            .bind(agent_name)
            .bind(&title)
            .execute(&mut *tx)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error creating conversation: {e}")))?;

            new_id
        }
    };

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(conv_id)
}

/// Execute the agent run (shared between sync and async paths).
pub async fn execute_run(
    state: &AppState,
    tenant_id: Uuid,
    agent_name: &str,
    conversation_id: Uuid,
    message: &str,
    author: &str,
    metadata: &serde_json::Value,
) -> Result<RunResponse, CasperError> {
    let correlation_id = Uuid::now_v7();

    let (assistant_text, usage, agent_steps) = {
        let engine = casper_agent::engine::AgentEngine::new(
            state.db_owner.clone(),
            state.http_client.clone(),
            casper_agent::tools::ToolDispatcher::new(),
            std::sync::Arc::new(state.llm.for_tenant(tenant_id)),
            Some(state.audit.clone()),
            Some(state.usage.clone()),
        );

        match engine
            .run(
                tenant_id,
                agent_name,
                conversation_id,
                message,
                author,
                metadata,
            )
            .await
        {
            Ok(resp) => {
                let usage = UsageResponse {
                    input_tokens: resp.usage.input_tokens,
                    output_tokens: resp.usage.output_tokens,
                    cache_read_tokens: resp.usage.cache_read_tokens,
                    cache_write_tokens: resp.usage.cache_write_tokens,
                    llm_calls: resp.usage.llm_calls,
                    tool_calls: resp.usage.tool_calls,
                    duration_ms: resp.usage.duration_ms,
                };
                let steps: Vec<StepResponse> = resp
                    .steps
                    .into_iter()
                    .map(|s| StepResponse {
                        thinking: s.thinking,
                        tool_calls: s.tool_calls.map(|tcs| {
                            tcs.into_iter()
                                .map(|tc| ToolCallStepResponse {
                                    name: tc.name,
                                    input: tc.input,
                                    result: tc.result,
                                    is_error: tc.is_error,
                                })
                                .collect()
                        }),
                    })
                    .collect();
                (resp.message, usage, steps)
            }
            Err(e) => {
                tracing::warn!(
                    agent = %agent_name,
                    error = %e,
                    "Agent engine failed, returning placeholder response"
                );
                let placeholder = format!(
                    "[Placeholder] Agent '{}' received your message but the LLM backend is unavailable. Message: {}",
                    agent_name,
                    if message.len() > 100 {
                        &message[..100]
                    } else {
                        message
                    }
                );
                let usage = UsageResponse {
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    llm_calls: 0,
                    tool_calls: 0,
                    duration_ms: 0,
                };
                (placeholder, usage, Vec::new())
            }
        }
    };

    // For the placeholder path (engine failure), store user+assistant messages manually
    let tid = TenantId(tenant_id);
    let tdb = TenantDb::new(state.db.clone(), tid);

    if usage.llm_calls == 0
        && let Ok(mut tx) = tdb.begin().await
    {
        sqlx::query(
                "INSERT INTO messages (id, tenant_id, conversation_id, role, content, token_count, author)
                 VALUES ($1, $2, $3, 'user', $4, $5, $6)
                 ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(conversation_id)
            .bind(serde_json::Value::String(message.to_string()))
            .bind((message.len() / 4) as i32)
            .bind(author)
            .execute(&mut *tx)
            .await
            .ok();

        sqlx::query(
                "INSERT INTO messages (id, tenant_id, conversation_id, role, content, token_count, author)
                 VALUES ($1, $2, $3, 'assistant', $4, $5, $6)",
            )
            .bind(Uuid::now_v7())
            .bind(tenant_id)
            .bind(conversation_id)
            .bind(serde_json::Value::String(assistant_text.clone()))
            .bind((assistant_text.len() / 4) as i32)
            .bind(agent_name)
            .execute(&mut *tx)
            .await
            .ok();

        tx.commit().await.ok();
    }

    // Get the most recent assistant message for the response
    let msg_row: Option<(Uuid, String, serde_json::Value, OffsetDateTime)> =
        if let Ok(mut tx) = tdb.begin().await {
            let row = sqlx::query_as(
                "SELECT id, role, content, created_at FROM messages
             WHERE conversation_id = $1 AND role = 'assistant'
             ORDER BY created_at DESC LIMIT 1",
            )
            .bind(conversation_id)
            .fetch_optional(&mut *tx)
            .await
            .ok()
            .flatten();
            tx.commit().await.ok();
            row
        } else {
            None
        };

    let (msg_id, msg_role, msg_content, msg_created) = msg_row.unwrap_or_else(|| {
        (
            Uuid::now_v7(),
            "assistant".to_string(),
            serde_json::Value::String(assistant_text.clone()),
            OffsetDateTime::now_utc(),
        )
    });

    Ok(RunResponse {
        conversation_id,
        message: MessageResponse {
            id: msg_id,
            role: msg_role,
            content: msg_content,
            created_at: msg_created,
        },
        usage,
        correlation_id,
        steps: agent_steps,
    })
}

/// Poll an async task by ID.
pub fn get_task_status(state: &AppState, task_id: Uuid) -> Result<TaskStatusResponse, CasperError> {
    match state.async_tasks.get(&task_id) {
        Some(entry) => {
            let value = entry.value();
            match value {
                Some(result) => Ok(TaskStatusResponse {
                    task_id,
                    status: "completed".to_string(),
                    result: Some(result.clone()),
                }),
                None => Ok(TaskStatusResponse {
                    task_id,
                    status: "pending".to_string(),
                    result: None,
                }),
            }
        }
        None => Err(CasperError::NotFound(format!("task {task_id}"))),
    }
}
