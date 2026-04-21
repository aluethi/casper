use axum::{Json, Router, extract::State, routing::get};
use casper_base::CasperError;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::helpers::to_rfc3339;

// ── Response types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct CatalogEntry {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub provider: String,
    pub cap_chat: bool,
    pub cap_embedding: bool,
    pub cap_thinking: bool,
    pub cap_vision: bool,
    pub cap_tool_use: bool,
    pub cap_json_output: bool,
    pub cap_audio_in: bool,
    pub cap_audio_out: bool,
    pub cap_image_gen: bool,
    pub context_window: Option<i32>,
    pub max_output_tokens: Option<i32>,
    pub embedding_dimensions: Option<i32>,
    pub has_quota: bool,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct CatalogRow {
    id: Uuid,
    name: String,
    display_name: String,
    provider: String,
    cap_chat: bool,
    cap_embedding: bool,
    cap_thinking: bool,
    cap_vision: bool,
    cap_tool_use: bool,
    cap_json_output: bool,
    cap_audio_in: bool,
    cap_audio_out: bool,
    cap_image_gen: bool,
    context_window: Option<i32>,
    max_output_tokens: Option<i32>,
    embedding_dimensions: Option<i32>,
    has_quota: bool,
    created_at: OffsetDateTime,
}

fn row_to_entry(r: CatalogRow) -> CatalogEntry {
    CatalogEntry {
        id: r.id,
        name: r.name,
        display_name: r.display_name,
        provider: r.provider,
        cap_chat: r.cap_chat,
        cap_embedding: r.cap_embedding,
        cap_thinking: r.cap_thinking,
        cap_vision: r.cap_vision,
        cap_tool_use: r.cap_tool_use,
        cap_json_output: r.cap_json_output,
        cap_audio_in: r.cap_audio_in,
        cap_audio_out: r.cap_audio_out,
        cap_image_gen: r.cap_image_gen,
        context_window: r.context_window,
        max_output_tokens: r.max_output_tokens,
        embedding_dimensions: r.embedding_dimensions,
        has_quota: r.has_quota,
        created_at: to_rfc3339(r.created_at),
    }
}

// ── Handler ────────────────────────────────────────────────────────

/// GET /api/v1/catalog — Published models with quota status for the caller's tenant.
async fn list_catalog(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<CatalogEntry>>, CasperError> {
    // No specific scope required — any authenticated user can see the catalog.
    let tenant_id = guard.0.tenant_id.0;

    let rows: Vec<CatalogRow> = sqlx::query_as(
        "SELECT
            m.id, m.name, m.display_name, m.provider,
            m.cap_chat, m.cap_embedding, m.cap_thinking, m.cap_vision,
            m.cap_tool_use, m.cap_json_output, m.cap_audio_in, m.cap_audio_out, m.cap_image_gen,
            m.context_window, m.max_output_tokens, m.embedding_dimensions,
            (mq.tenant_id IS NOT NULL) AS has_quota,
            m.created_at
         FROM models m
         LEFT JOIN model_quotas mq ON mq.model_id = m.id AND mq.tenant_id = $1
         WHERE m.published = true AND m.is_active = true
         ORDER BY m.provider, m.name",
    )
    .bind(tenant_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data = rows.into_iter().map(row_to_entry).collect();
    Ok(Json(data))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn catalog_router() -> Router<AppState> {
    Router::new().route("/api/v1/catalog", get(list_catalog))
}
