/// MCP client, tool discovery, OAuth 2.1, elicitation forwarding.
pub mod client;
pub mod oauth;
pub mod types;

pub use client::McpClient;
pub use types::{McpError, McpToolDef};
