use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use casper_base::CasperError;
use bigdecimal::BigDecimal;
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
pub struct CreateModelRequest {
    pub name: String,
    pub display_name: String,
    pub provider: String,
    #[serde(default)]
    pub cap_chat: bool,
    #[serde(default)]
    pub cap_embedding: bool,
    #[serde(default)]
    pub cap_thinking: bool,
    #[serde(default)]
    pub cap_vision: bool,
    #[serde(default)]
    pub cap_tool_use: bool,
    #[serde(default)]
    pub cap_json_output: bool,
    #[serde(default)]
    pub cap_audio_in: bool,
    #[serde(default)]
    pub cap_audio_out: bool,
    #[serde(default)]
    pub cap_image_gen: bool,
    pub context_window: Option<i32>,
    pub max_output_tokens: Option<i32>,
    pub embedding_dimensions: Option<i32>,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
    pub cost_per_1k_cache_read: Option<f64>,
    pub cost_per_1k_cache_write: Option<f64>,
    #[serde(default)]
    pub published: bool,
}

#[derive(Deserialize)]
pub struct UpdateModelRequest {
    pub display_name: Option<String>,
    pub provider: Option<String>,
    pub cap_chat: Option<bool>,
    pub cap_embedding: Option<bool>,
    pub cap_thinking: Option<bool>,
    pub cap_vision: Option<bool>,
    pub cap_tool_use: Option<bool>,
    pub cap_json_output: Option<bool>,
    pub cap_audio_in: Option<bool>,
    pub cap_audio_out: Option<bool>,
    pub cap_image_gen: Option<bool>,
    pub context_window: Option<i32>,
    pub max_output_tokens: Option<i32>,
    pub embedding_dimensions: Option<i32>,
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
    pub cost_per_1k_cache_read: Option<f64>,
    pub cost_per_1k_cache_write: Option<f64>,
    pub published: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Serialize)]
pub struct ModelResponse {
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
    pub cost_per_1k_input: Option<f64>,
    pub cost_per_1k_output: Option<f64>,
    pub cost_per_1k_cache_read: Option<f64>,
    pub cost_per_1k_cache_write: Option<f64>,
    pub published: bool,
    pub is_active: bool,
    pub created_at: String,
}

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

// ── Row mapping ────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ModelRow {
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
    cost_per_1k_input: Option<BigDecimal>,
    cost_per_1k_output: Option<BigDecimal>,
    cost_per_1k_cache_read: Option<BigDecimal>,
    cost_per_1k_cache_write: Option<BigDecimal>,
    published: bool,
    is_active: bool,
    created_at: OffsetDateTime,
}

fn bd_to_f64(bd: BigDecimal) -> f64 {
    use bigdecimal::ToPrimitive;
    bd.to_f64().unwrap_or(0.0)
}

fn row_to_response(r: ModelRow) -> ModelResponse {
    ModelResponse {
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
        cost_per_1k_input: r.cost_per_1k_input.map(bd_to_f64),
        cost_per_1k_output: r.cost_per_1k_output.map(bd_to_f64),
        cost_per_1k_cache_read: r.cost_per_1k_cache_read.map(bd_to_f64),
        cost_per_1k_cache_write: r.cost_per_1k_cache_write.map(bd_to_f64),
        published: r.published,
        is_active: r.is_active,
        created_at: to_rfc3339(r.created_at),
    }
}

const MODEL_COLUMNS: &str =
    "id, name, display_name, provider, \
     cap_chat, cap_embedding, cap_thinking, cap_vision, \
     cap_tool_use, cap_json_output, cap_audio_in, cap_audio_out, cap_image_gen, \
     context_window, max_output_tokens, embedding_dimensions, \
     cost_per_1k_input, cost_per_1k_output, cost_per_1k_cache_read, cost_per_1k_cache_write, \
     published, is_active, created_at";

// ── Handlers ───────────────────────────────────────────────────────

