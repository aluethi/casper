use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
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

fn default_format() -> String {
    "jsonl".to_string()
}

#[derive(Deserialize)]
pub struct ExportParams {
    #[serde(default = "default_format")]
    pub format: String,
    pub tier: Option<String>,
    pub agent: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    1000
}

/// A conversation with quality tier and its messages, for training export.
#[derive(Serialize)]
pub struct TrainingConversation {
    pub conversation_id: Uuid,
    pub agent_name: String,
    pub quality_tier: String,
    pub outcome: Option<String>,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
    pub messages: Vec<TrainingMessage>,
}

#[derive(Serialize)]
pub struct TrainingMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

/// A chosen/rejected pair for training from corrections.
#[derive(Serialize)]
pub struct TrainingPair {
    pub conversation_id: Uuid,
    pub agent_name: String,
    pub user_message: serde_json::Value,
    pub chosen: String,
    pub rejected: serde_json::Value,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ExportResponse {
    Conversations(Vec<TrainingConversation>),
    Pairs(Vec<TrainingPair>),
}

const VALID_TIERS: &[&str] = &["gold", "silver", "bronze", "excluded"];
const VALID_FORMATS: &[&str] = &["jsonl", "pairs"];

// ── Handlers ────────────────────────────────────────────────────

/// GET /api/v1/training/export -- Export training data.
async fn export_training(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ExportParams>,
) -> Result<Json<ExportResponse>, CasperError> {
    guard.require("training:export")?;

    let tenant_id = guard.0.tenant_id.0;

    // Validate format
    if !VALID_FORMATS.contains(&params.format.as_str()) {
        return Err(CasperError::BadRequest(format!(
            "invalid format '{}', must be one of: {}",
            params.format,
            VALID_FORMATS.join(", ")
        )));
    }

    // Validate tier if provided
    if let Some(ref tier) = params.tier {
        if !VALID_TIERS.contains(&tier.as_str()) {
            return Err(CasperError::BadRequest(format!(
                "invalid tier '{}', must be one of: {}",
                tier,
                VALID_TIERS.join(", ")
            )));
        }
    }

    if params.format == "pairs" {
        return export_pairs(&state, tenant_id, &params).await;
    }

    // JSONL format: return conversations with messages and quality tier

    // Query the conversation_quality view for matching conversations
    let conv_rows: Vec<(Uuid, String, Option<String>, OffsetDateTime, String)> = sqlx::query_as(
        "SELECT cq.id, cq.agent_name, cq.outcome, cq.created_at, cq.quality_tier
         FROM conversation_quality cq
         WHERE cq.tenant_id = $1
           AND ($2::TEXT IS NULL OR cq.quality_tier = $2)
           AND ($3::TEXT IS NULL OR cq.agent_name = $3)
           AND ($4::TIMESTAMPTZ IS NULL OR cq.created_at >= $4::TIMESTAMPTZ)
           AND ($5::TIMESTAMPTZ IS NULL OR cq.created_at <= $5::TIMESTAMPTZ)
         ORDER BY cq.created_at DESC
         LIMIT $6",
    )
    .bind(tenant_id)
    .bind(&params.tier)
    .bind(&params.agent)
    .bind(&params.from)
    .bind(&params.to)
    .bind(params.limit)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error querying conversation_quality: {e}")))?;

    let conv_ids: Vec<Uuid> = conv_rows.iter().map(|r| r.0).collect();

    // Fetch messages for all matching conversations in one query
    let msg_rows: Vec<(Uuid, String, serde_json::Value, OffsetDateTime)> = if conv_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            "SELECT conversation_id, role, content, created_at
             FROM messages
             WHERE tenant_id = $1 AND conversation_id = ANY($2)
             ORDER BY conversation_id, created_at",
        )
        .bind(tenant_id)
        .bind(&conv_ids)
        .fetch_all(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error fetching messages: {e}")))?
    };

    // Group messages by conversation_id
    let mut msg_map: std::collections::HashMap<Uuid, Vec<TrainingMessage>> =
        std::collections::HashMap::new();
    for (conv_id, role, content, created_at) in msg_rows {
        msg_map
            .entry(conv_id)
            .or_default()
            .push(TrainingMessage {
                role,
                content,
                created_at,
            });
    }

    // Build result
    let conversations: Vec<TrainingConversation> = conv_rows
        .into_iter()
        .map(|(id, agent_name, outcome, created_at, quality_tier)| {
            let messages = msg_map.remove(&id).unwrap_or_default();
            TrainingConversation {
                conversation_id: id,
                agent_name,
                quality_tier,
                outcome,
                created_at,
                messages,
            }
        })
        .collect();

    Ok(Json(ExportResponse::Conversations(conversations)))
}

/// Export correction pairs for RLHF training.
async fn export_pairs(
    state: &AppState,
    tenant_id: Uuid,
    params: &ExportParams,
) -> Result<Json<ExportResponse>, CasperError> {
    // Find corrections: feedback entries with type 'correction' and the original message
    let rows: Vec<(Uuid, String, serde_json::Value, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT f.conversation_id, f.agent_name, m.content, f.correction, f.created_at
         FROM message_feedback f
         JOIN messages m ON m.id = f.message_id
         WHERE f.tenant_id = $1
           AND f.feedback_type = 'correction'
           AND f.correction IS NOT NULL
           AND ($2::TEXT IS NULL OR f.agent_name = $2)
           AND ($3::TIMESTAMPTZ IS NULL OR f.created_at >= $3::TIMESTAMPTZ)
           AND ($4::TIMESTAMPTZ IS NULL OR f.created_at <= $4::TIMESTAMPTZ)
         ORDER BY f.created_at DESC
         LIMIT $5",
    )
    .bind(tenant_id)
    .bind(&params.agent)
    .bind(&params.from)
    .bind(&params.to)
    .bind(params.limit)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // For each correction, try to find the preceding user message
    let mut pairs = Vec::new();
    for (conv_id, agent_name, rejected_content, correction, created_at) in rows {
        // Get the most recent user message before the corrected assistant message
        let user_msg: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT content FROM messages
             WHERE conversation_id = $1 AND tenant_id = $2 AND role = 'user'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(conv_id)
        .bind(tenant_id)
        .fetch_optional(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

        let user_message = user_msg
            .map(|(c,)| c)
            .unwrap_or(serde_json::Value::String("".to_string()));

        pairs.push(TrainingPair {
            conversation_id: conv_id,
            agent_name,
            user_message,
            chosen: correction,
            rejected: rejected_content,
            created_at,
        });
    }

    Ok(Json(ExportResponse::Pairs(pairs)))
}

// ── Router ──────────────────────────────────────────────────────

pub fn training_router() -> Router<AppState> {
    Router::new().route("/api/v1/training/export", get(export_training))
}
