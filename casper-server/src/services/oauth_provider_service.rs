//! Service layer for OAuth provider management (platform scope).

use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}

// ── Domain types ────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize)]
pub struct OAuthProviderResponse {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub authorization_url: String,
    pub token_url: String,
    pub revocation_url: Option<String>,
    pub client_id: String,
    pub default_scopes: String,
    pub icon_url: Option<String>,
    pub is_active: bool,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

#[derive(Deserialize)]
pub struct CreateProviderRequest {
    pub name: String,
    pub display_name: String,
    pub authorization_url: String,
    pub token_url: String,
    pub revocation_url: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub default_scopes: String,
    pub icon_url: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateProviderRequest {
    pub display_name: Option<String>,
    pub authorization_url: Option<String>,
    pub token_url: Option<String>,
    pub revocation_url: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub default_scopes: Option<String>,
    pub icon_url: Option<String>,
    pub is_active: Option<bool>,
}

// ── Service functions ───────────────────────────────────────────

/// Create a new OAuth provider. Platform admin only.
pub async fn create(
    db: &PgPool,
    vault: &casper_base::Vault,
    req: &CreateProviderRequest,
) -> Result<OAuthProviderResponse, CasperError> {
    let id = Uuid::now_v7();

    // Encrypt client_secret with platform-level key (tenant_id = nil for platform scope)
    let client_secret_enc = vault.encrypt_value(
        casper_base::TenantId(Uuid::nil()),
        req.client_secret.as_bytes(),
    )?;

    sqlx::query(
        "INSERT INTO oauth_providers (id, name, display_name, authorization_url, token_url,
         revocation_url, client_id, client_secret_enc, default_scopes, icon_url)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.display_name)
    .bind(&req.authorization_url)
    .bind(&req.token_url)
    .bind(&req.revocation_url)
    .bind(&req.client_id)
    .bind(&client_secret_enc)
    .bind(&req.default_scopes)
    .bind(&req.icon_url)
    .execute(db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate key") {
            CasperError::Conflict(format!("provider '{}' already exists", req.name))
        } else {
            CasperError::Internal(format!("DB error: {e}"))
        }
    })?;

    get_by_name(db, &req.name).await
}

/// List all OAuth providers (active only by default).
pub async fn list(db: &PgPool) -> Result<Vec<OAuthProviderResponse>, CasperError> {
    let rows: Vec<OAuthProviderResponse> = sqlx::query_as(
        "SELECT id, name, display_name, authorization_url, token_url, revocation_url,
                client_id, default_scopes, icon_url, is_active, created_at
         FROM oauth_providers
         ORDER BY display_name",
    )
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows)
}

/// Get a single provider by name.
pub async fn get_by_name(db: &PgPool, name: &str) -> Result<OAuthProviderResponse, CasperError> {
    let row: OAuthProviderResponse = sqlx::query_as(
        "SELECT id, name, display_name, authorization_url, token_url, revocation_url,
                client_id, default_scopes, icon_url, is_active, created_at
         FROM oauth_providers
         WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    .ok_or_else(|| CasperError::NotFound(format!("provider '{name}'")))?;

    Ok(row)
}

/// Update a provider.
pub async fn update(
    db: &PgPool,
    vault: &casper_base::Vault,
    name: &str,
    req: &UpdateProviderRequest,
) -> Result<OAuthProviderResponse, CasperError> {
    // Verify provider exists
    let _existing = get_by_name(db, name).await?;

    if let Some(ref display_name) = req.display_name {
        sqlx::query("UPDATE oauth_providers SET display_name = $1 WHERE name = $2")
            .bind(display_name)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref auth_url) = req.authorization_url {
        sqlx::query("UPDATE oauth_providers SET authorization_url = $1 WHERE name = $2")
            .bind(auth_url)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref token_url) = req.token_url {
        sqlx::query("UPDATE oauth_providers SET token_url = $1 WHERE name = $2")
            .bind(token_url)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref revocation_url) = req.revocation_url {
        sqlx::query("UPDATE oauth_providers SET revocation_url = $1 WHERE name = $2")
            .bind(revocation_url)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref client_id) = req.client_id {
        sqlx::query("UPDATE oauth_providers SET client_id = $1 WHERE name = $2")
            .bind(client_id)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref client_secret) = req.client_secret {
        let enc =
            vault.encrypt_value(casper_base::TenantId(Uuid::nil()), client_secret.as_bytes())?;
        sqlx::query("UPDATE oauth_providers SET client_secret_enc = $1 WHERE name = $2")
            .bind(&enc)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref default_scopes) = req.default_scopes {
        sqlx::query("UPDATE oauth_providers SET default_scopes = $1 WHERE name = $2")
            .bind(default_scopes)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(ref icon_url) = req.icon_url {
        sqlx::query("UPDATE oauth_providers SET icon_url = $1 WHERE name = $2")
            .bind(icon_url)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }
    if let Some(is_active) = req.is_active {
        sqlx::query("UPDATE oauth_providers SET is_active = $1 WHERE name = $2")
            .bind(is_active)
            .bind(name)
            .execute(db)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    }

    get_by_name(db, name).await
}

/// Delete (deactivate) a provider.
pub async fn delete(db: &PgPool, name: &str) -> Result<(), CasperError> {
    let result = sqlx::query("UPDATE oauth_providers SET is_active = false WHERE name = $1")
        .bind(name)
        .execute(db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("provider '{name}'")));
    }

    Ok(())
}

