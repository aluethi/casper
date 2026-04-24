//! Routes for tenant-scoped MCP server connections.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use casper_base::CasperError;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::mcp_connection_service::{
    self, CreateMcpConnectionRequest, McpConnectionResponse, UpdateMcpConnectionRequest,
};

async fn create_connection(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateMcpConnectionRequest>,
) -> Result<Json<McpConnectionResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = guard.0.tenant_id;
    let conn =
        mcp_connection_service::create(&state.db, &state.vault, tenant_id, &guard.0.actor(), &body)
            .await?;
    Ok(Json(conn))
}

async fn list_connections(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<McpConnectionResponse>>, CasperError> {
    guard.require("agents:run")?;
    let tenant_id = guard.0.tenant_id;
    let conns = mcp_connection_service::list(&state.db, tenant_id).await?;
    Ok(Json(conns))
}

async fn get_connection(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<McpConnectionResponse>, CasperError> {
    guard.require("agents:run")?;
    let tenant_id = guard.0.tenant_id;
    let conn = mcp_connection_service::get_by_name(&state.db, tenant_id, &name).await?;
    Ok(Json(conn))
}

async fn update_connection(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<UpdateMcpConnectionRequest>,
) -> Result<Json<McpConnectionResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = guard.0.tenant_id;
    let conn =
        mcp_connection_service::update(&state.db, &state.vault, tenant_id, &name, &body).await?;
    Ok(Json(conn))
}

async fn delete_connection(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<(), CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = guard.0.tenant_id;
    mcp_connection_service::delete(&state.db, tenant_id, &name).await
}

pub fn mcp_connection_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/mcp-connections",
            post(create_connection).get(list_connections),
        )
        .route(
            "/api/v1/mcp-connections/{name}",
            get(get_connection)
                .patch(update_connection)
                .delete(delete_connection),
        )
}
