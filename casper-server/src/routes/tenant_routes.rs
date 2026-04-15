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

type TenantRow = (Uuid, String, String, String, serde_json::Value, OffsetDateTime, OffsetDateTime);

fn row_to_response(r: TenantRow) -> TenantResponse {
    TenantResponse {
        id: r.0,
        slug: r.1,
        display_name: r.2,
        status: r.3,
        settings: r.4,
        created_at: to_rfc3339(r.5),
        updated_at: to_rfc3339(r.6),
    }
}

#[derive(Deserialize)]
pub struct CreateTenantRequest {
    pub slug: String,
    pub display_name: String,
    #[serde(default)]
    pub settings: serde_json::Value,
    /// Owner user email
    pub owner_email: String,
    /// Owner user display name
    pub owner_name: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTenantRequest {
    pub display_name: Option<String>,
    pub status: Option<String>,
    pub settings: Option<serde_json::Value>,
}

#[derive(Serialize)]
pub struct TenantResponse {
    pub id: Uuid,
    pub slug: String,
    pub display_name: String,
    pub status: String,
    pub settings: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

/// POST /api/v1/tenants — Create tenant + owner user.
async fn create_tenant(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateTenantRequest>,
) -> Result<Json<TenantResponse>, CasperError> {
    guard.require("platform:admin")?;

    let tenant_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let settings = if body.settings.is_null() {
        serde_json::json!({})
    } else {
        body.settings
    };

    // Create tenant
    let row: TenantRow = sqlx::query_as(
            "INSERT INTO tenants (id, slug, display_name, settings)
             VALUES ($1, $2, $3, $4)
             RETURNING id, slug, display_name, status, settings, created_at, updated_at"
        )
        .bind(tenant_id)
        .bind(&body.slug)
        .bind(&body.display_name)
        .bind(&settings)
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("tenants_slug_key") => {
                CasperError::Conflict(format!("tenant slug '{}' already exists", body.slug))
            }
            _ => CasperError::Internal(format!("DB error: {e}")),
        })?;

    // Create owner user
    let subject = format!("user:{}", body.owner_email);
    sqlx::query(
        "INSERT INTO tenant_users (id, tenant_id, subject, role, scopes, email, display_name, created_by)
         VALUES ($1, $2, $3, 'owner', '{admin:*}', $4, $5, $6)"
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(&subject)
    .bind(&body.owner_email)
    .bind(body.owner_name.as_deref().unwrap_or(&body.owner_email))
    .bind(guard.0.actor())
    .execute(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("Failed to create owner: {e}")))?;

    // Initialize tenant memory
    sqlx::query(
        "INSERT INTO tenant_memory (tenant_id, content, updated_by) VALUES ($1, '', $2)
         ON CONFLICT DO NOTHING"
    )
    .bind(tenant_id)
    .bind(guard.0.actor())
    .execute(&state.db_owner)
    .await
    .ok();

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/tenants — List tenants.
async fn list_tenants(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<TenantResponse>>, CasperError> {
    guard.require("platform:admin")?;

    let offset = params.offset();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenants")
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<TenantRow> = sqlx::query_as(
            "SELECT id, slug, display_name, status, settings, created_at, updated_at
             FROM tenants ORDER BY created_at DESC LIMIT $1 OFFSET $2"
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

/// GET /api/v1/tenants/:id
async fn get_tenant(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<TenantResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<TenantRow> = sqlx::query_as(
            "SELECT id, slug, display_name, status, settings, created_at, updated_at
             FROM tenants WHERE id = $1"
        )
        .bind(id)
        .fetch_optional(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("tenant {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/tenants/:id
async fn update_tenant(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTenantRequest>,
) -> Result<Json<TenantResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<TenantRow> = sqlx::query_as(
            "UPDATE tenants SET
                display_name = COALESCE($2, display_name),
                status = COALESCE($3, status),
                settings = COALESCE($4, settings),
                updated_at = now()
             WHERE id = $1
             RETURNING id, slug, display_name, status, settings, created_at, updated_at"
        )
        .bind(id)
        .bind(&body.display_name)
        .bind(&body.status)
        .bind(&body.settings)
        .fetch_optional(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("tenant {id}")))?;
    Ok(Json(row_to_response(r)))
}

pub fn tenant_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tenants", post(create_tenant).get(list_tenants))
        .route("/api/v1/tenants/{id}", get(get_tenant).patch(update_tenant))
}
