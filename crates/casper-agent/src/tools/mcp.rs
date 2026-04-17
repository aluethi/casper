//! MCP tool wrapper: bridges `casper_mcp::McpClient` into the agent `Tool` trait.
//!
//! Each MCP server may expose many tools. After discovery, each tool is wrapped
//! in an `McpTool` struct and registered in the `ToolDispatcher` like any
//! built-in tool. The prefix `mcp_{server}_{tool}` avoids name collisions.

use std::sync::Arc;

use async_trait::async_trait;
use casper_base::CasperError;
use casper_mcp::McpClient;
use serde_json::json;

use super::{Tool, ToolContext, ToolResult};

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
}

impl McpTool {
    pub fn new(
        server_name: &str,
        remote_name: String,
        description: String,
        input_schema: serde_json::Value,
        client: Arc<McpClient>,
    ) -> Self {
        let qualified_name = format!("mcp__{server_name}__{remote_name}");
        Self {
            qualified_name,
            remote_name,
            description,
            input_schema,
            client,
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
        _ctx: &ToolContext,
    ) -> Result<ToolResult, CasperError> {
        tracing::debug!(
            tool = %self.qualified_name,
            remote = %self.remote_name,
            url = %self.client.url(),
            "calling MCP tool"
        );

        match self.client.call_tool(&self.remote_name, input).await {
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
        );
        assert_eq!(tool.parameters_schema(), schema);
    }
}
