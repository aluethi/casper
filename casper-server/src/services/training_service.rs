use casper_base::{CasperError, TenantId};
use casper_base::TenantDb;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

// ── Domain types ─────────────────────────────────────────────────

fn default_format() -> String {
    "jsonl".to_string()
}

fn default_limit() -> i64 {
    1000
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

// ── Service functions ────────────────────────────────────────────

pub async fn export(
    db: &PgPool,
    tenant_id: TenantId,
    params: &ExportParams,
) -> Result<ExportResponse, CasperError> {
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
        return export_pairs(db, tenant_id, params).await;
    }

    // JSONL format: return conversations with messages and quality tier
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

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
    .bind(tenant_id.0)
    .bind(&params.tier)
    .bind(&params.agent)
    .bind(&params.from)
    .bind(&params.to)
    .bind(params.limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error querying conversation_quality: {e}")))?;

    let conv_ids: Vec<Uuid> = conv_rows.iter().map(|r| r.0).collect();

    let msg_rows: Vec<(Uuid, String, serde_json::Value, OffsetDateTime)> = if conv_ids.is_empty() {
        vec![]
    } else {
        sqlx::query_as(
            "SELECT conversation_id, role, content, created_at
             FROM messages
             WHERE tenant_id = $1 AND conversation_id = ANY($2)
             ORDER BY conversation_id, created_at",
        )
        .bind(tenant_id.0)
        .bind(&conv_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error fetching messages: {e}")))?
    };

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

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

    Ok(ExportResponse::Conversations(conversations))
}

async fn export_pairs(
    db: &PgPool,
    tenant_id: TenantId,
    params: &ExportParams,
) -> Result<ExportResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

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
    .bind(tenant_id.0)
    .bind(&params.agent)
    .bind(&params.from)
    .bind(&params.to)
    .bind(params.limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let mut pairs = Vec::new();
    for (conv_id, agent_name, rejected_content, correction, created_at) in rows {
        let user_msg: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT content FROM messages
             WHERE conversation_id = $1 AND tenant_id = $2 AND role = 'user'
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(conv_id)
        .bind(tenant_id.0)
        .fetch_optional(&mut *tx)
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

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(ExportResponse::Pairs(pairs))
}
