use bigdecimal::BigDecimal;
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

// ── Domain types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AllocateQuotaRequest {
    pub model_id: Uuid,
    #[serde(default)]
    pub requests_per_minute: i32,
    #[serde(default)]
    pub tokens_per_day: i64,
    #[serde(default)]
    pub cache_tokens_per_day: i64,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
    pub cost_per_1k_cache_read: Option<f64>,
    pub cost_per_1k_cache_write: Option<f64>,
}

#[derive(Deserialize)]
pub struct UpdateQuotaRequest {
    pub requests_per_minute: Option<i32>,
    pub tokens_per_day: Option<i64>,
    pub cache_tokens_per_day: Option<i64>,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
    pub cost_per_1k_cache_read: Option<f64>,
    pub cost_per_1k_cache_write: Option<f64>,
}

#[derive(Serialize)]
pub struct QuotaResponse {
    pub tenant_id: Uuid,
    pub model_id: Uuid,
    pub requests_per_minute: i32,
    pub tokens_per_day: i64,
    pub cache_tokens_per_day: i64,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
    pub cost_per_1k_cache_read: Option<f64>,
    pub cost_per_1k_cache_write: Option<f64>,
    pub allocated_by: String,
    pub allocated_at: String,
}

#[derive(sqlx::FromRow)]
struct QuotaRow {
    tenant_id: Uuid,
    model_id: Uuid,
    requests_per_minute: i32,
    tokens_per_day: i64,
    cache_tokens_per_day: i64,
    cost_per_1k_input: Option<BigDecimal>,
    cost_per_1k_output: Option<BigDecimal>,
    cost_per_1k_cache_read: Option<BigDecimal>,
    cost_per_1k_cache_write: Option<BigDecimal>,
    allocated_by: String,
    allocated_at: OffsetDateTime,
}

fn bd_to_f64(bd: BigDecimal) -> f64 {
    use bigdecimal::ToPrimitive;
    bd.to_f64().unwrap_or(0.0)
}

fn row_to_response(r: QuotaRow) -> QuotaResponse {
    QuotaResponse {
        tenant_id: r.tenant_id,
        model_id: r.model_id,
        requests_per_minute: r.requests_per_minute,
        tokens_per_day: r.tokens_per_day,
        cache_tokens_per_day: r.cache_tokens_per_day,
        cost_per_1k_input: r.cost_per_1k_input.map(bd_to_f64),
        cost_per_1k_output: r.cost_per_1k_output.map(bd_to_f64),
        cost_per_1k_cache_read: r.cost_per_1k_cache_read.map(bd_to_f64),
        cost_per_1k_cache_write: r.cost_per_1k_cache_write.map(bd_to_f64),
        allocated_by: r.allocated_by,
        allocated_at: to_rfc3339(r.allocated_at),
    }
}

const QUOTA_COLUMNS: &str = "tenant_id, model_id, requests_per_minute, tokens_per_day, cache_tokens_per_day, \
     cost_per_1k_input, cost_per_1k_output, cost_per_1k_cache_read, cost_per_1k_cache_write, \
     allocated_by, allocated_at";

// ── Service functions (platform-scoped: takes db_owner directly) ─

pub async fn allocate(
    db: &PgPool,
    tenant_id: Uuid,
    req: &AllocateQuotaRequest,
    actor: &str,
) -> Result<QuotaResponse, CasperError> {
    let row: QuotaRow = sqlx::query_as(&format!(
        "INSERT INTO model_quotas (
            tenant_id, model_id, requests_per_minute, tokens_per_day, cache_tokens_per_day,
            cost_per_1k_input, cost_per_1k_output, cost_per_1k_cache_read, cost_per_1k_cache_write,
            allocated_by
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING {QUOTA_COLUMNS}"
    ))
    .bind(tenant_id)
    .bind(req.model_id)
    .bind(req.requests_per_minute)
    .bind(req.tokens_per_day)
    .bind(req.cache_tokens_per_day)
    .bind(req.cost_per_1k_input)
    .bind(req.cost_per_1k_output)
    .bind(req.cost_per_1k_cache_read)
    .bind(req.cost_per_1k_cache_write)
    .bind(actor)
    .fetch_one(db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("model_quotas_pkey") => {
            CasperError::Conflict(format!(
                "quota already exists for model {} in tenant {tenant_id}",
                req.model_id
            ))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(row_to_response(row))
}

pub async fn list(db: &PgPool, tenant_id: Uuid) -> Result<Vec<QuotaResponse>, CasperError> {
    let rows: Vec<QuotaRow> = sqlx::query_as(&format!(
        "SELECT {QUOTA_COLUMNS} FROM model_quotas WHERE tenant_id = $1 ORDER BY allocated_at DESC"
    ))
    .bind(tenant_id)
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows.into_iter().map(row_to_response).collect())
}

pub async fn update(
    db: &PgPool,
    tenant_id: Uuid,
    model_id: Uuid,
    req: &UpdateQuotaRequest,
    actor: &str,
) -> Result<QuotaResponse, CasperError> {
    let row: Option<QuotaRow> = sqlx::query_as(&format!(
        "UPDATE model_quotas SET
            requests_per_minute     = COALESCE($3, requests_per_minute),
            tokens_per_day          = COALESCE($4, tokens_per_day),
            cache_tokens_per_day    = COALESCE($5, cache_tokens_per_day),
            cost_per_1k_input       = COALESCE($6, cost_per_1k_input),
            cost_per_1k_output      = COALESCE($7, cost_per_1k_output),
            cost_per_1k_cache_read  = COALESCE($8, cost_per_1k_cache_read),
            cost_per_1k_cache_write = COALESCE($9, cost_per_1k_cache_write),
            allocated_by            = $10,
            allocated_at            = now()
         WHERE tenant_id = $1 AND model_id = $2
         RETURNING {QUOTA_COLUMNS}"
    ))
    .bind(tenant_id)
    .bind(model_id)
    .bind(req.requests_per_minute)
    .bind(req.tokens_per_day)
    .bind(req.cache_tokens_per_day)
    .bind(req.cost_per_1k_input)
    .bind(req.cost_per_1k_output)
    .bind(req.cost_per_1k_cache_read)
    .bind(req.cost_per_1k_cache_write)
    .bind(actor)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response).ok_or_else(|| {
        CasperError::NotFound(format!("quota for model {model_id} in tenant {tenant_id}"))
    })
}

pub async fn delete(
    db: &PgPool,
    tenant_id: Uuid,
    model_id: Uuid,
) -> Result<serde_json::Value, CasperError> {
    let result = sqlx::query("DELETE FROM model_quotas WHERE tenant_id = $1 AND model_id = $2")
        .bind(tenant_id)
        .bind(model_id)
        .execute(db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!(
            "quota for model {model_id} in tenant {tenant_id}"
        )));
    }

    Ok(serde_json::json!({ "deleted": true }))
}
