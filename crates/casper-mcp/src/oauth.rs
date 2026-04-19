//! MCP OAuth 2.1 discovery and token management.
//!
//! Implements the full MCP OAuth dance:
//! 1. Probe server → 401 with `WWW-Authenticate: Bearer resource_metadata="..."`
//! 2. Fetch Protected Resource Metadata (RFC 9728)
//! 3. Fetch Authorization Server Metadata (RFC 8414)
//! 4. Dynamic Client Registration (RFC 7591), cached per tenant
//! 5. PKCE + authorization URL with resource indicator (RFC 8707)
//! 6. Token exchange with resource binding
//! 7. Token refresh with rotation

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::types::McpError;

// ── Metadata types ──────────────────────────────────────────────

/// Protected Resource Metadata (RFC 9728).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub bearer_methods_supported: Vec<String>,
}

/// Authorization Server Metadata (RFC 8414).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub grant_types_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
}

/// Dynamic Client Registration response (RFC 7591).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DcrResponse {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub client_id_issued_at: Option<i64>,
    pub client_secret_expires_at: Option<i64>,
}

/// Result of the full metadata discovery (steps 1-3).
#[derive(Debug, Clone)]
pub struct DiscoveredAuth {
    pub prm: ProtectedResourceMetadata,
    pub as_metadata: AuthServerMetadata,
}

/// Token response from the authorization server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: Option<String>,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// PKCE code verifier + challenge pair.
pub struct PkcePair {
    pub verifier: String,
    pub challenge: String,
}

// ── Step 1: Probe ───────────────────────────────────────────────

/// Probe an MCP server with an unauthenticated request.
/// Returns the `resource_metadata` URL from the 401 `WWW-Authenticate` header,
/// or `None` if the server doesn't require OAuth.
pub async fn probe(
    http: &reqwest::Client,
    mcp_url: &str,
) -> Result<Option<String>, McpError> {
    let resp = http
        .post(mcp_url)
        .header("content-type", "application/json")
        .json(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "casper", "version": "0.1.0"}
        }}))
        .send()
        .await
        .map_err(|e| McpError::Http(e))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        if let Some(www_auth) = resp.headers().get("www-authenticate") {
            if let Ok(header) = www_auth.to_str() {
                return Ok(parse_resource_metadata_url(header));
            }
        }
        return Err(McpError::InvalidResponse("401 without WWW-Authenticate header".into()));
    }

    // Server didn't return 401 — no OAuth needed
    Ok(None)
}

// ── Step 2: Protected Resource Metadata ─────────────────────────

/// Fetch Protected Resource Metadata (RFC 9728) and validate the origin.
pub async fn fetch_prm(
    http: &reqwest::Client,
    resource_metadata_url: &str,
    expected_origin: &str,
) -> Result<ProtectedResourceMetadata, McpError> {
    let resp = http.get(resource_metadata_url).send().await
        .map_err(|e| McpError::InvalidResponse(format!("PRM fetch failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(McpError::InvalidResponse(format!(
            "PRM returned {}", resp.status()
        )));
    }

    let prm: ProtectedResourceMetadata = resp.json().await
        .map_err(|e| McpError::InvalidResponse(format!("invalid PRM JSON: {e}")))?;

    // Anti-phishing: resource must match the origin we're connecting to
    let resource_origin = url::Url::parse(&prm.resource)
        .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
        .unwrap_or_default();
    if !expected_origin.starts_with(&resource_origin) {
        return Err(McpError::InvalidResponse(format!(
            "PRM resource origin mismatch: expected {expected_origin}, got {}",
            prm.resource
        )));
    }

    if prm.authorization_servers.is_empty() {
        return Err(McpError::InvalidResponse(
            "PRM has no authorization_servers".into()
        ));
    }

    Ok(prm)
}

// ── Step 3: AS Metadata ─────────────────────────────────────────

/// Fetch Authorization Server Metadata (RFC 8414).
pub async fn fetch_as_metadata(
    http: &reqwest::Client,
    as_issuer: &str,
) -> Result<AuthServerMetadata, McpError> {
    let base = as_issuer.trim_end_matches('/');
    let url = format!("{base}/.well-known/oauth-authorization-server");

    let resp = http.get(&url).send().await
        .map_err(|e| McpError::InvalidResponse(format!("AS metadata fetch failed: {e}")))?;

    if !resp.status().is_success() {
        // Fall back to OpenID Connect discovery
        let oidc_url = format!("{base}/.well-known/openid-configuration");
        let resp2 = http.get(&oidc_url).send().await
            .map_err(|e| McpError::InvalidResponse(format!("AS metadata fetch failed: {e}")))?;

        if !resp2.status().is_success() {
            return Err(McpError::InvalidResponse(format!(
                "AS metadata not found at {url} or {oidc_url}"
            )));
        }

        return resp2.json().await
            .map_err(|e| McpError::InvalidResponse(format!("invalid AS metadata: {e}")));
    }

    let meta: AuthServerMetadata = resp.json().await
        .map_err(|e| McpError::InvalidResponse(format!("invalid AS metadata: {e}")))?;

    // OAuth 2.1 requires PKCE with S256
    if !meta.code_challenge_methods_supported.is_empty()
        && !meta.code_challenge_methods_supported.contains(&"S256".to_string())
    {
        return Err(McpError::InvalidResponse(
            "AS does not support S256 PKCE — required by OAuth 2.1".into()
        ));
    }

    Ok(meta)
}

