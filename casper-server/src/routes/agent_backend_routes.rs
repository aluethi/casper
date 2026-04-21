use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, post},
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;

// ── Request / Response types ──────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateAgentKeyRequest {
    pub name: String,
}

#[derive(Serialize)]
pub struct CreateAgentKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub key: String, // plaintext — shown once
    pub backend_id: Uuid,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct AgentKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key_prefix: String,
    pub backend_id: Uuid,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct AgentKeyRow {
    id: Uuid,
    name: String,
    key_prefix: String,
    backend_id: Uuid,
    is_active: bool,
    created_at: OffsetDateTime,
}

// ── Handlers ──────────────────────────────────────────────────────

/// POST /api/v1/backends/:id/keys — Create an agent backend key.
async fn create_agent_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(backend_id): Path<Uuid>,
    Json(body): Json<CreateAgentKeyRequest>,
) -> Result<Json<CreateAgentKeyResponse>, CasperError> {
    guard.require("platform:admin")?;

    // Verify backend exists
    let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM platform_backends WHERE id = $1")
        .bind(backend_id)
        .fetch_optional(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if exists.is_none() {
        return Err(CasperError::NotFound(format!("backend {backend_id}")));
    }

    // Generate key: csa-{UUIDv7}
    let key_uuid = Uuid::now_v7();
    let plaintext_key = format!("csa-{}", key_uuid.simple());
    let key_prefix = &plaintext_key[..12]; // "csa-" + 8 hex chars
    let key_hash = hex::encode(Sha256::digest(plaintext_key.as_bytes()));

    let id = Uuid::now_v7();
    let created_by = guard.0.subject.to_string();

    let row: AgentKeyRow = sqlx::query_as(
        "INSERT INTO agent_backend_keys (id, name, key_hash, key_prefix, backend_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, name, key_prefix, backend_id, is_active, created_at",
    )
    .bind(id)
    .bind(&body.name)
    .bind(&key_hash)
    .bind(key_prefix)
    .bind(backend_id)
    .bind(&created_by)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(CreateAgentKeyResponse {
        id: row.id,
        name: row.name,
        key_prefix: row.key_prefix,
        key: plaintext_key, // returned once, never stored
        backend_id: row.backend_id,
        created_at: to_rfc3339(row.created_at),
    }))
}

/// GET /api/v1/backends/:id/keys — List agent keys (prefix only, never hash).
async fn list_agent_keys(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(backend_id): Path<Uuid>,
) -> Result<Json<Vec<AgentKeyResponse>>, CasperError> {
    guard.require("platform:admin")?;

    let rows: Vec<AgentKeyRow> = sqlx::query_as(
        "SELECT id, name, key_prefix, backend_id, is_active, created_at
         FROM agent_backend_keys
         WHERE backend_id = $1
         ORDER BY created_at DESC",
    )
    .bind(backend_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows
        .into_iter()
        .map(|r| AgentKeyResponse {
            id: r.id,
            name: r.name,
            key_prefix: r.key_prefix,
            backend_id: r.backend_id,
            is_active: r.is_active,
            created_at: to_rfc3339(r.created_at),
        })
        .collect();

    Ok(Json(data))
}

/// DELETE /api/v1/backends/:id/keys/:key_id — Revoke agent key (soft-delete).
async fn revoke_agent_key(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((backend_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("platform:admin")?;

    let result = sqlx::query(
        "UPDATE agent_backend_keys SET is_active = false
         WHERE id = $1 AND backend_id = $2",
    )
    .bind(key_id)
    .bind(backend_id)
    .execute(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!(
            "agent key {key_id} for backend {backend_id}"
        )));
    }

    Ok(Json(serde_json::json!({ "revoked": true })))
}

// ── Router ────────────────────────────────────────────────────────

pub fn agent_backend_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/backends/{id}/keys",
            post(create_agent_key).get(list_agent_keys),
        )
        .route(
            "/api/v1/backends/{backend_id}/keys/{key_id}",
            delete(revoke_agent_key),
        )
}
