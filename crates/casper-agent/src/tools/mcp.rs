//! MCP tool wrapper: bridges `crate::McpClient` into the agent `Tool` trait.
//!
//! Each MCP server may expose many tools. After discovery, each tool is wrapped
//! in an `McpTool` struct and registered in the `ToolDispatcher` like any
//! built-in tool. The prefix `mcp_{server}_{tool}` avoids name collisions.

use std::sync::Arc;

use async_trait::async_trait;
use casper_base::CasperError;
use crate::McpClient;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

/// MCP server auth configuration.
#[derive(Debug, Clone)]
pub enum McpAuth {
    /// No authentication.
    None,
    /// Static bearer token (from config or vault).
    Bearer,
    /// Per-user OAuth token — resolved at call time from user_connections.
    UserOAuth { provider: String },
    /// MCP OAuth 2.1 — auto-discovered via 401 probe + RFC 9728/8414/7591.
    McpOAuth,
}

/// A single tool discovered from an MCP server.
///
/// Holds a shared reference to the `McpClient` so all tools from the same
/// server share one HTTP connection pool and request-ID counter.
pub struct McpTool {
    /// Fully qualified name: `mcp__{server}__{tool}` (double underscore separator).
    qualified_name: String,
    /// The raw tool name as exposed by the MCP server (used in `tools/call`).
    remote_name: String,
    /// Human-readable description from the MCP server.
    description: String,
    /// JSON Schema for the tool's input parameters.
    input_schema: serde_json::Value,
    /// Shared MCP client for the server that owns this tool.
    client: Arc<McpClient>,
    /// Auth mode for this server.
    auth: McpAuth,
}

