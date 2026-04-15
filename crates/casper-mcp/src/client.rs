use crate::types::{JsonRpcRequest, JsonRpcResponse, McpError, McpToolDef};
use serde_json::json;
use tracing::debug;

/// Client for communicating with an MCP (Model Context Protocol) server.
///
/// MCP uses JSON-RPC 2.0 over HTTP (with optional SSE streaming).
/// This V1 implementation covers the basic request/response flow;
/// SSE streaming will be added in a future iteration.
pub struct McpClient {
    url: String,
    auth_token: Option<String>,
    http_client: reqwest::Client,
    next_id: std::sync::atomic::AtomicU64,
}

impl McpClient {
    /// Create a new MCP client.
    ///
    /// - `url`: Base URL of the MCP server (e.g. `http://localhost:8080/mcp`).
    /// - `auth_token`: Optional bearer token for authentication.
    /// - `http_client`: A shared `reqwest::Client` instance.
    pub fn new(url: impl Into<String>, auth_token: Option<String>, http_client: reqwest::Client) -> Self {
        Self {
            url: url.into(),
            auth_token,
            http_client,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Return the base URL this client is configured to talk to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Discover available tools by calling `tools/list` on the MCP server.
    pub async fn discover_tools(&self) -> Result<Vec<McpToolDef>, McpError> {
        let response = self.rpc_call("tools/list", json!({})).await?;

        let tools_value = response
            .get("tools")
            .ok_or_else(|| McpError::InvalidResponse("missing 'tools' field in response".into()))?;

        let tools: Vec<McpToolDef> = serde_json::from_value(tools_value.clone())
            .map_err(|e| McpError::InvalidResponse(format!("failed to parse tools: {e}")))?;

        debug!(count = tools.len(), "discovered MCP tools");
        Ok(tools)
    }

    /// Call a tool on the MCP server.
    ///
    /// - `name`: The tool name (must match one from `discover_tools`).
    /// - `input`: JSON object of input parameters matching the tool's schema.
    pub async fn call_tool(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let params = json!({
            "name": name,
            "arguments": input,
        });

        let response = self.rpc_call("tools/call", params).await?;
        debug!(tool = name, "MCP tool call completed");
        Ok(response)
    }

    /// Send a JSON-RPC 2.0 request to the MCP server and return the result.
    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };

        let mut http_req = self.http_client.post(&self.url).json(&request);

        if let Some(ref token) = self.auth_token {
            http_req = http_req.bearer_auth(token);
        }

        let http_resp = http_req.send().await?;
        let rpc_resp: JsonRpcResponse = http_resp.json().await?;

        if let Some(err) = rpc_resp.error {
            return Err(McpError::JsonRpc {
                code: err.code,
                message: err.message,
            });
        }

        rpc_resp
            .result
            .ok_or_else(|| McpError::InvalidResponse("response has neither result nor error".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_stores_url_and_token() {
        let client = McpClient::new(
            "http://localhost:8080/mcp",
            Some("test-token".into()),
            reqwest::Client::new(),
        );
        assert_eq!(client.url(), "http://localhost:8080/mcp");
        assert_eq!(client.auth_token.as_deref(), Some("test-token"));
    }

    #[test]
    fn client_without_token() {
        let client = McpClient::new(
            "http://mcp.example.com",
            None,
            reqwest::Client::new(),
        );
        assert_eq!(client.url(), "http://mcp.example.com");
        assert!(client.auth_token.is_none());
    }
}
