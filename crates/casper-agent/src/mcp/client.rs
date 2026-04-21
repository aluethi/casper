use super::types::{JsonRpcRequest, JsonRpcResponse, McpError, McpToolDef};
use serde_json::json;
use tokio::sync::OnceCell;
use tracing::debug;

/// Client for communicating with an MCP (Model Context Protocol) server.
///
/// MCP uses JSON-RPC 2.0 over HTTP. The server requires an `initialize`
/// handshake before any other calls. The returned `Mcp-Session-Id` header
/// must be sent on all subsequent requests.
pub struct McpClient {
    url: String,
    auth_token: Option<String>,
    http_client: reqwest::Client,
    next_id: std::sync::atomic::AtomicU64,
    /// Session ID obtained from the `initialize` handshake.
    session_id: OnceCell<String>,
}

impl McpClient {
    /// Create a new MCP client.
    ///
    /// - `url`: Base URL of the MCP server (e.g. `http://localhost:8080/mcp`).
    /// - `auth_token`: Optional bearer token for authentication.
    /// - `http_client`: A shared `reqwest::Client` instance.
    pub fn new(
        url: impl Into<String>,
        auth_token: Option<String>,
        http_client: reqwest::Client,
    ) -> Self {
        Self {
            url: url.into(),
            auth_token,
            http_client,
            next_id: std::sync::atomic::AtomicU64::new(1),
            session_id: OnceCell::new(),
        }
    }

    /// Return the base URL this client is configured to talk to.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Perform the MCP `initialize` handshake and capture the session ID.
    async fn ensure_initialized(&self) -> Result<&str, McpError> {
        self.session_id
            .get_or_try_init(|| async {
                let id = self
                    .next_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                let request = JsonRpcRequest {
                    jsonrpc: "2.0",
                    id,
                    method: "initialize",
                    params: json!({
                        "protocolVersion": "2025-03-26",
                        "capabilities": {},
                        "clientInfo": {
                            "name": "casper",
                            "version": "0.1.0"
                        }
                    }),
                };

                let mut http_req = self.http_client.post(&self.url).json(&request);
                if let Some(ref token) = self.auth_token {
                    http_req = http_req.bearer_auth(token);
                }

                let http_resp = http_req.send().await?;

                let sid = http_resp
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
                    .unwrap_or_default();

                let rpc_resp: JsonRpcResponse = http_resp.json().await?;

                if let Some(err) = rpc_resp.error {
                    return Err(McpError::JsonRpc {
                        code: err.code,
                        message: err.message,
                    });
                }

                debug!(session_id = %sid, "MCP session initialized");
                Ok(sid)
            })
            .await
            .map(|s| s.as_str())
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
        self.call_tool_with_auth(name, input, None).await
    }

    /// Call a tool with an optional auth token override (for per-user OAuth tokens).
    pub async fn call_tool_with_auth(
        &self,
        name: &str,
        input: serde_json::Value,
        auth_override: Option<&str>,
    ) -> Result<serde_json::Value, McpError> {
        let params = json!({
            "name": name,
            "arguments": input,
        });

        let response = self
            .rpc_call_with_auth("tools/call", params, auth_override)
            .await?;
        debug!(tool = name, "MCP tool call completed");
        Ok(response)
    }

    /// Send a JSON-RPC 2.0 request to the MCP server and return the result.
    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        self.rpc_call_with_auth(method, params, None).await
    }

    /// Send a JSON-RPC 2.0 request with an optional auth token override.
    async fn rpc_call_with_auth(
        &self,
        method: &str,
        params: serde_json::Value,
        auth_override: Option<&str>,
    ) -> Result<serde_json::Value, McpError> {
        self.ensure_initialized().await?;

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

        // Auth override takes precedence (for per-user OAuth tokens)
        if let Some(token) = auth_override {
            http_req = http_req.bearer_auth(token);
        } else if let Some(ref token) = self.auth_token {
            http_req = http_req.bearer_auth(token);
        }

        // Attach session ID from the initialize handshake
        if let Some(sid) = self.session_id.get()
            && !sid.is_empty()
        {
            http_req = http_req.header("Mcp-Session-Id", sid);
        }

        let http_resp = http_req.send().await?;
        let rpc_resp: JsonRpcResponse = http_resp.json().await?;

        if let Some(err) = rpc_resp.error {
            return Err(McpError::JsonRpc {
                code: err.code,
                message: err.message,
            });
        }

        rpc_resp.result.ok_or_else(|| {
            McpError::InvalidResponse("response has neither result nor error".into())
        })
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
        let client = McpClient::new("http://mcp.example.com", None, reqwest::Client::new());
        assert_eq!(client.url(), "http://mcp.example.com");
        assert!(client.auth_token.is_none());
    }
}
