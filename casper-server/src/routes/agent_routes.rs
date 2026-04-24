use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use casper_base::CasperError;
use serde::Deserialize;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::agent_service::{self, AgentResponse, CreateAgentRequest, UpdateAgentRequest};

// ── Route-specific types ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListAgentsParams {
    #[serde(default)]
    pub include_inactive: bool,
}

// ── Handlers ─────────────────────────────────────────────────────

/// POST /api/v1/agents -- Create agent.
async fn create_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateAgentRequest>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let agent = agent_service::create(&state.db, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(agent))
}

/// GET /api/v1/agents -- List agents for tenant.
async fn list_agents(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ListAgentsParams>,
) -> Result<Json<Vec<AgentResponse>>, CasperError> {
    guard.require("agents:run")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let agents = agent_service::list(&state.db, tenant_id, params.include_inactive).await?;
    Ok(Json(agents))
}

/// GET /api/v1/agents/:name -- Get agent by name.
async fn get_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:run")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let agent = agent_service::get_by_name(&state.db, tenant_id, &name).await?;
    Ok(Json(agent))
}

/// PATCH /api/v1/agents/:name -- Update agent. Increments version.
async fn update_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<UpdateAgentRequest>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let agent = agent_service::update(&state.db, tenant_id, &name, &body).await?;
    Ok(Json(agent))
}

/// DELETE /api/v1/agents/:name -- Soft delete (is_active=false).
async fn delete_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let agent = agent_service::delete(&state.db, tenant_id, &name).await?;
    Ok(Json(agent))
}

/// GET /api/v1/agents/:name/export -- Export agent configuration as YAML.
async fn export_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<axum::response::Response, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let (yaml, agent_name) = agent_service::export_yaml(&state.db, tenant_id, &name).await?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/x-yaml".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}.yaml\"", agent_name)
            .parse()
            .unwrap(),
    );

    Ok((StatusCode::OK, headers, yaml).into_response())
}

/// GET /api/v1/agents/:name/prompt -- Preview the assembled system prompt.
async fn preview_prompt(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<String, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    agent_service::preview_prompt(
        &state.db_owner,
        &state.vault,
        &state.http_client,
        tenant_id,
        &name,
    )
    .await
}

/// POST /api/v1/agents/import -- Import agent from YAML body.
async fn import_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    body: String,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let agent = agent_service::import_yaml(&state.db, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(agent))
}

// ── Router ───────────────────────────────────────────────────────

pub fn agent_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents", post(create_agent).get(list_agents))
        .route(
            "/api/v1/agents/{name}",
            get(get_agent).patch(update_agent).delete(delete_agent),
        )
        .route("/api/v1/agents/{name}/export", get(export_agent))
        .route("/api/v1/agents/{name}/prompt", get(preview_prompt))
        .route("/api/v1/agents/import", post(import_agent))
}
