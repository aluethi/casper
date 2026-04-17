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
use crate::services::user_service::{
    self, CreateUserRequest, UpdateUserRequest, UserResponse,
};

// ── Handlers ──────────────────────────────────────────────────────

/// POST /api/v1/users -- Create user.
async fn create_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, CasperError> {
    guard.require("users:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = user_service::create(&state.db, tenant_id, &body, &guard.0.actor()).await?;
    Ok(Json(result))
}

/// GET /api/v1/users -- List users.
async fn list_users(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<crate::pagination::PaginatedResponse<UserResponse>>, CasperError> {
    guard.require("users:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = user_service::list(&state.db, tenant_id, &params).await?;
    Ok(Json(result))
}

/// GET /api/v1/users/:id -- Get user.
async fn get_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<UserResponse>, CasperError> {
    guard.require("users:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = user_service::get(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

/// PATCH /api/v1/users/:id -- Update user role/scopes/display_name.
async fn update_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, CasperError> {
    guard.require("users:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = user_service::update(&state.db, tenant_id, id, &body).await?;
    Ok(Json(result))
}

/// DELETE /api/v1/users/:id -- Delete user.
async fn delete_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("users:manage")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let result = user_service::delete(&state.db, tenant_id, id).await?;
    Ok(Json(result))
}

// ── Router ────────────────────────────────────────────────────────

pub fn user_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/users", post(create_user).get(list_users))
        .route("/api/v1/users/{id}", get(get_user).patch(update_user).delete(delete_user))
}
