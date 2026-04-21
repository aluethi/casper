use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use casper_base::CasperError;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::snippet_service::{
    self, CreateSnippetRequest, SnippetResponse, UpdateSnippetRequest,
};

// ── Handlers ──────────────────────────────────────────────────────

/// POST /api/v1/snippets -- Create snippet.
async fn create_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateSnippetRequest>,
) -> Result<Json<SnippetResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = snippet_service::create(&state.db, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(result))
}

/// GET /api/v1/snippets -- List snippets for tenant.
async fn list_snippets(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<SnippetResponse>>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = snippet_service::list(&state.db, tenant_id).await?;
    Ok(Json(result))
}

/// GET /api/v1/snippets/:id -- Get single snippet.
async fn get_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<SnippetResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = snippet_service::get(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/snippets/:id -- Update snippet (recompute token_estimate if content changes).
async fn update_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateSnippetRequest>,
) -> Result<Json<SnippetResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = snippet_service::update(&state.db, tenant_id, id, &body).await?;
    Ok(Json(result))
}

/// DELETE /api/v1/snippets/:id -- Delete snippet.
async fn delete_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = snippet_service::delete(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────────────────────────

pub fn snippet_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/snippets", post(create_snippet).get(list_snippets))
        .route(
            "/api/v1/snippets/{id}",
            get(get_snippet)
                .patch(update_snippet)
                .delete(delete_snippet),
        )
}
