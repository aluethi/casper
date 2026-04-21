use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use casper_base::CasperError;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::quota_service::{
    self, AllocateQuotaRequest, QuotaResponse, UpdateQuotaRequest,
};

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/tenants/:id/quotas -- Allocate quota.
async fn allocate_quota(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<AllocateQuotaRequest>,
) -> Result<Json<QuotaResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result =
        quota_service::allocate(&state.db_owner, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(result))
}

/// GET /api/v1/tenants/:id/quotas -- List quotas for tenant.
async fn list_quotas(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<Vec<QuotaResponse>>, CasperError> {
    guard.require("platform:admin")?;
    let result = quota_service::list(&state.db_owner, tenant_id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/tenants/:id/quotas/:model_id -- Update quota.
async fn update_quota(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((tenant_id, model_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateQuotaRequest>,
) -> Result<Json<QuotaResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = quota_service::update(
        &state.db_owner,
        tenant_id,
        model_id,
        &body,
        &guard.0.actor(),
    )
    .await?;
    Ok(Json(result))
}

/// DELETE /api/v1/tenants/:id/quotas/:model_id -- Remove quota.
async fn delete_quota(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((tenant_id, model_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("platform:admin")?;
    let result = quota_service::delete(&state.db_owner, tenant_id, model_id).await?;
    Ok(Json(result))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn quota_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/tenants/{id}/quotas",
            post(allocate_quota).get(list_quotas),
        )
        .route(
            "/api/v1/tenants/{id}/quotas/{model_id}",
            axum::routing::patch(update_quota).delete(delete_quota),
        )
}
