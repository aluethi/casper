//! Agent engine: drives the ReAct (Reason + Act) loop.
//!
//! The engine:
//! 1. Loads agent configuration from the database
//! 2. Assembles the prompt (system blocks + conversation history)
//! 3. Calls the LLM (via casper-llm routing + dispatch)
//! 4. If the LLM returns tool_use blocks: dispatches tools, appends results, loops
//! 5. If the LLM returns end_turn: returns the final response
//! 6. Enforces a maximum number of turns to prevent infinite loops

mod helpers;

use std::sync::Arc;
use std::time::Instant;

use casper_base::CasperError;
use casper_base::{AuditWriter, UsageRecorder};
use casper_llm::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmMessage, LlmProvider, LlmRole,
    StopReason, TokenUsage,
};

use crate::stream_event::StreamEvent;
use futures::StreamExt;
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::actor::{AgentResponse, AgentUsage};
use crate::tools::{ToolContext, ToolDispatcher};

pub struct RunStreamRequest {
    pub tenant_id: Uuid,
    pub agent_name: String,
    pub conversation_id: Uuid,
    pub user_message: String,
    pub author: String,
    pub tx: mpsc::Sender<StreamEvent>,
    pub ask_rx: mpsc::Receiver<String>,
}

/// Maximum number of ReAct loop iterations before we bail out.
const DEFAULT_MAX_TURNS: usize = 25;

// ── Agent configuration row from DB ──────────────────────────────

/// Agent configuration loaded from the database.
#[derive(Debug)]
struct AgentConfig {
    pub deployment_slug: String,
    pub prompt_stack: serde_json::Value,
    pub tools: serde_json::Value,
    pub tenant_name: String,
    pub system_prompt: String,
    pub max_turns: i32,
    pub max_tokens: i32,
    pub temperature: f64,
}

/// Row type for the agent config query.
type AgentConfigRow = (
    String,            // deployment_slug
    serde_json::Value, // prompt_stack
    serde_json::Value, // tools
    serde_json::Value, // config
);

// ── Agent engine ─────────────────────────────────────────────────

/// The agent engine drives the ReAct cycle.
pub struct AgentEngine {
    pub db: PgPool,
    pub http_client: reqwest::Client,
    pub tool_dispatcher: ToolDispatcher,
    pub llm_provider: Arc<dyn LlmProvider>,
    pub audit_writer: Option<AuditWriter>,
    pub usage_recorder: Option<UsageRecorder>,
    delegation_depth: u32,
}

impl AgentEngine {
    pub fn new(
        db: PgPool,
        http_client: reqwest::Client,
        tool_dispatcher: ToolDispatcher,
        llm_provider: Arc<dyn LlmProvider>,
        audit_writer: Option<AuditWriter>,
        usage_recorder: Option<UsageRecorder>,
    ) -> Self {
        Self {
            db,
            http_client,
            tool_dispatcher,
            llm_provider,
            audit_writer,
            usage_recorder,
            delegation_depth: 0,
        }
    }

