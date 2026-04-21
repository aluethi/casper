//! `web_search` tool: searches the web via a SearXNG instance.
//!
//! Calls SearXNG's JSON API (`/search?format=json`) and returns
//! a condensed list of results (title, URL, snippet).

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

/// Built-in tool that searches the web via a SearXNG instance.
pub struct WebSearchTool {
    /// URL of the SearXNG instance (e.g. `https://search.arc126.io`).
    pub searxng_url: String,
    /// Maximum number of results to return to the LLM.
    pub max_results: i32,
    /// Shared HTTP client.
    pub http_client: reqwest::Client,
}

impl WebSearchTool {
    pub fn new(searxng_url: String, max_results: i32, http_client: reqwest::Client) -> Self {
        Self {
            searxng_url,
            max_results,
            http_client,
        }
    }

    /// Construct from a tools-config JSON entry + a shared HTTP client.
    /// Expected keys: `max_results` (int), `searxng_url` (string, falls back to env `SEARXNG_URL`).
    pub fn from_config(config: &serde_json::Value, http_client: reqwest::Client) -> Self {
        let url = config
            .get("searxng_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                std::env::var("SEARXNG_URL").unwrap_or_else(|_| "https://search.arc126.io".into())
            });
        Self {
            searxng_url: url,
            max_results: config
                .get("max_results")
                .and_then(|v| v.as_i64())
                .unwrap_or(10) as i32,
            http_client,
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
            "web_search executing"
        );

        let base = self.searxng_url.trim_end_matches('/');
        let url = format!("{base}/search");

        let response = self
            .http_client
            .get(&url)
            .query(&[("q", query), ("format", "json"), ("categories", "general")])
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "SearXNG request failed");
                CasperError::BadGateway(format!("SearXNG request failed: {e}"))
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Ok(ToolResult::error(format!("SearXNG returned HTTP {status}")));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CasperError::BadGateway(format!("Invalid SearXNG JSON: {e}")))?;

        // Extract results array and trim to max_results
        let raw_results = body
            .get("results")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let results: Vec<serde_json::Value> = raw_results
            .into_iter()
            .take(self.max_results as usize)
            .map(|r| {
                json!({
                    "title": r.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    "url": r.get("url").and_then(|v| v.as_str()).unwrap_or(""),
                    "snippet": r.get("content").and_then(|v| v.as_str()).unwrap_or(""),
                    "engine": r.get("engine").and_then(|v| v.as_str()).unwrap_or(""),
                })
            })
            .collect();

        let total = results.len();

        // Include suggestions if available
        let suggestions: Vec<String> = body
            .get("suggestions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        tracing::debug!(
            agent = %ctx.agent_name,
            query = %query,
            results = total,
            "web_search completed"
        );

        let mut result = json!({
            "results": results,
            "total": total,
        });

        if !suggestions.is_empty() {
            result["suggestions"] = json!(suggestions);
        }

        Ok(ToolResult::ok(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let tool = WebSearchTool::new("http://localhost:8080".into(), 5, reqwest::Client::new());
        assert_eq!(tool.name(), "web_search");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["query"].is_object());
    }
}
