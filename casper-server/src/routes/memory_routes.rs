use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use casper_base::CasperError;
use serde::Deserialize;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::memory_service::{
    self, AgentMemoryHistoryResponse, AgentMemoryResponse, TenantMemoryHistoryResponse,
    TenantMemoryResponse,
};

// ── Route-specific request types ─────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateMemoryRequest {
    pub content: String,
}

// ══════════════════════════════════════════════════════════════════
// Agent memory handlers
// ══════════════════════════════════════════════════════════════════

/// GET /api/v1/agents/:name/memory -- Get current memory document.
async fn get_agent_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<AgentMemoryResponse>, CasperError> {
    guard.require("memory:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let mem = memory_service::get_agent_memory(&state.db, tenant_id, &name).await?;
    Ok(Json(mem))
}

/// PUT /api/v1/agents/:name/memory -- Update memory. Old version goes to history.
async fn update_agent_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<UpdateMemoryRequest>,
) -> Result<Json<AgentMemoryResponse>, CasperError> {
    guard.require("memory:write")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let mem = memory_service::update_agent_memory(
        &state.db,
        tenant_id,
        &name,
        &body.content,
        &guard.0.actor(),
    )
    .await?;
    Ok(Json(mem))
}

/// GET /api/v1/agents/:name/memory/history -- List memory versions.
async fn list_agent_memory_history(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<Vec<AgentMemoryHistoryResponse>>, CasperError> {
    guard.require("memory:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let history =
        memory_service::list_agent_memory_history(&state.db, tenant_id, &name).await?;
    Ok(Json(history))
}

/// GET /api/v1/agents/:name/memory/history/:version -- Get specific version.
async fn get_agent_memory_version(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((name, version)): Path<(String, i32)>,
) -> Result<Json<AgentMemoryHistoryResponse>, CasperError> {
    guard.require("memory:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let entry =
        memory_service::get_agent_memory_version(&state.db, tenant_id, &name, version).await?;
    Ok(Json(entry))
}

// ══════════════════════════════════════════════════════════════════
// Tenant memory handlers
// ══════════════════════════════════════════════════════════════════

/// GET /api/v1/tenant-memory -- Get current tenant memory.
async fn get_tenant_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<TenantMemoryResponse>, CasperError> {
    guard.require("memory:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let mem = memory_service::get_tenant_memory(&state.db, tenant_id).await?;
    Ok(Json(mem))
}

/// PUT /api/v1/tenant-memory -- Update tenant memory.
async fn update_tenant_memory(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<UpdateMemoryRequest>,
) -> Result<Json<TenantMemoryResponse>, CasperError> {
    guard.require("memory:write")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let mem = memory_service::update_tenant_memory(
        &state.db,
        tenant_id,
        &body.content,
        &guard.0.actor(),
    )
    .await?;
    Ok(Json(mem))
}

/// GET /api/v1/tenant-memory/history -- List tenant memory versions.
async fn list_tenant_memory_history(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<TenantMemoryHistoryResponse>>, CasperError> {
    guard.require("memory:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let history = memory_service::list_tenant_memory_history(&state.db, tenant_id).await?;
    Ok(Json(history))
}

/// GET /api/v1/tenant-memory/history/:version -- Get specific version.
async fn get_tenant_memory_version(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(version): Path<i32>,
) -> Result<Json<TenantMemoryHistoryResponse>, CasperError> {
    guard.require("memory:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let entry =
        memory_service::get_tenant_memory_version(&state.db, tenant_id, version).await?;
    Ok(Json(entry))
}

// ══════════════════════════════════════════════════════════════════
// Router
// ══════════════════════════════════════════════════════════════════

pub fn memory_router() -> Router<AppState> {
    Router::new()
        // Agent memory
        .route(
            "/api/v1/agents/{name}/memory",
            get(get_agent_memory).put(update_agent_memory),
        )
        .route(
            "/api/v1/agents/{name}/memory/history",
            get(list_agent_memory_history),
        )
        .route(
            "/api/v1/agents/{name}/memory/history/{version}",
            get(get_agent_memory_version),
        )
        // Tenant memory
        .route(
            "/api/v1/tenant-memory",
            get(get_tenant_memory).put(update_tenant_memory),
        )
        .route(
            "/api/v1/tenant-memory/history",
            get(list_tenant_memory_history),
        )
        .route(
            "/api/v1/tenant-memory/history/{version}",
            get(get_tenant_memory_version),
        )
}
