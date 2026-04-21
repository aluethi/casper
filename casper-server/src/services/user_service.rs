use casper_base::{CasperError, TenantId};
use casper_base::TenantDb;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;
use crate::pagination::{PaginatedResponse, Pagination, PaginationParams};

// ── Domain types ─────────────────────────────────────────────────

fn opt_to_rfc3339(dt: Option<OffsetDateTime>) -> Option<String> {
    dt.map(to_rfc3339)
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

// ── Service functions ────────────────────────────────────────────

pub async fn create(
    db: &PgPool,
    tenant_id: TenantId,
    req: &CreateUserRequest,
    actor: &str,
) -> Result<UserResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let id = Uuid::now_v7();

    let row: UserRow = sqlx::query_as(
        "INSERT INTO tenant_users (id, tenant_id, subject, role, scopes, email, display_name, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by"
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&req.subject)
    .bind(&req.role)
    .bind(&req.scopes)
    .bind(&req.email)
    .bind(&req.display_name)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("tenant_users_tenant_id_subject_key") => {
            CasperError::Conflict(format!("user '{}' already exists in tenant", req.subject))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(row_to_response(row))
}

pub async fn list(
    db: &PgPool,
    tenant_id: TenantId,
    params: &PaginationParams,
) -> Result<PaginatedResponse<UserResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let offset = params.offset();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenant_users")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by
         FROM tenant_users ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    )
    .bind(params.limit())
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();

    Ok(PaginatedResponse {
        data,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: total.0,
        },
    })
}

pub async fn get(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<UserResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<UserRow> = sqlx::query_as(
        "SELECT id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by
         FROM tenant_users WHERE id = $1"
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("user {id}")))
}

pub async fn update(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
    req: &UpdateUserRequest,
) -> Result<UserResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<UserRow> = sqlx::query_as(
        "UPDATE tenant_users SET
            role = COALESCE($2, role),
            scopes = COALESCE($3, scopes),
            display_name = COALESCE($4, display_name)
         WHERE id = $1
         RETURNING id, tenant_id, subject, role, scopes, email, display_name, last_login_at, created_at, created_by"
    )
    .bind(id)
    .bind(&req.role)
    .bind(&req.scopes)
    .bind(&req.display_name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("user {id}")))
}

pub async fn delete(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<serde_json::Value, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let result = sqlx::query("DELETE FROM tenant_users WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("user {id}")));
    }

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(serde_json::json!({ "deleted": true }))
}
