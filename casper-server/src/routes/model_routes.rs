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
use crate::services::model_service::{
    self, CreateModelRequest, ModelResponse, UpdateModelRequest,
};

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/models -- Create model.
async fn create_model(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateModelRequest>,
) -> Result<Json<ModelResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = model_service::create(&state.db_owner, &body).await?;
    Ok(Json(result))
}

/// GET /api/v1/models -- List all models (including unpublished).
async fn list_models(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<crate::pagination::PaginatedResponse<ModelResponse>>, CasperError> {
    guard.require("platform:admin")?;
    let result = model_service::list(&state.db_owner, &params).await?;
    Ok(Json(result))
}

/// GET /api/v1/models/:id -- Get single model.
async fn get_model(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ModelResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = model_service::get(&state.db_owner, id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/models/:id -- Update model.
async fn update_model(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateModelRequest>,
) -> Result<Json<ModelResponse>, CasperError> {
    guard.require("platform:admin")?;
    let result = model_service::update(&state.db_owner, id, &body).await?;
    Ok(Json(result))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn model_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/models", post(create_model).get(list_models))
        .route("/api/v1/models/{id}", get(get_model).patch(update_model))
}