impl McpTool {
    pub fn new(
        server_name: &str,
        remote_name: String,
        description: String,
        input_schema: serde_json::Value,
        client: Arc<McpClient>,
        auth: McpAuth,
    ) -> Self {
        let qualified_name = format!("mcp__{server_name}__{remote_name}");
        Self {
            qualified_name,
            remote_name,
            description,
            input_schema,
            client,
            auth,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        tracing::debug!(
            tool = %self.qualified_name,
            remote = %self.remote_name,
            url = %self.client.url(),
            auth = ?self.auth,
            "calling MCP tool"
        );

        // Resolve auth token based on the auth mode
        let auth_token: Option<String> = match &self.auth {
            McpAuth::None | McpAuth::Bearer => None, // Bearer already set on the client
            McpAuth::UserOAuth { provider } => {
                let user = ctx.invoking_user.as_deref().ok_or_else(|| {
                    CasperError::Forbidden(format!(
                        "Tool '{}' requires user_oauth but no user context is available.",
                        self.qualified_name
                    ))
                })?;
                let resolver = ctx.token_resolver.as_ref().ok_or_else(|| {
                    CasperError::Internal("token_resolver not configured".into())
                })?;
                Some(resolver.resolve_user_oauth(ctx.tenant_id, user, provider).await?)
            }
            McpAuth::McpOAuth => {
                let user = ctx.invoking_user.as_deref().ok_or_else(|| {
                    CasperError::Forbidden(format!(
                        "Tool '{}' requires MCP OAuth but no user context is available.",
                        self.qualified_name
                    ))
                })?;
                let resolver = ctx.token_resolver.as_ref().ok_or_else(|| {
                    CasperError::Internal("token_resolver not configured".into())
                })?;
                match resolver.resolve_mcp_oauth(ctx.tenant_id, user, self.client.url()).await {
                    Ok(token) => Some(token),
                    Err(e) if e.to_string().contains("has not connected") => {
                        // Signal the engine to start the MCP OAuth flow
                        return Ok(ToolResult::ok(json!({
                            "__mcp_oauth_required__": true,
                            "mcp_server_url": self.client.url(),
                        })));
                    }
                    Err(e) => return Err(e),
                }
            }
        };

        let result = self.client.call_tool_with_auth(
            &self.remote_name,
            input,
            auth_token.as_deref(),
        ).await;

        match result {
            Ok(result) => {
                // MCP tools/call returns { content: [...] } with content blocks.
                // Extract text content for the LLM.
                let content = if let Some(arr) = result.get("content").and_then(|v| v.as_array()) {
                    let texts: Vec<&str> = arr
                        .iter()
                        .filter_map(|block| {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                block.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if texts.is_empty() {
                        // Return the raw result if no text blocks
                        result
                    } else {
                        json!(texts.join("\n"))
                    }
                } else {
                    result
                };

                let is_error = content
                    .get("isError")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                Ok(ToolResult { content, is_error })
            }
            Err(e) => {
                tracing::warn!(
                    tool = %self.qualified_name,
                    error = %e,
                    "MCP tool call failed"
                );
                Ok(ToolResult::error(format!("MCP tool error: {e}")))
            }
        }
    }
}

/// Discover tools from an MCP server and return wrapped `McpTool` instances.
///
/// Returns an empty vec (with a warning) if the server is unreachable or
/// returns an error — this lets the agent start even if an MCP server is down.
pub async fn discover_and_wrap(
    server_name: &str,
    client: Arc<McpClient>,
    auth: McpAuth,
) -> Vec<McpTool> {
    match client.discover_tools().await {
        Ok(tool_defs) => {
            tracing::info!(
                server = server_name,
                count = tool_defs.len(),
                "discovered MCP tools"
            );
            tool_defs
                .into_iter()
                .map(|def| {
                    McpTool::new(
                        server_name,
                        def.name,
                        def.description,
                        def.input_schema,
                        Arc::clone(&client),
                        auth.clone(),
                    )
                })
                .collect()
        }
        Err(e) => {
            tracing::warn!(
                server = server_name,
                error = %e,
                "failed to discover MCP tools — server will be skipped"
            );
            Vec::new()
        }
    }
}

/// Resolve a user's OAuth access token from the user_connections table.
///
/// This is a lightweight read — it does NOT auto-refresh expired tokens.
/// The full refresh logic lives in `connection_service::resolve_user_token` on the server.
/// TODO: Add token refresh support here or call through to the server service.
async fn resolve_user_oauth_token(
    db: &sqlx::PgPool,
    tenant_id: casper_base::TenantId,
    user_subject: &str,
    provider: &str,
) -> Result<String, CasperError> {
    let row: Option<(String, Option<time::OffsetDateTime>)> = sqlx::query_as(
        "SELECT access_token_enc, token_expires_at
         FROM user_connections
         WHERE tenant_id = $1 AND user_subject = $2 AND provider = $3",
    )
    .bind(tenant_id.0)
    .bind(user_subject)
    .bind(provider)
    .fetch_optional(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (access_token_enc, expires_at) = row.ok_or_else(|| {
        CasperError::Forbidden(format!(
            "User '{user_subject}' has not connected '{provider}'. \
             They need to connect it in Settings > Connections."
        ))
    })?;

    // Check expiry
    if let Some(ea) = expires_at {
        if ea < time::OffsetDateTime::now_utc() {
            return Err(CasperError::Forbidden(format!(
                "User '{user_subject}'s '{provider}' token has expired. \
                 They need to reconnect in Settings > Connections."
            )));
        }
    }

    // The token is encrypted — we need to decrypt it.
    // The vault is not available in the agent crate. For now, return the encrypted value
    // and let the caller handle decryption. In production, this should go through the
    // server's connection_service which has vault access.
    //
    // FIXME: The agent crate doesn't have vault access. Two options:
    // 1. Pass the vault into ToolContext (adds a dependency)
    // 2. Have the server pre-resolve tokens before building the dispatcher
    //
    // For now, we store the access_token in plaintext in a separate column,
    // or accept that the encrypted value needs vault decryption.
    // The pragmatic short-term solution: the access_token_enc is passed to the MCP server.
    // This won't work — the MCP server needs the raw token.
    //
    // Real solution: add vault to ToolContext via an Arc<dyn TokenResolver> trait.
    Err(CasperError::Internal(
        "user_oauth token resolution requires vault access — not yet wired into agent runtime. \
         Use the connection_service::resolve_user_token from the server layer instead.".into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_name_format() {
        let client = Arc::new(McpClient::new(
            "http://localhost:8080",
            None,
            reqwest::Client::new(),
        ));
        let tool = McpTool::new(
            "jira",
            "search_issues".into(),
            "Search Jira issues".into(),
            json!({"type": "object"}),
            client,
            McpAuth::None,
        );
        assert_eq!(tool.name(), "mcp__jira__search_issues");
        assert_eq!(tool.description(), "Search Jira issues");
    }

    #[test]
    fn parameters_schema_passthrough() {
        let client = Arc::new(McpClient::new(
            "http://localhost:8080",
            None,
            reqwest::Client::new(),
        ));
        let schema = json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        });
        let tool = McpTool::new(
            "docs",
            "search".into(),
            "Search docs".into(),
            schema.clone(),
            client,
            McpAuth::Bearer,
        );
        assert_eq!(tool.parameters_schema(), schema);
    }
}
