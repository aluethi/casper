use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;
use crate::pagination::{PaginationParams, PaginatedResponse, Pagination};

fn opt_to_rfc3339(dt: Option<OffsetDateTime>) -> Option<String> {
    dt.map(|d| to_rfc3339(d))
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub subject: String,
    pub role: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub role: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub display_name: Option<String>,
}

#[derive(Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub subject: String,
    pub role: String,
    pub scopes: Vec<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub created_by: String,
}

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    tenant_id: Uuid,
    subject: String,
    role: String,
    scopes: Vec<String>,
    email: Option<String>,
    display_name: Option<String>,
    last_login_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
    created_by: String,
}

fn row_to_response(r: UserRow) -> UserResponse {
    UserResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        subject: r.subject,
        role: r.role,
        scopes: r.scopes,
        email: r.email,
        display_name: r.display_name,
        last_login_at: opt_to_rfc3339(r.last_login_at),
        created_at: to_rfc3339(r.created_at),
        created_by: r.created_by,
    }
}

/// POST /api/v1/users — Create user.
async fn create_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<UserResponse>, CasperError> {
    guard.require("users:manage")?;

    let id = Uuid::now_v7();

    let row: UserRow = sqlx::query_as(
        "INSERT INTO tenant_users (id, tenant_id, subject, role, scopes, email, display_name, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by"
    )
    .bind(id)
    .bind(guard.0.tenant_id.0)
    .bind(&body.subject)
    .bind(&body.role)
    .bind(&body.scopes)
    .bind(&body.email)
    .bind(&body.display_name)
    .bind(guard.0.actor())
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("tenant_users_tenant_id_subject_key") => {
            CasperError::Conflict(format!("user '{}' already exists in tenant", body.subject))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/users — List users.
async fn list_users(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<UserResponse>>, CasperError> {
    guard.require("users:manage")?;

    let offset = params.offset();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenant_users")
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by
         FROM tenant_users ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(params.limit())
    .bind(offset)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();

    Ok(Json(PaginatedResponse {
        data,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: total.0,
        },
    }))
}

/// GET /api/v1/users/:id — Get user.
async fn get_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<UserResponse>, CasperError> {
    guard.require("users:manage")?;

    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by
         FROM tenant_users WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("user {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/users/:id — Update user role/scopes/display_name.
async fn update_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, CasperError> {
    guard.require("users:manage")?;

    let row: Option<UserRow> = sqlx::query_as(
        "UPDATE tenant_users SET
            role = COALESCE($2, role),
            scopes = COALESCE($3, scopes),
            display_name = COALESCE($4, display_name)
         WHERE id = $1
         RETURNING id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by"
    )
    .bind(id)
    .bind(&body.role)
    .bind(&body.scopes)
    .bind(&body.display_name)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("user {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// DELETE /api/v1/users/:id — Delete user.
async fn delete_user(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("users:manage")?;

    let result = sqlx::query("DELETE FROM tenant_users WHERE id = $1")
        .bind(id)
        .execute(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("user {id}")));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub fn user_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/users", post(create_user).get(list_users))
        .route("/api/v1/users/{id}", get(get_user).patch(update_user).delete(delete_user))
}
