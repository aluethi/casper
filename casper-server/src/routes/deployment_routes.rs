use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::pagination::PaginationParams;
use crate::services::deployment_service::{
    self, AvailableBackend, AvailableModel, CreateDeploymentRequest, DeploymentResponse,
    TestRouteResponse, UpdateDeploymentRequest,
};

#[derive(Deserialize)]
struct ModelIdQuery {
    model_id: Uuid,
}

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/deployments -- Create deployment.
async fn create_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateDeploymentRequest>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("config:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let dep = deployment_service::create(&state.db, tenant_id, &body).await?;
    Ok(Json(dep))
}

/// GET /api/v1/deployments -- List deployments for tenant.
async fn list_deployments(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<crate::pagination::PaginatedResponse<DeploymentResponse>>, CasperError> {
    guard.require("inference:call")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = deployment_service::list(&state.db, tenant_id, &params).await?;
    Ok(Json(result))
}

/// GET /api/v1/deployments/:id -- Get single deployment.
async fn get_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("inference:call")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let dep = deployment_service::get(&state.db, tenant_id, id).await?;
    Ok(Json(dep))
}

/// PATCH /api/v1/deployments/:id -- Update deployment.
async fn update_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDeploymentRequest>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("config:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let dep = deployment_service::update(&state.db, tenant_id, id, &body).await?;
    Ok(Json(dep))
}

/// DELETE /api/v1/deployments/:id -- Soft delete (is_active=false).
async fn delete_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("config:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let dep = deployment_service::delete(&state.db, tenant_id, id).await?;
    Ok(Json(dep))
}

/// POST /api/v1/deployments/:id/test -- Dry-run: resolve routing.
async fn test_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<TestRouteResponse>, CasperError> {
    guard.require("inference:call")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = deployment_service::test_route(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

/// GET /api/v1/deployments/available-models -- Published models with tenant quota.
async fn available_models(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<AvailableModel>>, CasperError> {
    guard.require("config:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let models = deployment_service::available_models(&state.db_owner, tenant_id).await?;
    Ok(Json(models))
}

/// GET /api/v1/deployments/available-backends?model_id=X -- Backends for a model.
async fn available_backends(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(q): Query<ModelIdQuery>,
) -> Result<Json<Vec<AvailableBackend>>, CasperError> {
    guard.require("config:manage")?;
    let backends = deployment_service::available_backends(&state.db_owner, q.model_id).await?;
    Ok(Json(backends))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn deployment_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/deployments", post(create_deployment).get(list_deployments))
        .route("/api/v1/deployments/available-models", get(available_models))
        .route("/api/v1/deployments/available-backends", get(available_backends))
        .route(
            "/api/v1/deployments/{id}",
            get(get_deployment).patch(update_deployment).delete(delete_deployment),
        )
        .route("/api/v1/deployments/{id}/test", post(test_deployment))
}
