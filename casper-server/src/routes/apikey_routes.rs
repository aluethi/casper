use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::pagination::PaginationParams;
use crate::services::apikey_service::{
    self, ApiKeyCreatedResponse, ApiKeyResponse, CreateApiKeyRequest, UpdateApiKeyRequest,
};

// ── Handlers ──────────────────────────────────────────────────────

/// POST /api/v1/api-keys -- Create API key, return plaintext once.
async fn create_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateApiKeyRequest>,
) -> Result<Json<ApiKeyCreatedResponse>, CasperError> {
    guard.require("keys:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = apikey_service::create(&state.db, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(result))
}

/// GET /api/v1/api-keys -- List API keys (never return key_hash).
async fn list_api_keys(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<crate::pagination::PaginatedResponse<ApiKeyResponse>>, CasperError> {
    guard.require("keys:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = apikey_service::list(&state.db, tenant_id, &params).await?;
    Ok(Json(result))
}

/// GET /api/v1/api-keys/:id -- Get single API key.
async fn get_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiKeyResponse>, CasperError> {
    guard.require("keys:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = apikey_service::get(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/api-keys/:id -- Update name/scopes (key unchanged).
async fn update_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateApiKeyRequest>,
) -> Result<Json<ApiKeyResponse>, CasperError> {
    guard.require("keys:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = apikey_service::update(&state.db, tenant_id, id, &body).await?;
    Ok(Json(result))
}

/// DELETE /api/v1/api-keys/:id -- Set is_active=false (soft delete).
async fn delete_api_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ApiKeyResponse>, CasperError> {
    guard.require("keys:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = apikey_service::delete(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────────────────────────

pub fn apikey_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/api-keys", post(create_api_key).get(list_api_keys))
        .route(
            "/api/v1/api-keys/{id}",
            get(get_api_key)
                .patch(update_api_key)
                .delete(delete_api_key),
        )
}
