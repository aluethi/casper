//! Prompt assembly: builds the system prompt from prompt-stack blocks
//! and generates the tool reference documentation.
//!
//! This module processes an agent's `prompt_stack` JSON into a fully-assembled
//! system prompt string. All blocks (including any safety preamble) are
//! configured in the agent's prompt stack — nothing is hardcoded.
//!
//! The tool reference section is auto-generated from the agent's tools config.

use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

use super::types::PromptBlock;

pub struct PromptContext<'a> {
    pub prompt_stack: &'a serde_json::Value,
    pub tools_config: &'a serde_json::Value,
    pub agent_name: &'a str,
    pub tenant_id: Uuid,
    pub tenant_name: &'a str,
    pub db: &'a PgPool,
    pub mcp_summaries: &'a HashMap<String, Vec<(String, String)>>,
}

pub async fn assemble_system_prompt(ctx: &PromptContext<'_>) -> String {
    let blocks: Vec<PromptBlock> =
        serde_json::from_value(ctx.prompt_stack.clone()).unwrap_or_default();
    let mut sections: Vec<String> = Vec::new();

    for block in &blocks {
        match block {
            PromptBlock::Text { content, .. } => {
                if !content.is_empty() {
                    sections.push(content.clone());
                }
            }
            PromptBlock::Environment { .. } => {
                let now_utc = chrono::Utc::now();
                let zurich: chrono_tz::Tz = "Europe/Zurich".parse().unwrap();
                let now_local = now_utc.with_timezone(&zurich);
                let tz_abbr = if now_local.format("%Z").to_string() == "CEST" {
                    "CEST"
                } else {
                    "CET"
                };
                let weekday = now_utc.format("%A");
                let day_month_year = now_utc.format("%-d %B %Y");
                let utc_time = now_utc.format("%H:%M");
                let local_time = now_local.format("%H:%M");
                let day_of_year = now_utc.format("%-j");
                let quarter = match now_utc.format("%m").to_string().parse::<u32>().unwrap_or(1) {
                    1..=3 => "Q1",
                    4..=6 => "Q2",
                    7..=9 => "Q3",
                    _ => "Q4",
                };
                let days_in_year = if now_utc
                    .format("%Y")
                    .to_string()
                    .parse::<i32>()
                    .unwrap_or(2026)
                    % 4
                    == 0
                {
                    366
                } else {
                    365
                };
                let datetime_str = format!(
                    "{weekday}, {day_month_year}, {utc_time} UTC ({local_time} {tz_abbr}, Europe/Zurich) — {quarter}, day {day_of_year}/{days_in_year}"
                );
                sections.push(format!(
                    "Current date/time: {datetime_str}\nAgent: {}\nTenant: {}",
                    ctx.agent_name, ctx.tenant_name
                ));
            }
            PromptBlock::TenantMemory { .. } => {
                let row: Option<(String,)> =
                    sqlx::query_as("SELECT content FROM tenant_memory WHERE tenant_id = $1")
                        .bind(ctx.tenant_id)
                        .fetch_optional(ctx.db)
                        .await
                        .ok()
                        .flatten();
                if let Some((content,)) = row
                    && !content.is_empty()
                {
                    sections.push(format!("## Shared Knowledge\n\n{content}"));
                }
            }
            PromptBlock::AgentMemory { .. } => {
                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT content FROM agent_memory WHERE tenant_id = $1 AND agent_name = $2",
                )
                .bind(ctx.tenant_id)
                .bind(ctx.agent_name)
                .fetch_optional(ctx.db)
                .await
                .ok()
                .flatten();
                if let Some((content,)) = row
                    && !content.is_empty()
                {
                    sections.push(format!("## Agent Memory\n\n{content}"));
                }
            }
            PromptBlock::Knowledge { .. } => {
                sections.push(
                    "## Knowledge Base Results\n\n\
                    (No results — the engine injects matching chunks here when the user's query\n\
                    triggers a knowledge_search at assembly time. If this section is empty, you\n\
                    should call `knowledge_search` explicitly before answering procedural or\n\
                    factual questions.)"
                        .to_string(),
                );
            }
            PromptBlock::Delegates { agents, .. } => {
                if !agents.is_empty() {
                    let mut s = String::from(
                        "## Available Agents\n\n\
                        You can hand off tasks to these agents using the `delegate` tool. Delegation is\n\
                        synchronous — you will wait for the target agent to return a result before\n\
                        continuing. If delegation times out (default: 5 minutes), you receive an error.\n\
                        In that case, inform the user and suggest an alternative.\n\n",
                    );
                    for agent in agents {
                        s.push_str(&format!(
                            "- **{}**: {}\n  {}\n\n",
                            agent.name, agent.description, agent.when
                        ));
                    }
                    sections.push(s);
                }
            }
            PromptBlock::Variable {
                key, value, label, ..
            } => {
                sections.push(format!("## {label}\n\n{key}: {value}"));
            }
            PromptBlock::Snippet { snippet_name, .. } => {
                let row: Option<(String,)> = sqlx::query_as(
                    "SELECT content FROM snippets WHERE tenant_id = $1 AND name = $2",
                )
                .bind(ctx.tenant_id)
                .bind(snippet_name.as_str())
                .fetch_optional(ctx.db)
                .await
                .ok()
                .flatten();
                if let Some((content,)) = row {
                    sections.push(content);
                }
            }
            PromptBlock::Datasource { .. } => {}
        }
    }

    let tool_doc = generate_tool_documentation(ctx.tools_config, ctx.mcp_summaries);
    if !tool_doc.is_empty() {
        sections.push(tool_doc);
    }

    sections.join("\n\n")
}

