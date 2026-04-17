use casper_base::{CasperError, TenantId};
use casper_db::TenantDb;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;
use crate::pagination::{PaginatedResponse, Pagination};

// ── Domain types ─────────────────────────────────────────────────

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

fn serialize_dt_opt<S: serde::Serializer>(dt: &Option<OffsetDateTime>, s: S) -> Result<S::Ok, S::Error> {
    match dt {
        Some(dt) => s.serialize_str(&to_rfc3339(*dt)),
        None => s.serialize_none(),
    }
}

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

// ── Service functions ────────────────────────────────────────────

pub async fn list(
    db: &PgPool,
    tenant_id: TenantId,
    params: &ListConversationsParams,
) -> Result<PaginatedResponse<ConversationResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let offset = (params.page.max(1) - 1) * params.per_page.clamp(1, 100);

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM conversations
         WHERE tenant_id = $1
           AND ($2::TEXT IS NULL OR agent_name = $2)
           AND ($3::TEXT IS NULL OR status = $3)
           AND ($4::TEXT IS NULL OR outcome = $4)"
    )
    .bind(tenant_id.0)
    .bind(&params.agent_name)
    .bind(&params.status)
    .bind(&params.outcome)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<ConversationResponse> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, status, title, outcome, outcome_notes, outcome_by, outcome_at, created_at, updated_at
         FROM conversations
         WHERE tenant_id = $1
           AND ($2::TEXT IS NULL OR agent_name = $2)
           AND ($3::TEXT IS NULL OR status = $3)
           AND ($4::TEXT IS NULL OR outcome = $4)
         ORDER BY created_at DESC
         LIMIT $5 OFFSET $6"
    )
    .bind(tenant_id.0)
    .bind(&params.agent_name)
    .bind(&params.status)
    .bind(&params.outcome)
    .bind(params.per_page.clamp(1, 100))
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(PaginatedResponse {
        data: rows,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: total.0,
        },
    })
}

pub async fn get_with_messages(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<ConversationDetailResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let conversation: Option<ConversationResponse> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, status, title, outcome, outcome_notes, outcome_by, outcome_at, created_at, updated_at
         FROM conversations
         WHERE id = $1 AND tenant_id = $2"
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
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
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(ConversationDetailResponse {
        conversation: conv,
        messages,
    })
}

pub async fn delete(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<serde_json::Value, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let result = sqlx::query("DELETE FROM conversations WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("conversation {id}")));
    }

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(serde_json::json!({ "deleted": true }))
}

pub async fn set_outcome(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
    req: &SetOutcomeRequest,
    actor: &str,
) -> Result<ConversationResponse, CasperError> {
    if !VALID_OUTCOMES.contains(&req.outcome.as_str()) {
        return Err(CasperError::BadRequest(format!(
            "invalid outcome '{}', must be one of: {}",
            req.outcome,
            VALID_OUTCOMES.join(", ")
        )));
    }

    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

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
    .bind(tenant_id.0)
    .bind(&req.outcome)
    .bind(&req.outcome_notes)
    .bind(actor)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.ok_or_else(|| CasperError::NotFound(format!("conversation {id}")))
}
