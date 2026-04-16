use casper_base::{CasperError, TenantId};
use casper_db::TenantDb;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;
use crate::pagination::{PaginatedResponse, Pagination, PaginationParams};

// ── Request / Response types ───────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDeploymentRequest {
    pub model_id: Uuid,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub backend_sequence: Vec<Uuid>,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: i32,
    #[serde(default = "default_retry_backoff_ms")]
    pub retry_backoff_ms: i32,
    #[serde(default = "default_true")]
    pub fallback_enabled: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: i32,
    #[serde(default = "default_json_obj")]
    pub default_params: serde_json::Value,
    pub rate_limit_rpm: Option<i32>,
}

fn default_retry_attempts() -> i32 { 1 }
fn default_retry_backoff_ms() -> i32 { 1000 }
fn default_true() -> bool { true }
fn default_timeout_ms() -> i32 { 30000 }
fn default_json_obj() -> serde_json::Value { serde_json::json!({}) }

#[derive(Deserialize)]
pub struct UpdateDeploymentRequest {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub backend_sequence: Option<Vec<Uuid>>,
    pub retry_attempts: Option<i32>,
    pub retry_backoff_ms: Option<i32>,
    pub fallback_enabled: Option<bool>,
    pub timeout_ms: Option<i32>,
    pub default_params: Option<serde_json::Value>,
    pub rate_limit_rpm: Option<i32>,
}

#[derive(Serialize)]
pub struct DeploymentResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub model_id: Uuid,
    pub name: String,
    pub slug: String,
    pub backend_sequence: Vec<Uuid>,
    pub retry_attempts: i32,
    pub retry_backoff_ms: i32,
    pub fallback_enabled: bool,
    pub timeout_ms: i32,
    pub default_params: serde_json::Value,
    pub rate_limit_rpm: Option<i32>,
    pub is_active: bool,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct DeploymentRow {
    id: Uuid,
    tenant_id: Uuid,
    model_id: Uuid,
    name: String,
    slug: String,
    backend_sequence: Vec<Uuid>,
    retry_attempts: i32,
    retry_backoff_ms: i32,
    fallback_enabled: bool,
    timeout_ms: i32,
    default_params: serde_json::Value,
    rate_limit_rpm: Option<i32>,
    is_active: bool,
    created_at: OffsetDateTime,
}

fn row_to_response(r: DeploymentRow) -> DeploymentResponse {
    DeploymentResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        model_id: r.model_id,
        name: r.name,
        slug: r.slug,
        backend_sequence: r.backend_sequence,
        retry_attempts: r.retry_attempts,
        retry_backoff_ms: r.retry_backoff_ms,
        fallback_enabled: r.fallback_enabled,
        timeout_ms: r.timeout_ms,
        default_params: r.default_params,
        rate_limit_rpm: r.rate_limit_rpm,
        is_active: r.is_active,
        created_at: to_rfc3339(r.created_at),
    }
}

const DEPLOYMENT_COLUMNS: &str =
    "id, tenant_id, model_id, name, slug, \
     backend_sequence, retry_attempts, retry_backoff_ms, fallback_enabled, timeout_ms, \
     default_params, rate_limit_rpm, is_active, created_at";

#[derive(Serialize)]
pub struct TestRouteResponse {
    pub deployment_id: Uuid,
    pub model_id: Uuid,
    pub backends: Vec<ResolvedBackend>,
}

#[derive(Serialize)]
pub struct ResolvedBackend {
    pub backend_id: Uuid,
    pub name: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub priority: i32,
}

#[derive(sqlx::FromRow)]
struct DeploymentTestRow {
    model_id: Uuid,
    backend_sequence: Vec<Uuid>,
}

#[derive(sqlx::FromRow)]
struct ResolvedBackendRow {
    id: Uuid,
    name: String,
    provider: String,
    base_url: Option<String>,
    priority: i32,
}

// ── Service functions ──────────────────────────────────────────────

