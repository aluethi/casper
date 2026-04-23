use casper_base::UsageEvent;
use casper_base::scope::has_scope;
use casper_base::{CasperError, Scope};
use casper_llm::{
    CompletionRequest, ContentBlock, LlmMessage, LlmProvider, LlmRole, ToolDefinition,
};

use super::routing::{resolve_deployment, resolve_deployment_by_id};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;

const MAX_FALLBACK_DEPTH: usize = 3;

// ── Request types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<serde_json::Value>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// ── Response types (OpenAI-compatible) ────────────────────────────

#[derive(Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Serialize)]
pub struct Choice {
    pub index: i32,
    pub message: ChoiceMessage,
    pub finish_reason: Option<String>,
}

#[derive(Serialize)]
pub struct ChoiceMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

#[derive(Serialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

// ── Models list response (OpenAI-compatible) ──────────────────────

#[derive(Serialize)]
pub struct ModelsListResponse {
    pub object: &'static str,
    pub data: Vec<ModelEntry>,
}

#[derive(Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub object: &'static str,
    pub owned_by: String,
}

// ── Service functions ────────────────────────────────────────────

pub async fn chat_completions(
    state: &AppState,
    tenant_id: Uuid,
    _scopes: &[Scope],
    correlation_id: Uuid,
    req: &ChatCompletionRequest,
) -> Result<ChatCompletionResponse, CasperError> {
    let slug = &req.model;

    if req.stream {
        return Err(CasperError::BadRequest(
            "streaming not yet supported; set stream=false".into(),
        ));
    }

    let messages: Vec<LlmMessage> = req
        .messages
        .iter()
        .map(|m| {
            let role_str = m["role"].as_str().unwrap_or("user");
            let role = match role_str {
                "system" => LlmRole::System,
                "assistant" => LlmRole::Assistant,
                "tool" => LlmRole::Tool,
                _ => LlmRole::User,
            };
            let content = parse_openai_message_content(role_str, m);
            LlmMessage { role, content }
        })
        .collect();

    let tools: Vec<ToolDefinition> = req
        .tools
        .as_ref()
        .map(|ts| {
            ts.iter()
                .filter_map(|t| {
                    let func = t.get("function")?;
                    Some(ToolDefinition {
                        name: func["name"].as_str()?.to_string(),
                        description: func
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input_schema: func
                            .get("parameters")
                            .cloned()
                            .unwrap_or(json!({"type": "object"})),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut current_slug = slug.clone();
    let mut last_error: Option<CasperError> = None;

    for depth in 0..=MAX_FALLBACK_DEPTH {
        let completion_request = CompletionRequest {
            messages: messages.clone(),
            model: Some(current_slug.clone()),
            max_tokens: req.max_tokens.unwrap_or(4096) as u32,
            temperature: req.temperature.unwrap_or(0.7) as f32,
            tools: tools.clone(),
            stop_sequences: vec![],
            extra: None,
        };

        let provider = state.llm.for_tenant(tenant_id);
        match provider.complete(completion_request).await {
            Ok(response) => {
                let backend_id = None; // TODO: propagate from RoutedProvider if needed
                let input_tokens = response.usage.input_tokens as i32;
                let output_tokens = response.usage.output_tokens as i32;

                let usage_event = UsageEvent {
                    tenant_id,
                    source: "inference".to_string(),
                    model: current_slug.clone(),
                    deployment_slug: Some(current_slug.clone()),
                    agent_name: None,
                    input_tokens,
                    output_tokens,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    backend_id,
                    correlation_id,
                };
                let usage_recorder = state.usage.clone();
                tokio::spawn(async move {
                    if let Err(e) = usage_recorder.record(usage_event).await {
                        tracing::warn!(error = %e, "failed to record usage event");
                    }
                });

                let text_content: String = response
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                let tool_calls: Option<Vec<serde_json::Value>> = {
                    let tcs: Vec<serde_json::Value> = response
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolUse { id, name, input } => Some(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
                                }
                            })),
                            _ => None,
                        })
                        .collect();
                    if tcs.is_empty() { None } else { Some(tcs) }
                };

                let thinking: Option<String> = {
                    let t: String = response
                        .reasoning
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Thinking { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");
                    if t.is_empty() { None } else { Some(t) }
                };

                let finish_reason = match response.stop_reason {
                    casper_llm::StopReason::EndTurn => Some("stop".to_string()),
                    casper_llm::StopReason::ToolUse => Some("tool_calls".to_string()),
                    casper_llm::StopReason::MaxTokens => Some("length".to_string()),
                    casper_llm::StopReason::StopSequence => Some("stop".to_string()),
                };

                let response_id = format!("chatcmpl-{}", Uuid::now_v7().simple());
                return Ok(ChatCompletionResponse {
                    id: response_id,
                    object: "chat.completion",
                    model: slug.clone(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChoiceMessage {
                            role: "assistant".to_string(),
                            content: if text_content.is_empty() {
                                None
                            } else {
                                Some(text_content)
                            },
                            tool_calls,
                            thinking,
                        },
                        finish_reason,
                    }],
                    usage: Usage {
                        prompt_tokens: input_tokens,
                        completion_tokens: output_tokens,
                        total_tokens: input_tokens + output_tokens,
                    },
                });
            }
            Err(e) => {
                if matches!(
                    e,
                    CasperError::BadRequest(_)
                        | CasperError::Unauthorized
                        | CasperError::Forbidden(_)
                        | CasperError::NotFound(_)
                ) {
                    return Err(e);
                }

                tracing::warn!(
                    deployment_slug = %current_slug,
                    depth,
                    error = %e,
                    "deployment backends exhausted"
                );
                last_error = Some(e);

                let deployment =
                    resolve_deployment(&state.db_owner, tenant_id, &current_slug).await?;
                if let Some(fallback_id) = deployment.fallback_deployment_id {
                    let fallback = resolve_deployment_by_id(&state.db_owner, fallback_id).await?;
                    tracing::info!(
                        from = %current_slug,
                        to = %fallback.slug,
                        "falling back to next deployment"
                    );
                    current_slug = fallback.slug;
                } else {
                    break;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| CasperError::Unavailable("all deployments exhausted".into())))
}

fn parse_openai_message_content(role: &str, m: &serde_json::Value) -> Vec<ContentBlock> {
    if role == "tool" {
        let tool_call_id = m["tool_call_id"].as_str().unwrap_or("").to_string();
        let content = m["content"].as_str().unwrap_or("").to_string();
        return vec![ContentBlock::ToolResult {
            tool_use_id: tool_call_id,
            content,
            is_error: false,
        }];
    }

    if role == "assistant" {
        let mut blocks = Vec::new();
        if let Some(text) = m["content"].as_str()
            && !text.is_empty()
        {
            blocks.push(ContentBlock::Text {
                text: text.to_string(),
            });
        }
        if let Some(tool_calls) = m["tool_calls"].as_array() {
            for tc in tool_calls {
                let id = tc["id"].as_str().unwrap_or("").to_string();
                let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                let input: serde_json::Value = tc["function"]["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or(json!({}));
                blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }
        if !blocks.is_empty() {
            return blocks;
        }
    }

    let text = m["content"].as_str().unwrap_or("").to_string();
    vec![ContentBlock::Text { text }]
}

pub async fn list_models(
    db: &PgPool,
    tenant_id: Uuid,
    scopes: &[Scope],
) -> Result<ModelsListResponse, CasperError> {
    let has_broad_scope = has_scope(scopes, &Scope::parse("inference:call").unwrap());

    #[derive(sqlx::FromRow)]
    struct DeploymentSlugRow {
        slug: String,
        provider: String,
    }

    let rows: Vec<DeploymentSlugRow> = sqlx::query_as(
        "SELECT d.slug, m.provider
         FROM model_deployments d
         JOIN models m ON m.id = d.model_id
         WHERE d.tenant_id = $1 AND d.is_active = true AND m.is_active = true
         ORDER BY d.slug",
    )
    .bind(tenant_id)
    .fetch_all(db)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data: Vec<ModelEntry> = rows
        .into_iter()
        .filter(|r| {
            if has_broad_scope {
                return true;
            }
            let scope_str = format!("inference:{}:call", r.slug);
            if let Ok(scope) = Scope::parse(&scope_str) {
                has_scope(scopes, &scope)
            } else {
                false
            }
        })
        .map(|r| ModelEntry {
            id: r.slug,
            object: "model",
            owned_by: r.provider,
        })
        .collect();

    Ok(ModelsListResponse {
        object: "list",
        data,
    })
}