    /// Run the ReAct loop for a single user message.
    ///
    /// This is the main entry point called by the actor task.
    pub async fn run(
        &self,
        tenant_id: Uuid,
        agent_name: &str,
        conversation_id: Uuid,
        user_message: &str,
        author: &str,
        _metadata: &serde_json::Value,
    ) -> Result<AgentResponse, CasperError> {
        let correlation_id = Uuid::now_v7();
        let start = Instant::now();

        tracing::info!(
            agent = %agent_name,
            conversation_id = %conversation_id,
            correlation_id = %correlation_id,
            "starting ReAct loop"
        );

        // 1. Load agent config (without system prompt — needs MCP tool info first)
        let mut config = self.load_agent_config(tenant_id, agent_name).await?;
        let max_turns = (config.max_turns as usize).min(DEFAULT_MAX_TURNS);

        // 2. Build tool dispatcher from agent's tools config (registers built-in + MCP tools)
        let dynamic_dispatcher =
            crate::tools::build_dispatcher(&config.tools, &self.http_client).await;
        // Use the dynamically built dispatcher if it has tools, otherwise fall back
        // to the pre-built one (allows callers to pre-register tools if needed).
        let active_dispatcher = if !dynamic_dispatcher.is_empty() {
            &dynamic_dispatcher
        } else {
            &self.tool_dispatcher
        };

        // 3. Assemble system prompt (after MCP discovery so tool docs include MCP tools)
        let mcp_summaries = active_dispatcher.mcp_tool_summaries();
        config.system_prompt =
            crate::prompt::assemble_system_prompt(&crate::prompt::PromptContext {
                prompt_stack: &config.prompt_stack,
                tools_config: &config.tools,
                agent_name,
                tenant_id,
                tenant_name: &config.tenant_name,
                db: &self.db,
                mcp_summaries: &mcp_summaries,
            })
            .await;
        let system_prompt = config.system_prompt.clone();
        let tool_defs = active_dispatcher.tool_definitions();

        // Load conversation history
        let history = crate::prompt::load_conversation_history(
            &self.db,
            tenant_id,
            conversation_id,
            8000, // token budget for history
        )
        .await
        .map_err(|e| CasperError::Internal(format!("Failed to load history: {e}")))?;

        // Convert history to messages — parse stored JSON back into ContentBlock
        let mut messages: Vec<LlmMessage> = history
            .into_iter()
            .filter_map(|h| {
                let role = parse_role(&h.role)?;
                let content = parse_history_content(role.clone(), &h.content);
                Some(LlmMessage { role, content })
            })
            .collect();

        // Append the new user message
        messages.push(LlmMessage {
            role: LlmRole::User,
            content: vec![ContentBlock::Text {
                text: user_message.to_string(),
            }],
        });

        // Store the user message in the DB
        self.store_message(
            tenant_id,
            conversation_id,
            "user",
            &serde_json::Value::String(user_message.to_string()),
            author,
        )
        .await?;

        // 3. ReAct loop
        let tool_ctx = ToolContext {
            tenant_id,
            agent_name: agent_name.to_string(),
            conversation_id,
            correlation_id,
            db: self.db.clone(),
            invoking_user: Some(author.to_string()),
            token_resolver: None,
        };

        let mut usage = AgentUsage::default();
        let mut steps: Vec<crate::actor::AgentStep> = Vec::new();

        for turn in 0..max_turns {
            tracing::debug!(
                agent = %agent_name,
                turn,
                messages = messages.len(),
                "ReAct turn"
            );

            // Build the LLM request — system prompt is prepended as the first message
            let mut request_messages = vec![LlmMessage {
                role: LlmRole::System,
                content: vec![ContentBlock::Text {
                    text: system_prompt.clone(),
                }],
            }];
            request_messages.extend(messages.clone());

            let request = CompletionRequest {
                messages: request_messages,
                model: Some(config.deployment_slug.clone()),
                max_tokens: config.max_tokens as u32,
                temperature: config.temperature as f32,
                tools: tool_defs.clone(),
                stop_sequences: vec![],
                extra: None,
            };

            let response = self.llm_provider.complete(request).await?;

            usage.input_tokens += response.usage.input_tokens as i32;
            usage.output_tokens += response.usage.output_tokens as i32;
            usage.llm_calls += 1;

            self.record_usage(
                tenant_id,
                agent_name,
                &config.deployment_slug,
                &response,
                None,
                correlation_id,
            )
            .await;

            // Check for tool calls
            let has_tools = response.stop_reason == StopReason::ToolUse;

            if has_tools {
                let step_thinking = extract_thinking(&response);

                // Store the full assistant response (may contain text + tool_use blocks)
                let assistant_msg = LlmMessage {
                    role: LlmRole::Assistant,
                    content: response.content.clone(),
                };
                messages.push(assistant_msg);

                // Serialize content blocks for DB storage
                let assistant_content_json =
                    serde_json::to_value(&response.content).unwrap_or_default();
                self.store_message(
                    tenant_id,
                    conversation_id,
                    "assistant",
                    &assistant_content_json,
                    agent_name,
                )
                .await?;

                // Execute each tool call from the content blocks
                let mut step_tool_calls: Vec<crate::actor::ToolCallStep> = Vec::new();

                for block in &response.content {
                    let (tool_id, tool_name, tool_input) = match block {
                        ContentBlock::ToolUse { id, name, input } => {
                            (id.clone(), name.clone(), input.clone())
                        }
                        _ => continue,
                    };

                    tracing::debug!(
                        tool = %tool_name,
                        tool_id = %tool_id,
                        "executing tool"
                    );

                    let mut tool_result = active_dispatcher
                        .dispatch(&tool_name, tool_input.clone(), &tool_ctx)
                        .await;

                    // Intercept delegation sentinel — execute the child agent
                    if let Ok(ref result) = tool_result
                        && result.content.get("__delegate__").and_then(|v| v.as_bool())
                            == Some(true)
                    {
                        let target = result.content["agent"].as_str().unwrap_or("");
                        let child_msg = result.content["message"].as_str().unwrap_or("");

                        // Read timeout/max_depth from delegate tool config
                        let delegate_cfg = config.tools["builtin"]
                            .as_array()
                            .and_then(|arr| arr.iter().find(|e| e["name"] == "delegate"));
                        let timeout_secs = delegate_cfg
                            .and_then(|c| c["timeout_secs"].as_u64())
                            .unwrap_or(300);
                        let max_depth = delegate_cfg
                            .and_then(|c| c["max_depth"].as_u64())
                            .unwrap_or(3) as u32;

                        tool_result = Ok(self
                            .execute_delegation(&helpers::DelegationRequest {
                                target_agent: target,
                                message: child_msg,
                                tenant_id,
                                parent_agent: agent_name,
                                timeout_secs,
                                max_depth,
                            })
                            .await);
                    }

                    usage.tool_calls += 1;

                    let (result_str, is_error) = match &tool_result {
                        Ok(result) => {
                            if result.is_error {
                                (format!("Error: {}", result.content), true)
                            } else {
                                (
                                    serde_json::to_string(&result.content)
                                        .unwrap_or_else(|_| result.content.to_string()),
                                    false,
                                )
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                tool = %tool_name,
                                error = %e,
                                "tool execution failed"
                            );
                            (format!("Tool error: {e}"), true)
                        }
                    };

                    // Capture for steps
                    step_tool_calls.push(crate::actor::ToolCallStep {
                        name: tool_name.to_string(),
                        input: tool_input,
                        result: result_str.clone(),
                        is_error,
                    });

                    // Add tool result as a message
                    let tool_msg = LlmMessage {
                        role: LlmRole::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: tool_id.clone(),
                            content: result_str,
                            is_error,
                        }],
                    };
                    messages.push(tool_msg.clone());

                    // Store tool result message
                    let tool_content_json =
                        serde_json::to_value(&tool_msg.content).unwrap_or_default();
                    self.store_message(
                        tenant_id,
                        conversation_id,
                        "tool",
                        &tool_content_json,
                        agent_name,
                    )
                    .await?;
                }

                steps.push(crate::actor::AgentStep {
                    thinking: step_thinking,
                    tool_calls: Some(step_tool_calls),
                });

                // Continue the loop — next iteration will send results to LLM
                continue;
            }

