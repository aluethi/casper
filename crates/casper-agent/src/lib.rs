/// Actors, ReAct loop, prompt assembler, built-in tools, delegation, memory.
pub mod actor;
pub mod engine;
pub mod mcp;
pub mod prompt;
pub mod reaper;
pub mod stream_event;
pub mod tools;

pub use actor::{ActorHandle, ActorKey, ActorMessage, ActorRegistry, AgentResponse, AgentUsage};
pub use engine::AgentEngine;
pub use mcp::{McpClient, McpError, McpToolDef};
pub use reaper::{ReaperConfig, start_reaper};
pub use stream_event::StreamEvent;
pub use tools::{ResolvedMcpConnection, Tool, ToolContext, ToolDispatcher, ToolResult};
