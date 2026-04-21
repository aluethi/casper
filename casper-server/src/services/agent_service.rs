use casper_agent::prompt::assemble_system_prompt;
use casper_base::{CasperError, TenantId};
use casper_base::TenantDb;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

// ── Domain types ─────────────────────────────────────────────────

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

// ── Request types (shared with routes) ───────────────────────────

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

fn default_json_array() -> serde_json::Value {
    serde_json::json!([])
}
fn default_json_obj() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub model_deployment: Option<String>,
    pub prompt_stack: Option<serde_json::Value>,
    pub tools: Option<serde_json::Value>,
    pub config: Option<serde_json::Value>,
}

// ── Service functions ────────────────────────────────────────────

pub async fn create(
    db: &PgPool,
    tenant_id: TenantId,
    req: &CreateAgentRequest,
    actor: &str,
) -> Result<AgentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Validate model_deployment references an active deployment for this tenant
    let deployment_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM model_deployments
            WHERE tenant_id = $1 AND slug = $2 AND is_active = true
        )",
    )
    .bind(tenant_id.0)
    .bind(&req.model_deployment)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !deployment_exists {
        return Err(CasperError::BadRequest(format!(
            "model deployment '{}' not found or inactive for this tenant",
            req.model_deployment
        )));
    }

    let id = Uuid::now_v7();

    let row: AgentResponse = sqlx::query_as(
        "INSERT INTO agents (id, tenant_id, name, display_name, description, model_deployment, prompt_stack, tools, config, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id, tenant_id, name, display_name, description, model_deployment,
                   prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by",
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&req.name)
    .bind(&req.display_name)
    .bind(&req.description)
    .bind(&req.model_deployment)
    .bind(&req.prompt_stack)
    .bind(&req.tools)
    .bind(&req.config)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("agents_tenant_id_name_key") =>
        {
            CasperError::Conflict(format!("agent '{}' already exists", req.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(row)
}

pub async fn list(
    db: &PgPool,
    tenant_id: TenantId,
    include_inactive: bool,
) -> Result<Vec<AgentResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<AgentResponse> = if include_inactive {
        sqlx::query_as(
            "SELECT id, tenant_id, name, display_name, description, model_deployment,
                    prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
             FROM agents WHERE tenant_id = $1
             ORDER BY name",
        )
        .bind(tenant_id.0)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    } else {
        sqlx::query_as(
            "SELECT id, tenant_id, name, display_name, description, model_deployment,
                    prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
             FROM agents WHERE tenant_id = $1 AND is_active = true
             ORDER BY name",
        )
        .bind(tenant_id.0)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    };

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows)
}

pub async fn get_by_name(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
) -> Result<AgentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<AgentResponse> = sqlx::query_as(
        "SELECT id, tenant_id, name, display_name, description, model_deployment,
                prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by
         FROM agents WHERE tenant_id = $1 AND name = $2",
    )
    .bind(tenant_id.0)
    .bind(name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))
}

pub async fn update(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
    req: &UpdateAgentRequest,
) -> Result<AgentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // If model_deployment is being updated, validate it
    if let Some(ref slug) = req.model_deployment {
        let deployment_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM model_deployments
                WHERE tenant_id = $1 AND slug = $2 AND is_active = true
            )",
        )
        .bind(tenant_id.0)
        .bind(slug)
        .fetch_one(&mut *tx)
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
                   prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by",
    )
    .bind(tenant_id.0)
    .bind(name)
    .bind(&req.display_name)
    .bind(&req.description)
    .bind(&req.model_deployment)
    .bind(&req.prompt_stack)
    .bind(&req.tools)
    .bind(&req.config)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))
}

