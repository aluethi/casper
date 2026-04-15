use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;
use crate::pagination::{PaginationParams, PaginatedResponse, Pagination};

// ── Request / Response types ───────────────────────────────────────

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

// Row: never select api_key_enc
type BackendRow = (
    Uuid, String, String, Option<String>, Option<String>,
    Option<String>, i32, i32, serde_json::Value, bool, OffsetDateTime,
);

fn row_to_response(r: BackendRow) -> BackendResponse {
    BackendResponse {
        id: r.0,
        name: r.1,
        provider: r.2,
        provider_label: r.3,
        base_url: r.4,
        region: r.5,
        priority: r.6,
        max_queue_depth: r.7,
        extra_config: r.8,
        is_active: r.9,
        created_at: to_rfc3339(r.10),
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

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/backends — Create backend.
async fn create_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateBackendRequest>,
) -> Result<Json<BackendResponse>, CasperError> {
    guard.require("platform:admin")?;

    let id = Uuid::now_v7();

    let row: BackendRow = sqlx::query_as(&format!(
        "INSERT INTO platform_backends (
            id, name, provider, provider_label, base_url,
            api_key_enc, region, priority, max_queue_depth, extra_config
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING {BACKEND_COLUMNS}"
    ))
    .bind(id)
    .bind(&body.name)
    .bind(&body.provider)
    .bind(&body.provider_label)
    .bind(&body.base_url)
    .bind(&body.api_key_enc)
    .bind(&body.region)
    .bind(body.priority)
    .bind(body.max_queue_depth)
    .bind(&body.extra_config)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("platform_backends_name_key") => {
            CasperError::Conflict(format!("backend '{}' already exists", body.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/backends — List backends (never returns api_key_enc).
async fn list_backends(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<BackendResponse>>, CasperError> {
    guard.require("platform:admin")?;

    let offset = (params.page - 1) * params.per_page;

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM platform_backends")
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<BackendRow> = sqlx::query_as(&format!(
        "SELECT {BACKEND_COLUMNS} FROM platform_backends ORDER BY priority, created_at DESC LIMIT $1 OFFSET $2"
    ))
    .bind(params.per_page)
    .bind(offset)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();

    Ok(Json(PaginatedResponse {
        data,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: total.0,
        },
    }))
}

/// GET /api/v1/backends/:id — Get single backend (never returns api_key_enc).
async fn get_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<BackendResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<BackendRow> = sqlx::query_as(&format!(
        "SELECT {BACKEND_COLUMNS} FROM platform_backends WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("backend {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/backends/:id — Update backend.
async fn update_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBackendRequest>,
) -> Result<Json<BackendResponse>, CasperError> {
    guard.require("platform:admin")?;

    // If api_key_enc is being updated, do that separately (it is not in RETURNING).
    if let Some(ref key) = body.api_key_enc {
        sqlx::query("UPDATE platform_backends SET api_key_enc = $2 WHERE id = $1")
            .bind(id)
            .bind(key)
            .execute(&state.db_owner)
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
    .bind(&body.name)
    .bind(&body.provider)
    .bind(&body.provider_label)
    .bind(&body.base_url)
    .bind(&body.region)
    .bind(body.priority)
    .bind(body.max_queue_depth)
    .bind(&body.extra_config)
    .bind(body.is_active)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("backend {id}")))?;
    Ok(Json(row_to_response(r)))
}

// ── Backend-model assignment handlers ──────────────────────────────

/// POST /api/v1/models/:id/backends — Assign backend to model.
async fn assign_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(model_id): Path<Uuid>,
    Json(body): Json<AssignBackendRequest>,
) -> Result<Json<BackendModelResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: (Uuid, Uuid, i32) = sqlx::query_as(
        "INSERT INTO platform_backend_models (backend_id, model_id, priority)
         VALUES ($1, $2, $3)
         ON CONFLICT (backend_id, model_id) DO UPDATE SET priority = EXCLUDED.priority
         RETURNING backend_id, model_id, priority"
    )
    .bind(body.backend_id)
    .bind(model_id)
    .bind(body.priority)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(BackendModelResponse {
        backend_id: row.0,
        model_id: row.1,
        priority: row.2,
    }))
}

/// GET /api/v1/models/:id/backends — List backends for model.
async fn list_model_backends(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(model_id): Path<Uuid>,
) -> Result<Json<Vec<BackendModelResponse>>, CasperError> {
    guard.require("platform:admin")?;

    let rows: Vec<(Uuid, Uuid, i32)> = sqlx::query_as(
        "SELECT backend_id, model_id, priority
         FROM platform_backend_models
         WHERE model_id = $1
         ORDER BY priority"
    )
    .bind(model_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows
        .into_iter()
        .map(|r| BackendModelResponse {
            backend_id: r.0,
            model_id: r.1,
            priority: r.2,
        })
        .collect();

    Ok(Json(data))
}

/// DELETE /api/v1/models/:model_id/backends/:backend_id — Remove assignment.
async fn remove_backend_assignment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((model_id, backend_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("platform:admin")?;

    let result = sqlx::query(
        "DELETE FROM platform_backend_models WHERE model_id = $1 AND backend_id = $2"
    )
    .bind(model_id)
    .bind(backend_id)
    .execute(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!(
            "backend assignment (model={model_id}, backend={backend_id})"
        )));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn backend_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/backends", post(create_backend).get(list_backends))
        .route("/api/v1/backends/{id}", get(get_backend).patch(update_backend))
        .route("/api/v1/models/{id}/backends", post(assign_backend).get(list_model_backends))
        .route(
            "/api/v1/models/{model_id}/backends/{backend_id}",
            axum::routing::delete(remove_backend_assignment),
        )
}
