use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;

#[derive(Deserialize)]
pub struct SetSecretRequest {
    pub key: String,
    pub value: String,
}

#[derive(Serialize)]
pub struct SecretKeyResponse {
    pub key: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow)]
struct SecretKeyRow {
    key: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

fn row_to_response(r: SecretKeyRow) -> SecretKeyResponse {
    SecretKeyResponse {
        key: r.key,
        created_at: to_rfc3339(r.created_at),
        updated_at: to_rfc3339(r.updated_at),
    }
}

/// POST /api/v1/secrets — Set (upsert) a secret.
async fn set_secret(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<SetSecretRequest>,
) -> Result<Json<SecretKeyResponse>, CasperError> {
    guard.require("secrets:write")?;

    let tenant_id = guard.0.tenant_id.0;
    let id = Uuid::now_v7();

    // Store plaintext as placeholder; proper Vault encryption will be wired later.
    // Using base64-encoded value as ciphertext_b64, "none" as nonce_b64.
    use base64::Engine;
    let ciphertext_b64 = base64::engine::general_purpose::STANDARD.encode(body.value.as_bytes());

    let row: SecretKeyRow = sqlx::query_as(
        "INSERT INTO tenant_secrets (id, tenant_id, key, ciphertext_b64, nonce_b64)
         VALUES ($1, $2, $3, $4, 'none')
         ON CONFLICT (tenant_id, key) DO UPDATE SET
            ciphertext_b64 = EXCLUDED.ciphertext_b64,
            updated_at = now()
         RETURNING key, created_at, updated_at"
    )
    .bind(id)
    .bind(tenant_id)
    .bind(&body.key)
    .bind(&ciphertext_b64)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/secrets — List secret keys only (never values).
async fn list_secrets(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<SecretKeyResponse>>, CasperError> {
    guard.require("secrets:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let rows: Vec<SecretKeyRow> = sqlx::query_as(
        "SELECT key, created_at, updated_at
         FROM tenant_secrets WHERE tenant_id = $1
         ORDER BY key"
    )
    .bind(tenant_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();
    Ok(Json(data))
}

/// DELETE /api/v1/secrets/:key — Delete a secret.
async fn delete_secret(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("secrets:write")?;

    let tenant_id = guard.0.tenant_id.0;

    let result = sqlx::query("DELETE FROM tenant_secrets WHERE tenant_id = $1 AND key = $2")
        .bind(tenant_id)
        .bind(&key)
        .execute(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("secret '{key}'")));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub fn secret_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/secrets", post(set_secret).get(list_secrets))
        .route("/api/v1/secrets/{key}", axum::routing::delete(delete_secret))
}
