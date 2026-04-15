//! Prompt assembler: builds the system prompt and conversation history
//! for an agent invocation by processing the agent's `prompt_stack` blocks.
//!
//! ## Architecture
//!
//! The prompt stack is an ordered array of typed blocks. Each block contributes
//! a section to the system prompt. The assembler processes blocks in order,
//! tracks token budgets, and loads dynamic content (memory, knowledge, etc.)
//! from the database.
//!
//! ## Block types
//!
//! - `text`          — Static instructions (markdown), supports template variables
//! - `environment`   — Runtime context (datetime, tenant, agent, source)
//! - `variable`      — Custom key-value pair injection
//! - `snippet`       — Reusable block from tenant's snippet library
//! - `agent_memory`  — Agent's versioned memory document
//! - `tenant_memory` — Shared tenant memory document
//! - `knowledge`     — RAG retrieval from knowledge base (budget-capped)
//! - `delegates`     — Available sub-agents with descriptions
//! - `datasource`    — External data fetched at assembly time (budget-capped)

pub mod assembler;
pub mod history;
pub mod types;

pub use assembler::assemble_system_prompt;
pub use history::load_conversation_history;
pub use types::*;
