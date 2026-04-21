//! Service layer for user connections (per-user OAuth tokens).

use casper_base::TenantDb;
use casper_base::{CasperError, TenantId};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use super::oauth_provider_service;
use crate::helpers::to_rfc3339;

fn serialize_dt<S: serde::Serializer>(dt: &OffsetDateTime, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&to_rfc3339(*dt))
}
fn serialize_dt_opt<S: serde::Serializer>(
    dt: &Option<OffsetDateTime>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match dt {
        Some(d) => s.serialize_str(&to_rfc3339(*d)),
        None => s.serialize_none(),
    }
}

// ── Domain types ────────────────────────────────────────────────

#[derive(sqlx::FromRow, Serialize)]
pub struct ConnectionResponse {
    pub id: Uuid,
    pub provider: String,
    pub granted_scopes: String,
    pub external_email: Option<String>,
    #[serde(serialize_with = "serialize_dt_opt")]
    pub token_expires_at: Option<OffsetDateTime>,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
    #[serde(serialize_with = "serialize_dt")]
    pub updated_at: OffsetDateTime,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct AdminConnectionResponse {
    pub id: Uuid,
    pub user_subject: String,
    pub provider: String,
    pub granted_scopes: String,
    pub external_email: Option<String>,
    #[serde(serialize_with = "serialize_dt_opt")]
    pub token_expires_at: Option<OffsetDateTime>,
    #[serde(serialize_with = "serialize_dt")]
    pub created_at: OffsetDateTime,
}

/// Available provider for the "connect" UI.
#[derive(Serialize)]
pub struct AvailableProvider {
    pub name: String,
    pub display_name: String,
    pub icon_url: Option<String>,
    pub connected: bool,
}

/// OAuth state parameter (encrypted, passed through the redirect).
#[derive(Serialize, Deserialize)]
pub struct OAuthState {
    pub tenant_id: Uuid,
    pub user_subject: String,
    pub provider: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

/// Token response from the OAuth provider.
#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
}

// ── Service functions ───────────────────────────────────────────

/// List the current user's connections.
pub async fn list_my_connections(
    db: &PgPool,
    tenant_id: TenantId,
    user_subject: &str,
) -> Result<Vec<ConnectionResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<ConnectionResponse> = sqlx::query_as(
        "SELECT id, provider, granted_scopes, external_email, token_expires_at, created_at, updated_at
         FROM user_connections
         WHERE tenant_id = $1 AND user_subject = $2
         ORDER BY provider",
    )
    .bind(tenant_id.0)
    .bind(user_subject)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    Ok(rows)
}

/// List available providers with connection status for the current user.
pub async fn list_available(
    db: &PgPool,
    tenant_id: TenantId,
    user_subject: &str,
) -> Result<Vec<AvailableProvider>, CasperError> {
    let providers = oauth_provider_service::list(db).await?;
    let connections = list_my_connections(db, tenant_id, user_subject).await?;

    let connected_providers: std::collections::HashSet<String> =
        connections.iter().map(|c| c.provider.clone()).collect();

    Ok(providers
        .into_iter()
        .filter(|p| p.is_active)
        .map(|p| AvailableProvider {
            connected: connected_providers.contains(&p.name),
            name: p.name,
            display_name: p.display_name,
            icon_url: p.icon_url,
        })
        .collect())
}

