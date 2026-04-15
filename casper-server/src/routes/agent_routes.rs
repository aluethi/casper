use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
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

// ── Request / Response types ──────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub model_deployment: String,
    #[serde(default = "default_json_array")]
    pub prompt_stack: serde_json::Value,
    #[serde(default = "default_json_obj")]
    pub tools: serde_json::Value,
    #[serde(default = "default_json_obj")]
    pub config: serde_json::Value,
}

fn default_json_array() -> serde_json::Value { serde_json::json!([]) }
fn default_json_obj() -> serde_json::Value { serde_json::json!({}) }

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub model_deployment: Option<String>,
    pub prompt_stack: Option<serde_json::Value>,
    pub tools: Option<serde_json::Value>,
    pub config: Option<serde_json::Value>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct AgentResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub model_deployment: String,
    pub prompt_stack: serde_json::Value,
    pub tools: serde_json::Value,
    pub config: serde_json::Value,
    pub version: i32,
    pub is_active: bool,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
    #[serde(serialize_with = "serialize_dt")]
    pub updated_at: OffsetDateTime,
    pub created_by: String,
}

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

#[derive(Deserialize)]
pub struct ListAgentsParams {
    #[serde(default)]
    pub include_inactive: bool,
}

// ── Handlers ──────────────────────────────────────────────────────

/// POST /api/v1/agents -- Create agent.
async fn create_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateAgentRequest>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    // Validate model_deployment references an active deployment for this tenant
    let deployment_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM model_deployments
            WHERE tenant_id = $1 AND slug = $2 AND is_active = true
        )"
    )
    .bind(tenant_id)
    .bind(&body.model_deployment)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !deployment_exists {
        return Err(CasperError::BadRequest(format!(
            "model deployment '{}' not found or inactive for this tenant",
            body.model_deployment
        )));
    }

    let id = Uuid::now_v7();

    let row: AgentResponse = sqlx::query_as(
        "INSERT INTO agents (id, tenant_id, name, display_name, description, model_deployment, prompt_stack, tools, config, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, tenant_id, name, display_name, description, model_deployment,
                   prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by"
    )
    .bind(id)
    .bind(tenant_id)
    .bind(&body.name)
    .bind(&body.display_name)
    .bind(&body.description)
    .bind(&body.model_deployment)
    .bind(&body.prompt_stack)
    .bind(&body.tools)
    .bind(&body.config)
    .bind(guard.0.actor())
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("agents_tenant_id_name_key") =>
        {
            CasperError::Conflict(format!("agent '{}' already exists", body.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row))
}

/// GET /api/v1/agents -- List agents for tenant.
async fn list_agents(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ListAgentsParams>,
) -> Result<Json<Vec<AgentResponse>>, CasperError> {
    guard.require("agents:run")?;

    let tenant_id = guard.0.tenant_id.0;

    let rows: Vec<AgentResponse> = if params.include_inactive {
        sqlx::query_as(
            "SELECT id, tenant_id, name, display_name, description, model_deployment,
                    prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
             FROM agents WHERE tenant_id = $1
             ORDER BY name"
        )
        .bind(tenant_id)
        .fetch_all(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    } else {
        sqlx::query_as(
            "SELECT id, tenant_id, name, display_name, description, model_deployment,
                    prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
             FROM agents WHERE tenant_id = $1 AND is_active = true
             ORDER BY name"
        )
        .bind(tenant_id)
        .fetch_all(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    };

    Ok(Json(rows))
}

/// GET /api/v1/agents/:name -- Get agent by name.
async fn get_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:run")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<AgentResponse> = sqlx::query_as(
        "SELECT id, tenant_id, name, display_name, description, model_deployment,
                prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
         FROM agents WHERE tenant_id = $1 AND name = $2"
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))?;
    Ok(Json(r))
}

/// PATCH /api/v1/agents/:name -- Update agent. Increments version.
async fn update_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<UpdateAgentRequest>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    // If model_deployment is being updated, validate it
    if let Some(ref slug) = body.model_deployment {
        let deployment_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM model_deployments
                WHERE tenant_id = $1 AND slug = $2 AND is_active = true
            )"
        )
        .bind(tenant_id)
        .bind(slug)
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

        if !deployment_exists {
            return Err(CasperError::BadRequest(format!(
                "model deployment '{slug}' not found or inactive for this tenant"
            )));
        }
    }

    let row: Option<AgentResponse> = sqlx::query_as(
        "UPDATE agents SET
            display_name     = COALESCE($3, display_name),
            description      = COALESCE($4, description),
            model_deployment = COALESCE($5, model_deployment),
            prompt_stack     = COALESCE($6, prompt_stack),
            tools            = COALESCE($7, tools),
            config           = COALESCE($8, config),
            version          = version + 1,
            updated_at       = now()
         WHERE tenant_id = $1 AND name = $2 AND is_active = true
         RETURNING id, tenant_id, name, display_name, description, model_deployment,
                   prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by"
    )
    .bind(tenant_id)
    .bind(&name)
    .bind(&body.display_name)
    .bind(&body.description)
    .bind(&body.model_deployment)
    .bind(&body.prompt_stack)
    .bind(&body.tools)
    .bind(&body.config)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))?;
    Ok(Json(r))
}

