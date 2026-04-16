use casper_base::{CasperError, TenantId};
use casper_db::TenantDb;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::helpers::to_rfc3339;

// ── Helpers ──────────────────────────────────────────────────────

/// Estimate token count: ~4 characters per token.
fn estimate_tokens(text: &str) -> i32 {
    (text.len() / 4).max(0) as i32
}

// ══════════════════════════════════════════════════════════════════
// Agent memory types
// ══════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct AgentMemoryResponse {
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub content: String,
    pub token_count: i32,
    pub version: i32,
    pub updated_at: String,
}

#[derive(sqlx::FromRow)]
struct AgentMemoryRow {
    tenant_id: Uuid,
    agent_name: String,
    content: String,
    token_count: i32,
    version: i32,
    updated_at: OffsetDateTime,
}

fn agent_memory_to_response(r: AgentMemoryRow) -> AgentMemoryResponse {
    AgentMemoryResponse {
        tenant_id: r.tenant_id,
        agent_name: r.agent_name,
        content: r.content,
        token_count: r.token_count,
        version: r.version,
        updated_at: to_rfc3339(r.updated_at),
    }
}

#[derive(Serialize)]
pub struct AgentMemoryHistoryResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub version: i32,
    pub content: String,
    pub token_count: i32,
    pub updated_by: String,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct AgentMemoryHistoryRow {
    id: Uuid,
    tenant_id: Uuid,
    agent_name: String,
    version: i32,
    content: String,
    token_count: i32,
    updated_by: String,
    created_at: OffsetDateTime,
}

fn agent_history_to_response(r: AgentMemoryHistoryRow) -> AgentMemoryHistoryResponse {
    AgentMemoryHistoryResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        agent_name: r.agent_name,
        version: r.version,
        content: r.content,
        token_count: r.token_count,
        updated_by: r.updated_by,
        created_at: to_rfc3339(r.created_at),
    }
}

// ══════════════════════════════════════════════════════════════════
// Tenant memory types
// ══════════════════════════════════════════════════════════════════

#[derive(Serialize)]
pub struct TenantMemoryResponse {
    pub tenant_id: Uuid,
    pub content: String,
    pub token_count: i32,
    pub version: i32,
    pub updated_at: String,
    pub updated_by: String,
}

#[derive(sqlx::FromRow)]
struct TenantMemoryRow {
    tenant_id: Uuid,
    content: String,
    token_count: i32,
    version: i32,
    updated_at: OffsetDateTime,
    updated_by: String,
}

fn tenant_memory_to_response(r: TenantMemoryRow) -> TenantMemoryResponse {
    TenantMemoryResponse {
        tenant_id: r.tenant_id,
        content: r.content,
        token_count: r.token_count,
        version: r.version,
        updated_at: to_rfc3339(r.updated_at),
        updated_by: r.updated_by,
    }
}

#[derive(Serialize)]
pub struct TenantMemoryHistoryResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub version: i32,
    pub content: String,
    pub token_count: i32,
    pub updated_by: String,
    pub created_at: String,
}

#[derive(sqlx::FromRow)]
struct TenantMemoryHistoryRow {
    id: Uuid,
    tenant_id: Uuid,
    version: i32,
    content: String,
    token_count: i32,
    updated_by: String,
    created_at: OffsetDateTime,
}

fn tenant_history_to_response(r: TenantMemoryHistoryRow) -> TenantMemoryHistoryResponse {
    TenantMemoryHistoryResponse {
        id: r.id,
        tenant_id: r.tenant_id,
        version: r.version,
        content: r.content,
        token_count: r.token_count,
        updated_by: r.updated_by,
        created_at: to_rfc3339(r.created_at),
    }
}

// ══════════════════════════════════════════════════════════════════
// Agent memory service functions
// ══════════════════════════════════════════════════════════════════

pub async fn get_agent_memory(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
) -> Result<AgentMemoryResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<AgentMemoryRow> = sqlx::query_as(
        "SELECT tenant_id, agent_name, content, token_count, version, updated_at
         FROM agent_memory
         WHERE tenant_id = $1 AND agent_name = $2",
    )
    .bind(tenant_id.0)
    .bind(name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound(format!("agent memory '{name}'")))?;
    Ok(agent_memory_to_response(r))
}