/// Generate tool documentation from the agent's tools config.
/// This is the "Tool Reference" block in the assembled prompt — it tells
/// the LLM what tools are available, how to call them, and their constraints.
///
/// `mcp_summaries` maps server name → `[(qualified_name, description)]` for
/// tools actually discovered from each MCP server at startup.
pub fn generate_tool_documentation(
    tools: &serde_json::Value,
    mcp_summaries: &HashMap<String, Vec<(String, String)>>,
) -> String {
    let builtin = tools.get("builtin").and_then(|v| v.as_array());
    let mcp = tools.get("mcp").and_then(|v| v.as_array());

    let has_tools = builtin.is_some_and(|b| !b.is_empty()) || mcp.is_some_and(|m| !m.is_empty());
    if !has_tools {
        return String::new();
    }

    let mut s = String::from(
        "## Tool Reference\n\n\
        Authentication for all tools is pre-configured. Do not run login \
        commands or attempt to handle authentication yourself. If a tool returns \
        `{\"error\": \"forbidden\"}`, you lack the required scope — report the error to \
        the user.\n\n",
    );

    if let Some(tools) = builtin {
        for tool in tools {
            let name = tool
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            match name {
                "knowledge_search" => {
                    let max = tool
                        .get("max_results")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(5);
                    s.push_str(&format!(
                        "### knowledge_search\n\
                        Search runbooks and documentation. **Use BEFORE answering factual or procedural \
                        questions** to ground your response.\n\n\
                        Usage: `knowledge_search({{ \"query\": \"<search terms>\", \"limit\": {max} }})`\n\n"
                    ));
                }
                "update_memory" => {
                    let max_tokens = tool
                        .get("max_document_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(4000);
                    s.push_str(&format!(
                        "### update_memory\n\
                        Replace your entire memory document with new content (max ~{max_tokens} tokens). \
                        This is a FULL REPLACEMENT — include everything you want to keep.\n\n\
                        Usage: `update_memory({{ \"content\": \"<full markdown document>\" }})`\n\n\
                        Use when you discover a new pattern, workaround, or known issue that \
                        will be useful in future conversations.\n\n"
                    ));
                }
                "web_search" => {
                    let max = tool
                        .get("max_results")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(10);
                    s.push_str(&format!(
                        "### web_search\n\
                        Search the web. Returns up to {max} results.\n\n\
                        Usage: `web_search({{ \"query\": \"<search query>\" }})`\n\n\
                        Use for current events, public documentation, or information not in the \
                        knowledge base.\n\n"
                    ));
                }
                "web_fetch" => {
                    let timeout = tool
                        .get("timeout_secs")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(30);
                    let max_bytes = tool
                        .get("max_response_bytes")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(1048576);
                    s.push_str(&format!(
                        "### web_fetch\n\
                        Fetch the content of a URL. Timeout: {timeout}s, max response: {}KB.\n\n\
                        Usage: `web_fetch({{ \"url\": \"<url>\" }})`\n\n\
                        Use to retrieve specific web pages, API documentation, or resources found \
                        via web_search.\n\n",
                        max_bytes / 1024
                    ));
                }
                "delegate" => {
                    s.push_str(
                        "### delegate\n\
                        Hand off a task to another agent and wait for the result.\n\n\
                        Usage: `delegate({{ \"agent\": \"<agent_name>\", \"message\": \"<task description>\" }})`\n\n\
                        Include all relevant context in the message — the target agent cannot see your \
                        conversation. See \"Available Agents\" above for valid targets.\n\n"
                    );
                }
                "ask_user" => {
                    s.push_str(
                        "### ask_user\n\
                        Ask the user a question and wait for their response. Use when you need \
                        clarification or confirmation before proceeding.\n\n\
                        Usage: `ask_user({{ \"question\": \"<text>\", \"options\": [\"A\", \"B\", \"C\"] }})`\n\n\
                        The `options` field is optional — omit it for free-form questions.\n\n"
                    );
                }
                other => {
                    s.push_str(&format!(
                        "### {other}\nBuilt-in tool. Refer to platform documentation for usage.\n\n"
                    ));
                }
            }
        }
    }

    if let Some(servers) = mcp {
        for server in servers {
            let server_name = server
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            s.push_str(&format!(
                "### MCP Server: {server_name}\n\
                External tools from this server are prefixed with `mcp__{server_name}__`. \
                Call them by their full qualified name with the parameters defined in their schema.\n\n"
            ));

            // Describe the auth mode so the LLM knows what to expect
            let auth_type = server
                .get("auth")
                .and_then(|a| a.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or(if server.get("api_key").is_some() {
                    "bearer"
                } else {
                    "none"
                });

            match auth_type {
                "user_oauth" => {
                    let provider = server
                        .get("auth")
                        .and_then(|a| a.get("provider"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("the provider");
                    s.push_str(&format!(
                        "**Authentication:** User OAuth ({provider}). These tools act as the current user. \
                        If the user has not connected their {provider} account, the platform will prompt them \
                        to sign in. You do not need to handle authentication — just call the tool and the \
                        platform handles the rest. If a tool returns a connection error, inform the user \
                        they need to connect their account.\n\n"
                    ));
                }
                "mcp_oauth" => {
                    s.push_str(
                        "**Authentication:** MCP OAuth 2.1 (auto-discovered). These tools act as the \
                        current user. When called for the first time, the platform will automatically \
                        prompt the user to authorize access via their browser. You do not need to handle \
                        authentication — just call the tool. If it fails with an auth error, tell the \
                        user the platform will prompt them to sign in.\n\n"
                    );
                }
                _ => {} // bearer and none need no special mention
            }

            if let Some(tools) = mcp_summaries.get(server_name) {
                s.push_str("Available tools:\n");
                for (qualified_name, description) in tools {
                    s.push_str(&format!("- **{qualified_name}**: {description}\n"));
                }
                s.push('\n');
            }
        }
    }

    s
}