/// DELETE /api/v1/agents/:name -- Soft delete (is_active=false).
async fn delete_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<AgentResponse> = sqlx::query_as(
        "UPDATE agents SET is_active = false, updated_at = now()
         WHERE tenant_id = $1 AND name = $2 AND is_active = true
         RETURNING id, tenant_id, name, display_name, description, model_deployment,
                   prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by"
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))?;
    Ok(Json(r))
}

// ── Export / Import types ─────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct AgentExport {
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub model_deployment: String,
    pub prompt_stack: serde_json::Value,
    pub tools: serde_json::Value,
    pub config: serde_json::Value,
}

// ── Export / Import handlers ─────────────────────────────────────

/// GET /api/v1/agents/:name/export -- Export agent configuration as YAML.
async fn export_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<axum::response::Response, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<AgentResponse> = sqlx::query_as(
        "SELECT id, tenant_id, name, display_name, description, model_deployment,
                prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
         FROM agents WHERE tenant_id = $1 AND name = $2",
    )
    .bind(tenant_id)
    .bind(&name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let agent = row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))?;

    let export = AgentExport {
        name: agent.name,
        display_name: agent.display_name,
        description: agent.description,
        model_deployment: agent.model_deployment,
        prompt_stack: agent.prompt_stack,
        tools: agent.tools,
        config: agent.config,
    };

    let yaml = serde_yaml::to_string(&export)
        .map_err(|e| CasperError::Internal(format!("YAML serialization error: {e}")))?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/x-yaml".parse().unwrap(),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}.yaml\"", name)
            .parse()
            .unwrap(),
    );

    Ok((StatusCode::OK, headers, yaml).into_response())
}

/// POST /api/v1/agents/import -- Import agent from YAML body.
async fn import_agent(
    State(state): State<AppState>,
    guard: ScopeGuard,
    body: String,
) -> Result<Json<AgentResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    let export: AgentExport = serde_yaml::from_str(&body)
        .map_err(|e| CasperError::BadRequest(format!("invalid YAML: {e}")))?;

    // Validate model_deployment references an active deployment for this tenant
    let deployment_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM model_deployments
            WHERE tenant_id = $1 AND slug = $2 AND is_active = true
        )",
    )
    .bind(tenant_id)
    .bind(&export.model_deployment)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !deployment_exists {
        return Err(CasperError::BadRequest(format!(
            "model deployment '{}' not found or inactive for this tenant",
            export.model_deployment
        )));
    }

    // Check if agent already exists — update if so, create if not
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM agents WHERE tenant_id = $1 AND name = $2",
    )
    .bind(tenant_id)
    .bind(&export.name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: AgentResponse = if let Some((existing_id,)) = existing {
        // Update existing agent
        sqlx::query_as(
            "UPDATE agents SET
                display_name     = $3,
                description      = $4,
                model_deployment = $5,
                prompt_stack     = $6,
                tools            = $7,
                config           = $8,
                version          = version + 1,
                is_active        = true,
                updated_at       = now()
             WHERE id = $1 AND tenant_id = $2
             RETURNING id, tenant_id, name, display_name, description, model_deployment,
                       prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by",
        )
        .bind(existing_id)
        .bind(tenant_id)
        .bind(&export.display_name)
        .bind(&export.description)
        .bind(&export.model_deployment)
        .bind(&export.prompt_stack)
        .bind(&export.tools)
        .bind(&export.config)
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    } else {
        // Create new agent
        let id = Uuid::now_v7();
        sqlx::query_as(
            "INSERT INTO agents (id, tenant_id, name, display_name, description, model_deployment, prompt_stack, tools, config, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             RETURNING id, tenant_id, name, display_name, description, model_deployment,
                       prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(&export.name)
        .bind(&export.display_name)
        .bind(&export.description)
        .bind(&export.model_deployment)
        .bind(&export.prompt_stack)
        .bind(&export.tools)
        .bind(&export.config)
        .bind(guard.0.actor())
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    };

    Ok(Json(row))
}

// ── Router ────────────────────────────────────────────────────────

pub fn agent_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/agents", post(create_agent).get(list_agents))
        .route(
            "/api/v1/agents/{name}",
            get(get_agent).patch(update_agent).delete(delete_agent),
        )
        .route("/api/v1/agents/{name}/export", get(export_agent))
        .route("/api/v1/agents/import", post(import_agent))
}
