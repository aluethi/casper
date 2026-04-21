use axum::{
    Json, Router,
    extract::{Query, State},
    routing::post,
};
use casper_base::CasperError;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::feedback_service::{
    self, CreateFeedbackRequest, FeedbackResponse, ListFeedbackParams,
};

// ── Handlers ────────────────────────────────────────────────────

/// POST /api/v1/feedback -- Submit feedback on a message.
async fn create_feedback(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateFeedbackRequest>,
) -> Result<Json<FeedbackResponse>, CasperError> {
    guard.require("feedback:write")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = feedback_service::create(&state.db, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(result))
}

/// GET /api/v1/feedback -- List feedback with filters.
async fn list_feedback(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ListFeedbackParams>,
) -> Result<Json<Vec<FeedbackResponse>>, CasperError> {
    // Require feedback:write or admin scope
    let has_feedback = guard.require("feedback:write").is_ok();
    let has_admin = guard.require("admin:*").is_ok();
    if !has_feedback && !has_admin {
        return Err(CasperError::Forbidden(
            "requires feedback:write or admin:* scope".into(),
        ));
    }

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = feedback_service::list(&state.db, tenant_id, &params).await?;
    Ok(Json(result))
}

// ── Router ──────────────────────────────────────────────────────

pub fn feedback_router() -> Router<AppState> {
    Router::new().route("/api/v1/feedback", post(create_feedback).get(list_feedback))
}
