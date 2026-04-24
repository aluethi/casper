//! Service layer for tenant-scoped MCP server connections.

use casper_base::{CasperError, TenantId};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

// ── Domain types ────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize, Clone)]
pub struct McpConnectionResponse {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub url: String,
    pub auth_type: String,
    pub auth_provider: Option<String>,
    pub is_active: bool,
    pub created_by: String,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
    #[serde(serialize_with = "serialize_dt")]
    pub updated_at: OffsetDateTime,
}

#[derive(Deserialize)]
pub struct CreateMcpConnectionRequest {
    pub name: String,
    pub display_name: String,
    pub url: String,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    pub api_key: Option<String>,
    pub auth_provider: Option<String>,
}

fn default_auth_type() -> String {
    "none".to_string()
}

#[derive(Deserialize)]
pub struct UpdateMcpConnectionRequest {
    pub display_name: Option<String>,
    pub url: Option<String>,
    pub auth_type: Option<String>,
    pub api_key: Option<String>,
    pub auth_provider: Option<String>,
    pub is_active: Option<bool>,
}

/// Resolved MCP connection with decrypted secrets, ready for the agent engine.
pub struct ResolvedMcpConnection {
    pub name: String,
    pub url: String,
    pub api_key: Option<String>,
    pub auth_type: String,
    pub auth_provider: Option<String>,
}

// ── Service functions ───────────────────────────────────────────

pub async fn create(
    db: &PgPool,
    vault: &casper_base::Vault,
    tenant_id: TenantId,
    actor: &str,
    req: &CreateMcpConnectionRequest,
) -> Result<McpConnectionResponse, CasperError> {
    let id = Uuid::now_v7();

    let api_key_enc = match (&req.auth_type as &str, &req.api_key) {
        ("bearer", Some(key)) if !key.is_empty() => {
            Some(vault.encrypt_value(tenant_id, key.as_bytes())?)
        }
        _ => None,
    };

    sqlx::query(
        "INSERT INTO mcp_connections (id, tenant_id, name, display_name, url, auth_type, api_key_enc, auth_provider, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&req.name)
    .bind(&req.display_name)
    .bind(&req.url)
    .bind(&req.auth_type)
    .bind(&api_key_enc)
    .bind(&req.auth_provider)
    .bind(actor)
    .execute(db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") {
            CasperError::Conflict(format!("MCP connection '{}' already exists", req.name))
        } else {
            CasperError::Internal(format!("DB error: {e}"))
        }
    })?;

    get_by_name(db, tenant_id, &req.name).await
}

pub async fn list(
    db: &PgPool,
    tenant_id: TenantId,
) -> Result<Vec<McpConnectionResponse>, CasperError> {
    let rows: Vec<McpConnectionResponse> = sqlx::query_as(
        "SELECT id, name, display_name, url, auth_type, auth_provider, is_active, created_by, created_at, updated_at
         FROM mcp_connections
         WHERE tenant_id = $1
         ORDER BY display_name",
    )
    .bind(tenant_id.0)
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows)
}

pub async fn get_by_name(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
) -> Result<McpConnectionResponse, CasperError> {
    let row: McpConnectionResponse = sqlx::query_as(
        "SELECT id, name, display_name, url, auth_type, auth_provider, is_active, created_by, created_at, updated_at
         FROM mcp_connections
         WHERE tenant_id = $1 AND name = $2",
    )
    .bind(tenant_id.0)
    .bind(name)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    .ok_or_else(|| CasperError::NotFound(format!("MCP connection '{name}'")))?;

    Ok(row)
}

