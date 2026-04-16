use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use casper_base::CasperError;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::training_service::{self, ExportParams, ExportResponse};

// ── Handlers ────────────────────────────────────────────────────

/// GET /api/v1/training/export -- Export training data.
async fn export_training(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<ExportParams>,
) -> Result<Json<ExportResponse>, CasperError> {
    guard.require("training:export")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = training_service::export(&state.db, tenant_id, &params).await?;
    Ok(Json(result))
}

// ── Router ──────────────────────────────────────────────────────

pub fn training_router() -> Router<AppState> {
    Router::new().route("/api/v1/training/export", get(export_training))
}
