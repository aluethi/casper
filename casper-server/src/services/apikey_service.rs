use casper_base::TenantDb;
use casper_base::{CasperError, TenantId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;
use crate::pagination::{PaginatedResponse, Pagination, PaginationParams};

// ── Domain types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Deserialize)]
pub struct UpdateApiKeyRequest {
    pub name: Option<String>,
    pub scopes: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ApiKeyResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub key_prefix: String,
    pub is_active: bool,
    pub created_at: String,
    pub created_by: String,
}

/// Response returned only on creation, includes the plaintext key.
#[derive(Serialize)]
pub struct ApiKeyCreatedResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub scopes: Vec<String>,
    pub key_prefix: String,
    pub key: String,
    pub is_active: bool,
    pub created_at: String,
    pub created_by: String,
}

#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    scopes: Vec<String>,
    key_prefix: String,
    is_active: bool,
    created_at: OffsetDateTime,
    created_by: String,
}

fn row_to_response(r: ApiKeyRow) -> ApiKeyResponse {
    ApiKeyResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        name: r.name,
        scopes: r.scopes,
        key_prefix: r.key_prefix,
        is_active: r.is_active,
        created_at: to_rfc3339(r.created_at),
        created_by: r.created_by,
    }
}

// ── Service functions ────────────────────────────────────────────

pub async fn create(
    db: &PgPool,
    tenant_id: TenantId,
    req: &CreateApiKeyRequest,
    actor: &str,
) -> Result<ApiKeyCreatedResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let id = Uuid::now_v7();
    let key = format!("csk-{}", Uuid::now_v7());
    let key_hash = hex::encode(Sha256::digest(key.as_bytes()));
    let key_prefix = key[..12.min(key.len())].to_string();

    let row: ApiKeyRow = sqlx::query_as(
        "INSERT INTO api_keys (id, tenant_id, name, scopes, key_hash, key_prefix, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by",
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&req.name)
    .bind(&req.scopes)
    .bind(&key_hash)
    .bind(&key_prefix)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("api_keys_tenant_id_name_key") =>
        {
            CasperError::Conflict(format!("API key '{}' already exists in tenant", req.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(ApiKeyCreatedResponse {
        id: row.id,
        tenant_id: row.tenant_id,
        name: row.name,
        scopes: row.scopes,
        key_prefix: row.key_prefix,
        key,
        is_active: row.is_active,
        created_at: to_rfc3339(row.created_at),
        created_by: row.created_by,
    })
}

pub async fn list(
    db: &PgPool,
    tenant_id: TenantId,
    params: &PaginationParams,
) -> Result<PaginatedResponse<ApiKeyResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let offset = params.offset();

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM api_keys")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<ApiKeyRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by
         FROM api_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2",
    )
    .bind(params.limit())
    .bind(offset)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
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
) -> Result<ApiKeyResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by
         FROM api_keys WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("API key {id}")))
}

pub async fn update(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
    req: &UpdateApiKeyRequest,
) -> Result<ApiKeyResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "UPDATE api_keys SET
            name = COALESCE($2, name),
            scopes = COALESCE($3, scopes)
         WHERE id = $1
         RETURNING id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by",
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.scopes)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("API key {id}")))
}

pub async fn delete(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<ApiKeyResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<ApiKeyRow> = sqlx::query_as(
        "UPDATE api_keys SET is_active = false
         WHERE id = $1
         RETURNING id, tenant_id, name, scopes, key_prefix, is_active, created_at, created_by",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("API key {id}")))
}
