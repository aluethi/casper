use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
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

// ── Handlers ────────────────────────────────────────────────────

/// POST /api/v1/agents/:name/run
async fn run_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<RunRequest>,
) -> Result<axum::response::Response, CasperError> {
    // Auth: check agents:{name}:run scope
    guard.require(&format!("agents:{name}:run"))?;

    let tenant_id = guard.0.tenant_id.0;
    let actor = guard.0.actor();

    // Verify agent exists and is active
    let agent_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM agents
            WHERE tenant_id = $1 AND name = $2 AND is_active = true
        )",
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !agent_exists {
        return Err(CasperError::NotFound(format!(
            "agent '{name}' not found or inactive"
        )));
    }

    // If no conversation_id, create a new conversation
    let conversation_id = match body.conversation_id {
        Some(id) => {
            // Verify conversation exists and belongs to this tenant
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(
                    SELECT 1 FROM conversations
                    WHERE id = $1 AND tenant_id = $2
                )",
            )
            .bind(id)
            .bind(tenant_id)
            .fetch_one(&state.db_owner)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

            if !exists {
                return Err(CasperError::NotFound(format!("conversation {id}")));
            }
            id
        }
        None => {
            let conv_id = Uuid::now_v7();
            let title: String = if body.message.len() > 50 {
                format!("{}...", &body.message[..50])
            } else {
                body.message.clone()
            };

            sqlx::query(
                "INSERT INTO conversations (id, tenant_id, agent_name, status, title)
                 VALUES ($1, $2, $3, 'active', $4)",
            )
            .bind(conv_id)
            .bind(tenant_id)
            .bind(&name)
            .bind(&title)
            .execute(&state.db_owner)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error creating conversation: {e}")))?;

            conv_id
        }
    };

    // If async mode, spawn a background task
    if body.r#async {
        let task_id = Uuid::now_v7();
        state.async_tasks.insert(task_id, None);

        let state_clone = state.clone();
        let name_clone = name.clone();
        let message = body.message.clone();
        let metadata = body.metadata.clone();
        let actor_clone = actor.clone();

        tokio::spawn(async move {
            let result = execute_run(
                &state_clone,
                tenant_id,
                &name_clone,
                conversation_id,
                &message,
                &actor_clone,
                &metadata,
            )
            .await;

            match result {
                Ok(run_resp) => {
                    let value = serde_json::to_value(&run_resp).unwrap_or_default();
                    state_clone.async_tasks.insert(task_id, Some(value));
                }
                Err(e) => {
                    let error_value = serde_json::json!({
                        "error": e.to_string()
                    });
                    state_clone.async_tasks.insert(task_id, Some(error_value));
                }
            }
        });

        let accepted = AsyncAccepted {
            task_id,
            status: "accepted",
            poll_url: format!("/api/v1/agents/{name}/tasks/{task_id}"),
        };

        Ok((StatusCode::ACCEPTED, Json(accepted)).into_response())
    } else {
        // Sync mode: run directly
        let run_resp = execute_run(
            &state,
            tenant_id,
            &name,
            conversation_id,
            &body.message,
            &actor,
            &body.metadata,
        )
        .await?;

        Ok(Json(run_resp).into_response())
    }
}

/// Execute the agent run (shared between sync and async paths).
async fn execute_run(
    state: &AppState,
    tenant_id: Uuid,
    agent_name: &str,
    conversation_id: Uuid,
    message: &str,
    author: &str,
    metadata: &serde_json::Value,
) -> Result<RunResponse, CasperError> {
    let correlation_id = Uuid::now_v7();

    // Store the user message
    let user_msg_id = Uuid::now_v7();
    let user_content = serde_json::Value::String(message.to_string());
    let token_estimate = (message.len() / 4) as i32;

    sqlx::query(
        "INSERT INTO messages (id, tenant_id, conversation_id, role, content, token_count, author)
         VALUES ($1, $2, $3, 'user', $4, $5, $6)",
    )
    .bind(user_msg_id)
    .bind(tenant_id)
    .bind(conversation_id)
    .bind(&user_content)
    .bind(token_estimate)
    .bind(author)
    .execute(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error storing user message: {e}")))?;

    // Try to run the agent engine; on failure, return a placeholder response
    let (assistant_text, usage) = {
        let engine = casper_agent::engine::AgentEngine::new(
            state.db_owner.clone(),
            state.http_client.clone(),
            casper_agent::tools::ToolDispatcher::new(),
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
                (resp.message, usage)
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
                    if message.len() > 100 { &message[..100] } else { message }
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
                (placeholder, usage)
            }
        }
    };

    // Store the assistant response message
    let assistant_msg_id = Uuid::now_v7();
    let assistant_content = serde_json::Value::String(assistant_text.clone());
    let assistant_token_est = (assistant_text.len() / 4) as i32;

    sqlx::query(
        "INSERT INTO messages (id, tenant_id, conversation_id, role, content, token_count, author)
         VALUES ($1, $2, $3, 'assistant', $4, $5, $6)",
    )
    .bind(assistant_msg_id)
    .bind(tenant_id)
    .bind(conversation_id)
    .bind(&assistant_content)
    .bind(assistant_token_est)
    .bind(agent_name)
    .execute(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error storing assistant message: {e}")))?;

    // Fetch the stored message to get the created_at timestamp
    let msg_row: (Uuid, String, serde_json::Value, OffsetDateTime) = sqlx::query_as(
        "SELECT id, role, content, created_at FROM messages WHERE id = $1",
    )
    .bind(assistant_msg_id)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error fetching message: {e}")))?;

    Ok(RunResponse {
        conversation_id,
        message: MessageResponse {
            id: msg_row.0,
            role: msg_row.1,
            content: msg_row.2,
            created_at: msg_row.3,
        },
        usage,
        correlation_id,
    })
}

/// GET /api/v1/agents/:name/tasks/:task_id -- Poll async task result.
async fn get_task_status(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((name, task_id)): Path<(String, Uuid)>,
) -> Result<Json<TaskStatusResponse>, CasperError> {
    guard.require(&format!("agents:{name}:run"))?;

    match state.async_tasks.get(&task_id) {
        Some(entry) => {
            let value = entry.value();
            match value {
                Some(result) => Ok(Json(TaskStatusResponse {
                    task_id,
                    status: "completed".to_string(),
                    result: Some(result.clone()),
                })),
                None => Ok(Json(TaskStatusResponse {
                    task_id,
                    status: "pending".to_string(),
                    result: None,
                })),
            }
        }
        None => Err(CasperError::NotFound(format!("task {task_id}"))),
    }
}

// ── Router ──────────────────────────────────────────────────────

pub fn run_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents/{name}/run", post(run_agent))
        .route(
            "/api/v1/agents/{name}/tasks/{task_id}",
            get(get_task_status),
        )
}
