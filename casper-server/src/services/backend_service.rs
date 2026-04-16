use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;
use crate::pagination::{PaginatedResponse, Pagination, PaginationParams};

// ── Domain types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateBackendRequest {
    pub name: String,
    pub provider: String,
    pub provider_label: Option<String>,
    pub base_url: Option<String>,
    pub api_key_enc: Option<String>,
    pub region: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub max_queue_depth: i32,
    #[serde(default = "default_json_obj")]
    pub extra_config: serde_json::Value,
}

fn default_priority() -> i32 { 100 }
fn default_json_obj() -> serde_json::Value { serde_json::json!({}) }

#[derive(Deserialize)]
pub struct UpdateBackendRequest {
    pub name: Option<String>,
    pub provider: Option<String>,
    pub provider_label: Option<String>,
    pub base_url: Option<String>,
    pub api_key_enc: Option<String>,
    pub region: Option<String>,
    pub priority: Option<i32>,
    pub max_queue_depth: Option<i32>,
    pub extra_config: Option<serde_json::Value>,
    pub is_active: Option<bool>,
}

#[derive(Serialize)]
pub struct BackendResponse {
    pub id: Uuid,
    pub name: String,
    pub provider: String,
    pub provider_label: Option<String>,
    pub base_url: Option<String>,
    pub region: Option<String>,
    pub priority: i32,
    pub max_queue_depth: i32,
    pub extra_config: serde_json::Value,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct BackendRow {
    id: Uuid,
    name: String,
    provider: String,
    provider_label: Option<String>,
    base_url: Option<String>,
    region: Option<String>,
    priority: i32,
    max_queue_depth: i32,
    extra_config: serde_json::Value,
    is_active: bool,
    created_at: OffsetDateTime,
}

fn row_to_response(r: BackendRow) -> BackendResponse {
    BackendResponse {
        id: r.id,
        name: r.name,
        provider: r.provider,
        provider_label: r.provider_label,
        base_url: r.base_url,
        region: r.region,
        priority: r.priority,
        max_queue_depth: r.max_queue_depth,
        extra_config: r.extra_config,
        is_active: r.is_active,
        created_at: to_rfc3339(r.created_at),
    }
}

const BACKEND_COLUMNS: &str =
    "id, name, provider, provider_label, base_url, \
     region, priority, max_queue_depth, extra_config, is_active, created_at";

// ── Backend-model assignment types ─────────────────────────────────

#[derive(Deserialize)]
pub struct AssignBackendRequest {
    pub backend_id: Uuid,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

#[derive(Serialize)]
pub struct BackendModelResponse {
    pub backend_id: Uuid,
    pub model_id: Uuid,
    pub priority: i32,
}

#[derive(sqlx::FromRow)]
struct BackendModelRow {
    backend_id: Uuid,
    model_id: Uuid,
    priority: i32,
}

// ── Service functions (platform-scoped: takes db_owner directly) ─

pub async fn create(
    db: &PgPool,
    req: &CreateBackendRequest,
) -> Result<BackendResponse, CasperError> {
    let id = Uuid::now_v7();

    let row: BackendRow = sqlx::query_as(&format!(
        "INSERT INTO platform_backends (
            id, name, provider, provider_label, base_url,
            api_key_enc, region, priority, max_queue_depth, extra_config
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING {BACKEND_COLUMNS}"
    ))
    .bind(id)
    .bind(&req.name)
    .bind(&req.provider)
    .bind(&req.provider_label)
    .bind(&req.base_url)
    .bind(&req.api_key_enc)
    .bind(&req.region)
    .bind(req.priority)
    .bind(req.max_queue_depth)
    .bind(&req.extra_config)
    .fetch_one(db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("platform_backends_name_key") => {
            CasperError::Conflict(format!("backend '{}' already exists", req.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(row_to_response(row))
}

pub async fn list(
    db: &PgPool,
    params: &PaginationParams,
) -> Result<PaginatedResponse<BackendResponse>, CasperError> {
    let offset = params.offset();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM platform_backends")
        .fetch_one(db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<BackendRow> = sqlx::query_as(&format!(
        "SELECT {BACKEND_COLUMNS} FROM platform_backends ORDER BY priority, created_at DESC LIMIT $1 OFFSET $2"
    ))
    .bind(params.limit())
    .bind(offset)
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();

    Ok(PaginatedResponse {
        data,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: total.0,
        },
    })
}

pub async fn get(
    db: &PgPool,
    id: Uuid,
) -> Result<BackendResponse, CasperError> {
    let row: Option<BackendRow> = sqlx::query_as(&format!(
        "SELECT {BACKEND_COLUMNS} FROM platform_backends WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("backend {id}")))
}

pub async fn update(
    db: &PgPool,
    id: Uuid,
    req: &UpdateBackendRequest,
) -> Result<BackendResponse, CasperError> {
    // If api_key_enc is being updated, do that separately (it is not in RETURNING).
    if let Some(ref key) = req.api_key_enc {
        sqlx::query("UPDATE platform_backends SET api_key_enc = $2 WHERE id = $1")
            .bind(id)
            .bind(key)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }

    let row: Option<BackendRow> = sqlx::query_as(&format!(
        "UPDATE platform_backends SET
            name            = COALESCE($2, name),
            provider        = COALESCE($3, provider),
            provider_label  = COALESCE($4, provider_label),
            base_url        = COALESCE($5, base_url),
            region          = COALESCE($6, region),
            priority        = COALESCE($7, priority),
            max_queue_depth = COALESCE($8, max_queue_depth),
            extra_config    = COALESCE($9, extra_config),
            is_active       = COALESCE($10, is_active)
         WHERE id = $1
         RETURNING {BACKEND_COLUMNS}"
    ))
    .bind(id)
    .bind(&req.name)
    .bind(&req.provider)
    .bind(&req.provider_label)
    .bind(&req.base_url)
    .bind(&req.region)
    .bind(req.priority)
    .bind(req.max_queue_depth)
    .bind(&req.extra_config)
    .bind(req.is_active)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("backend {id}")))
}

// ── Backend-model assignment functions ────────────────────────────

pub async fn assign_model(
    db: &PgPool,
    model_id: Uuid,
    req: &AssignBackendRequest,
) -> Result<BackendModelResponse, CasperError> {
    let row: BackendModelRow = sqlx::query_as(
        "INSERT INTO platform_backend_models (backend_id, model_id, priority)
         VALUES ($1, $2, $3)
         ON CONFLICT (backend_id, model_id) DO UPDATE SET priority = EXCLUDED.priority
         RETURNING backend_id, model_id, priority"
    )
    .bind(req.backend_id)
    .bind(model_id)
    .bind(req.priority)
    .fetch_one(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(BackendModelResponse {
        backend_id: row.backend_id,
        model_id: row.model_id,
        priority: row.priority,
    })
}

pub async fn list_model_backends(
    db: &PgPool,
    model_id: Uuid,
) -> Result<Vec<BackendModelResponse>, CasperError> {
    let rows: Vec<BackendModelRow> = sqlx::query_as(
        "SELECT backend_id, model_id, priority
         FROM platform_backend_models
         WHERE model_id = $1
         ORDER BY priority"
    )
    .bind(model_id)
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|r| BackendModelResponse {
            backend_id: r.backend_id,
            model_id: r.model_id,
            priority: r.priority,
        })
        .collect())
}

pub async fn remove_model_backend(
    db: &PgPool,
    model_id: Uuid,
    backend_id: Uuid,
) -> Result<serde_json::Value, CasperError> {
    let result = sqlx::query(
        "DELETE FROM platform_backend_models WHERE model_id = $1 AND backend_id = $2"
    )
    .bind(model_id)
    .bind(backend_id)
    .execute(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!(
            "backend assignment (model={model_id}, backend={backend_id})"
        )));
    }

    Ok(serde_json::json!({ "deleted": true }))
}
