use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use casper_base::{CasperError, TenantId};

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::secret_service::{self, SecretKeyResponse, SetSecretRequest};

// ── Handlers ────────────────────────────────────────────────────

/// POST /api/v1/secrets -- Set (upsert) a secret with AES-256-GCM encryption.
async fn set_secret(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<SetSecretRequest>,
) -> Result<Json<SecretKeyResponse>, CasperError> {
    guard.require("secrets:write")?;
    let tenant_id = TenantId(guard.0.tenant_id.0);
    let result = secret_service::set(&state.db, &state.db_owner, &state.vault, tenant_id, &body).await?;
    Ok(Json(result))
}

/// GET /api/v1/secrets -- List secret keys only (never values).
async fn list_secrets(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<SecretKeyResponse>>, CasperError> {
    guard.require("secrets:read")?;
    let tenant_id = TenantId(guard.0.tenant_id.0);
    let result = secret_service::list(&state.db, tenant_id).await?;
    Ok(Json(result))
}

/// DELETE /api/v1/secrets/:key -- Delete a secret.
async fn delete_secret(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("secrets:write")?;
    let tenant_id = TenantId(guard.0.tenant_id.0);
    let result = secret_service::delete(&state.db_owner, &state.vault, tenant_id, &key).await?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────────────────────────

pub fn secret_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/secrets", post(set_secret).get(list_secrets))
        .route("/api/v1/secrets/{key}", axum::routing::delete(delete_secret))
}
