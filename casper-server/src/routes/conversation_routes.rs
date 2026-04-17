use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use casper_base::CasperError;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::conversation_service::{
    self, ConversationDetailResponse, ConversationResponse, ListConversationsParams,
    SetOutcomeRequest,
};

// ── Handlers ──────────────────────────────────────────────────────

/// GET /api/v1/conversations -- List conversations with filtering and pagination.
async fn list_conversations(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ListConversationsParams>,
) -> Result<Json<crate::pagination::PaginatedResponse<ConversationResponse>>, CasperError> {
    guard.require("agents:run")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = conversation_service::list(&state.db, tenant_id, &params).await?;
    Ok(Json(result))
}

/// GET /api/v1/conversations/:id -- Get conversation with messages.
async fn get_conversation(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ConversationDetailResponse>, CasperError> {
    guard.require("agents:run")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let detail = conversation_service::get_with_messages(&state.db, tenant_id, id).await?;
    Ok(Json(detail))
}

/// DELETE /api/v1/conversations/:id -- Delete conversation (CASCADE deletes messages).
async fn delete_conversation(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = conversation_service::delete(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/conversations/:id/outcome -- Set conversation outcome.
async fn set_outcome(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<SetOutcomeRequest>,
) -> Result<Json<ConversationResponse>, CasperError> {
    guard.require("agents:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = conversation_service::set_outcome(&state.db, tenant_id, id, &body, &guard.0.actor()).await?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────────────────────────

pub fn conversation_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/conversations", get(list_conversations))
        .route(
            "/api/v1/conversations/{id}",
            get(get_conversation).delete(delete_conversation),
        )
        .route("/api/v1/conversations/{id}/outcome", axum::routing::patch(set_outcome))
}