/// Start the OAuth flow: generate PKCE, build authorization URL, return redirect.
pub async fn start_oauth_flow(
    db: &PgPool,
    vault: &casper_base::Vault,
    http_client: &reqwest::Client,
    tenant_id: TenantId,
    user_subject: &str,
    provider_name: &str,
    redirect_base: &str,
) -> Result<String, CasperError> {
    let provider =
        oauth_provider_service::load_provider_with_secret(db, vault, provider_name).await?;

    // Generate PKCE verifier and challenge
    let pkce_verifier = generate_random_string(64);
    let pkce_challenge = base64_url_encode(&sha256(pkce_verifier.as_bytes()));

    // Build state parameter (encrypted)
    let state = OAuthState {
        tenant_id: tenant_id.0,
        user_subject: user_subject.to_string(),
        provider: provider_name.to_string(),
        pkce_verifier: pkce_verifier.clone(),
        nonce: generate_random_string(32),
    };
    let state_json = serde_json::to_string(&state)
        .map_err(|e| CasperError::Internal(format!("state serialization error: {e}")))?;
    let encrypted_state = vault.encrypt_value(tenant_id, state_json.as_bytes())?;
    let state_param = base64_url_encode(encrypted_state.as_bytes());

    let callback_url = format!("{redirect_base}/api/v1/connections/callback");

    let params = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", &provider.client_id)
        .append_pair("redirect_uri", &callback_url)
        .append_pair("response_type", "code")
        .append_pair("scope", &provider.default_scopes)
        .append_pair("state", &state_param)
        .append_pair("code_challenge", &pkce_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .finish();

    let auth_url = format!("{}?{}", provider.authorization_url, params);

    Ok(auth_url)
}

/// Handle the OAuth callback: exchange code for tokens, store connection.
/// Provider name is derived from the encrypted state parameter, so no path param needed.
pub async fn handle_callback(
    db: &PgPool,
    vault: &casper_base::Vault,
    http_client: &reqwest::Client,
    code: &str,
    state_param: &str,
    redirect_base: &str,
) -> Result<(TenantId, String), CasperError> {
    // Decrypt and validate state
    let state_bytes = base64_url_decode(state_param)
        .map_err(|_| CasperError::BadRequest("invalid state parameter".into()))?;
    let state_str = String::from_utf8(state_bytes)
        .map_err(|_| CasperError::BadRequest("invalid state encoding".into()))?;

    let decrypted = vault
        .decrypt_value(casper_base::TenantId(Uuid::nil()), &state_str)
        .map_err(|_| CasperError::BadRequest("invalid or expired state parameter".into()))?;
    let state: OAuthState = serde_json::from_slice(decrypted.expose())
        .map_err(|_| CasperError::BadRequest("corrupted state parameter".into()))?;

    let provider_name = &state.provider;
    let tenant_id = TenantId(state.tenant_id);

    // Load provider config
    let provider =
        oauth_provider_service::load_provider_with_secret(db, vault, provider_name).await?;

    let callback_url = format!("{redirect_base}/api/v1/connections/callback");

    // Exchange code for tokens
    let token_resp = http_client
        .post(&provider.token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &callback_url),
            ("client_id", &provider.client_id),
            ("client_secret", &provider.client_secret),
            ("code_verifier", &state.pkce_verifier),
        ])
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("token exchange failed: {e}")))?;

    if !token_resp.status().is_success() {
        let body = token_resp.text().await.unwrap_or_default();
        return Err(CasperError::BadGateway(format!(
            "token exchange error: {body}"
        )));
    }

    let tokens: TokenResponse = token_resp
        .json()
        .await
        .map_err(|e| CasperError::BadGateway(format!("invalid token response: {e}")))?;

    // Encrypt tokens
    let access_token_enc = vault.encrypt_value(tenant_id, tokens.access_token.as_bytes())?;
    let refresh_token_enc = tokens
        .refresh_token
        .as_ref()
        .map(|rt| vault.encrypt_value(tenant_id, rt.as_bytes()))
        .transpose()?;

    let expires_at = tokens
        .expires_in
        .map(|secs| OffsetDateTime::now_utc() + time::Duration::seconds(secs));

    let granted_scopes = tokens
        .scope
        .unwrap_or_else(|| provider.default_scopes.clone());

    // Upsert connection
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO user_connections (id, tenant_id, user_subject, provider,
         access_token_enc, refresh_token_enc, token_expires_at, granted_scopes, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
         ON CONFLICT (tenant_id, user_subject, provider)
         DO UPDATE SET access_token_enc = EXCLUDED.access_token_enc,
                       refresh_token_enc = EXCLUDED.refresh_token_enc,
                       token_expires_at = EXCLUDED.token_expires_at,
                       granted_scopes = EXCLUDED.granted_scopes,
                       updated_at = now()",
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&state.user_subject)
    .bind(provider_name)
    .bind(&access_token_enc)
    .bind(&refresh_token_enc)
    .bind(expires_at)
    .bind(&granted_scopes)
    .execute(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error storing connection: {e}")))?;

    Ok((tenant_id, provider_name.to_string()))
}

