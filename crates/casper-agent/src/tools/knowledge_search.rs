//! `knowledge_search` tool: searches the agent's knowledge base using ILIKE.
//!
//! Performs a text-based search on `document_chunks` for the agent's tenant,
//! returning the most relevant chunks up to `max_results`.

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;
use uuid::Uuid;

use super::{Tool, ToolContext, ToolResult};

/// Built-in tool that searches the tenant's knowledge base.
pub struct KnowledgeSearchTool {
    /// Maximum number of results to return.
    pub max_results: i32,
    /// Minimum relevance threshold (0.0 - 1.0). Currently unused pending
    /// vector search; included for forward compatibility.
    pub relevance_threshold: f64,
}

impl KnowledgeSearchTool {
    pub fn new(max_results: i32, relevance_threshold: f64) -> Self {
        Self {
            max_results,
            relevance_threshold,
        }
    }

    /// Construct from a tools-config JSON entry.
    /// Expected keys: `max_results` (int), `relevance_threshold` (float).
    pub fn from_config(config: &serde_json::Value) -> Self {
        Self {
            max_results: config
                .get("max_results")
                .and_then(|v| v.as_i64())
                .unwrap_or(5) as i32,
            relevance_threshold: config
                .get("relevance_threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.7),
        }
    }
}

/// A single search result from document_chunks.
#[derive(Debug, sqlx::FromRow)]
struct ChunkRow {
    id: Uuid,
    document_id: Uuid,
    chunk_index: i32,
    content: String,
}

#[async_trait]
impl Tool for KnowledgeSearchTool {
    fn name(&self) -> &str {
        "knowledge_search"
    }

    fn description(&self) -> &str {
        "Search the knowledge base for relevant information. Returns text chunks \
         matching the query from ingested documents."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to find relevant knowledge."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CasperError::BadRequest("missing 'query' string".into()))?;

        if query.trim().is_empty() {
            return Ok(ToolResult::error("query must not be empty"));
        }

        // ILIKE search on document_chunks
        let pattern = format!("%{query}%");
        let rows: Vec<ChunkRow> = sqlx::query_as(
            "SELECT dc.id, dc.document_id, dc.chunk_index, dc.content
             FROM document_chunks dc
             JOIN documents d ON d.id = dc.document_id
             WHERE d.tenant_id = $1 AND dc.content ILIKE $2
             ORDER BY dc.chunk_index
             LIMIT $3",
        )
        .bind(ctx.tenant_id)
        .bind(&pattern)
        .bind(self.max_results)
        .fetch_all(&ctx.db)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error searching knowledge: {e}")))?;

        let results: Vec<serde_json::Value> = rows
            .into_iter()
            .map(|row| {
                json!({
                    "chunk_id": row.id.to_string(),
                    "document_id": row.document_id.to_string(),
                    "chunk_index": row.chunk_index,
                    "content": row.content,
                })
            })
            .collect();

        tracing::debug!(
            agent = %ctx.agent_name,
            query = %query,
            results = results.len(),
            "knowledge search completed"
        );

        Ok(ToolResult::ok(json!({
            "results": results,
            "total": results.len()
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = KnowledgeSearchTool::new(10, 0.5);
        assert_eq!(tool.name(), "knowledge_search");
        let schema = tool.parameters_schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"]["query"].is_object());
    }
}
