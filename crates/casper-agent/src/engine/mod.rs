//! Agent engine: drives the ReAct (Reason + Act) loop.
//!
//! The engine:
//! 1. Loads agent configuration from the database
//! 2. Assembles the prompt (system blocks + conversation history)
//! 3. Calls the LLM (via casper-catalog routing + casper-proxy dispatch)
//! 4. If the LLM returns tool_use blocks: dispatches tools, appends results, loops
//! 5. If the LLM returns end_turn: returns the final response
//! 6. Enforces a maximum number of turns to prevent infinite loops

mod helpers;
pub mod llm;

use std::sync::Arc;
use std::time::Instant;

use casper_base::CasperError;
use casper_observe::{AuditWriter, UsageRecorder};
use casper_proxy::{LlmRequest, Message, MessageRole, StreamEvent};
use serde_json::json;
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::actor::{AgentResponse, AgentUsage};
use crate::tools::{ToolContext, ToolDispatcher};

pub use llm::{LlmCaller, RealLlmCaller};
#[cfg(test)]
pub use llm::MockLlmCaller;

/// Maximum number of ReAct loop iterations before we bail out.
const DEFAULT_MAX_TURNS: usize = 25;

// ── Agent configuration row from DB ──────────────────────────────

/// Agent configuration loaded from the database.
#[derive(Debug)]
struct AgentConfig {
    pub deployment_slug: String,
    pub description: String,
    pub prompt_stack: serde_json::Value,
    pub tools: serde_json::Value,
    pub config: serde_json::Value,
    pub tenant_name: String,
    pub system_prompt: String,
    pub max_turns: i32,
    pub max_tokens: i32,
    pub temperature: f64,
}

/// Row type for the agent config query.
type AgentConfigRow = (String, Option<String>, serde_json::Value, serde_json::Value, serde_json::Value);

// ── Agent engine ─────────────────────────────────────────────────

/// The agent engine drives the ReAct cycle.
pub struct AgentEngine {
    pub db: PgPool,
    pub http_client: reqwest::Client,
    pub tool_dispatcher: ToolDispatcher,
    pub llm_caller: Arc<dyn LlmCaller>,
    pub audit_writer: Option<AuditWriter>,
    pub usage_recorder: Option<UsageRecorder>,
    /// Current delegation depth (0 = top-level call).
    delegation_depth: u32,
}

impl AgentEngine {
    /// Create an engine with the real LLM caller.
    pub fn new(
        db: PgPool,
        http_client: reqwest::Client,
        tool_dispatcher: ToolDispatcher,
        audit_writer: Option<AuditWriter>,
        usage_recorder: Option<UsageRecorder>,
    ) -> Self {
        let llm_caller = Arc::new(RealLlmCaller {
            db: db.clone(),
            http_client: http_client.clone(),
        });
        Self {
            db,
            http_client,
            tool_dispatcher,
            llm_caller,
            audit_writer,
            usage_recorder,
            delegation_depth: 0,
        }
    }

