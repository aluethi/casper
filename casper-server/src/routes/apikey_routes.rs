use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;
use crate::pagination::{PaginationParams, PaginatedResponse, Pagination};

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateApiKeyRequest {
    pub name: Option<String>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ApiKeyResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: String,
    pub created_by: String,
}

/// Response returned only on creation, includes the plaintext key.
#[derive(Serialize)]
pub struct ApiKeyCreatedResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub key_prefix: String,
    pub key: String,
    pub is_active: bool,
    pub created_at: String,
    pub created_by: String,
}

type ApiKeyRow = (Uuid, Uuid, String, Vec<String>, String, bool, OffsetDateTime, String);

fn row_to_response(r: ApiKeyRow) -> ApiKeyResponse {
    ApiKeyResponse {
        id: r.0,
        tenant_id: r.1,
        name: r.2,
        scopes: r.3,
        key_prefix: r.4,
        is_active: r.5,
        created_at: to_rfc3339(r.6),
        created_by: r.7,
    }
}

/// POST /api/v1/api-keys — Create API key, return plaintext once.
async fn create_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyCreatedResponse>, CasperError> {
    guard.require("keys:manage")?;

    let id = Uuid::now_v7();
    let key = format!("csk-{}", Uuid::now_v7());
    let key_hash = hex::encode(Sha256::digest(key.as_bytes()));
    let key_prefix = key[..12.min(key.len())].to_string();

    let row: ApiKeyRow = sqlx::query_as(
        "INSERT INTO api_keys (id, tenant_id, name, scopes, key_hash, key_prefix, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by"
    )
    .bind(id)
    .bind(guard.0.tenant_id.0)
    .bind(&body.name)
    .bind(&body.scopes)
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(guard.0.actor())
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("api_keys_tenant_id_name_key") => {
            CasperError::Conflict(format!("API key '{}' already exists in tenant", body.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(ApiKeyCreatedResponse {
        id: row.0,
        tenant_id: row.1,
        name: row.2,
        scopes: row.3,
        key_prefix: row.4,
        key,
        is_active: row.5,
        created_at: to_rfc3339(row.6),
        created_by: row.7,
    }))
}

/// GET /api/v1/api-keys — List API keys (never return key_hash).
async fn list_api_keys(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<ApiKeyResponse>>, CasperError> {
    guard.require("keys:manage")?;

    let offset = params.offset();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys")
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<ApiKeyRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by
         FROM api_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(params.limit())
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

/// GET /api/v1/api-keys/:id — Get single API key.
async fn get_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiKeyResponse>, CasperError> {
    guard.require("keys:manage")?;

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by
         FROM api_keys WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("API key {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/api-keys/:id — Update name/scopes (key unchanged).
async fn update_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, CasperError> {
    guard.require("keys:manage")?;

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "UPDATE api_keys SET
            name = COALESCE($2, name),
            scopes = COALESCE($3, scopes)
         WHERE id = $1
         RETURNING id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by"
    )
    .bind(id)
    .bind(&body.name)
    .bind(&body.scopes)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("API key {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// DELETE /api/v1/api-keys/:id — Set is_active=false (soft delete).
async fn delete_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiKeyResponse>, CasperError> {
    guard.require("keys:manage")?;

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "UPDATE api_keys SET is_active = false
         WHERE id = $1
         RETURNING id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by"
    )
    .bind(id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("API key {id}")))?;
    Ok(Json(row_to_response(r)))
}

pub fn apikey_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/api-keys", post(create_api_key).get(list_api_keys))
        .route("/api/v1/api-keys/{id}", get(get_api_key).patch(update_api_key).delete(delete_api_key))
}
