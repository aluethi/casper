use axum::{
    Json, Router,
    extract::State,
    http::header::AUTHORIZATION,
    routing::post,
};
use casper_base::{CasperError, RevocationCheck, TenantId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

#[derive(Deserialize)]
pub struct DevLoginRequest {
    pub email: String,
}

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub subject: String,
    pub tenant_id: Uuid,
    pub role: String,
    pub scopes: Vec<String>,
}

/// POST /auth/login — Dev mode login: email → find tenant_user → sign JWT pair.
async fn dev_login(
    State(state): State<AppState>,
    Json(body): Json<DevLoginRequest>,
) -> Result<Json<TokenResponse>, CasperError> {
    if !state.config.auth.dev_auth {
        return Err(CasperError::NotFound("dev login not enabled".into()));
    }

    let signer = state.jwt_signer.as_ref().ok_or_else(|| {
        CasperError::Internal("JWT signer not configured".into())
    })?;

    let row: Option<(Uuid, String, String, Vec<String>)> = sqlx::query_as(
        "SELECT tu.tenant_id, tu.subject, tu.role, tu.scopes
         FROM tenant_users tu
         WHERE tu.email = $1
         LIMIT 1"
    )
    .bind(&body.email)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (tenant_id, subject, role, scopes) =
        row.ok_or_else(|| CasperError::NotFound(format!("user not found: {}", body.email)))?;

    let _ = sqlx::query("UPDATE tenant_users SET last_login_at = now() WHERE email = $1")
        .bind(&body.email)
        .execute(&state.db_owner)
        .await;

    let (access_token, _) =
        signer.sign_access_token(TenantId(tenant_id), &subject, &role, scopes)?;
    let (refresh_token, _) =
        signer.sign_refresh_token(TenantId(tenant_id), &subject)?;

    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
        expires_in: 900,
    }))
}

/// POST /auth/refresh — Validate refresh token, rotate, return new pair.
async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, CasperError> {
    let verifier = state.jwt_verifier.as_ref().ok_or_else(|| {
        CasperError::Internal("JWT verifier not configured".into())
    })?;
    let signer = state.jwt_signer.as_ref().ok_or_else(|| {
        CasperError::Internal("JWT signer not configured".into())
    })?;

    let claims = verifier.verify(&body.refresh_token)?;

    if claims.role != "refresh" {
        return Err(CasperError::BadRequest("not a refresh token".into()));
    }

    if state.revocation_cache.is_revoked(&claims.jti) {
        return Err(CasperError::Unauthorized);
    }

    // Revoke old refresh token
    state.revocation_cache.revoke(&claims.jti);
    let _ = sqlx::query(
        "INSERT INTO token_revocations (jti, tenant_id, revoked_by) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING"
    )
    .bind(&claims.jti)
    .bind(claims.tid)
    .bind(&claims.sub)
    .execute(&state.db)
    .await;

    let row: Option<(String, Vec<String>)> = sqlx::query_as(
        "SELECT role, scopes FROM tenant_users
         WHERE tenant_id = $1 AND subject = $2"
    )
    .bind(claims.tid)
    .bind(&claims.sub)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (role, scopes) = row.ok_or_else(|| CasperError::NotFound("user not found".into()))?;

    let tid = TenantId(claims.tid);
    let (access_token, _) = signer.sign_access_token(tid, &claims.sub, &role, scopes)?;
    let (refresh_token, _) = signer.sign_refresh_token(tid, &claims.sub)?;

    Ok(Json(TokenResponse {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
        expires_in: 900,
    }))
}

/// POST /auth/logout — Add the token's JTI to revocation list.
async fn logout(
    State(state): State<AppState>,
    request: axum::http::Request<axum::body::Body>,
) -> Result<Json<serde_json::Value>, CasperError> {
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(CasperError::Unauthorized)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(CasperError::Unauthorized)?;

    let verifier = state.jwt_verifier.as_ref().ok_or_else(|| {
        CasperError::Internal("JWT verifier not configured".into())
    })?;

    let claims = verifier.verify(token)?;

    state.revocation_cache.revoke(&claims.jti);
    let _ = sqlx::query(
        "INSERT INTO token_revocations (jti, tenant_id, revoked_by) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING"
    )
    .bind(&claims.jti)
    .bind(claims.tid)
    .bind(&claims.sub)
    .execute(&state.db)
    .await;

    Ok(Json(serde_json::json!({ "status": "logged_out" })))
}

/// GET /auth/status — Return identity from TenantContext (requires auth middleware).
pub async fn auth_status(
    ctx: crate::auth::ScopeGuard,
) -> Result<Json<StatusResponse>, CasperError> {
    let tc = &ctx.0;
    Ok(Json(StatusResponse {
        subject: tc.subject.to_string(),
        tenant_id: tc.tenant_id.0,
        role: tc.role.to_string(),
        scopes: tc.scopes.iter().map(|s| s.to_string()).collect(),
    }))
}

/// Public auth routes (no auth middleware required).
pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(dev_login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
}
