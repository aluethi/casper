use axum::{
    Json, Router,
    extract::{Multipart, Path, State},
    routing::{get, post},
};
use casper_base::CasperError;
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;
use crate::services::knowledge_service::{
    self, DocumentDetailResponse, DocumentResponse, SearchResult, UploadInput,
};

// ── Route-specific request types ─────────────────────────────────

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    20
}

// ── Handlers ─────────────────────────────────────────────────────

/// POST /api/v1/knowledge -- Upload a document via multipart form.
async fn upload_document(
    State(state): State<AppState>,
    guard: ScopeGuard,
    mut multipart: Multipart,
) -> Result<Json<DocumentResponse>, CasperError> {
    guard.require("knowledge:write")?;

    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);

    let mut file_data: Option<Vec<u8>> = None;
    let mut file_name: Option<String> = None;
    let mut doc_name: Option<String> = None;
    let mut source: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| CasperError::BadRequest(format!("multipart error: {e}")))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| CasperError::BadRequest(format!("failed to read file: {e}")))?;
                file_data = Some(data.to_vec());
            }
            "name" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| CasperError::BadRequest(format!("failed to read name: {e}")))?;
                doc_name = Some(text);
            }
            "source" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| CasperError::BadRequest(format!("failed to read source: {e}")))?;
                source = Some(text);
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    let file_data =
        file_data.ok_or_else(|| CasperError::BadRequest("missing 'file' field".into()))?;
    let original_filename = file_name.unwrap_or_else(|| "upload.txt".to_string());
    let doc_name = doc_name.unwrap_or_else(|| original_filename.clone());
    let source = source.unwrap_or_default();

    let input = UploadInput {
        file_data,
        original_filename,
        doc_name,
        source,
    };

    let doc = knowledge_service::upload(&state.db, tenant_id, input, &guard.0.actor()).await?;
    Ok(Json(doc))
}

/// GET /api/v1/knowledge -- List documents for tenant.
async fn list_documents(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<Vec<DocumentResponse>>, CasperError> {
    guard.require("knowledge:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let docs = knowledge_service::list_documents(&state.db, tenant_id).await?;
    Ok(Json(docs))
}

/// GET /api/v1/knowledge/:id -- Get single document with its chunks.
async fn get_document(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<DocumentDetailResponse>, CasperError> {
    guard.require("knowledge:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let detail = knowledge_service::get_document(&state.db, tenant_id, id).await?;
    Ok(Json(detail))
}

/// DELETE /api/v1/knowledge/:id -- Delete document, chunks, and file.
async fn delete_document(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, CasperError> {
    guard.require("knowledge:write")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    knowledge_service::delete_document(&state.db, tenant_id, id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// POST /api/v1/knowledge/search -- Simple text search on chunk content.
async fn search_knowledge(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<SearchRequest>,
) -> Result<Json<Vec<SearchResult>>, CasperError> {
    guard.require("knowledge:read")?;
    let tenant_id = casper_base::TenantId(guard.0.tenant_id.0);
    let results = knowledge_service::search(&state.db, tenant_id, &body.query, body.limit).await?;
    Ok(Json(results))
}

// ── Router ───────────────────────────────────────────────────────

pub fn knowledge_router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/knowledge",
            post(upload_document).get(list_documents),
        )
        .route(
            "/api/v1/knowledge/{id}",
            get(get_document).delete(delete_document),
        )
        .route("/api/v1/knowledge/search", post(search_knowledge))
}