/// Full discovery: probe → PRM → AS metadata.
pub async fn discover(
    http: &reqwest::Client,
    mcp_url: &str,
) -> Result<Option<DiscoveredAuth>, McpError> {
    let rm_url = match probe(http, mcp_url).await? {
        Some(url) => url,
        None => return Ok(None),
    };

    let origin = url::Url::parse(mcp_url)
        .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
        .unwrap_or_default();

    let prm = fetch_prm(http, &rm_url, &origin).await?;
    let as_issuer = &prm.authorization_servers[0];
    let as_metadata = fetch_as_metadata(http, as_issuer).await?;

    Ok(Some(DiscoveredAuth { prm, as_metadata }))
}

// ── Step 4: Dynamic Client Registration ─────────────────────────

/// Register a client via DCR (RFC 7591).
pub async fn register_client(
    http: &reqwest::Client,
    registration_endpoint: &str,
    redirect_uri: &str,
    client_name: &str,
) -> Result<DcrResponse, McpError> {
    let resp = http
        .post(registration_endpoint)
        .json(&json!({
            "client_name": client_name,
            "redirect_uris": [redirect_uri],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await
        .map_err(|e| McpError::InvalidResponse(format!("DCR request failed: {e}")))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(McpError::InvalidResponse(format!("DCR failed: {body}")));
    }

    resp.json().await
        .map_err(|e| McpError::InvalidResponse(format!("invalid DCR response: {e}")))
}

// ── Step 5: PKCE + Authorization URL ────────────────────────────

/// Generate a PKCE pair (S256).
pub fn pkce_pair() -> PkcePair {
    use sha2::Digest;
    use base64::Engine;

    let verifier_bytes: [u8; 32] = rand::random();
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(sha2::Sha256::digest(verifier.as_bytes()));

    PkcePair { verifier, challenge }
}

/// Generate a random state string for CSRF protection.
pub fn random_state() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Build the authorization URL (step 6).
pub fn build_authorization_url(
    auth: &DiscoveredAuth,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce_challenge: &str,
    scopes: &str,
) -> String {
    let params = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scopes)
        .append_pair("code_challenge", pkce_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("resource", &auth.prm.resource) // RFC 8707
        .finish();

    format!("{}?{}", auth.as_metadata.authorization_endpoint, params)
}

// ── Step 7: Token Exchange ──────────────────────────────────────

/// Exchange an authorization code for tokens (step 8).
pub async fn exchange_code(
    http: &reqwest::Client,
    token_endpoint: &str,
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    pkce_verifier: &str,
    resource: &str,
) -> Result<TokenResponse, McpError> {
    let resp = http
        .post(token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", pkce_verifier),
            ("resource", resource),
        ])
        .send()
        .await
        .map_err(|e| McpError::InvalidResponse(format!("token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(McpError::InvalidResponse(format!("token exchange error: {body}")));
    }

    resp.json().await
        .map_err(|e| McpError::InvalidResponse(format!("invalid token response: {e}")))
}

// ── Step 9: Token Refresh ───────────────────────────────────────

/// Refresh an access token. Returns new tokens (with rotated refresh token).
pub async fn refresh_token(
    http: &reqwest::Client,
    token_endpoint: &str,
    refresh_token_value: &str,
    client_id: &str,
    resource: &str,
) -> Result<TokenResponse, McpError> {
    let resp = http
        .post(token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token_value),
            ("client_id", client_id),
            ("resource", resource),
        ])
        .send()
        .await
        .map_err(|e| McpError::InvalidResponse(format!("token refresh failed: {e}")))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(McpError::InvalidResponse(format!("token refresh error: {body}")));
    }

    resp.json().await
        .map_err(|e| McpError::InvalidResponse(format!("invalid refresh response: {e}")))
}

// ── Helpers ─────────────────────────────────────────────────────

/// Parse `resource_metadata` URL from a `WWW-Authenticate: Bearer ...` header.
pub fn parse_resource_metadata_url(header: &str) -> Option<String> {
    // WWW-Authenticate: Bearer resource_metadata="https://..."
    let prefix = "resource_metadata=\"";
    let start = header.find(prefix)? + prefix.len();
    let end = header[start..].find('"')? + start;
    Some(header[start..end].to_string())
}

/// Derive a stable provider key from an MCP server URL.
/// E.g., "https://asana-mcp.example.com/mcp" → "mcp:asana-mcp.example.com"
pub fn mcp_provider_key(mcp_url: &str) -> String {
    url::Url::parse(mcp_url)
        .map(|u| format!("mcp:{}", u.host_str().unwrap_or("unknown")))
        .unwrap_or_else(|_| format!("mcp:{mcp_url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_www_authenticate_header() {
        let header = r#"Bearer resource_metadata="https://example.com/.well-known/oauth-protected-resource""#;
        assert_eq!(
            parse_resource_metadata_url(header),
            Some("https://example.com/.well-known/oauth-protected-resource".to_string())
        );
    }

    #[test]
    fn parse_www_authenticate_no_resource() {
        assert_eq!(parse_resource_metadata_url("Bearer"), None);
        assert_eq!(parse_resource_metadata_url("Basic realm=\"test\""), None);
    }

    #[test]
    fn mcp_provider_key_from_url() {
        assert_eq!(mcp_provider_key("https://asana-mcp.example.com/mcp"), "mcp:asana-mcp.example.com");
        assert_eq!(mcp_provider_key("https://mcp.ventoo.ai/apps/mcp"), "mcp:mcp.ventoo.ai");
    }

    #[test]
    fn pkce_pair_generates_valid_pair() {
        let pair = pkce_pair();
        assert!(!pair.verifier.is_empty());
        assert!(!pair.challenge.is_empty());
        assert_ne!(pair.verifier, pair.challenge);
    }
}
