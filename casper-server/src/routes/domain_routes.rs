use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use casper_base::CasperError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;

#[derive(Deserialize)]
pub struct AddDomainRequest {
    pub domain: String,
}

#[derive(Serialize)]
pub struct DomainResponse {
    pub domain: String,
    pub tenant_id: Uuid,
    pub verified: bool,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct DomainRow {
    domain: String,
    tenant_id: Uuid,
    verified: bool,
    created_at: OffsetDateTime,
}

fn row_to_response(r: DomainRow) -> DomainResponse {
    DomainResponse {
        domain: r.domain,
        tenant_id: r.tenant_id,
        verified: r.verified,
        created_at: to_rfc3339(r.created_at),
    }
}

/// POST /api/v1/tenants/:id/domains — Add email domain.
async fn add_domain(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<AddDomainRequest>,
) -> Result<Json<DomainResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: DomainRow = sqlx::query_as(
        "INSERT INTO email_domains (domain, tenant_id)
         VALUES ($1, $2)
         RETURNING domain, tenant_id, verified, created_at"
    )
    .bind(&body.domain)
    .bind(tenant_id)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("email_domains_pkey") => {
            CasperError::Conflict(format!("domain '{}' already exists", body.domain))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/tenants/:id/domains — List email domains.
async fn list_domains(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<Vec<DomainResponse>>, CasperError> {
    guard.require("platform:admin")?;

    let rows: Vec<DomainRow> = sqlx::query_as(
        "SELECT domain, tenant_id, verified, created_at
         FROM email_domains WHERE tenant_id = $1
         ORDER BY created_at DESC"
    )
    .bind(tenant_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();
    Ok(Json(data))
}

/// DELETE /api/v1/tenants/:id/domains/:domain — Remove email domain.
async fn delete_domain(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path((tenant_id, domain)): Path<(Uuid, String)>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("platform:admin")?;

    let result = sqlx::query("DELETE FROM email_domains WHERE domain = $1 AND tenant_id = $2")
        .bind(&domain)
        .bind(tenant_id)
        .execute(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("domain '{domain}' for tenant {tenant_id}")));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub fn domain_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tenants/{id}/domains", post(add_domain).get(list_domains))
        .route("/api/v1/tenants/{id}/domains/{domain}", axum::routing::delete(delete_domain))
}