/// Disconnect: delete a user's connection to a provider.
pub async fn disconnect(
    db: &PgPool,
    vault: &casper_base::Vault,
    http_client: &reqwest::Client,
    tenant_id: TenantId,
    user_subject: &str,
    provider_name: &str,
) -> Result<(), CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Load the connection to attempt token revocation
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT access_token_enc, refresh_token_enc FROM user_connections
         WHERE tenant_id = $1 AND user_subject = $2 AND provider = $3",
    )
    .bind(tenant_id.0)
    .bind(user_subject)
    .bind(provider_name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if row.is_none() {
        tx.commit().await.ok();
        return Err(CasperError::NotFound(format!(
            "connection to '{provider_name}'"
        )));
    }

    // Best-effort token revocation
    if let Ok(provider) =
        oauth_provider_service::load_provider_with_secret(db, vault, provider_name).await
    {
        if let Some(ref revocation_url) = provider.revocation_url {
            if let Some((ref access_enc, _)) = row {
                if let Ok(token) = vault.decrypt_value(tenant_id, access_enc) {
                    let _ = http_client
                        .post(revocation_url)
                        .form(&[("token", token.expose_str().unwrap_or(""))])
                        .send()
                        .await;
                }
            }
        }
    }

    // Delete the connection
    sqlx::query(
        "DELETE FROM user_connections WHERE tenant_id = $1 AND user_subject = $2 AND provider = $3",
    )
    .bind(tenant_id.0)
    .bind(user_subject)
    .bind(provider_name)
    .execute(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    Ok(())
}

/// Admin: list all connections for the tenant.
pub async fn list_all(
    db: &PgPool,
    tenant_id: TenantId,
) -> Result<Vec<AdminConnectionResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<AdminConnectionResponse> = sqlx::query_as(
        "SELECT id, user_subject, provider, granted_scopes, external_email, token_expires_at, created_at
         FROM user_connections
         WHERE tenant_id = $1
         ORDER BY user_subject, provider",
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;
    Ok(rows)
}

// ── Token resolution (called by agent runtime) ──────────────────

/// Resolve a user's access token for a provider, auto-refreshing if expired.
pub async fn resolve_user_token(
    db: &PgPool,
    vault: &casper_base::Vault,
    http_client: &reqwest::Client,
    tenant_id: TenantId,
    user_subject: &str,
    provider_name: &str,
) -> Result<String, CasperError> {
    let row: Option<(String, Option<String>, Option<OffsetDateTime>)> = sqlx::query_as(
        "SELECT access_token_enc, refresh_token_enc, token_expires_at
         FROM user_connections
         WHERE tenant_id = $1 AND user_subject = $2 AND provider = $3",
    )
    .bind(tenant_id.0)
    .bind(user_subject)
    .bind(provider_name)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (access_enc, refresh_enc, expires_at) = row.ok_or_else(|| {
        CasperError::Forbidden(format!(
            "User '{user_subject}' has not connected '{provider_name}'. \
             They need to connect it in Settings > Connections."
        ))
    })?;

    // Check if token is expired (with 5-minute buffer)
    let needs_refresh = expires_at
        .map(|ea| ea < OffsetDateTime::now_utc() + time::Duration::minutes(5))
        .unwrap_or(false);

    if needs_refresh {
        let refresh_token_enc = refresh_enc.ok_or_else(|| {
            CasperError::Forbidden(format!(
                "User '{user_subject}'s '{provider_name}' token has expired and cannot be refreshed. \
                 They need to reconnect in Settings > Connections."
            ))
        })?;

        let refresh_token = vault.decrypt_value(tenant_id, &refresh_token_enc)?;
        let refresh_str = refresh_token
            .expose_str()
            .map_err(|e| CasperError::Internal(format!("invalid refresh token: {e}")))?;

        let provider =
            oauth_provider_service::load_provider_with_secret(db, vault, provider_name).await?;

        let token_resp = http_client
            .post(&provider.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_str),
                ("client_id", &provider.client_id),
                ("client_secret", &provider.client_secret),
            ])
            .send()
            .await
            .map_err(|e| CasperError::BadGateway(format!("token refresh failed: {e}")))?;

        if !token_resp.status().is_success() {
            let body = token_resp.text().await.unwrap_or_default();
            return Err(CasperError::Forbidden(format!(
                "Token refresh failed for '{user_subject}' on '{provider_name}': {body}. \
                 They may need to reconnect."
            )));
        }

        let tokens: TokenResponse = token_resp
            .json()
            .await
            .map_err(|e| CasperError::BadGateway(format!("invalid refresh response: {e}")))?;

        let new_access_enc = vault.encrypt_value(tenant_id, tokens.access_token.as_bytes())?;
        let new_refresh_enc = tokens
            .refresh_token
            .as_ref()
            .map(|rt| vault.encrypt_value(tenant_id, rt.as_bytes()))
            .transpose()?;
        let new_expires = tokens
            .expires_in
            .map(|s| OffsetDateTime::now_utc() + time::Duration::seconds(s));

        sqlx::query(
            "UPDATE user_connections SET access_token_enc = $1, refresh_token_enc = COALESCE($2, refresh_token_enc),
             token_expires_at = $3, updated_at = now()
             WHERE tenant_id = $4 AND user_subject = $5 AND provider = $6",
        )
        .bind(&new_access_enc)
        .bind(&new_refresh_enc)
        .bind(new_expires)
        .bind(tenant_id.0)
        .bind(user_subject)
        .bind(provider_name)
        .execute(db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error updating tokens: {e}")))?;

        return Ok(tokens.access_token);
    }

    let access_token = vault.decrypt_value(tenant_id, &access_enc)?;
    Ok(access_token
        .expose_str()
        .map_err(|e| CasperError::Internal(format!("invalid access token: {e}")))?
        .to_string())
}

// ── Helpers ─────────────────────────────────────────────────────

fn generate_random_string(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..62);
            let c = if idx < 10 {
                (b'0' + idx) as char
            } else if idx < 36 {
                (b'a' + idx - 10) as char
            } else {
                (b'A' + idx - 36) as char
            };
            c
        })
        .collect()
}

fn sha256(data: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn base64_url_decode(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(data)
}
