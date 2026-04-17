use axum::{Json, Router, extract::State, routing::{get, post}};
use casper_base::CasperError;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::inference_service::{
    self, ChatCompletionRequest, ChatCompletionResponse, ModelsListResponse,
};

// ── Handlers ──────────────────────────────────────────────────────

/// POST /v1/chat/completions -- Proxy an LLM request through a deployment.
async fn chat_completions(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, CasperError> {
    let slug = &body.model;
    let scope_str = format!("inference:{slug}:call");
    guard.require(&scope_str)?;

    let result = inference_service::chat_completions(
        &state,
        guard.0.tenant_id.0,
        &guard.0.scopes,
        guard.0.correlation_id.0,
        &body,
    )
    .await?;

    Ok(Json(result))
}

/// GET /v1/models -- List models (deployments) accessible to the caller.
async fn list_models(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<ModelsListResponse>, CasperError> {
    let result = inference_service::list_models(
        &state.db_owner,
        guard.0.tenant_id.0,
        &guard.0.scopes,
    )
    .await?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────────────────────────

pub fn inference_router() -> Router<AppState> {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
}
