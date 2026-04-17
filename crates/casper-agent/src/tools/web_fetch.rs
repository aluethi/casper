//! `web_fetch` tool: HTTP GET with size and timeout limits.
//!
//! Fetches a URL and returns the response body as text, respecting
//! configurable timeout and max response size.

use async_trait::async_trait;
use casper_base::CasperError;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

/// Built-in tool that fetches a URL via HTTP GET.
pub struct WebFetchTool {
    /// Shared HTTP client.
    pub http_client: reqwest::Client,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Maximum response body size in bytes.
    pub max_response_bytes: usize,
}

impl WebFetchTool {
    pub fn new(http_client: reqwest::Client, timeout_secs: u64, max_response_bytes: usize) -> Self {
        Self {
            http_client,
            timeout_secs,
            max_response_bytes,
        }
    }

    /// Construct from a tools-config JSON entry + a shared HTTP client.
    /// Expected keys: `timeout_secs` (int), `max_response_bytes` (int).
    pub fn from_config_with_client(config: &serde_json::Value, http_client: reqwest::Client) -> Self {
        Self {
            http_client,
            timeout_secs: config.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(30),
            max_response_bytes: config.get("max_response_bytes").and_then(|v| v.as_u64()).unwrap_or(1_048_576) as usize,
        }
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch the content of a web page by URL. Returns the response body as text. \
         Respects timeout and size limits."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch."
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CasperError::BadRequest("missing 'url' string".into()))?;

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(
                "URL must start with http:// or https://",
            ));
        }

        tracing::debug!(
            agent = %ctx.agent_name,
            url = %url,
            "web_fetch executing"
        );

        let response = self
            .http_client
            .get(url)
            .timeout(std::time::Duration::from_secs(self.timeout_secs))
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(url = %url, error = %e, "web_fetch request failed");
                CasperError::BadGateway(format!("Failed to fetch URL: {e}"))
            })?;

        let status = response.status().as_u16();

        if !response.status().is_success() {
            return Ok(ToolResult::error(format!(
                "HTTP {status} fetching {url}"
            )));
        }

        // Read body with size limit
        let content_length = response
            .content_length()
            .unwrap_or(0) as usize;

        if content_length > self.max_response_bytes {
            return Ok(ToolResult::error(format!(
                "Response too large: {content_length} bytes exceeds limit of {} bytes",
                self.max_response_bytes
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| CasperError::BadGateway(format!("Failed to read response body: {e}")))?;

        if bytes.len() > self.max_response_bytes {
            return Ok(ToolResult::error(format!(
                "Response too large: {} bytes exceeds limit of {} bytes",
                bytes.len(),
                self.max_response_bytes
            )));
        }

        let body = String::from_utf8_lossy(&bytes).to_string();

        Ok(ToolResult::ok(json!({
            "status": status,
            "url": url,
            "content": body,
            "bytes": bytes.len()
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_metadata() {
        let client = reqwest::Client::new();
        let tool = WebFetchTool::new(client, 30, 1_000_000);
        assert_eq!(tool.name(), "web_fetch");
        let schema = tool.parameters_schema();
        assert!(schema["properties"]["url"].is_object());
    }
}
