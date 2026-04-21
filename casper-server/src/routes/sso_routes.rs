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
pub struct CreateSsoRequest {
    pub name: String,
    #[serde(default = "default_provider_type")]
    pub provider_type: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_scopes")]
    pub scopes: String,
}

fn default_provider_type() -> String {
    "oidc".to_string()
}

fn default_scopes() -> String {
    "openid email profile".to_string()
}

#[derive(Deserialize)]
pub struct UpdateSsoRequest {
    pub name: Option<String>,
    pub issuer_url: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub scopes: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Serialize)]
pub struct SsoResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub provider_type: String,
    pub issuer_url: String,
    pub client_id: String,
    pub scopes: String,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct SsoRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    provider_type: String,
    issuer_url: String,
    client_id: String,
    scopes: String,
    is_active: bool,
    created_at: OffsetDateTime,
}

fn row_to_response(r: SsoRow) -> SsoResponse {
    SsoResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        name: r.name,
        provider_type: r.provider_type,
        issuer_url: r.issuer_url,
        client_id: r.client_id,
        scopes: r.scopes,
        is_active: r.is_active,
        created_at: to_rfc3339(r.created_at),
    }
}

/// POST /api/v1/tenants/:id/sso — Create OIDC config.
async fn create_sso(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<CreateSsoRequest>,
) -> Result<Json<SsoResponse>, CasperError> {
    guard.require("platform:admin")?;

    let id = Uuid::now_v7();

    // Store client_secret as-is in client_secret_enc for now;
    // proper Vault encryption will be wired in later.
    let row: SsoRow = sqlx::query_as(
        "INSERT INTO sso_providers (id, tenant_id, name, provider_type, issuer_url, client_id, client_secret_enc, scopes)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING id, tenant_id, name, provider_type, issuer_url, client_id, scopes, is_active, created_at"
    )
    .bind(id)
    .bind(tenant_id)
    .bind(&body.name)
    .bind(&body.provider_type)
    .bind(&body.issuer_url)
    .bind(&body.client_id)
    .bind(&body.client_secret)
    .bind(&body.scopes)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("sso_providers_tenant_id_key") => {
            CasperError::Conflict(format!("SSO config already exists for tenant {tenant_id}"))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/tenants/:id/sso — Return SSO config (never return client_secret_enc).
async fn get_sso(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<SsoResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<SsoRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, provider_type, issuer_url, client_id, scopes, is_active, created_at
         FROM sso_providers WHERE tenant_id = $1"
    )
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r =
        row.ok_or_else(|| CasperError::NotFound(format!("SSO config for tenant {tenant_id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/tenants/:id/sso — Update SSO config.
async fn update_sso(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<UpdateSsoRequest>,
) -> Result<Json<SsoResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<SsoRow> = sqlx::query_as(
        "UPDATE sso_providers SET
            name = COALESCE($2, name),
            issuer_url = COALESCE($3, issuer_url),
            client_id = COALESCE($4, client_id),
            client_secret_enc = COALESCE($5, client_secret_enc),
            scopes = COALESCE($6, scopes),
            is_active = COALESCE($7, is_active)
         WHERE tenant_id = $1
         RETURNING id, tenant_id, name, provider_type, issuer_url, client_id, scopes, is_active, created_at"
    )
    .bind(tenant_id)
    .bind(&body.name)
    .bind(&body.issuer_url)
    .bind(&body.client_id)
    .bind(&body.client_secret)
    .bind(&body.scopes)
    .bind(body.is_active)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r =
        row.ok_or_else(|| CasperError::NotFound(format!("SSO config for tenant {tenant_id}")))?;
    Ok(Json(row_to_response(r)))
}

/// DELETE /api/v1/tenants/:id/sso — Remove SSO config.
async fn delete_sso(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("platform:admin")?;

    let result = sqlx::query("DELETE FROM sso_providers WHERE tenant_id = $1")
        .bind(tenant_id)
        .execute(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!(
            "SSO config for tenant {tenant_id}"
        )));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

pub fn sso_router() -> Router<AppState> {
    Router::new().route(
        "/api/v1/tenants/{id}/sso",
        post(create_sso)
            .get(get_sso)
            .patch(update_sso)
            .delete(delete_sso),
    )
}
