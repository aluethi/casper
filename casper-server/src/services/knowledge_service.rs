use casper_base::{CasperError, TenantId};
use casper_base::TenantDb;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

// ── Domain types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct DocumentResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub source: String,
    pub content_type: String,
    pub file_path: String,
    pub token_count: i32,
    pub chunk_count: i32,
    pub created_at: String,
    pub created_by: String,
}

#[derive(sqlx::FromRow)]
struct DocumentRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    source: String,
    content_type: String,
    file_path: String,
    token_count: i32,
    chunk_count: i32,
    created_at: OffsetDateTime,
    created_by: String,
}

fn row_to_response(r: DocumentRow) -> DocumentResponse {
    DocumentResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        name: r.name,
        source: r.source,
        content_type: r.content_type,
        file_path: r.file_path,
        token_count: r.token_count,
        chunk_count: r.chunk_count,
        created_at: to_rfc3339(r.created_at),
        created_by: r.created_by,
    }
}

#[derive(Serialize)]
pub struct ChunkResponse {
    pub id: Uuid,
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub token_count: i32,
    pub metadata: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct ChunkRow {
    id: Uuid,
    document_id: Uuid,
    chunk_index: i32,
    content: String,
    token_count: i32,
    metadata: serde_json::Value,
}

fn chunk_row_to_response(r: ChunkRow) -> ChunkResponse {
    ChunkResponse {
        id: r.id,
        document_id: r.document_id,
        chunk_index: r.chunk_index,
        content: r.content,
        token_count: r.token_count,
        metadata: r.metadata,
    }
}

#[derive(Serialize)]
pub struct DocumentDetailResponse {
    #[serde(flatten)]
    pub document: DocumentResponse,
    pub chunks: Vec<ChunkResponse>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub document_name: String,
    pub chunk_index: i32,
    pub content: String,
    pub token_count: i32,
}

#[derive(sqlx::FromRow)]
struct SearchRow {
    #[sqlx(rename = "id")]
    chunk_id: Uuid,
    document_id: Uuid,
    #[sqlx(rename = "name")]
    document_name: String,
    chunk_index: i32,
    content: String,
    token_count: i32,
}

fn search_row_to_response(r: SearchRow) -> SearchResult {
    SearchResult {
        chunk_id: r.chunk_id,
        document_id: r.document_id,
        document_name: r.document_name,
        chunk_index: r.chunk_index,
        content: r.content,
        token_count: r.token_count,
    }
}

// ── Chunking / file helpers ──────────────────────────────────────

/// Split text into chunks of approximately `target_chars` characters.
/// First split on double newlines (paragraphs), then on single newlines
/// if a paragraph is too large.
fn chunk_text(text: &str, target_chars: usize) -> Vec<String> {
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for para in paragraphs {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }

        // If adding this paragraph would exceed the target, flush current chunk
        if !current.is_empty() && current.len() + para.len() + 2 > target_chars {
            chunks.push(std::mem::take(&mut current));
        }

        // If the paragraph itself exceeds target, split on single newlines
        if para.len() > target_chars {
            // Flush anything accumulated
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let lines: Vec<&str> = para.split('\n').collect();
            for line in lines {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if !current.is_empty() && current.len() + line.len() + 1 > target_chars {
                    chunks.push(std::mem::take(&mut current));
                }
                if !current.is_empty() {
                    current.push('\n');
                }
                current.push_str(line);
            }
        } else {
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(para);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    // If nothing was produced, create at least one chunk with the original text
    if chunks.is_empty() && !text.trim().is_empty() {
        chunks.push(text.trim().to_string());
    }

    chunks
}

/// Estimate token count: ~4 characters per token.
fn estimate_tokens(text: &str) -> i32 {
    (text.len() / 4).max(1) as i32
}

/// Derive content type from file extension.
fn content_type_from_ext(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        _ => "application/octet-stream",
    }
}

/// Extract file extension from filename.
fn file_extension(filename: &str) -> &str {
    filename.rsplit('.').next().unwrap_or("bin")
}

// ── Upload input (no axum types) ─────────────────────────────────

pub struct UploadInput {
    pub file_data: Vec<u8>,
    pub original_filename: String,
    pub doc_name: String,
    pub source: String,
}

// ── Service functions ────────────────────────────────────────────

pub async fn upload(
    db: &PgPool,
    tenant_id: TenantId,
    input: UploadInput,
    actor: &str,
) -> Result<DocumentResponse, CasperError> {
    let document_id = Uuid::now_v7();

    let ext = file_extension(&input.original_filename);
    let content_type = content_type_from_ext(&input.original_filename);

    // Create storage directory
    let dir = format!("data/knowledge/{}", tenant_id.0);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| CasperError::Internal(format!("failed to create directory: {e}")))?;