pub async fn delete(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
) -> Result<AgentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<AgentResponse> = sqlx::query_as(
        "UPDATE agents SET is_active = false, updated_at = now()
         WHERE tenant_id = $1 AND name = $2 AND is_active = true
         RETURNING id, tenant_id, name, display_name, description, model_deployment,
                   prompt_stack, tools, config, version, is_active, created_at, updated_at, created_by",
    )
    .bind(tenant_id.0)
    .bind(name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))
}

pub async fn export_yaml(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
) -> Result<(String, String), CasperError> {
    let agent = get_by_name(db, tenant_id, name).await?;

    let export = AgentExport {
        name: agent.name.clone(),
        display_name: agent.display_name,
        description: agent.description,
        model_deployment: agent.model_deployment,
        prompt_stack: agent.prompt_stack,
        tools: agent.tools,
        config: agent.config,
    };

    let yaml = serde_yaml::to_string(&export)
        .map_err(|e| CasperError::Internal(format!("YAML serialization error: {e}")))?;

    Ok((yaml, agent.name))
}

pub async fn import_yaml(
    db: &PgPool,
    tenant_id: TenantId,
    yaml: &str,
    actor: &str,
) -> Result<AgentResponse, CasperError> {
    let export: AgentExport = serde_yaml::from_str(yaml)
        .map_err(|e| CasperError::BadRequest(format!("invalid YAML: {e}")))?;

    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Validate model_deployment references an active deployment for this tenant
    let deployment_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM model_deployments
            WHERE tenant_id = $1 AND slug = $2 AND is_active = true
        )",
    )
    .bind(tenant_id.0)
    .bind(&export.model_deployment)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !deployment_exists {
        return Err(CasperError::BadRequest(format!(
            "model deployment '{}' not found or inactive for this tenant",
            export.model_deployment
        )));
    }

    // Check if agent already exists -- update if so, create if not
    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM agents WHERE tenant_id = $1 AND name = $2")
            .bind(tenant_id.0)
            .bind(&export.name)
            .fetch_optional(&mut *tx)
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
        .bind(tenant_id.0)
        .bind(&export.display_name)
        .bind(&export.description)
        .bind(&export.model_deployment)
        .bind(&export.prompt_stack)
        .bind(&export.tools)
        .bind(&export.config)
        .fetch_one(&mut *tx)
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
        .bind(tenant_id.0)
        .bind(&export.name)
        .bind(&export.display_name)
        .bind(&export.description)
        .bind(&export.model_deployment)
        .bind(&export.prompt_stack)
        .bind(&export.tools)
        .bind(&export.config)
        .bind(actor)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    };

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(row)
}

/// Assemble and return the full system prompt for an agent (for debugging).
///
/// Uses the same pipeline as the agent engine: builds a tool dispatcher
/// (which discovers MCP tools), then assembles the prompt with full tool docs.
pub async fn preview_prompt(
    db: &PgPool,
    http_client: &reqwest::Client,
    tenant_id: TenantId,
    name: &str,
) -> Result<String, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<(String, Option<String>, serde_json::Value, serde_json::Value)> =
        sqlx::query_as(
            "SELECT name, description, prompt_stack, tools
             FROM agents WHERE tenant_id = $1 AND name = $2 AND is_active = true",
        )
        .bind(tenant_id.0)
        .bind(name)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (agent_name, description, prompt_stack, tools) =
        row.ok_or_else(|| CasperError::NotFound(format!("agent '{name}'")))?;

    let tenant_name: String = sqlx::query_scalar(
        "SELECT display_name FROM tenants WHERE id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "Unknown".to_string());

    // Discover MCP tools so the preview includes their documentation
    let dispatcher = casper_agent::tools::build_dispatcher(&tools, http_client).await;
    let mcp_summaries = dispatcher.mcp_tool_summaries();

    let prompt = assemble_system_prompt(
        &prompt_stack,
        &tools,
        &agent_name,
        description.as_deref().unwrap_or(""),
        tenant_id.0,
        &tenant_name,
        db,
        &mcp_summaries,
    )
    .await;

    Ok(prompt)
}
