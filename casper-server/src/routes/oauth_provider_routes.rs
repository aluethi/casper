//! Routes for OAuth provider management (platform:admin scope).

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::oauth_provider_service::{
    self, CreateProviderRequest, OAuthProviderResponse, RegisterMcpRequest, UpdateProviderRequest,
};

async fn create_provider(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateProviderRequest>,
) -> Result<Json<OAuthProviderResponse>, CasperError> {
    guard.require("platform:admin")?;
    let provider = oauth_provider_service::create(&state.db_owner, &state.vault, &body).await?;
    Ok(Json(provider))
}

async fn list_providers(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<OAuthProviderResponse>>, CasperError> {
    guard.require("platform:admin")?;
    let providers = oauth_provider_service::list(&state.db_owner).await?;
    Ok(Json(providers))
}

async fn get_provider(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<Json<OAuthProviderResponse>, CasperError> {
    guard.require("platform:admin")?;
    let provider = oauth_provider_service::get_by_name(&state.db_owner, &name).await?;
    Ok(Json(provider))
}

async fn update_provider(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
    Json(body): Json<UpdateProviderRequest>,
) -> Result<Json<OAuthProviderResponse>, CasperError> {
    guard.require("platform:admin")?;
    let provider =
        oauth_provider_service::update(&state.db_owner, &state.vault, &name, &body).await?;
    Ok(Json(provider))
}

async fn delete_provider(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(name): Path<String>,
) -> Result<(), CasperError> {
    guard.require("platform:admin")?;
    oauth_provider_service::delete(&state.db_owner, &name).await
}

/// GET /api/v1/oauth-providers/discover?url=... -- Auto-discover OAuth config from .well-known.
#[derive(Deserialize)]
struct DiscoverQuery {
    url: String,
}

#[derive(Serialize)]
struct DiscoverResponse {
    authorization_url: Option<String>,
    token_url: Option<String>,
    revocation_url: Option<String>,
    scopes_supported: Vec<String>,
    issuer: Option<String>,
}

async fn discover_provider(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<DiscoverQuery>,
) -> Result<Json<DiscoverResponse>, CasperError> {
    guard.require("platform:admin")?;

    let base = params.url.trim_end_matches('/');

    // Try well-known URLs in order
    let urls_to_try = if base.contains("/.well-known/") {
        vec![base.to_string()]
    } else {
        vec![
            format!("{base}/.well-known/openid-configuration"),
            format!("{base}/.well-known/oauth-authorization-server"),
        ]
    };

    let mut last_error = String::new();
    for url in &urls_to_try {
        match state.http_client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let doc: serde_json::Value = resp.json().await.map_err(|e| {
                    CasperError::BadGateway(format!("invalid JSON from {url}: {e}"))
                })?;

                return Ok(Json(DiscoverResponse {
                    authorization_url: doc["authorization_endpoint"].as_str().map(String::from),
                    token_url: doc["token_endpoint"].as_str().map(String::from),
                    revocation_url: doc["revocation_endpoint"]
                        .as_str()
                        .map(String::from)
                        .or_else(|| doc["end_session_endpoint"].as_str().map(String::from)),
                    scopes_supported: doc["scopes_supported"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    issuer: doc["issuer"].as_str().map(String::from),
                }));
            }
            Ok(resp) => {
                last_error = format!("{url} returned {}", resp.status());
            }
            Err(e) => {
                last_error = format!("{url}: {e}");
            }
        }
    }

    Err(CasperError::BadGateway(format!(
        "could not discover OAuth config: {last_error}"
    )))
}

/// POST /api/v1/oauth-providers/register-mcp — auto-discover + DCR for an MCP server.
async fn register_mcp(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<RegisterMcpRequest>,
) -> Result<Json<OAuthProviderResponse>, CasperError> {
    guard.require("platform:admin")?;

    let redirect_base = state
        .config
        .listen
        .public_url
        .clone()
        .unwrap_or_else(|| "http://localhost:3000".to_string());
    let redirect_uri = format!("{redirect_base}/api/v1/connections/callback");

    let provider = oauth_provider_service::register_mcp(
        &state.db_owner,
        &state.vault,
        &state.http_client,
        &redirect_uri,
        &body,
    )
    .await?;

    Ok(Json(provider))
}

pub fn oauth_provider_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/oauth-providers",
            post(create_provider).get(list_providers),
        )
        .route("/api/v1/oauth-providers/discover", get(discover_provider))
        .route("/api/v1/oauth-providers/register-mcp", post(register_mcp))
        .route(
            "/api/v1/oauth-providers/{name}",
            get(get_provider)
                .patch(update_provider)
                .delete(delete_provider),
        )
}
