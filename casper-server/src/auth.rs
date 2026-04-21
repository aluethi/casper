use axum::{
    extract::{FromRequestParts, Request},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use casper_base::{CasperError, JwtVerifier, RevocationCache, Scope, TenantContext};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;

/// Shared auth state.
#[derive(Clone)]
pub struct AuthState {
    pub jwt_verifier: Arc<JwtVerifier>,
    pub revocation_cache: RevocationCache,
    pub db: PgPool,
}

/// Auth middleware: extract Bearer token, verify JWT or API key, set TenantContext.
pub async fn auth_middleware(
    axum::extract::State(auth): axum::extract::State<AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, CasperError> {
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(CasperError::Unauthorized)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(CasperError::Unauthorized)?;

    let ctx = if token.starts_with("csk-") {
        // API key authentication
        authenticate_api_key(&auth.db, token).await?
    } else {
        // JWT authentication
        auth.jwt_verifier
            .authenticate(token, &auth.revocation_cache)?
    };

    request.extensions_mut().insert(ctx);
    Ok(next.run(request).await)
}

/// Authenticate via API key: hash the key, look up in database, build TenantContext.
async fn authenticate_api_key(pool: &PgPool, key: &str) -> Result<TenantContext, CasperError> {
    let hash = hex::encode(Sha256::digest(key.as_bytes()));

    let row: Option<(uuid::Uuid, String, Vec<String>, bool)> = sqlx::query_as(
        "SELECT ak.tenant_id, ak.name, ak.scopes, ak.is_active
         FROM api_keys ak
         WHERE ak.key_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (tenant_id, key_name, scope_strings, is_active) = row.ok_or(CasperError::Unauthorized)?;

    if !is_active {
        return Err(CasperError::Unauthorized);
    }

    let scopes: Vec<Scope> = scope_strings
        .iter()
        .filter_map(|s| Scope::parse(s).ok())
        .collect();

    Ok(TenantContext {
        tenant_id: casper_base::TenantId(tenant_id),
        subject: casper_base::Subject::ApiKey(key_name),
        role: casper_base::Role::Operator, // API keys default to operator role
        scopes,
        token_id: format!("apikey:{}", &key[..8.min(key.len())]),
        correlation_id: casper_base::CorrelationId::new(),
    })
}

/// Extractor that retrieves TenantContext and provides scope checking.
pub struct ScopeGuard(pub TenantContext);

impl ScopeGuard {
    pub fn require(&self, scope: &str) -> Result<(), CasperError> {
        self.0.require_scope(scope)
    }
}

impl<S> FromRequestParts<S> for ScopeGuard
where
    S: Send + Sync,
{
    type Rejection = CasperError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<TenantContext>()
            .cloned()
            .ok_or(CasperError::Unauthorized)?;
        Ok(ScopeGuard(ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_hash() {
        let key = "csk-test-key-123";
        let hash = hex::encode(Sha256::digest(key.as_bytes()));
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }
}