    /// Create an engine with a custom LLM caller (for testing).
    #[cfg(test)]
    pub fn with_caller(
        db: PgPool,
        tool_dispatcher: ToolDispatcher,
        llm_caller: Arc<dyn LlmCaller>,
    ) -> Self {
        Self {
            db: db.clone(),
            http_client: reqwest::Client::new(),
            tool_dispatcher,
            llm_caller,
            audit_writer: None,
            usage_recorder: None,
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
        let dynamic_dispatcher = crate::tools::build_dispatcher(
            &config.tools,
            &self.http_client,
        ).await;
        // Use the dynamically built dispatcher if it has tools, otherwise fall back
        // to the pre-built one (allows callers to pre-register tools if needed).
        let active_dispatcher = if !dynamic_dispatcher.is_empty() {
            &dynamic_dispatcher
        } else {
            &self.tool_dispatcher
        };

        // 3. Assemble system prompt (after MCP discovery so tool docs include MCP tools)
        let mcp_summaries = active_dispatcher.mcp_tool_summaries();
        config.system_prompt = crate::prompt::assemble_system_prompt(
            &config.prompt_stack,
            &config.tools,
            agent_name,
            &config.description,
            tenant_id,
            &config.tenant_name,
            &self.db,
            &mcp_summaries,
        ).await;
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

        // Convert history to messages
        let mut messages: Vec<Message> = history
            .into_iter()
            .filter_map(|h| {
                let role: MessageRole = serde_json::from_value(
                    serde_json::Value::String(h.role.clone()),
                ).ok()?;
                Some(Message {
                    role,
                    content: h.content,
                })
            })
            .collect();

        // Append the new user message
        messages.push(Message {
            role: MessageRole::User,
            content: serde_json::Value::String(user_message.to_string()),
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

            // Build the LLM request
            let request = LlmRequest {
                messages: messages.clone(),
                model: config.deployment_slug.clone(),
                max_tokens: Some(config.max_tokens),
                temperature: Some(config.temperature),
                stream: false,
                tools: if tool_defs.is_empty() {
                    None
                } else {
                    Some(tool_defs.clone())
                },
                extra: json!({
                    "system": system_prompt,
                }),
            };

            // Call LLM
            let (response, backend_id) = self.llm_caller.call(tenant_id, &request).await?;

            // Accumulate usage
            usage.input_tokens += response.input_tokens;
            usage.output_tokens += response.output_tokens;
            usage.cache_read_tokens += response.cache_read_tokens.unwrap_or(0);
            usage.cache_write_tokens += response.cache_write_tokens.unwrap_or(0);
            usage.llm_calls += 1;

            // Record usage event
            self.record_usage(
                tenant_id,
                agent_name,
                &config.deployment_slug,
                &response,
                backend_id,
                correlation_id,
            )
            .await;

            // Check for tool calls
            let has_tools = response
                .tool_calls
                .as_ref()
                .is_some_and(|tc| !tc.is_empty());

            if has_tools {
                let tool_calls = response.tool_calls.as_ref().unwrap();
                let step_thinking = response.thinking.clone();

                // Build assistant message in OpenAI format:
                // { role: "assistant", content: text|null, tool_calls: [...] }
                let assistant_content = if response.content.is_empty() {
                    serde_json::Value::Null
                } else {
                    json!(response.content)
                };
                let assistant_msg = Message {
                    role: MessageRole::Assistant,
                    content: json!({
                        "content": assistant_content,
                        "tool_calls": tool_calls,
                    }),
                };
                messages.push(assistant_msg.clone());

                // Store assistant message
                self.store_message(
                    tenant_id,
                    conversation_id,
                    "assistant",
                    &assistant_msg.content,
                    agent_name,
                )
                .await?;

                // Execute each tool call (OpenAI format):
                // { id: "call_123", type: "function", function: { name: "...", arguments: "..." } }
                let mut step_tool_calls: Vec<crate::actor::ToolCallStep> = Vec::new();

                for tc in tool_calls {
                    let func = &tc["function"];
                    let tool_name = func
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let tool_id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let tool_input: serde_json::Value = func
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(json!({}));

                    tracing::debug!(
                        tool = %tool_name,
                        tool_id = %tool_id,
                        "executing tool"
                    );

                    let mut tool_result = active_dispatcher
                        .dispatch(tool_name, tool_input.clone(), &tool_ctx)
                        .await;

                    // Intercept delegation sentinel — execute the child agent
                    if let Ok(ref result) = tool_result {
                        if result.content.get("__delegate__").and_then(|v| v.as_bool()) == Some(true) {
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

                            tool_result = Ok(self.execute_delegation(
                                target,
                                child_msg,
                                tenant_id,
                                agent_name,
                                correlation_id,
                                timeout_secs,
                                max_depth,
                            ).await);
                        }
                    }

                    usage.tool_calls += 1;

                    // Build OpenAI tool result message:
                    // { role: "tool", tool_call_id: "...", content: "..." }
                    let (result_str, is_error) = match &tool_result {
                        Ok(result) => {
                            if result.is_error {
                                (format!("Error: {}", result.content), true)
                            } else {
                                (serde_json::to_string(&result.content)
                                    .unwrap_or_else(|_| result.content.to_string()), false)
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

                    let tool_msg_content = json!({
                        "tool_call_id": tool_id,
                        "content": result_str,
                    });

                    // Add tool result as a message
                    let tool_msg = Message {
                        role: MessageRole::Tool,
                        content: tool_msg_content.clone(),
                    };
                    messages.push(tool_msg);

                    // Store tool result message
                    self.store_message(
                        tenant_id,
                        conversation_id,
                        "tool",
                        &tool_msg_content,
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
            let final_message = response.content.clone();

            // Capture final thinking (if any)
            if response.thinking.is_some() {
                steps.push(crate::actor::AgentStep {
                    thinking: response.thinking.clone(),
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
    pub async fn run_stream(
        &self,
        tenant_id: Uuid,
        agent_name: &str,
        conversation_id: Uuid,
        user_message: &str,
        author: &str,
        _metadata: &serde_json::Value,
        tx: mpsc::Sender<StreamEvent>,
        mut ask_rx: mpsc::Receiver<String>,
    ) -> Result<AgentResponse, CasperError> {
        let correlation_id = Uuid::now_v7();
        let start = Instant::now();

        // 1. Load config + build dispatcher (same as run)
        let mut config = self.load_agent_config(tenant_id, agent_name).await?;
        let max_turns = (config.max_turns as usize).min(DEFAULT_MAX_TURNS);

        let dynamic_dispatcher = crate::tools::build_dispatcher(
            &config.tools,
            &self.http_client,
        ).await;
        let active_dispatcher = if !dynamic_dispatcher.is_empty() {
            &dynamic_dispatcher
        } else {
            &self.tool_dispatcher
        };

        let mcp_summaries = active_dispatcher.mcp_tool_summaries();
        config.system_prompt = crate::prompt::assemble_system_prompt(
            &config.prompt_stack,
            &config.tools,
            agent_name,
            &config.description,
            tenant_id,
            &config.tenant_name,
            &self.db,
            &mcp_summaries,
        ).await;
        let system_prompt = config.system_prompt.clone();
        let tool_defs = active_dispatcher.tool_definitions();

        // Load history + append user message
        let history = crate::prompt::load_conversation_history(
            &self.db, tenant_id, conversation_id, 8000,
        ).await.map_err(|e| CasperError::Internal(format!("Failed to load history: {e}")))?;

        let mut messages: Vec<Message> = history
            .into_iter()
            .filter_map(|h| {
                let role: MessageRole = serde_json::from_value(
                    serde_json::Value::String(h.role.clone()),
                ).ok()?;
                Some(Message { role, content: h.content })
            })
            .collect();

        messages.push(Message {
            role: MessageRole::User,
            content: serde_json::Value::String(user_message.to_string()),
        });

        self.store_message(tenant_id, conversation_id, "user",
            &serde_json::Value::String(user_message.to_string()), author).await?;

        let tool_ctx = ToolContext {
            tenant_id, agent_name: agent_name.to_string(),
            conversation_id, correlation_id, db: self.db.clone(),
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

            let request = LlmRequest {
                messages: messages.clone(),
                model: config.deployment_slug.clone(),
                max_tokens: Some(config.max_tokens),
                temperature: Some(config.temperature),
                stream: true,
                tools: if tool_defs.is_empty() { None } else { Some(tool_defs.clone()) },
                extra: json!({ "system": system_prompt }),
            };

            // Stream LLM call — thinking and content deltas flow through tx
            let (response, backend_id) = self.llm_caller.call_stream(tenant_id, &request, tx.clone()).await?;

            usage.input_tokens += response.input_tokens;
            usage.output_tokens += response.output_tokens;
            usage.cache_read_tokens += response.cache_read_tokens.unwrap_or(0);
            usage.cache_write_tokens += response.cache_write_tokens.unwrap_or(0);
            usage.llm_calls += 1;

            self.record_usage(tenant_id, agent_name, &config.deployment_slug,
                &response, backend_id, correlation_id).await;

            let has_tools = response.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty());

            if has_tools {
                let tool_calls = response.tool_calls.as_ref().unwrap();
                let step_thinking = response.thinking.clone();

                let assistant_content = if response.content.is_empty() {
                    serde_json::Value::Null
                } else {
                    json!(response.content)
                };
                let assistant_msg = Message {
                    role: MessageRole::Assistant,
                    content: json!({ "content": assistant_content, "tool_calls": tool_calls }),
                };
                messages.push(assistant_msg.clone());
                self.store_message(tenant_id, conversation_id, "assistant",
                    &assistant_msg.content, agent_name).await?;

                let mut step_tool_calls: Vec<crate::actor::ToolCallStep> = Vec::new();

                for tc in tool_calls {
                    let func = &tc["function"];
                    let tool_name = func.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let tool_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let tool_input: serde_json::Value = func
                        .get("arguments")
                        .and_then(|v| v.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or(json!({}));

                    let mut tool_result = active_dispatcher
                        .dispatch(tool_name, tool_input.clone(), &tool_ctx).await;

                    // Handle delegation sentinel
                    if let Ok(ref result) = tool_result {
                        if result.content.get("__delegate__").and_then(|v| v.as_bool()) == Some(true) {
                            let target = result.content["agent"].as_str().unwrap_or("");
                            let child_msg = result.content["message"].as_str().unwrap_or("");
                            let delegate_cfg = config.tools["builtin"].as_array()
                                .and_then(|arr| arr.iter().find(|e| e["name"] == "delegate"));
                            let timeout_secs = delegate_cfg.and_then(|c| c["timeout_secs"].as_u64()).unwrap_or(300);
                            let max_depth = delegate_cfg.and_then(|c| c["max_depth"].as_u64()).unwrap_or(3) as u32;
                            tool_result = Ok(self.execute_delegation(
                                target, child_msg, tenant_id, agent_name,
                                correlation_id, timeout_secs, max_depth,
                            ).await);
                        }
                    }

                    // Handle ask_user sentinel — pause and wait for user input
                    if let Ok(ref result) = tool_result {
                        if result.content.get("__ask_user__").and_then(|v| v.as_bool()) == Some(true) {
                            let question = result.content["question"].as_str().unwrap_or("").to_string();
                            let options: Vec<String> = result.content["options"]
                                .as_array()
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                                .unwrap_or_default();

                            let question_id = Uuid::now_v7().to_string();

                            // Send the question to the client
                            let _ = tx.send(StreamEvent::AskUser {
                                question_id: question_id.clone(),
                                question: question.clone(),
                                options: options.clone(),
                            }).await;

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
                            ).await;

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

                            tool_result = Ok(crate::tools::ToolResult::ok(
                                serde_json::Value::String(answer_text),
                            ));
                        }
                    }

                    // Handle missing user_oauth connection — prompt to connect
                    if let Err(ref e) = tool_result {
                        let err_msg = e.to_string();
                        if err_msg.contains("has not connected") {
                            // Extract the provider name from the error
                            let provider = err_msg
                                .split('\'').nth(3)  // "...has not connected '<provider>'..."
                                .unwrap_or("unknown")
                                .to_string();

                            let _ = tx.send(StreamEvent::ConnectRequired {
                                provider: provider.clone(),
                                display_name: provider.clone(),
                            }).await;

                            tracing::info!(
                                agent = %agent_name,
                                provider = %provider,
                                "waiting for user to connect OAuth provider"
                            );

                            // Wait for the user to complete the OAuth flow
                            let connected = tokio::time::timeout(
                                std::time::Duration::from_secs(300),
                                ask_rx.recv(),
                            ).await;

                            match connected {
                                Ok(Some(_)) => {
                                    // Retry the tool call now that the user is connected
                                    tracing::info!(agent = %agent_name, provider = %provider, "user connected, retrying tool");
                                    tool_result = active_dispatcher
                                        .dispatch(tool_name, tool_input.clone(), &tool_ctx).await;
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
                    if let Ok(ref result) = tool_result {
                        if result.content.get("__mcp_oauth_required__").and_then(|v| v.as_bool()) == Some(true) {
                            let mcp_url = result.content["mcp_server_url"].as_str().unwrap_or("").to_string();

                            // Try to start the OAuth flow via the token resolver
                            let auth_url = if let (Some(user), Some(resolver)) =
                                (tool_ctx.invoking_user.as_deref(), tool_ctx.token_resolver.as_ref())
                            {
                                resolver.start_mcp_oauth_flow(tenant_id, user, &mcp_url).await.ok()
                            } else {
                                None
                            };

                            if let Some(auth_url) = auth_url {
                                let _ = tx.send(StreamEvent::McpOAuthRequired {
                                    mcp_server_url: mcp_url.clone(),
                                    authorization_url: auth_url,
                                }).await;

                                // Wait for user to complete OAuth
                                let connected = tokio::time::timeout(
                                    std::time::Duration::from_secs(300),
                                    ask_rx.recv(),
                                ).await;

                                match connected {
                                    Ok(Some(_)) => {
                                        tool_result = active_dispatcher
                                            .dispatch(tool_name, tool_input.clone(), &tool_ctx).await;
                                    }
                                    _ => {
                                        tool_result = Ok(crate::tools::ToolResult::error(
                                            "User did not complete MCP OAuth within the timeout."
                                        ));
                                    }
                                }
                            } else {
                                tool_result = Ok(crate::tools::ToolResult::error(format!(
                                    "MCP server at {mcp_url} requires OAuth but the flow could not be started."
                                )));
                            }
                        }
                    }

                    usage.tool_calls += 1;

                    let (result_str, is_error) = match &tool_result {
                        Ok(result) => {
                            if result.is_error {
                                (format!("Error: {}", result.content), true)
                            } else {
                                (serde_json::to_string(&result.content)
                                    .unwrap_or_else(|_| result.content.to_string()), false)
                            }
                        }
                        Err(e) => (format!("Tool error: {e}"), true),
                    };

                    // Stream tool result event
                    let _ = tx.send(StreamEvent::ToolResult {
                        id: tool_id.to_string(),
                        name: tool_name.to_string(),
                        content: result_str.clone(),
                        is_error,
                    }).await;

                    step_tool_calls.push(crate::actor::ToolCallStep {
                        name: tool_name.to_string(),
                        input: tool_input,
                        result: result_str.clone(),
                        is_error,
                    });

                    let tool_msg = Message {
                        role: MessageRole::Tool,
                        content: json!({ "tool_call_id": tool_id, "content": result_str }),
                    };
                    messages.push(tool_msg.clone());
                    self.store_message(tenant_id, conversation_id, "tool",
                        &tool_msg.content, agent_name).await?;
                }

                steps.push(crate::actor::AgentStep {
                    thinking: step_thinking, tool_calls: Some(step_tool_calls),
                });
                continue;
            }

            // Final response — content was already streamed via tx
            let final_message = response.content.clone();

            if response.thinking.is_some() {
                steps.push(crate::actor::AgentStep {
                    thinking: response.thinking.clone(), tool_calls: None,
                });
            }

            self.store_message(tenant_id, conversation_id, "assistant",
                &serde_json::Value::String(final_message.clone()), agent_name).await?;

            self.record_audit(tenant_id, author, agent_name, conversation_id,
                correlation_id, &usage);

            usage.duration_ms = start.elapsed().as_millis() as u64;

            // Send done event
            let _ = tx.send(StreamEvent::Done {
                conversation_id: conversation_id.to_string(),
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cache_read_tokens: Some(usage.cache_read_tokens),
                cache_write_tokens: Some(usage.cache_write_tokens),
            }).await;

            return Ok(AgentResponse {
                message: final_message, conversation_id, usage, correlation_id, steps,
            });
        }

        let _ = tx.send(StreamEvent::Error {
            message: format!("Agent '{agent_name}' exceeded maximum turns ({max_turns})"),
        }).await;
        Err(CasperError::Internal(format!(
            "Agent '{agent_name}' exceeded maximum turns ({max_turns})"
        )))
    }
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{Tool, ToolResult};
    use std::sync::Arc;

    /// A trivial echo tool for testing.
    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn description(&self) -> &str { "Echoes input back." }
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
        let caller = Arc::new(MockLlmCaller::simple("Hello, I'm the agent!"));

        let engine = AgentEngine::with_caller(pool, dispatcher, caller);

        // We can't actually run the full engine because it tries to load from DB,
        // but we can verify the mock caller works.
        let request = LlmRequest {
            messages: vec![Message {
                role: MessageRole::User,
                content: json!("Hi"),
            }],
            model: "test".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stream: false,
            tools: None,
            extra: json!({}),
        };

        let (response, backend_id) = engine.llm_caller.call(Uuid::nil(), &request).await.unwrap();
        assert_eq!(response.content, "Hello, I'm the agent!");
        assert_eq!(response.finish_reason.as_deref(), Some("end_turn"));
        assert!(backend_id.is_none());
    }

    #[tokio::test]
    async fn mock_with_tool_call() {
        let pool = test_pool();
        let mut dispatcher = ToolDispatcher::new();
        dispatcher.register(Arc::new(EchoTool));

        let caller = Arc::new(MockLlmCaller::with_tool_call(
            "echo",
            json!({"message": "test"}),
            "Done echoing!",
        ));

        let engine = AgentEngine::with_caller(pool, dispatcher, caller);

        // Verify the mock produces tool calls then a final response
        let request = LlmRequest {
            messages: vec![],
            model: "test".to_string(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stream: false,
            tools: None,
            extra: json!({}),
        };

        let (r1, _) = engine.llm_caller.call(Uuid::nil(), &request).await.unwrap();
        assert!(r1.tool_calls.is_some());
        assert_eq!(r1.finish_reason.as_deref(), Some("tool_use"));

        let (r2, _) = engine.llm_caller.call(Uuid::nil(), &request).await.unwrap();
        assert!(r2.tool_calls.is_none());
        assert_eq!(r2.content, "Done echoing!");
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
}