pub async fn update_agent_memory(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
    content: &str,
    actor: &str,
) -> Result<AgentMemoryResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let token_count = estimate_tokens(content);

    // Try to get existing memory to archive it
    let existing: Option<(String, i32, i32)> = sqlx::query_as(
        "SELECT content, token_count, version
         FROM agent_memory
         WHERE tenant_id = $1 AND agent_name = $2",
    )
    .bind(tenant_id.0)
    .bind(name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let new_version = match existing {
        Some((old_content, old_tokens, old_version)) => {
            // Archive current version to history
            let history_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO agent_memory_history (id, tenant_id, agent_name, version, content, token_count, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(history_id)
            .bind(tenant_id.0)
            .bind(name)
            .bind(old_version)
            .bind(&old_content)
            .bind(old_tokens)
            .bind(actor)
            .execute(&mut *tx)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error archiving memory: {e}")))?;

            old_version + 1
        }
        None => 1, // First version
    };

    // Upsert current memory
    let row: AgentMemoryRow = sqlx::query_as(
        "INSERT INTO agent_memory (tenant_id, agent_name, content, token_count, version, updated_at)
         VALUES ($1, $2, $3, $4, $5, now())
         ON CONFLICT (tenant_id, agent_name) DO UPDATE SET
            content = EXCLUDED.content,
            token_count = EXCLUDED.token_count,
            version = EXCLUDED.version,
            updated_at = now()
         RETURNING tenant_id, agent_name, content, token_count, version, updated_at",
    )
    .bind(tenant_id.0)
    .bind(name)
    .bind(content)
    .bind(token_count)
    .bind(new_version)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(agent_memory_to_response(row))
}

pub async fn list_agent_memory_history(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
) -> Result<Vec<AgentMemoryHistoryResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<AgentMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, version, content, token_count, updated_by, created_at
         FROM agent_memory_history
         WHERE tenant_id = $1 AND agent_name = $2
         ORDER BY version DESC",
    )
    .bind(tenant_id.0)
    .bind(name)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows.into_iter().map(agent_history_to_response).collect())
}

pub async fn get_agent_memory_version(
    db: &PgPool,
    tenant_id: TenantId,
    name: &str,
    version: i32,
) -> Result<AgentMemoryHistoryResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<AgentMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, agent_name, version, content, token_count, updated_by, created_at
         FROM agent_memory_history
         WHERE tenant_id = $1 AND agent_name = $2 AND version = $3",
    )
    .bind(tenant_id.0)
    .bind(name)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.ok_or_else(|| {
        CasperError::NotFound(format!("agent memory '{name}' version {version}"))
    })
    .map(agent_history_to_response)
}

// ══════════════════════════════════════════════════════════════════
// Tenant memory service functions
// ══════════════════════════════════════════════════════════════════

pub async fn get_tenant_memory(
    db: &PgPool,
    tenant_id: TenantId,
) -> Result<TenantMemoryResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<TenantMemoryRow> = sqlx::query_as(
        "SELECT tenant_id, content, token_count, version, updated_at, updated_by
         FROM tenant_memory
         WHERE tenant_id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let r = row.ok_or_else(|| CasperError::NotFound("tenant memory".to_string()))?;
    Ok(tenant_memory_to_response(r))
}

pub async fn update_tenant_memory(
    db: &PgPool,
    tenant_id: TenantId,
    content: &str,
    actor: &str,
) -> Result<TenantMemoryResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let token_count = estimate_tokens(content);

    // Get existing to archive
    let existing: Option<(String, i32, i32, String)> = sqlx::query_as(
        "SELECT content, token_count, version, updated_by
         FROM tenant_memory
         WHERE tenant_id = $1",
    )
    .bind(tenant_id.0)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let new_version = match existing {
        Some((old_content, old_tokens, old_version, old_updated_by)) => {
            // Archive current version to history
            let history_id = Uuid::now_v7();
            sqlx::query(
                "INSERT INTO tenant_memory_history (id, tenant_id, version, content, token_count, updated_by)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(history_id)
            .bind(tenant_id.0)
            .bind(old_version)
            .bind(&old_content)
            .bind(old_tokens)
            .bind(&old_updated_by)
            .execute(&mut *tx)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error archiving memory: {e}")))?;

            old_version + 1
        }
        None => 1,
    };

    // Upsert current tenant memory
    let row: TenantMemoryRow = sqlx::query_as(
        "INSERT INTO tenant_memory (tenant_id, content, token_count, version, updated_at, updated_by)
         VALUES ($1, $2, $3, $4, now(), $5)
         ON CONFLICT (tenant_id) DO UPDATE SET
            content = EXCLUDED.content,
            token_count = EXCLUDED.token_count,
            version = EXCLUDED.version,
            updated_at = now(),
            updated_by = EXCLUDED.updated_by
         RETURNING tenant_id, content, token_count, version, updated_at, updated_by",
    )
    .bind(tenant_id.0)
    .bind(content)
    .bind(token_count)
    .bind(new_version)
    .bind(actor)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(tenant_memory_to_response(row))
}

pub async fn list_tenant_memory_history(
    db: &PgPool,
    tenant_id: TenantId,
) -> Result<Vec<TenantMemoryHistoryResponse>, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let rows: Vec<TenantMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, version, content, token_count, updated_by, created_at
         FROM tenant_memory_history
         WHERE tenant_id = $1
         ORDER BY version DESC",
    )
    .bind(tenant_id.0)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    Ok(rows.into_iter().map(tenant_history_to_response).collect())
}

pub async fn get_tenant_memory_version(
    db: &PgPool,
    tenant_id: TenantId,
    version: i32,
) -> Result<TenantMemoryHistoryResponse, CasperError> {
    let tdb = TenantDb::new(db.clone(), tenant_id);
    let mut tx = tdb
        .begin()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let row: Option<TenantMemoryHistoryRow> = sqlx::query_as(
        "SELECT id, tenant_id, version, content, token_count, updated_by, created_at
         FROM tenant_memory_history
         WHERE tenant_id = $1 AND version = $2",
    )
    .bind(tenant_id.0)
    .bind(version)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    row.ok_or_else(|| CasperError::NotFound(format!("tenant memory version {version}")))
        .map(tenant_history_to_response)
}