pub async fn create(
    db: &PgPool,
    tenant_id: TenantId,
    req: &CreateDeploymentRequest,
) -> Result<DeploymentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Validate model exists and is published
    let model_check: Option<(bool, bool)> = sqlx::query_as(
        "SELECT published, is_active FROM models WHERE id = $1"
    )
    .bind(req.model_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    match model_check {
        None => return Err(CasperError::NotFound(format!("model {}", req.model_id))),
        Some((published, is_active)) => {
            if !published || !is_active {
                return Err(CasperError::BadRequest(
                    "model is not published or not active".into(),
                ));
            }
        }
    }

    // Validate quota exists for this tenant + model
    let has_quota: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM model_quotas WHERE tenant_id = $1 AND model_id = $2)"
    )
    .bind(tenant_id.0)
    .bind(req.model_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if !has_quota {
        return Err(CasperError::BadRequest(
            "no quota allocated for this model".into(),
        ));
    }

    let id = Uuid::now_v7();

    let row: DeploymentRow = sqlx::query_as(&format!(
        "INSERT INTO model_deployments (
            id, tenant_id, model_id, name, slug,
            backend_sequence, retry_attempts, retry_backoff_ms, fallback_enabled, timeout_ms,
            default_params, rate_limit_rpm
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         RETURNING {DEPLOYMENT_COLUMNS}"
    ))
    .bind(id)
    .bind(tenant_id.0)
    .bind(req.model_id)
    .bind(&req.name)
    .bind(&req.slug)
    .bind(&req.backend_sequence)
    .bind(req.retry_attempts)
    .bind(req.retry_backoff_ms)
    .bind(req.fallback_enabled)
    .bind(req.timeout_ms)
    .bind(&req.default_params)
    .bind(req.rate_limit_rpm)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("model_deployments_tenant_id_slug_key") =>
        {
            CasperError::Conflict(format!("deployment slug '{}' already exists", req.slug))
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
) -> Result<PaginatedResponse<DeploymentResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let offset = params.offset();

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM model_deployments WHERE tenant_id = $1"
    )
    .bind(tenant_id.0)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<DeploymentRow> = sqlx::query_as(&format!(
        "SELECT {DEPLOYMENT_COLUMNS} FROM model_deployments
         WHERE tenant_id = $1
         ORDER BY created_at DESC LIMIT $2 OFFSET $3"
    ))
    .bind(tenant_id.0)
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
) -> Result<DeploymentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<DeploymentRow> = sqlx::query_as(&format!(
        "SELECT {DEPLOYMENT_COLUMNS} FROM model_deployments WHERE id = $1 AND tenant_id = $2"
    ))
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("deployment {id}")))
}

pub async fn update(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
    req: &UpdateDeploymentRequest,
) -> Result<DeploymentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<DeploymentRow> = sqlx::query_as(&format!(
        "UPDATE model_deployments SET
            name                = COALESCE($3, name),
            slug                = COALESCE($4, slug),
            backend_sequence    = COALESCE($5, backend_sequence),
            retry_attempts      = COALESCE($6, retry_attempts),
            retry_backoff_ms    = COALESCE($7, retry_backoff_ms),
            fallback_enabled    = COALESCE($8, fallback_enabled),
            timeout_ms          = COALESCE($9, timeout_ms),
            default_params      = COALESCE($10, default_params),
            rate_limit_rpm      = COALESCE($11, rate_limit_rpm)
         WHERE id = $1 AND tenant_id = $2
         RETURNING {DEPLOYMENT_COLUMNS}"
    ))
    .bind(id)
    .bind(tenant_id.0)
    .bind(&req.name)
    .bind(&req.slug)
    .bind(&req.backend_sequence)
    .bind(req.retry_attempts)
    .bind(req.retry_backoff_ms)
    .bind(req.fallback_enabled)
    .bind(req.timeout_ms)
    .bind(&req.default_params)
    .bind(req.rate_limit_rpm)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("model_deployments_tenant_id_slug_key") =>
        {
            CasperError::Conflict("deployment slug already exists".into())
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("deployment {id}")))
}

pub async fn delete(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<DeploymentResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<DeploymentRow> = sqlx::query_as(&format!(
        "UPDATE model_deployments SET is_active = false
         WHERE id = $1 AND tenant_id = $2
         RETURNING {DEPLOYMENT_COLUMNS}"
    ))
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.map(row_to_response)
        .ok_or_else(|| CasperError::NotFound(format!("deployment {id}")))
}

pub async fn test_route(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<TestRouteResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let dep: Option<DeploymentTestRow> = sqlx::query_as(
        "SELECT model_id, backend_sequence FROM model_deployments
         WHERE id = $1 AND tenant_id = $2 AND is_active = true"
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let dep = dep.ok_or_else(|| {
        CasperError::NotFound(format!("active deployment {id}"))
    })?;
    let model_id = dep.model_id;
    let backend_sequence = dep.backend_sequence;

    let backends: Vec<ResolvedBackendRow> = if backend_sequence.is_empty() {
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, pbm.priority
             FROM platform_backend_models pbm
             JOIN platform_backends pb ON pb.id = pbm.backend_id
             WHERE pbm.model_id = $1 AND pb.is_active = true
             ORDER BY pbm.priority"
        )
        .bind(model_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    } else {
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, s.ord::INT AS priority
             FROM unnest($1::UUID[]) WITH ORDINALITY AS s(backend_id, ord)
             JOIN platform_backends pb ON pb.id = s.backend_id
             WHERE pb.is_active = true
             ORDER BY s.ord"
        )
        .bind(&backend_sequence)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    };

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let resolved = backends
        .into_iter()
        .map(|r| ResolvedBackend {
            backend_id: r.id,
            name: r.name,
            provider: r.provider,
            base_url: r.base_url,
            priority: r.priority,
        })
        .collect();

    Ok(TestRouteResponse {
        deployment_id: id,
        model_id,
        backends: resolved,
    })
}
