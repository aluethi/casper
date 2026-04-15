use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;

fn to_rfc3339(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

fn serialize_dt_opt<S: serde::Serializer>(dt: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
    match dt {
        Some(dt) => s.serialize_str(&to_rfc3339(*dt)),
        None => s.serialize_none(),
    }
}

// ── Types ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListConversationsParams {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
    pub agent_name: Option<String>,
    pub status: Option<String>,
    pub outcome: Option<String>,
}

fn default_page() -> i64 { 1 }
fn default_per_page() -> i64 { 50 }

#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

#[derive(Serialize)]
pub struct Pagination {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct ConversationResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub status: String,
    pub title: Option<String>,
    pub outcome: Option<String>,
    pub outcome_notes: Option<String>,
    pub outcome_by: Option<String>,
    #[serde(serialize_with = "serialize_dt_opt")]
    pub outcome_at: Option<OffsetDateTime>,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
    #[serde(serialize_with = "serialize_dt")]
    pub updated_at: OffsetDateTime,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: String,
    pub author: Option<String>,
    pub content: serde_json::Value,
    pub token_count: Option<i32>,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize)]
pub struct ConversationDetailResponse {
    #[serde(flatten)]
    pub conversation: ConversationResponse,
    pub messages: Vec<MessageResponse>,
}

#[derive(Deserialize)]
pub struct SetOutcomeRequest {
    pub outcome: String,
    pub outcome_notes: Option<String>,
}

const VALID_OUTCOMES: &[&str] = &["resolved", "unresolved", "escalated", "duplicate"];

// ── Handlers ──────────────────────────────────────────────────────

/// GET /api/v1/conversations -- List conversations with filtering and pagination.
async fn list_conversations(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ListConversationsParams>,
) -> Result<Json<PaginatedResponse<ConversationResponse>>, CasperError> {
    guard.require("agents:run")?;

    let tenant_id = guard.0.tenant_id.0;
    let offset = (params.page - 1) * params.per_page;

    let count_sql = "SELECT COUNT(*) FROM conversations
        WHERE tenant_id = $1
          AND ($2::TEXT IS NULL OR agent_name = $2)
          AND ($3::TEXT IS NULL OR status = $3)
          AND ($4::TEXT IS NULL OR outcome = $4)";

    let total: (i64,) = sqlx::query_as(count_sql)
        .bind(tenant_id)
        .bind(&params.agent_name)
        .bind(&params.status)
        .bind(&params.outcome)
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let list_sql =
        "SELECT id, tenant_id, agent_name, status, title, outcome, outcome_notes, outcome_by, outcome_at, created_at, updated_at
         FROM conversations
         WHERE tenant_id = $1
           AND ($2::TEXT IS NULL OR agent_name = $2)
           AND ($3::TEXT IS NULL OR status = $3)
           AND ($4::TEXT IS NULL OR outcome = $4)
         ORDER BY created_at DESC
         LIMIT $5 OFFSET $6";

    let rows: Vec<ConversationResponse> = sqlx::query_as(list_sql)
        .bind(tenant_id)
        .bind(&params.agent_name)
        .bind(&params.status)
        .bind(&params.outcome)
        .bind(params.per_page)
        .bind(offset)
        .fetch_all(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(PaginatedResponse {
        data: rows,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: total.0,
        },
    }))
}

/// GET /api/v1/conversations/:id -- Get conversation with messages.
async fn get_conversation(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ConversationDetailResponse>, CasperError> {
    guard.require("agents:run")?;

    let tenant_id = guard.0.tenant_id.0;

    let conversation: Option<ConversationResponse> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, status, title, outcome, outcome_notes, outcome_by, outcome_at, created_at, updated_at
         FROM conversations
         WHERE id = $1 AND tenant_id = $2"
    )
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let conv = conversation.ok_or_else(|| CasperError::NotFound(format!("conversation {id}")))?;

    let messages: Vec<MessageResponse> = sqlx::query_as(
        "SELECT id, conversation_id, role, author, content, token_count, created_at
         FROM messages
         WHERE conversation_id = $1 AND tenant_id = $2
         ORDER BY created_at"
    )
    .bind(id)
    .bind(tenant_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(ConversationDetailResponse {
        conversation: conv,
        messages,
    }))
}

/// DELETE /api/v1/conversations/:id -- Delete conversation (CASCADE deletes messages).
async fn delete_conversation(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    let result = sqlx::query("DELETE FROM conversations WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id)
        .execute(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("conversation {id}")));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// PATCH /api/v1/conversations/:id/outcome -- Set conversation outcome.
async fn set_outcome(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<SetOutcomeRequest>,
) -> Result<Json<ConversationResponse>, CasperError> {
    guard.require("agents:manage")?;

    if !VALID_OUTCOMES.contains(&body.outcome.as_str()) {
        return Err(CasperError::BadRequest(format!(
            "invalid outcome '{}', must be one of: {}",
            body.outcome,
            VALID_OUTCOMES.join(", ")
        )));
    }

    let tenant_id = guard.0.tenant_id.0;
    let actor = guard.0.actor();

    let row: Option<ConversationResponse> = sqlx::query_as(
        "UPDATE conversations SET
            outcome = $3,
            outcome_notes = $4,
            outcome_by = $5,
            outcome_at = now(),
            updated_at = now()
         WHERE id = $1 AND tenant_id = $2
         RETURNING id, tenant_id, agent_name, status, title, outcome, outcome_notes, outcome_by, outcome_at, created_at, updated_at"
    )
    .bind(id)
    .bind(tenant_id)
    .bind(&body.outcome)
    .bind(&body.outcome_notes)
    .bind(&actor)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("conversation {id}")))?;
    Ok(Json(r))
}

// ── Router ────────────────────────────────────────────────────────

pub fn conversation_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/conversations", get(list_conversations))
        .route(
            "/api/v1/conversations/{id}",
            get(get_conversation).delete(delete_conversation),
        )
        .route("/api/v1/conversations/{id}/outcome", axum::routing::patch(set_outcome))
}
