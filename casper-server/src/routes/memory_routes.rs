use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;

fn to_rfc3339(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

/// Estimate token count: ~4 characters per token.
fn estimate_tokens(text: &str) -> i32 {
    (text.len() / 4).max(0) as i32
}

// ══════════════════════════════════════════════════════════════════
// Agent memory types
// ══════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct AgentMemoryResponse {
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub content: String,
    pub token_count: i32,
    pub version: i32,
    pub updated_at: String,
}

type AgentMemoryRow = (Uuid, String, String, i32, i32, OffsetDateTime);

fn agent_memory_to_response(r: AgentMemoryRow) -> AgentMemoryResponse {
    AgentMemoryResponse {
        tenant_id: r.0,
        agent_name: r.1,
        content: r.2,
        token_count: r.3,
        version: r.4,
        updated_at: to_rfc3339(r.5),
    }
}

#[derive(Deserialize)]
pub struct UpdateMemoryRequest {
    pub content: String,
}

#[derive(Serialize)]
pub struct AgentMemoryHistoryResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub version: i32,
    pub content: String,
    pub token_count: i32,
    pub updated_by: String,
    pub created_at: String,
}

type AgentMemoryHistoryRow = (Uuid, Uuid, String, i32, String, i32, String, OffsetDateTime);

fn agent_history_to_response(r: AgentMemoryHistoryRow) -> AgentMemoryHistoryResponse {
    AgentMemoryHistoryResponse {
        id: r.0,
        tenant_id: r.1,
        agent_name: r.2,
        version: r.3,
        content: r.4,
        token_count: r.5,
        updated_by: r.6,
        created_at: to_rfc3339(r.7),
    }
}

// ══════════════════════════════════════════════════════════════════
// Agent memory handlers
// ══════════════════════════════════════════════════════════════════

/// GET /api/v1/agents/:name/memory — Get current memory document.
async fn get_agent_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<AgentMemoryResponse>, CasperError> {
    guard.require("memory:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<AgentMemoryRow> = sqlx::query_as(
        "SELECT tenant_id, agent_name, content, token_count, version, updated_at
         FROM agent_memory
         WHERE tenant_id = $1 AND agent_name = $2"
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("agent memory '{name}'")))?;
    Ok(Json(agent_memory_to_response(r)))
}

/// PUT /api/v1/agents/:name/memory — Update memory. Old version goes to history.
async fn update_agent_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<UpdateMemoryRequest>,
) -> Result<Json<AgentMemoryResponse>, CasperError> {
    guard.require("memory:write")?;

    let tenant_id = guard.0.tenant_id.0;
    let token_count = estimate_tokens(&body.content);
    let actor = guard.0.actor();

    // Try to get existing memory to archive it
    let existing: Option<(String, i32, i32)> = sqlx::query_as(
        "SELECT content, token_count, version
         FROM agent_memory
         WHERE tenant_id = $1 AND agent_name = $2"
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let new_version = match existing {
        Some((old_content, old_tokens, old_version)) => {
            // Archive current version to history
            let history_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO agent_memory_history (id, tenant_id, agent_name, version, content, token_count, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)"
            )
            .bind(history_id)
            .bind(tenant_id)
            .bind(&name)
            .bind(old_version)
            .bind(&old_content)
            .bind(old_tokens)
            .bind(&actor)
            .execute(&state.db_owner)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error archiving memory: {e}")))?;

            old_version + 1
        }
        None => 1, // First version
    };

    // Upsert current memory
    let row: AgentMemoryRow = sqlx::query_as(
        "INSERT INTO agent_memory (tenant_id, agent_name, content, token_count, version, updated_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (tenant_id, agent_name) DO UPDATE SET
            content = EXCLUDED.content,
            token_count = EXCLUDED.token_count,
            version = EXCLUDED.version,
            updated_at = now()
         RETURNING tenant_id, agent_name, content, token_count, version, updated_at"
    )
    .bind(tenant_id)
    .bind(&name)
    .bind(&body.content)
    .bind(token_count)
    .bind(new_version)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(agent_memory_to_response(row)))
}

/// GET /api/v1/agents/:name/memory/history — List memory versions.
async fn list_agent_memory_history(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<Vec<AgentMemoryHistoryResponse>>, CasperError> {
    guard.require("memory:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let rows: Vec<AgentMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, version, content, token_count, updated_by, created_at
         FROM agent_memory_history
         WHERE tenant_id = $1 AND agent_name = $2
         ORDER BY version DESC"
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(agent_history_to_response).collect();
    Ok(Json(data))
}

/// GET /api/v1/agents/:name/memory/history/:version — Get specific version.
async fn get_agent_memory_version(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((name, version)): Path<(String, i32)>,
) -> Result<Json<AgentMemoryHistoryResponse>, CasperError> {
    guard.require("memory:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<AgentMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, version, content, token_count, updated_by, created_at
         FROM agent_memory_history
         WHERE tenant_id = $1 AND agent_name = $2 AND version = $3"
    )
    .bind(tenant_id)
    .bind(&name)
    .bind(version)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("agent memory '{name}' version {version}")))?;
    Ok(Json(agent_history_to_response(r)))
}

// ══════════════════════════════════════════════════════════════════
// Tenant memory types
// ══════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct TenantMemoryResponse {
    pub tenant_id: Uuid,
    pub content: String,
    pub token_count: i32,
    pub version: i32,
    pub updated_at: String,
    pub updated_by: String,
}

