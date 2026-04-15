use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use casper_base::CasperError;
use casper_db::TenantDb;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;

/// Estimate token count: ~4 characters per token.
fn estimate_tokens(text: &str) -> i32 {
    (text.len() / 4).max(0) as i32
}

// ── Types ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSnippetRequest {
    pub name: String,
    pub display_name: String,
    pub content: String,
}

#[derive(Deserialize)]
pub struct UpdateSnippetRequest {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub content: Option<String>,
}

#[derive(Serialize)]
pub struct SnippetResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub display_name: String,
    pub content: String,
    pub token_estimate: i32,
    pub created_at: String,
    pub updated_at: String,
    pub created_by: String,
}

#[derive(sqlx::FromRow)]
struct SnippetRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    display_name: String,
    content: String,
    token_estimate: i32,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    created_by: String,
}

fn row_to_response(r: SnippetRow) -> SnippetResponse {
    SnippetResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        name: r.name,
        display_name: r.display_name,
        content: r.content,
        token_estimate: r.token_estimate,
        created_at: to_rfc3339(r.created_at),
        updated_at: to_rfc3339(r.updated_at),
        created_by: r.created_by,
    }
}

// ── Handlers ──────────────────────────────────────────────────────

/// POST /api/v1/snippets — Create snippet.
async fn create_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateSnippetRequest>,
) -> Result<Json<SnippetResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let tdb = TenantDb::new(state.db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let id = Uuid::now_v7();
    let token_estimate = estimate_tokens(&body.content);

    let row: SnippetRow = sqlx::query_as(
        "INSERT INTO snippets (id, tenant_id, name, display_name, content, token_estimate, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, tenant_id, name, display_name, content, token_estimate, created_at, updated_at, created_by"
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&body.name)
    .bind(&body.display_name)
    .bind(&body.content)
    .bind(token_estimate)
    .bind(guard.0.actor())
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("snippets_tenant_id_name_key") => {
            CasperError::Conflict(format!("snippet '{}' already exists", body.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/snippets — List snippets for tenant.
async fn list_snippets(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<SnippetResponse>>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let tdb = TenantDb::new(state.db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<SnippetRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, display_name, content, token_estimate, created_at, updated_at, created_by
         FROM snippets WHERE tenant_id = $1
         ORDER BY name"
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_response).collect();
    Ok(Json(data))
}

/// GET /api/v1/snippets/:id — Get single snippet.
async fn get_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<SnippetResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let tdb = TenantDb::new(state.db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<SnippetRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, display_name, content, token_estimate, created_at, updated_at, created_by
         FROM snippets WHERE id = $1 AND tenant_id = $2"
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("snippet {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/snippets/:id — Update snippet (recompute token_estimate if content changes).
async fn update_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateSnippetRequest>,
) -> Result<Json<SnippetResponse>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let tdb = TenantDb::new(state.db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // If content is being updated, compute new token_estimate
    let new_token_estimate: Option<i32> = body.content.as_ref().map(|c| estimate_tokens(c));

    let row: Option<SnippetRow> = sqlx::query_as(
        "UPDATE snippets SET
            name = COALESCE($3, name),
            display_name = COALESCE($4, display_name),
            content = COALESCE($5, content),
            token_estimate = COALESCE($6, token_estimate),
            updated_at = now()
         WHERE id = $1 AND tenant_id = $2
         RETURNING id, tenant_id, name, display_name, content, token_estimate, created_at, updated_at, created_by"
    )
    .bind(id)
    .bind(tenant_id.0)
    .bind(&body.name)
    .bind(&body.display_name)
    .bind(&body.content)
    .bind(new_token_estimate)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("snippets_tenant_id_name_key") => {
            CasperError::Conflict("snippet name already exists".to_string())
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("snippet {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// DELETE /api/v1/snippets/:id — Delete snippet.
async fn delete_snippet(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("agents:manage")?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let tdb = TenantDb::new(state.db.clone(), tenant_id);
    let mut tx = tdb.begin().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let result = sqlx::query("DELETE FROM snippets WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(CasperError::NotFound(format!("snippet {id}")));
    }

    tx.commit().await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Router ────────────────────────────────────────────────────────

pub fn snippet_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/snippets", post(create_snippet).get(list_snippets))
        .route("/api/v1/snippets/{id}", get(get_snippet).patch(update_snippet).delete(delete_snippet))
}