/// POST /api/v1/models — Create model.
async fn create_model(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<CreateModelRequest>,
) -> Result<Json<ModelResponse>, CasperError> {
    guard.require("platform:admin")?;

    let id = Uuid::now_v7();

    let row: ModelRow = sqlx::query_as(&format!(
        "INSERT INTO models (
            id, name, display_name, provider,
            cap_chat, cap_embedding, cap_thinking, cap_vision,
            cap_tool_use, cap_json_output, cap_audio_in, cap_audio_out, cap_image_gen,
            context_window, max_output_tokens, embedding_dimensions,
            cost_per_1k_input, cost_per_1k_output, cost_per_1k_cache_read, cost_per_1k_cache_write,
            published
         ) VALUES (
            $1, $2, $3, $4,
            $5, $6, $7, $8, $9, $10, $11, $12, $13,
            $14, $15, $16,
            $17, $18, $19, $20,
            $21
         ) RETURNING {MODEL_COLUMNS}"
    ))
    .bind(id)
    .bind(&body.name)
    .bind(&body.display_name)
    .bind(&body.provider)
    .bind(body.cap_chat)
    .bind(body.cap_embedding)
    .bind(body.cap_thinking)
    .bind(body.cap_vision)
    .bind(body.cap_tool_use)
    .bind(body.cap_json_output)
    .bind(body.cap_audio_in)
    .bind(body.cap_audio_out)
    .bind(body.cap_image_gen)
    .bind(body.context_window)
    .bind(body.max_output_tokens)
    .bind(body.embedding_dimensions)
    .bind(body.cost_per_1k_input)
    .bind(body.cost_per_1k_output)
    .bind(body.cost_per_1k_cache_read)
    .bind(body.cost_per_1k_cache_write)
    .bind(body.published)
    .fetch_one(&state.db_owner)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint() == Some("models_name_key") => {
            CasperError::Conflict(format!("model '{}' already exists", body.name))
        }
        _ => CasperError::Internal(format!("DB error: {e}")),
    })?;

    Ok(Json(row_to_response(row)))
}

/// GET /api/v1/models — List all models (including unpublished).
async fn list_models(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Query(params): Query<PaginationParams>,
) -> Result<Json<PaginatedResponse<ModelResponse>>, CasperError> {
    guard.require("platform:admin")?;

    let offset = (params.page - 1) * params.per_page;

    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM models")
        .fetch_one(&state.db_owner)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<ModelRow> = sqlx::query_as(&format!(
        "SELECT {MODEL_COLUMNS} FROM models ORDER BY created_at DESC LIMIT $1 OFFSET $2"
    ))
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

/// GET /api/v1/models/:id — Get single model.
async fn get_model(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<ModelResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<ModelRow> = sqlx::query_as(&format!(
        "SELECT {MODEL_COLUMNS} FROM models WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("model {id}")))?;
    Ok(Json(row_to_response(r)))
}

/// PATCH /api/v1/models/:id — Update model.
async fn update_model(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateModelRequest>,
) -> Result<Json<ModelResponse>, CasperError> {
    guard.require("platform:admin")?;

    let row: Option<ModelRow> = sqlx::query_as(&format!(
        "UPDATE models SET
            display_name        = COALESCE($2, display_name),
            provider            = COALESCE($3, provider),
            cap_chat            = COALESCE($4, cap_chat),
            cap_embedding       = COALESCE($5, cap_embedding),
            cap_thinking        = COALESCE($6, cap_thinking),
            cap_vision          = COALESCE($7, cap_vision),
            cap_tool_use        = COALESCE($8, cap_tool_use),
            cap_json_output     = COALESCE($9, cap_json_output),
            cap_audio_in        = COALESCE($10, cap_audio_in),
            cap_audio_out       = COALESCE($11, cap_audio_out),
            cap_image_gen       = COALESCE($12, cap_image_gen),
            context_window      = COALESCE($13, context_window),
            max_output_tokens   = COALESCE($14, max_output_tokens),
            embedding_dimensions= COALESCE($15, embedding_dimensions),
            cost_per_1k_input   = COALESCE($16, cost_per_1k_input),
            cost_per_1k_output  = COALESCE($17, cost_per_1k_output),
            cost_per_1k_cache_read  = COALESCE($18, cost_per_1k_cache_read),
            cost_per_1k_cache_write = COALESCE($19, cost_per_1k_cache_write),
            published           = COALESCE($20, published),
            is_active           = COALESCE($21, is_active)
         WHERE id = $1
         RETURNING {MODEL_COLUMNS}"
    ))
    .bind(id)
    .bind(&body.display_name)
    .bind(&body.provider)
    .bind(body.cap_chat)
    .bind(body.cap_embedding)
    .bind(body.cap_thinking)
    .bind(body.cap_vision)
    .bind(body.cap_tool_use)
    .bind(body.cap_json_output)
    .bind(body.cap_audio_in)
    .bind(body.cap_audio_out)
    .bind(body.cap_image_gen)
    .bind(body.context_window)
    .bind(body.max_output_tokens)
    .bind(body.embedding_dimensions)
    .bind(body.cost_per_1k_input)
    .bind(body.cost_per_1k_output)
    .bind(body.cost_per_1k_cache_read)
    .bind(body.cost_per_1k_cache_write)
    .bind(body.published)
    .bind(body.is_active)
    .fetch_optional(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("model {id}")))?;
    Ok(Json(row_to_response(r)))
}

// ── Router ─────────────────────────────────────────────────────────

pub fn model_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/models", post(create_model).get(list_models))
        .route("/api/v1/models/{id}", get(get_model).patch(update_model))
}