// ── MCP OAuth 2.1 auto-discovery + DCR ─────────────────────────

/// Request for registering an MCP server as an OAuth provider via discovery + DCR.
#[derive(Deserialize)]
pub struct RegisterMcpRequest {
    pub mcp_url: String,
    /// Optional display name override; derived from URL if omitted.
    pub display_name: Option<String>,
}

/// Register an MCP server as an OAuth provider using the MCP OAuth 2.1 flow:
/// probe → PRM → AS metadata → DCR → create provider.
pub async fn register_mcp(
    db: &PgPool,
    vault: &casper_base::Vault,
    http_client: &reqwest::Client,
    redirect_uri: &str,
    req: &RegisterMcpRequest,
) -> Result<OAuthProviderResponse, CasperError> {
    let mcp_url = req.mcp_url.trim_end_matches('/');

    // Step 1-3: Discover PRM + AS metadata
    let auth = casper_agent::mcp::oauth::discover(http_client, mcp_url)
        .await
        .map_err(|e| CasperError::BadGateway(format!("MCP OAuth discovery failed: {e}")))?
        .ok_or_else(|| {
            CasperError::BadRequest(
                "MCP server did not return 401 — it may not require OAuth".into(),
            )
        })?;

    // Step 4: Dynamic Client Registration
    let reg_endpoint = auth
        .as_metadata
        .registration_endpoint
        .as_deref()
        .ok_or_else(|| {
            CasperError::BadGateway(
                "Authorization server does not support Dynamic Client Registration".into(),
            )
        })?;

    let dcr = casper_agent::mcp::oauth::register_client(
        http_client,
        reg_endpoint,
        redirect_uri,
        "Casper",
    )
    .await
    .map_err(|e| CasperError::BadGateway(format!("DCR failed: {e}")))?;

    // Derive provider name from MCP URL
    let name = casper_agent::mcp::oauth::mcp_provider_key(mcp_url);
    let display_name = req.display_name.clone().unwrap_or_else(|| {
        url::Url::parse(mcp_url)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| name.clone())
    });

    let scopes = if !auth.prm.scopes_supported.is_empty() {
        auth.prm.scopes_supported.join(" ")
    } else if !auth.as_metadata.scopes_supported.is_empty() {
        auth.as_metadata.scopes_supported.join(" ")
    } else {
        String::new()
    };

    // Encrypt client_secret (may be empty for public clients)
    let client_secret = dcr.client_secret.as_deref().unwrap_or("");
    let client_secret_enc =
        vault.encrypt_value(casper_base::TenantId(Uuid::nil()), client_secret.as_bytes())?;

    let id = Uuid::now_v7();

    // Upsert: if the same MCP server was registered before, update it
    sqlx::query(
        "INSERT INTO oauth_providers (id, name, display_name, authorization_url, token_url,
         revocation_url, client_id, client_secret_enc, default_scopes, is_active)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, true)
         ON CONFLICT (name) DO UPDATE SET
           authorization_url = EXCLUDED.authorization_url,
           token_url = EXCLUDED.token_url,
           revocation_url = EXCLUDED.revocation_url,
           client_id = EXCLUDED.client_id,
           client_secret_enc = EXCLUDED.client_secret_enc,
           default_scopes = EXCLUDED.default_scopes,
           is_active = true",
    )
    .bind(id)
    .bind(&name)
    .bind(&display_name)
    .bind(&auth.as_metadata.authorization_endpoint)
    .bind(&auth.as_metadata.token_endpoint)
    .bind(&auth.as_metadata.revocation_endpoint)
    .bind(&dcr.client_id)
    .bind(&client_secret_enc)
    .bind(&scopes)
    .execute(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    get_by_name(db, &name).await
}

// ── Internal helpers for token exchange ─────────────────────────

/// Load a provider's full config including decrypted client_secret (for token exchange).
pub async fn load_provider_with_secret(
    db: &PgPool,
    vault: &casper_base::Vault,
    name: &str,
) -> Result<ProviderConfig, CasperError> {
    let row: (String, String, Option<String>, String, String, String) = sqlx::query_as(
        "SELECT authorization_url, token_url, revocation_url, client_id, client_secret_enc, default_scopes
         FROM oauth_providers WHERE name = $1 AND is_active = true",
    )
    .bind(name)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    .ok_or_else(|| CasperError::NotFound(format!("provider '{name}' not found or inactive")))?;

    let client_secret = vault.decrypt_value(casper_base::TenantId(Uuid::nil()), &row.4)?;

    Ok(ProviderConfig {
        authorization_url: row.0,
        token_url: row.1,
        revocation_url: row.2,
        client_id: row.3,
        client_secret: client_secret
            .expose_str()
            .map_err(|e| CasperError::Internal(format!("invalid client_secret encoding: {e}")))?
            .to_string(),
        default_scopes: row.5,
    })
}

/// Decrypted provider configuration used for token exchange.
pub struct ProviderConfig {
    pub authorization_url: String,
    pub token_url: String,
    pub revocation_url: Option<String>,
    pub client_id: String,
    pub client_secret: String,
    pub default_scopes: String,
}
