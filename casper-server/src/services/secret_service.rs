use casper_base::TenantDb;
use casper_base::{CasperError, TenantId};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use time::OffsetDateTime;

use crate::helpers::to_rfc3339;

// ── Domain types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SetSecretRequest {
    pub key: String,
    pub value: String,
}

#[derive(Serialize)]
pub struct SecretKeyResponse {
    pub key: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(sqlx::FromRow)]
struct SecretKeyRow {
    key: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

fn row_to_response(r: SecretKeyRow) -> SecretKeyResponse {
    SecretKeyResponse {
        key: r.key,
        created_at: to_rfc3339(r.created_at),
        updated_at: to_rfc3339(r.updated_at),
    }
}

// ── Service functions ────────────────────────────────────────────

pub async fn set(
    db: &PgPool,
    db_owner: &PgPool,
    vault: &Arc<casper_base::Vault>,
    tenant_id: TenantId,
    req: &SetSecretRequest,
) -> Result<SecretKeyResponse, CasperError> {
    // Encrypt via Vault (HKDF per-tenant key derivation + AES-256-GCM)
    vault
        .set(db_owner, tenant_id, &req.key, req.value.as_bytes())
        .await?;

    // Fetch the row back for timestamps via RLS-scoped connection
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: SecretKeyRow = sqlx::query_as(
        "SELECT key, created_at, updated_at FROM tenant_secrets WHERE tenant_id = $1 AND key = $2",
    )
    .bind(tenant_id.0)
    .bind(&req.key)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(row_to_response(row))
}

pub async fn list(db: &PgPool, tenant_id: TenantId) -> Result<Vec<SecretKeyResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<SecretKeyRow> = sqlx::query_as(
        "SELECT key, created_at, updated_at
         FROM tenant_secrets WHERE tenant_id = $1
         ORDER BY key",
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows.into_iter().map(row_to_response).collect())
}

pub async fn delete(
    db_owner: &PgPool,
    vault: &Arc<casper_base::Vault>,
    tenant_id: TenantId,
    key: &str,
) -> Result<serde_json::Value, CasperError> {
    let deleted = vault.delete(db_owner, tenant_id, key).await?;

    if !deleted {
        return Err(CasperError::NotFound(format!("secret '{key}'")));
    }

    Ok(serde_json::json!({ "deleted": true }))
}