            // No tool calls — this is the final response
            let final_message = extract_text(&response.content);

            // Capture final thinking (if any)
            let thinking_text = extract_thinking(&response);
            if thinking_text.is_some() {
                steps.push(crate::actor::AgentStep {
                    thinking: thinking_text,
                    tool_calls: None,
                });
            }

            // Store assistant response
            self.store_message(
                tenant_id,
                conversation_id,
                "assistant",
                &serde_json::Value::String(final_message.clone()),
                agent_name,
            )
            .await?;

            // Record audit
            self.record_audit(
                tenant_id,
                author,
                agent_name,
                conversation_id,
                correlation_id,
                &usage,
            );

            usage.duration_ms = start.elapsed().as_millis() as u64;

            tracing::info!(
                agent = %agent_name,
                turns = turn + 1,
                llm_calls = usage.llm_calls,
                tool_calls = usage.tool_calls,
                duration_ms = usage.duration_ms,
                "ReAct loop completed"
            );

            return Ok(AgentResponse {
                message: final_message,
                conversation_id,
                usage,
                correlation_id,
                steps,
            });
        }

        // Max turns exceeded
        let _duration_ms = start.elapsed().as_millis() as u64;
        Err(CasperError::Internal(format!(
            "Agent '{agent_name}' exceeded maximum turns ({max_turns})"
        )))
    }

    /// Streaming variant of `run`. Sends `StreamEvent`s through `tx` as the
    /// ReAct loop progresses: thinking, content deltas, tool calls, tool results.
    /// Returns the same `AgentResponse` as `run()` for DB persistence.
    pub async fn run_stream(&self, req: RunStreamRequest) -> Result<AgentResponse, CasperError> {
        let tenant_id = req.tenant_id;
        let conversation_id = req.conversation_id;
        let tx = req.tx;
        let mut ask_rx = req.ask_rx;
        let agent_name: &str = &req.agent_name;
        let user_message: &str = &req.user_message;
        let author: &str = &req.author;
        let correlation_id = Uuid::now_v7();
        let start = Instant::now();

        // 1. Load config + build dispatcher (same as run)
        let mut config = self.load_agent_config(tenant_id, agent_name).await?;
        let max_turns = (config.max_turns as usize).min(DEFAULT_MAX_TURNS);

        let dynamic_dispatcher =
            crate::tools::build_dispatcher(&config.tools, &self.http_client).await;
        let active_dispatcher = if !dynamic_dispatcher.is_empty() {
            &dynamic_dispatcher
        } else {
            &self.tool_dispatcher
        };

        let mcp_summaries = active_dispatcher.mcp_tool_summaries();
        config.system_prompt =
            crate::prompt::assemble_system_prompt(&crate::prompt::PromptContext {
                prompt_stack: &config.prompt_stack,
                tools_config: &config.tools,
                agent_name,
                tenant_id,
                tenant_name: &config.tenant_name,
                db: &self.db,
                mcp_summaries: &mcp_summaries,
            })
            .await;
        let system_prompt = config.system_prompt.clone();
        let tool_defs = active_dispatcher.tool_definitions();

        // Load history + append user message
        let history =
            crate::prompt::load_conversation_history(&self.db, tenant_id, conversation_id, 8000)
                .await
                .map_err(|e| CasperError::Internal(format!("Failed to load history: {e}")))?;

        let mut messages: Vec<LlmMessage> = history
            .into_iter()
            .filter_map(|h| {
                let role = parse_role(&h.role)?;
                let content = parse_history_content(role.clone(), &h.content);
                Some(LlmMessage { role, content })
            })
            .collect();

        messages.push(LlmMessage {
            role: LlmRole::User,
            content: vec![ContentBlock::Text {
                text: user_message.to_string(),
            }],
        });

        self.store_message(
            tenant_id,
            conversation_id,
            "user",
            &serde_json::Value::String(user_message.to_string()),
            author,
        )
        .await?;

        let tool_ctx = ToolContext {
            tenant_id,
            agent_name: agent_name.to_string(),
            conversation_id,
            correlation_id,
            db: self.db.clone(),
            invoking_user: Some(author.to_string()),
            token_resolver: None,
        };

        let mut usage = AgentUsage::default();
        let mut steps: Vec<crate::actor::AgentStep> = Vec::new();

        // 2. ReAct loop with streaming
        for turn in 0..max_turns {
            // Abort early if the client disconnected (receiver dropped)
            if tx.is_closed() {
                tracing::info!(agent = %agent_name, turn, "SSE client disconnected, aborting");
                return Err(CasperError::Internal("client disconnected".into()));
            }

            // Build the LLM request — system prompt is prepended as the first message
            let mut request_messages = vec![LlmMessage {
                role: LlmRole::System,
                content: vec![ContentBlock::Text {
                    text: system_prompt.clone(),
                }],
            }];
            request_messages.extend(messages.clone());

            let request = CompletionRequest {
                messages: request_messages,
                model: Some(config.deployment_slug.clone()),
                max_tokens: config.max_tokens as u32,
                temperature: config.temperature as f32,
                tools: tool_defs.clone(),
                stop_sequences: vec![],
                extra: None,
            };

            // Stream LLM response — forward events to SSE while accumulating
            let mut stream = self.llm_provider.complete_stream(request).await?;
            let mut content_blocks = Vec::new();
            let mut reasoning_blocks = Vec::new();

            while let Some(item) = stream.next().await {
                let block = item?;
                match &block {
                    ContentBlock::Text { text } => {
                        let _ = tx
                            .send(StreamEvent::ContentDelta {
                                delta: text.clone(),
                            })
                            .await;
                        content_blocks.push(block);
                    }
                    ContentBlock::Thinking { text } => {
                        let _ = tx
                            .send(StreamEvent::Thinking {
                                delta: text.clone(),
                            })
                            .await;
                        reasoning_blocks.push(block);
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let _ = tx
                            .send(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                            })
                            .await;
                        content_blocks.push(block);
                    }
                    _ => {
                        content_blocks.push(block);
                    }
                }
            }

            let has_tool_use = content_blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

            let response = CompletionResponse {
                content: content_blocks,
                reasoning: reasoning_blocks,
                stop_reason: if has_tool_use {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                },
                usage: TokenUsage::default(),
                model: String::new(),
                latency: std::time::Duration::ZERO,
            };

            usage.llm_calls += 1;

            self.record_usage(
                tenant_id,
                agent_name,
                &config.deployment_slug,
                &response,
                None,
                correlation_id,
            )
            .await;

            let has_tools = response.stop_reason == StopReason::ToolUse;

            if has_tools {
                let step_thinking = extract_thinking(&response);

                // Store the full assistant response (may contain text + tool_use blocks)
                let assistant_msg = LlmMessage {
                    role: LlmRole::Assistant,
                    content: response.content.clone(),
                };
                messages.push(assistant_msg);

                let assistant_content_json =
                    serde_json::to_value(&response.content).unwrap_or_default();
                self.store_message(
                    tenant_id,
                    conversation_id,
                    "assistant",
                    &assistant_content_json,
                    agent_name,
                )
                .await?;

                let mut step_tool_calls: Vec<crate::actor::ToolCallStep> = Vec::new();

                for block in &response.content {
                    let (tool_id, tool_name, tool_input) = match block {
                        ContentBlock::ToolUse { id, name, input } => {
                            (id.clone(), name.clone(), input.clone())
                        }
                        _ => continue,
                    };

                    let mut tool_result = active_dispatcher
                        .dispatch(&tool_name, tool_input.clone(), &tool_ctx)
                        .await;

                    // Handle delegation sentinel
                    if let Ok(ref result) = tool_result
                        && result.content.get("__delegate__").and_then(|v| v.as_bool())
                            == Some(true)
                    {
                        let target = result.content["agent"].as_str().unwrap_or("");
                        let child_msg = result.content["message"].as_str().unwrap_or("");
                        let delegate_cfg = config.tools["builtin"]
                            .as_array()
                            .and_then(|arr| arr.iter().find(|e| e["name"] == "delegate"));
                        let timeout_secs = delegate_cfg
                            .and_then(|c| c["timeout_secs"].as_u64())
                            .unwrap_or(300);
                        let max_depth = delegate_cfg
                            .and_then(|c| c["max_depth"].as_u64())
                            .unwrap_or(3) as u32;
                        tool_result = Ok(self
                            .execute_delegation(&helpers::DelegationRequest {
                                target_agent: target,
                                message: child_msg,
                                tenant_id,
                                parent_agent: agent_name,
                                timeout_secs,
                                max_depth,
                            })
                            .await);
                    }

                    // Handle ask_user sentinel — pause and wait for user input
                    if let Ok(ref result) = tool_result
                        && result.content.get("__ask_user__").and_then(|v| v.as_bool())
                            == Some(true)
                    {
                        let question = result.content["question"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let options: Vec<String> = result.content["options"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let question_id = Uuid::now_v7().to_string();

                        // Send the question to the client
                        let _ = tx
                            .send(StreamEvent::AskUser {
                                question_id: question_id.clone(),
                                question: question.clone(),
                                options: options.clone(),
                            })
                            .await;

                        tracing::info!(
                            agent = %agent_name,
                            question_id = %question_id,
                            question = %question,
                            "waiting for user input"
                        );

                        // Wait for the user's answer (5-minute timeout)
                        let answer = tokio::time::timeout(
                            std::time::Duration::from_secs(300),
                            ask_rx.recv(),
                        )
                        .await;

                        let answer_text = match answer {
                            Ok(Some(text)) => text,
                            Ok(None) => "User did not respond (channel closed).".to_string(),
                            Err(_) => "User did not respond within 5 minutes.".to_string(),
                        };

                        tracing::info!(
                            agent = %agent_name,
                            question_id = %question_id,
                            answer_len = answer_text.len(),
                            "received user input"
                        );

                        tool_result = Ok(crate::tools::ToolResult::ok(serde_json::Value::String(
                            answer_text,
                        )));
                    }

                    // Handle missing user_oauth connection — prompt to connect
                    if let Err(ref e) = tool_result {
                        let err_msg = e.to_string();
                        if err_msg.contains("has not connected") {
                            // Extract the provider name from the error
                            let provider = err_msg
                                .split('\'')
                                .nth(3) // "...has not connected '<provider>'..."
                                .unwrap_or("unknown")
                                .to_string();

                            let _ = tx
                                .send(StreamEvent::ConnectRequired {
                                    provider: provider.clone(),
                                    display_name: provider.clone(),
                                })
                                .await;

                            tracing::info!(
                                agent = %agent_name,
                                provider = %provider,
                                "waiting for user to connect OAuth provider"
                            );

                            // Wait for the user to complete the OAuth flow
                            let connected = tokio::time::timeout(
                                std::time::Duration::from_secs(300),
                                ask_rx.recv(),
                            )
                            .await;

                            match connected {
                                Ok(Some(_)) => {
                                    // Retry the tool call now that the user is connected
                                    tracing::info!(agent = %agent_name, provider = %provider, "user connected, retrying tool");
                                    tool_result = active_dispatcher
                                        .dispatch(&tool_name, tool_input.clone(), &tool_ctx)
                                        .await;
                                }
                                _ => {
                                    tool_result = Ok(crate::tools::ToolResult::error(format!(
                                        "User did not connect {provider} within the timeout."
                                    )));
                                }
                            }
                        }
                    }

                    // Handle MCP OAuth 2.1 required sentinel
                    if let Ok(ref result) = tool_result
                        && result
                            .content
                            .get("__mcp_oauth_required__")
                            .and_then(|v| v.as_bool())
                            == Some(true)
                    {
                        let mcp_url = result.content["mcp_server_url"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();

                        // Try to start the OAuth flow via the token resolver
                        let auth_url = if let (Some(user), Some(resolver)) = (
                            tool_ctx.invoking_user.as_deref(),
                            tool_ctx.token_resolver.as_ref(),
                        ) {
                            resolver
                                .start_mcp_oauth_flow(tenant_id, user, &mcp_url)
                                .await
                                .ok()
                        } else {
                            None
                        };

                        if let Some(auth_url) = auth_url {
                            let _ = tx
                                .send(StreamEvent::McpOAuthRequired {
                                    mcp_server_url: mcp_url.clone(),
                                    authorization_url: auth_url,
                                })
                                .await;

                            // Wait for user to complete OAuth
                            let connected = tokio::time::timeout(
                                std::time::Duration::from_secs(300),
                                ask_rx.recv(),
                            )
                            .await;

                            match connected {
                                Ok(Some(_)) => {
                                    tool_result = active_dispatcher
                                        .dispatch(&tool_name, tool_input.clone(), &tool_ctx)
                                        .await;
                                }
                                _ => {
                                    tool_result = Ok(crate::tools::ToolResult::error(
                                        "User did not complete MCP OAuth within the timeout.",
                                    ));
                                }
                            }
                        } else {
                            tool_result = Ok(crate::tools::ToolResult::error(format!(
                                "MCP server at {mcp_url} requires OAuth but the flow could not be started."
                            )));
                        }
                    }

                    usage.tool_calls += 1;

                    let (result_str, is_error) = match &tool_result {
                        Ok(result) => {
                            if result.is_error {
                                (format!("Error: {}", result.content), true)
                            } else {
                                (
                                    serde_json::to_string(&result.content)
                                        .unwrap_or_else(|_| result.content.to_string()),
                                    false,
                                )
                            }
                        }
                        Err(e) => (format!("Tool error: {e}"), true),
                    };

                    // Stream tool result event
                    let _ = tx
                        .send(StreamEvent::ToolResult {
                            id: tool_id.to_string(),
                            name: tool_name.to_string(),
                            content: result_str.clone(),
                            is_error,
                        })
                        .await;

                    step_tool_calls.push(crate::actor::ToolCallStep {
                        name: tool_name.to_string(),
                        input: tool_input,
                        result: result_str.clone(),
                        is_error,
                    });

                    let tool_msg = LlmMessage {
                        role: LlmRole::Tool,
                        content: vec![ContentBlock::ToolResult {
                            tool_use_id: tool_id.clone(),
                            content: result_str,
                            is_error,
                        }],
                    };
                    messages.push(tool_msg.clone());

                    let tool_content_json =
                        serde_json::to_value(&tool_msg.content).unwrap_or_default();
                    self.store_message(
                        tenant_id,
                        conversation_id,
                        "tool",
                        &tool_content_json,
                        agent_name,
                    )
                    .await?;
                }

                steps.push(crate::actor::AgentStep {
                    thinking: step_thinking,
                    tool_calls: Some(step_tool_calls),
                });
                continue;
            }

            // Final response — content was already streamed via tx
            let final_message = extract_text(&response.content);

            let thinking_text = extract_thinking(&response);
            if thinking_text.is_some() {
                steps.push(crate::actor::AgentStep {
                    thinking: thinking_text,
                    tool_calls: None,
                });
            }

            self.store_message(
                tenant_id,
                conversation_id,
                "assistant",
                &serde_json::Value::String(final_message.clone()),
                agent_name,
            )
            .await?;

            self.record_audit(
                tenant_id,
                author,
                agent_name,
                conversation_id,
                correlation_id,
                &usage,
            );

            usage.duration_ms = start.elapsed().as_millis() as u64;

            // Send done event
            let _ = tx
                .send(StreamEvent::Done {
                    conversation_id: conversation_id.to_string(),
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_read_tokens: Some(usage.cache_read_tokens),
                    cache_write_tokens: Some(usage.cache_write_tokens),
                })
                .await;

            return Ok(AgentResponse {
                message: final_message,
                conversation_id,
                usage,
                correlation_id,
                steps,
            });
        }

        let _ = tx
            .send(StreamEvent::Error {
                message: format!("Agent '{agent_name}' exceeded maximum turns ({max_turns})"),
            })
            .await;
        Err(CasperError::Internal(format!(
            "Agent '{agent_name}' exceeded maximum turns ({max_turns})"
        )))
    }
}

// ── Helpers for content block extraction ────────────────────────

/// Extract all text from content blocks, concatenated.
fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Extract thinking text from the response's reasoning blocks.
fn extract_thinking(response: &CompletionResponse) -> Option<String> {
    if response.reasoning.is_empty() {
        return None;
    }
    let text: String = response
        .reasoning
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Thinking { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() { None } else { Some(text) }
}

/// Parse a role string from the database into an `LlmRole`.
fn parse_role(role: &str) -> Option<LlmRole> {
    match role {
        "system" => Some(LlmRole::System),
        "user" => Some(LlmRole::User),
        "assistant" => Some(LlmRole::Assistant),
        "tool" => Some(LlmRole::Tool),
        _ => None,
    }
}

/// Parse stored history content (JSON) back into `Vec<ContentBlock>`.
///
/// The DB stores content as `serde_json::Value`. This function handles:
/// - Plain strings -> `[ContentBlock::Text { text }]`
/// - Arrays of ContentBlock (serialized by the new engine) -> deserialized directly
/// - Legacy assistant messages with `{ "content": ..., "tool_calls": [...] }`
/// - Legacy tool messages with `{ "tool_call_id": ..., "content": ... }`
fn parse_history_content(role: LlmRole, value: &serde_json::Value) -> Vec<ContentBlock> {
    // 1. Plain string — wrap in Text block
    if let Some(text) = value.as_str() {
        return vec![ContentBlock::Text {
            text: text.to_string(),
        }];
    }

    // 2. Array — try to deserialize as Vec<ContentBlock> (new format)
    if let Some(arr) = value.as_array() {
        if let Ok(blocks) = serde_json::from_value::<Vec<ContentBlock>>(value.clone())
            && !blocks.is_empty()
        {
            return blocks;
        }
        // Fallback: if it's an array but not ContentBlock, stringify it
        if !arr.is_empty() {
            return vec![ContentBlock::Text {
                text: value.to_string(),
            }];
        }
        return vec![ContentBlock::Text {
            text: String::new(),
        }];
    }

    // 3. Object — handle legacy formats
    if let Some(obj) = value.as_object() {
        match role {
            LlmRole::Assistant => {
                // Legacy: { "content": "...", "tool_calls": [...] }
                let mut blocks = Vec::new();

                // Extract text content
                if let Some(content_val) = obj.get("content") {
                    if let Some(text) = content_val.as_str() {
                        if !text.is_empty() {
                            blocks.push(ContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    } else if !content_val.is_null() {
                        let text = content_val.to_string();
                        if !text.is_empty() {
                            blocks.push(ContentBlock::Text { text });
                        }
                    }
                }

                // Extract tool calls from legacy format
                if let Some(tool_calls) = obj.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        // Legacy OpenAI format: { id, type, function: { name, arguments } }
                        if let Some(func) = tc.get("function") {
                            let name = func
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let input: serde_json::Value = func
                                .get("arguments")
                                .and_then(|v| v.as_str())
                                .and_then(|s| serde_json::from_str(s).ok())
                                .unwrap_or(serde_json::json!({}));
                            blocks.push(ContentBlock::ToolUse { id, name, input });
                        }
                        // Anthropic-style: { type: "tool_use", id, name, input }
                        else if tc.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            let id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let name = tc
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let input = tc.get("input").cloned().unwrap_or(serde_json::json!({}));
                            blocks.push(ContentBlock::ToolUse { id, name, input });
                        }
                    }
                }

                if blocks.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: value.to_string(),
                    });
                }
                return blocks;
            }
            LlmRole::Tool => {
                // Legacy: { "tool_call_id": "...", "content": "..." }
                if let Some(tool_call_id) = obj.get("tool_call_id").and_then(|v| v.as_str()) {
                    let content = obj
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return vec![ContentBlock::ToolResult {
                        tool_use_id: tool_call_id.to_string(),
                        content,
                        is_error: false,
                    }];
                }
            }
            _ => {}
        }
    }

    // Fallback: stringify whatever we got
    vec![ContentBlock::Text {
        text: value.to_string(),
    }]
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolResult};
    use casper_llm::MockLlmProvider;
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Duration;

    /// A trivial echo tool for testing.
    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echoes input back."
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            })
        }
        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: &crate::tools::ToolContext,
        ) -> Result<ToolResult, CasperError> {
            let msg = input.get("message").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolResult::ok(json!({ "echoed": msg })))
        }
    }

    fn test_pool() -> PgPool {
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://localhost/casper_test_nonexistent")
            .unwrap()
    }

    #[tokio::test]
    async fn mock_simple_response() {
        let pool = test_pool();
        let dispatcher = ToolDispatcher::new();
        let provider = Arc::new(MockLlmProvider::simple("Hello, I'm the agent!"));

        let engine = AgentEngine::new(
            pool,
            reqwest::Client::new(),
            dispatcher,
            provider,
            None,
            None,
        );

        let request = CompletionRequest {
            messages: vec![LlmMessage {
                role: LlmRole::User,
                content: vec![ContentBlock::Text {
                    text: "Hi".to_string(),
                }],
            }],
            model: Some("test".to_string()),
            max_tokens: 1024,
            temperature: 0.7,
            tools: vec![],
            stop_sequences: vec![],
            extra: None,
        };

        let response = engine.llm_provider.complete(request).await.unwrap();
        assert_eq!(extract_text(&response.content), "Hello, I'm the agent!");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[tokio::test]
    async fn mock_with_tool_call() {
        let pool = test_pool();
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Arc::new(EchoTool));

        let provider = Arc::new(MockLlmProvider::with_tool_call(
            "echo",
            json!({"message": "test"}),
            "Done echoing!",
        ));

        let engine = AgentEngine::new(
            pool,
            reqwest::Client::new(),
            dispatcher,
            provider,
            None,
            None,
        );

        let request = CompletionRequest {
            messages: vec![],
            model: Some("test".to_string()),
            max_tokens: 1024,
            temperature: 0.7,
            tools: vec![],
            stop_sequences: vec![],
            extra: None,
        };

        let r1 = engine.llm_provider.complete(request.clone()).await.unwrap();
        assert_eq!(r1.stop_reason, StopReason::ToolUse);
        assert!(
            r1.content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        );

        let r2 = engine.llm_provider.complete(request).await.unwrap();
        assert_eq!(r2.stop_reason, StopReason::EndTurn);
        assert_eq!(extract_text(&r2.content), "Done echoing!");
    }

    #[test]
    fn usage_accumulation() {
        let mut usage = AgentUsage::default();
        usage.input_tokens += 100;
        usage.output_tokens += 50;
        usage.llm_calls += 1;
        usage.tool_calls += 2;

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.llm_calls, 1);
        assert_eq!(usage.tool_calls, 2);
    }

    #[test]
    fn parse_role_works() {
        assert_eq!(parse_role("user"), Some(LlmRole::User));
        assert_eq!(parse_role("assistant"), Some(LlmRole::Assistant));
        assert_eq!(parse_role("tool"), Some(LlmRole::Tool));
        assert_eq!(parse_role("system"), Some(LlmRole::System));
        assert_eq!(parse_role("bogus"), None);
    }

    #[test]
    fn parse_history_content_plain_string() {
        let val = serde_json::Value::String("hello".to_string());
        let blocks = parse_history_content(LlmRole::User, &val);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0],
            ContentBlock::Text {
                text: "hello".to_string()
            }
        );
    }

    #[test]
    fn parse_history_content_new_format_array() {
        let blocks_json = serde_json::to_value(&vec![
            ContentBlock::Text {
                text: "thinking...".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "echo".to_string(),
                input: json!({"message": "hi"}),
            },
        ])
        .unwrap();

        let parsed = parse_history_content(LlmRole::Assistant, &blocks_json);
        assert_eq!(parsed.len(), 2);
        assert!(matches!(&parsed[0], ContentBlock::Text { text } if text == "thinking..."));
        assert!(matches!(&parsed[1], ContentBlock::ToolUse { name, .. } if name == "echo"));
    }

    #[test]
    fn parse_history_content_legacy_tool_result() {
        let val = json!({
            "tool_call_id": "call_123",
            "content": "result text"
        });
        let blocks = parse_history_content(LlmRole::Tool, &val);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(
            &blocks[0],
            ContentBlock::ToolResult { tool_use_id, content, is_error }
            if tool_use_id == "call_123" && content == "result text" && !is_error
        ));
    }

    #[test]
    fn parse_history_content_legacy_assistant_with_tool_calls() {
        let val = json!({
            "content": "Let me help.",
            "tool_calls": [{
                "id": "call_001",
                "type": "function",
                "function": {
                    "name": "search",
                    "arguments": "{\"query\": \"rust\"}"
                }
            }]
        });
        let blocks = parse_history_content(LlmRole::Assistant, &val);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "Let me help."));
        assert!(
            matches!(&blocks[1], ContentBlock::ToolUse { id, name, .. } if id == "call_001" && name == "search")
        );
    }

    #[test]
    fn extract_text_concatenates() {
        let blocks = vec![
            ContentBlock::Text {
                text: "Hello ".to_string(),
            },
            ContentBlock::ToolUse {
                id: "x".to_string(),
                name: "y".to_string(),
                input: json!({}),
            },
            ContentBlock::Text {
                text: "world".to_string(),
            },
        ];
        assert_eq!(extract_text(&blocks), "Hello world");
    }

    #[test]
    fn extract_thinking_from_reasoning() {
        let response = CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "answer".to_string(),
            }],
            reasoning: vec![ContentBlock::Thinking {
                text: "I need to think about this.".to_string(),
            }],
            stop_reason: StopReason::EndTurn,
            usage: casper_llm::TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            model: "test".to_string(),
            latency: Duration::from_millis(1),
        };
        assert_eq!(
            extract_thinking(&response),
            Some("I need to think about this.".to_string())
        );
    }

    #[test]
    fn extract_thinking_empty_reasoning() {
        let response = CompletionResponse {
            content: vec![ContentBlock::Text {
                text: "answer".to_string(),
            }],
            reasoning: vec![],
            stop_reason: StopReason::EndTurn,
            usage: casper_llm::TokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
            model: "test".to_string(),
            latency: Duration::from_millis(1),
        };
        assert_eq!(extract_thinking(&response), None);
    }
}
