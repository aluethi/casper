use axum::{
    Json, Router,
    extract::{Query, State},
    routing::post,
};
use casper_base::CasperError;
use casper_db::TenantDb;
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

#[derive(Deserialize)]
pub struct CreateFeedbackRequest {
    pub message_id: Uuid,
    pub feedback_type: String,
    pub rating: Option<i32>,
    pub correction: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct FeedbackResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub message_id: Uuid,
    pub conversation_id: Uuid,
    pub agent_name: String,
    pub feedback_type: String,
    pub rating: Option<i32>,
    pub correction: Option<String>,
    pub tags: Option<Vec<String>>,
    pub created_by: String,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

#[derive(Deserialize)]
pub struct ListFeedbackParams {
    pub agent_name: Option<String>,
    pub feedback_type: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    100
}

const VALID_FEEDBACK_TYPES: &[&str] = &["rating", "correction", "tag"];

// ── Handlers ────────────────────────────────────────────────────

/// POST /api/v1/feedback -- Submit feedback on a message.
async fn create_feedback(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateFeedbackRequest>,
) -> Result<Json<FeedbackResponse>, CasperError> {
    guard.require("feedback:write")?;

    let tenant_id = guard.0.tenant_id.0;
    let actor = guard.0.actor();

    // Validate feedback_type
    if !VALID_FEEDBACK_TYPES.contains(&body.feedback_type.as_str()) {
        return Err(CasperError::BadRequest(format!(
            "invalid feedback_type '{}', must be one of: {}",
            body.feedback_type,
            VALID_FEEDBACK_TYPES.join(", ")
        )));
    }

    // Validate type-specific fields
    match body.feedback_type.as_str() {
        "rating" => {
            match body.rating {
                Some(r) if r == 1 || r == -1 => {}
                Some(r) => {
                    return Err(CasperError::BadRequest(format!(
                        "rating must be +1 or -1, got {r}"
                    )));
                }
                None => {
                    return Err(CasperError::BadRequest(
                        "rating field is required for feedback_type 'rating'".into(),
                    ));
                }
            }
        }
        "correction" => {
            if body.correction.is_none() || body.correction.as_deref() == Some("") {
                return Err(CasperError::BadRequest(
                    "correction field is required for feedback_type 'correction'".into(),
                ));
            }
        }
        "tag" => {
            if body.tags.is_empty() {
                return Err(CasperError::BadRequest(
                    "tags field must not be empty for feedback_type 'tag'".into(),
                ));
            }
        }
        _ => {}
    }

    let tdb = TenantDb::new(state.db.clone(), casper_base::TenantId(tenant_id));
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Validate message exists and get its conversation_id + agent_name
    let msg_row: Option<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT m.id, m.conversation_id, c.agent_name
         FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE m.id = $1 AND m.tenant_id = $2",
    )
    .bind(body.message_id)
    .bind(tenant_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (_msg_id, conversation_id, agent_name) = msg_row.ok_or_else(|| {
        CasperError::NotFound(format!("message {}", body.message_id))
    })?;

    let id = Uuid::now_v7();
    let tags_val: Option<&[String]> = if body.tags.is_empty() {
        None
    } else {
        Some(&body.tags)
    };

    let row: FeedbackResponse = sqlx::query_as(
        "INSERT INTO message_feedback (id, tenant_id, message_id, conversation_id, agent_name, feedback_type, rating, correction, tags, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, tenant_id, message_id, conversation_id, agent_name, feedback_type, rating, correction, tags, created_by, created_at",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(body.message_id)
    .bind(conversation_id)
    .bind(&agent_name)
    .bind(&body.feedback_type)
    .bind(body.rating)
    .bind(&body.correction)
    .bind(tags_val)
    .bind(&actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(row))
}

/// GET /api/v1/feedback -- List feedback with filters.
async fn list_feedback(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ListFeedbackParams>,
) -> Result<Json<Vec<FeedbackResponse>>, CasperError> {
    // Require feedback:write or admin scope
    let has_feedback = guard.require("feedback:write").is_ok();
    let has_admin = guard.require("admin:*").is_ok();
    if !has_feedback && !has_admin {
        return Err(CasperError::Forbidden(
            "requires feedback:write or admin:* scope".into(),
        ));
    }

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let tdb = TenantDb::new(state.db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<FeedbackResponse> = sqlx::query_as(
        "SELECT id, tenant_id, message_id, conversation_id, agent_name, feedback_type,
                rating, correction, tags, created_by, created_at
         FROM message_feedback
         WHERE tenant_id = $1
           AND ($2::TEXT IS NULL OR agent_name = $2)
           AND ($3::TEXT IS NULL OR feedback_type = $3)
           AND ($4::TIMESTAMPTZ IS NULL OR created_at >= $4::TIMESTAMPTZ)
           AND ($5::TIMESTAMPTZ IS NULL OR created_at <= $5::TIMESTAMPTZ)
         ORDER BY created_at DESC
         LIMIT $6 OFFSET $7",
    )
    .bind(tenant_id.0)
    .bind(&params.agent_name)
    .bind(&params.feedback_type)
    .bind(&params.from)
    .bind(&params.to)
    .bind(params.limit)
    .bind(params.offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(rows))
}

// ── Router ──────────────────────────────────────────────────────

pub fn feedback_router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/feedback",
        post(create_feedback).get(list_feedback),
    )
}
