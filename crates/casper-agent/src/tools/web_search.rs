//! `web_search` tool: searches the web via SearXNG.
//!
//! Currently returns a stub/placeholder since SearXNG is not yet running.
//! The structure is in place for real integration.

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

/// Built-in tool that searches the web via a SearXNG instance.
pub struct WebSearchTool {
    /// URL of the SearXNG instance.
    pub searxng_url: String,
    /// Maximum number of results to return.
    pub max_results: i32,
}

impl WebSearchTool {
    pub fn new(searxng_url: String, max_results: i32) -> Self {
        Self {
            searxng_url,
            max_results,
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for current information. Returns a list of results \
         with titles, URLs, and snippets."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query."
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

        tracing::info!(
            agent = %ctx.agent_name,
            query = %query,
            searxng_url = %self.searxng_url,
            "web_search invoked (stub)"
        );

        // TODO: Call SearXNG API when available.
        // For now, return a stub indicating the tool is not yet wired.
        Ok(ToolResult::ok(json!({
            "results": [],
            "total": 0,
            "note": "Web search is not yet connected to a SearXNG instance. This is a placeholder response."
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = WebSearchTool::new("http://localhost:8080".into(), 5);
        assert_eq!(tool.name(), "web_search");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["query"].is_object());
    }
}
