use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::pagination::PaginationParams;
use crate::services::backend_service::{
    self, AssignBackendRequest, BackendModelResponse, BackendResponse, CreateBackendRequest,
    UpdateBackendRequest,
};

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/backends -- Create backend.
async fn create_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateBackendRequest>,
) -> Result<Json<BackendResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = backend_service::create(&state.db_owner, &body).await?;
    Ok(Json(result))
}

/// GET /api/v1/backends -- List backends (never returns api_key_enc).
async fn list_backends(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<crate::pagination::PaginatedResponse<BackendResponse>>, CasperError> {
    guard.require("platform:admin")?;
    let result = backend_service::list(&state.db_owner, &params).await?;
    Ok(Json(result))
}

/// GET /api/v1/backends/:id -- Get single backend (never returns api_key_enc).
async fn get_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<BackendResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = backend_service::get(&state.db_owner, id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/backends/:id -- Update backend.
async fn update_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateBackendRequest>,
) -> Result<Json<BackendResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = backend_service::update(&state.db_owner, id, &body).await?;
    Ok(Json(result))
}

// ── Backend-model assignment handlers ──────────────────────────────

/// POST /api/v1/models/:id/backends -- Assign backend to model.
async fn assign_backend(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(model_id): Path<Uuid>,
    Json(body): Json<AssignBackendRequest>,
) -> Result<Json<BackendModelResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = backend_service::assign_model(&state.db_owner, model_id, &body).await?;
    Ok(Json(result))
}

/// GET /api/v1/models/:id/backends -- List backends for model.
async fn list_model_backends(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(model_id): Path<Uuid>,
) -> Result<Json<Vec<BackendModelResponse>>, CasperError> {
    guard.require("platform:admin")?;
    let result = backend_service::list_model_backends(&state.db_owner, model_id).await?;
    Ok(Json(result))
}

/// DELETE /api/v1/models/:model_id/backends/:backend_id -- Remove assignment.
async fn remove_backend_assignment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((model_id, backend_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("platform:admin")?;
    let result =
        backend_service::remove_model_backend(&state.db_owner, model_id, backend_id).await?;
    Ok(Json(result))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn backend_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/backends", post(create_backend).get(list_backends))
        .route(
            "/api/v1/backends/{id}",
            get(get_backend).patch(update_backend),
        )
        .route(
            "/api/v1/models/{id}/backends",
            post(assign_backend).get(list_model_backends),
        )
        .route(
            "/api/v1/models/{model_id}/backends/{backend_id}",
            axum::routing::delete(remove_backend_assignment),
        )
}