    let file_path = format!("{dir}/{document_id}.{ext}");
    tokio::fs::write(&file_path, &input.file_data)
        .await
        .map_err(|e| CasperError::Internal(format!("failed to write file: {e}")))?;

    // Extract text content (raw bytes to string for now; PDF/DOCX parsing deferred)
    let text_content = String::from_utf8_lossy(&input.file_data).to_string();
    let total_tokens = estimate_tokens(&text_content);

    // Chunk the text (~2000 chars per chunk = ~500 tokens)
    let chunks = chunk_text(&text_content, 2000);
    let chunk_count = chunks.len() as i32;

    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Insert document
    let row: DocumentRow = sqlx::query_as(
        "INSERT INTO documents (id, tenant_id, name, source, content_type, file_path, token_count, chunk_count, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, tenant_id, name, source, content_type, file_path, token_count, chunk_count, created_at, created_by",
    )
    .bind(document_id)
    .bind(tenant_id.0)
    .bind(&input.doc_name)
    .bind(&input.source)
    .bind(content_type)
    .bind(&file_path)
    .bind(total_tokens)
    .bind(chunk_count)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Insert chunks
    for (i, chunk_content) in chunks.iter().enumerate() {
        let chunk_id = Uuid::now_v7();
        let chunk_tokens = estimate_tokens(chunk_content);

        sqlx::query(
            "INSERT INTO document_chunks (id, tenant_id, document_id, chunk_index, content, token_count)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(chunk_id)
        .bind(tenant_id.0)
        .bind(document_id)
        .bind(i as i32)
        .bind(chunk_content)
        .bind(chunk_tokens)
        .execute(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error inserting chunk: {e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(row_to_response(row))
}

pub async fn list_documents(
    db: &PgPool,
    tenant_id: TenantId,
) -> Result<Vec<DocumentResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<DocumentRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, source, content_type, file_path, token_count, chunk_count, created_at, created_by
         FROM documents WHERE tenant_id = $1
         ORDER BY created_at DESC",
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows.into_iter().map(row_to_response).collect())
}

pub async fn get_document(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<DocumentDetailResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<DocumentRow> = sqlx::query_as(
        "SELECT id, tenant_id, name, source, content_type, file_path, token_count, chunk_count, created_at, created_by
         FROM documents WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let doc = row.ok_or_else(|| CasperError::NotFound(format!("document {id}")))?;

    let chunk_rows: Vec<ChunkRow> = sqlx::query_as(
        "SELECT id, document_id, chunk_index, content, token_count, metadata
         FROM document_chunks WHERE document_id = $1 AND tenant_id = $2
         ORDER BY chunk_index",
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let chunks: Vec<ChunkResponse> = chunk_rows.into_iter().map(chunk_row_to_response).collect();

    Ok(DocumentDetailResponse {
        document: row_to_response(doc),
        chunks,
    })
}

pub async fn delete_document(
    db: &PgPool,
    tenant_id: TenantId,
    id: Uuid,
) -> Result<(), CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Get file path before deleting
    let file_path: Option<(String,)> = sqlx::query_as(
        "SELECT file_path FROM documents WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let file_path = file_path.ok_or_else(|| CasperError::NotFound(format!("document {id}")))?;

    // Delete chunks
    sqlx::query("DELETE FROM document_chunks WHERE document_id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Delete document
    sqlx::query("DELETE FROM documents WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(tenant_id.0)
        .execute(&mut *tx)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    // Delete file (best-effort, after DB commit)
    let _ = tokio::fs::remove_file(&file_path.0).await;

    Ok(())
}

pub async fn search(
    db: &PgPool,
    tenant_id: TenantId,
    query: &str,
    limit: i64,
) -> Result<Vec<SearchResult>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let pattern = format!("%{query}%");
    let limit = limit.min(100);

    let rows: Vec<SearchRow> = sqlx::query_as(
        "SELECT dc.id, dc.document_id, d.name, dc.chunk_index, dc.content, dc.token_count
         FROM document_chunks dc
         JOIN documents d ON d.id = dc.document_id
         WHERE dc.tenant_id = $1 AND dc.content ILIKE $2
         ORDER BY dc.chunk_index
         LIMIT $3",
    )
    .bind(tenant_id.0)
    .bind(&pattern)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows.into_iter().map(search_row_to_response).collect())
}