type TenantMemoryRow = (Uuid, String, i32, i32, OffsetDateTime, String);

fn tenant_memory_to_response(r: TenantMemoryRow) -> TenantMemoryResponse {
    TenantMemoryResponse {
        tenant_id: r.0,
        content: r.1,
        token_count: r.2,
        version: r.3,
        updated_at: to_rfc3339(r.4),
        updated_by: r.5,
    }
}

#[derive(Serialize)]
pub struct TenantMemoryHistoryResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub version: i32,
    pub content: String,
    pub token_count: i32,
    pub updated_by: String,
    pub created_at: String,
}

type TenantMemoryHistoryRow = (Uuid, Uuid, i32, String, i32, String, OffsetDateTime);

fn tenant_history_to_response(r: TenantMemoryHistoryRow) -> TenantMemoryHistoryResponse {
    TenantMemoryHistoryResponse {
        id: r.0,
        tenant_id: r.1,
        version: r.2,
        content: r.3,
        token_count: r.4,
        updated_by: r.5,
        created_at: to_rfc3339(r.6),
    }
}

// ══════════════════════════════════════════════════════════════════
// Tenant memory handlers
// ══════════════════════════════════════════════════════════════════

/// GET /api/v1/tenant-memory — Get current tenant memory.
async fn get_tenant_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<TenantMemoryResponse>, CasperError> {
    guard.require("memory:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<TenantMemoryRow> = sqlx::query_as(
        "SELECT tenant_id, content, token_count, version, updated_at, updated_by
         FROM tenant_memory
         WHERE tenant_id = $1"
    )
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound("tenant memory".to_string()))?;
    Ok(Json(tenant_memory_to_response(r)))
}

/// PUT /api/v1/tenant-memory — Update tenant memory.
async fn update_tenant_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<UpdateMemoryRequest>,
) -> Result<Json<TenantMemoryResponse>, CasperError> {
    guard.require("memory:write")?;

    let tenant_id = guard.0.tenant_id.0;
    let token_count = estimate_tokens(&body.content);
    let actor = guard.0.actor();

    // Get existing to archive
    let existing: Option<(String, i32, i32, String)> = sqlx::query_as(
        "SELECT content, token_count, version, updated_by
         FROM tenant_memory
         WHERE tenant_id = $1"
    )
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let new_version = match existing {
        Some((old_content, old_tokens, old_version, old_updated_by)) => {
            // Archive current version to history
            let history_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO tenant_memory_history (id, tenant_id, version, content, token_count, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6)"
            )
            .bind(history_id)
            .bind(tenant_id)
            .bind(old_version)
            .bind(&old_content)
            .bind(old_tokens)
            .bind(&old_updated_by)
            .execute(&state.db_owner)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error archiving memory: {e}")))?;

            old_version + 1
        }
        None => 1,
    };

    // Upsert current tenant memory
    let row: TenantMemoryRow = sqlx::query_as(
        "INSERT INTO tenant_memory (tenant_id, content, token_count, version, updated_at, updated_by)
         VALUES ($1, $2, $3, $4, now(), $5)
         ON CONFLICT (tenant_id) DO UPDATE SET
            content = EXCLUDED.content,
            token_count = EXCLUDED.token_count,
            version = EXCLUDED.version,
            updated_at = now(),
            updated_by = EXCLUDED.updated_by
         RETURNING tenant_id, content, token_count, version, updated_at, updated_by"
    )
    .bind(tenant_id)
    .bind(&body.content)
    .bind(token_count)
    .bind(new_version)
    .bind(&actor)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(tenant_memory_to_response(row)))
}

/// GET /api/v1/tenant-memory/history — List tenant memory versions.
async fn list_tenant_memory_history(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<TenantMemoryHistoryResponse>>, CasperError> {
    guard.require("memory:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let rows: Vec<TenantMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, version, content, token_count, updated_by, created_at
         FROM tenant_memory_history
         WHERE tenant_id = $1
         ORDER BY version DESC"
    )
    .bind(tenant_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(tenant_history_to_response).collect();
    Ok(Json(data))
}

/// GET /api/v1/tenant-memory/history/:version — Get specific version.
async fn get_tenant_memory_version(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(version): Path<i32>,
) -> Result<Json<TenantMemoryHistoryResponse>, CasperError> {
    guard.require("memory:read")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<TenantMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, version, content, token_count, updated_by, created_at
         FROM tenant_memory_history
         WHERE tenant_id = $1 AND version = $2"
    )
    .bind(tenant_id)
    .bind(version)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("tenant memory version {version}")))?;
    Ok(Json(tenant_history_to_response(r)))
}

// ══════════════════════════════════════════════════════════════════
// Router
// ══════════════════════════════════════════════════════════════════

pub fn memory_router() -> Router<AppState> {
    Router::new()
        // Agent memory
        .route("/api/v1/agents/{name}/memory", get(get_agent_memory).put(update_agent_memory))
        .route("/api/v1/agents/{name}/memory/history", get(list_agent_memory_history))
        .route("/api/v1/agents/{name}/memory/history/{version}", get(get_agent_memory_version))
        // Tenant memory
        .route("/api/v1/tenant-memory", get(get_tenant_memory).put(update_tenant_memory))
        .route("/api/v1/tenant-memory/history", get(list_tenant_memory_history))
        .route("/api/v1/tenant-memory/history/{version}", get(get_tenant_memory_version))
}
