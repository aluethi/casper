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

fn to_rfc3339(dt: OffsetDateTime) -> String {
    dt.format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

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

type DeploymentRow = (
    Uuid, Uuid, Uuid, String, String,
    Vec<Uuid>, i32, i32, bool, i32,
    serde_json::Value, Option<i32>, bool, OffsetDateTime,
);

fn row_to_response(r: DeploymentRow) -> DeploymentResponse {
    DeploymentResponse {
        id: r.0,
        tenant_id: r.1,
        model_id: r.2,
        name: r.3,
        slug: r.4,
        backend_sequence: r.5,
        retry_attempts: r.6,
        retry_backoff_ms: r.7,
        fallback_enabled: r.8,
        timeout_ms: r.9,
        default_params: r.10,
        rate_limit_rpm: r.11,
        is_active: r.12,
        created_at: to_rfc3339(r.13),
    }
}

const DEPLOYMENT_COLUMNS: &str =
    "id, tenant_id, model_id, name, slug, \
     backend_sequence, retry_attempts, retry_backoff_ms, fallback_enabled, timeout_ms, \
     default_params, rate_limit_rpm, is_active, created_at";

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_page")]
    pub page: i64,
    #[serde(default = "default_per_page")]
    pub per_page: i64,
}

fn default_page() -> i64 { 1 }
fn default_per_page() -> i64 { 50 }

#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: Pagination,
}

#[derive(Serialize)]
pub struct Pagination {
    pub page: i64,
    pub per_page: i64,
    pub total: i64,
}

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

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/deployments — Create deployment.
async fn create_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateDeploymentRequest>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("config:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    // Validate model exists and is published
    let model_check: Option<(bool, bool)> = sqlx::query_as(
        "SELECT published, is_active FROM models WHERE id = $1"
    )
    .bind(body.model_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    match model_check {
        None => return Err(CasperError::NotFound(format!("model {}", body.model_id))),
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
    .bind(tenant_id)
    .bind(body.model_id)
    .fetch_one(&state.db_owner)
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
    .bind(tenant_id)
    .bind(body.model_id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(&body.backend_sequence)
    .bind(body.retry_attempts)
    .bind(body.retry_backoff_ms)
    .bind(body.fallback_enabled)
    .bind(body.timeout_ms)
    .bind(&body.default_params)
    .bind(body.rate_limit_rpm)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("model_deployments_tenant_id_slug_key") =>
        {
            CasperError::Conflict(format!("deployment slug '{}' already exists", body.slug))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/deployments — List deployments for tenant.
async fn list_deployments(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<DeploymentResponse>>, CasperError> {
    guard.require("inference:call")?;

    let tenant_id = guard.0.tenant_id.0;
    let offset = (params.page - 1) * params.per_page;

    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM model_deployments WHERE tenant_id = $1"
    )
    .bind(tenant_id)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<DeploymentRow> = sqlx::query_as(&format!(
        "SELECT {DEPLOYMENT_COLUMNS} FROM model_deployments
         WHERE tenant_id = $1
         ORDER BY created_at DESC LIMIT $2 OFFSET $3"
    ))
    .bind(tenant_id)
    .bind(params.per_page)
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

/// GET /api/v1/deployments/:id — Get single deployment.
async fn get_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("inference:call")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<DeploymentRow> = sqlx::query_as(&format!(
        "SELECT {DEPLOYMENT_COLUMNS} FROM model_deployments WHERE id = $1 AND tenant_id = $2"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("deployment {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/deployments/:id — Update deployment.
async fn update_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateDeploymentRequest>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("config:manage")?;

    let tenant_id = guard.0.tenant_id.0;

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
    .bind(tenant_id)
    .bind(&body.name)
    .bind(&body.slug)
    .bind(&body.backend_sequence)
    .bind(body.retry_attempts)
    .bind(body.retry_backoff_ms)
    .bind(body.fallback_enabled)
    .bind(body.timeout_ms)
    .bind(&body.default_params)
    .bind(body.rate_limit_rpm)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err)
            if db_err.constraint() == Some("model_deployments_tenant_id_slug_key") =>
        {
            CasperError::Conflict("deployment slug already exists".into())
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("deployment {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// DELETE /api/v1/deployments/:id — Soft delete (is_active=false).
async fn delete_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<DeploymentResponse>, CasperError> {
    guard.require("config:manage")?;

    let tenant_id = guard.0.tenant_id.0;

    let row: Option<DeploymentRow> = sqlx::query_as(&format!(
        "UPDATE model_deployments SET is_active = false
         WHERE id = $1 AND tenant_id = $2
         RETURNING {DEPLOYMENT_COLUMNS}"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("deployment {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// POST /api/v1/deployments/:id/test — Dry-run: resolve routing.
async fn test_deployment(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<TestRouteResponse>, CasperError> {
    guard.require("inference:call")?;

    let tenant_id = guard.0.tenant_id.0;

    // Fetch the deployment
    let dep: Option<(Uuid, Vec<Uuid>)> = sqlx::query_as(
        "SELECT model_id, backend_sequence FROM model_deployments
         WHERE id = $1 AND tenant_id = $2 AND is_active = true"
    )
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (model_id, backend_sequence) = dep.ok_or_else(|| {
        CasperError::NotFound(format!("active deployment {id}"))
    })?;

    // Resolve backends: if backend_sequence is specified, use those in order;
    // otherwise fall back to platform_backend_models for the model.
    let backends: Vec<(Uuid, String, String, Option<String>, i32)> = if backend_sequence.is_empty() {
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, pbm.priority
             FROM platform_backend_models pbm
             JOIN platform_backends pb ON pb.id = pbm.backend_id
             WHERE pbm.model_id = $1 AND pb.is_active = true
             ORDER BY pbm.priority"
        )
        .bind(model_id)
        .fetch_all(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    } else {
        // Fetch in the order specified by backend_sequence.
        // Use unnest to preserve ordering.
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, s.ord::INT AS priority
             FROM unnest($1::UUID[]) WITH ORDINALITY AS s(backend_id, ord)
             JOIN platform_backends pb ON pb.id = s.backend_id
             WHERE pb.is_active = true
             ORDER BY s.ord"
        )
        .bind(&backend_sequence)
        .fetch_all(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?
    };

    let resolved = backends
        .into_iter()
        .map(|r| ResolvedBackend {
            backend_id: r.0,
            name: r.1,
            provider: r.2,
            base_url: r.3,
            priority: r.4,
        })
        .collect();

    Ok(Json(TestRouteResponse {
        deployment_id: id,
        model_id,
        backends: resolved,
    }))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn deployment_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/deployments", post(create_deployment).get(list_deployments))
        .route(
            "/api/v1/deployments/{id}",
            get(get_deployment).patch(update_deployment).delete(delete_deployment),
        )
        .route("/api/v1/deployments/{id}/test", post(test_deployment))
}
