//! Routes for user connections (per-user OAuth tokens).

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::Redirect,
    routing::{get, post, delete},
};
use casper_base::CasperError;
use serde::Deserialize;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::connection_service::{
    self, AdminConnectionResponse, AvailableProvider, ConnectionResponse,
};

// ── User routes (any authenticated user) ────────────────────────

/// GET /api/v1/connections — list my connections.
async fn list_my(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<ConnectionResponse>>, CasperError> {
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let user = guard.0.actor();
    let conns = connection_service::list_my_connections(&state.db, tenant_id, &user).await?;
    Ok(Json(conns))
}

/// GET /api/v1/connections/available — list available providers with status.
async fn list_available(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<AvailableProvider>>, CasperError> {
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let user = guard.0.actor();
    let available = connection_service::list_available(&state.db_owner, tenant_id, &user).await?;
    Ok(Json(available))
}

/// POST /api/v1/connections/:provider/start — start OAuth flow.
async fn start_flow(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, CasperError> {
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let user = guard.0.actor();

    // Determine the base URL for the callback
    let redirect_base = state.config.listen.public_url.clone()
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    let auth_url = connection_service::start_oauth_flow(
        &state.db_owner, &state.vault, &state.http_client,
        tenant_id, &user, &provider, &redirect_base,
    ).await?;

    Ok(Json(serde_json::json!({ "redirect_url": auth_url })))
}

/// GET /api/v1/connections/callback — single OAuth callback for all providers.
#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<CallbackQuery>,
) -> Result<Redirect, CasperError> {
    let redirect_base = state.config.listen.public_url.clone()
        .unwrap_or_else(|| "http://localhost:3000".to_string());

    let (_tenant_id, provider_name) = connection_service::handle_callback(
        &state.db_owner, &state.vault, &state.http_client,
        &params.code, &params.state, &redirect_base,
    ).await?;

    // Redirect to portal connections page
    Ok(Redirect::to(&format!("/settings/connections?connected={provider_name}")))
}

/// DELETE /api/v1/connections/:provider — disconnect.
async fn disconnect(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(provider): Path<String>,
) -> Result<Json<serde_json::Value>, CasperError> {
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let user = guard.0.actor();

    connection_service::disconnect(
        &state.db, &state.vault, &state.http_client,
        tenant_id, &user, &provider,
    ).await?;

    Ok(Json(serde_json::json!({ "disconnected": provider })))
}

// ── Admin routes ────────────────────────────────────────────────

/// GET /api/v1/connections/all — list all connections for the tenant (admin).
async fn list_all(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<AdminConnectionResponse>>, CasperError> {
    guard.require("connections:admin")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let conns = connection_service::list_all(&state.db, tenant_id).await?;
    Ok(Json(conns))
}

/// DELETE /api/v1/connections/:user_subject/:provider — admin disconnect a user.
async fn admin_disconnect(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((user_subject, provider)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("connections:admin")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);

    connection_service::disconnect(
        &state.db, &state.vault, &state.http_client,
        tenant_id, &user_subject, &provider,
    ).await?;

    Ok(Json(serde_json::json!({ "disconnected": provider, "user": user_subject })))
}

// ── Router ──────────────────────────────────────────────────────

pub fn connection_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/connections", get(list_my))
        .route("/api/v1/connections/available", get(list_available))
        .route("/api/v1/connections/all", get(list_all))
        .route("/api/v1/connections/{provider}/start", post(start_flow))
        .route("/api/v1/connections/{provider}", delete(disconnect))
        .route("/api/v1/connections/{user_subject}/{provider}", delete(admin_disconnect))
}

/// Callback route — registered as a public route (no auth middleware, browser redirect).
/// Single URL for all providers; the provider is derived from the encrypted state param.
pub fn connection_callback_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/connections/callback", get(oauth_callback))
}