pub async fn update(
    db: &PgPool,
    vault: &casper_base::Vault,
    tenant_id: TenantId,
    name: &str,
    req: &UpdateMcpConnectionRequest,
) -> Result<McpConnectionResponse, CasperError> {
    let _existing = get_by_name(db, tenant_id, name).await?;

    if let Some(ref display_name) = req.display_name {
        sqlx::query("UPDATE mcp_connections SET display_name = $1, updated_at = now() WHERE tenant_id = $2 AND name = $3")
            .bind(display_name)
            .bind(tenant_id.0)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref url) = req.url {
        sqlx::query("UPDATE mcp_connections SET url = $1, updated_at = now() WHERE tenant_id = $2 AND name = $3")
            .bind(url)
            .bind(tenant_id.0)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref auth_type) = req.auth_type {
        sqlx::query("UPDATE mcp_connections SET auth_type = $1, updated_at = now() WHERE tenant_id = $2 AND name = $3")
            .bind(auth_type)
            .bind(tenant_id.0)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref api_key) = req.api_key {
        let enc = if api_key.is_empty() {
            None
        } else {
            Some(vault.encrypt_value(tenant_id, api_key.as_bytes())?)
        };
        sqlx::query("UPDATE mcp_connections SET api_key_enc = $1, updated_at = now() WHERE tenant_id = $2 AND name = $3")
            .bind(&enc)
            .bind(tenant_id.0)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref auth_provider) = req.auth_provider {
        sqlx::query("UPDATE mcp_connections SET auth_provider = $1, updated_at = now() WHERE tenant_id = $2 AND name = $3")
            .bind(auth_provider)
            .bind(tenant_id.0)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(is_active) = req.is_active {
        sqlx::query("UPDATE mcp_connections SET is_active = $1, updated_at = now() WHERE tenant_id = $2 AND name = $3")
            .bind(is_active)
            .bind(tenant_id.0)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }

    get_by_name(db, tenant_id, name).await
}

pub async fn delete(db: &PgPool, tenant_id: TenantId, name: &str) -> Result<(), CasperError> {
    let result = sqlx::query(
        "UPDATE mcp_connections SET is_active = false, updated_at = now() WHERE tenant_id = $1 AND name = $2",
    )
    .bind(tenant_id.0)
    .bind(name)
    .execute(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("MCP connection '{name}'")));
    }

    Ok(())
}

/// Resolve MCP connections for an agent by name.
/// Queries the agent's tools config from DB, extracts connection names, resolves them.
pub async fn resolve_for_agent_by_name(
    db: &PgPool,
    vault: &casper_base::Vault,
    tenant_id: TenantId,
    agent_name: &str,
) -> Result<Vec<ResolvedMcpConnection>, CasperError> {
    let tools: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT tools FROM agents WHERE tenant_id = $1 AND name = $2 AND is_active = true",
    )
    .bind(tenant_id.0)
    .bind(agent_name)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    match tools {
        Some(ref t) => resolve_for_agent_config(db, vault, tenant_id, t).await,
        None => Ok(Vec::new()),
    }
}

/// Extract MCP connection names from an agent's tools JSON.
/// Handles the new format where `tools.mcp` is an array of strings.
pub fn extract_mcp_names(tools: &serde_json::Value) -> Vec<String> {
    tools
        .get("mcp")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Resolve MCP connections for an agent given its tools JSON config.
/// Extracts connection names, fetches from DB, decrypts secrets.
pub async fn resolve_for_agent_config(
    db: &PgPool,
    vault: &casper_base::Vault,
    tenant_id: TenantId,
    tools: &serde_json::Value,
) -> Result<Vec<ResolvedMcpConnection>, CasperError> {
    let names = extract_mcp_names(tools);
    resolve_for_agent(db, vault, tenant_id, &names).await
}

/// Resolve a list of MCP connection names into full configs with decrypted bearer tokens.
/// Used by the agent engine before building the tool dispatcher.
pub async fn resolve_for_agent(
    db: &PgPool,
    vault: &casper_base::Vault,
    tenant_id: TenantId,
    names: &[String],
) -> Result<Vec<ResolvedMcpConnection>, CasperError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    #[derive(sqlx::FromRow)]
    struct Row {
        name: String,
        url: String,
        auth_type: String,
        api_key_enc: Option<String>,
        auth_provider: Option<String>,
    }

    let rows: Vec<Row> = sqlx::query_as(
        "SELECT name, url, auth_type, api_key_enc, auth_provider
         FROM mcp_connections
         WHERE tenant_id = $1 AND name = ANY($2) AND is_active = true",
    )
    .bind(tenant_id.0)
    .bind(names)
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let mut resolved = Vec::with_capacity(rows.len());
    for Row {
        name,
        url,
        auth_type,
        api_key_enc,
        auth_provider,
    } in rows
    {
        let api_key = match api_key_enc {
            Some(ref enc) if !enc.is_empty() => {
                let decrypted = vault.decrypt_value(tenant_id, enc)?;
                Some(
                    decrypted
                        .expose_str()
                        .map_err(|e| {
                            CasperError::Internal(format!("invalid api_key encoding: {e}"))
                        })?
                        .to_string(),
                )
            }
            _ => None,
        };
        resolved.push(ResolvedMcpConnection {
            name,
            url,
            api_key,
            auth_type,
            auth_provider,
        });
    }

    Ok(resolved)
}
