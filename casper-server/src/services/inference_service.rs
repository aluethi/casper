use casper_base::UsageEvent;
use casper_base::scope::has_scope;
use casper_base::{CasperError, Scope};
use casper_catalog::{LlmRequest, Message, resolve_deployment, resolve_deployment_by_id};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;

/// Maximum deployment fallback chain depth to prevent cycles.
const MAX_FALLBACK_DEPTH: usize = 3;

// ── Request types ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ChatCompletionRequest {
    /// Deployment slug (e.g. "sonnet-fast").
    pub model: String,
    /// Chat messages.
    pub messages: Vec<serde_json::Value>,
    /// Maximum tokens to generate.
    #[serde(default)]
    pub max_tokens: Option<i32>,
    /// Sampling temperature.
    #[serde(default)]
    pub temperature: Option<f64>,
    /// Whether to stream (not yet implemented).
    #[serde(default)]
    pub stream: bool,
    /// Tool definitions.
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    /// Any other fields are captured here.
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

    let messages: Vec<Message> = req
        .messages
        .iter()
        .map(|m| {
            let role_str = m["role"].as_str().unwrap_or("user");
            let role: casper_catalog::MessageRole =
                serde_json::from_value(serde_json::Value::String(role_str.to_string()))
                    .unwrap_or(casper_catalog::MessageRole::User);
            let content = m.get("content").cloned().unwrap_or(serde_json::Value::Null);
            Message { role, content }
        })
        .collect();

    // Walk the deployment fallback chain.
    // Each call to llm_caller.call() handles backend-level retry/fallback internally.
    let mut current_slug = slug.clone();
    let mut last_error: Option<CasperError> = None;

    for depth in 0..=MAX_FALLBACK_DEPTH {
        let llm_request = LlmRequest {
            messages: messages.clone(),
            model: current_slug.clone(),
            max_tokens: req.max_tokens,
            temperature: req.temperature,
            stream: false,
            tools: req.tools.clone(),
            extra: req.extra.clone(),
        };

        match state.llm_caller.call(tenant_id, &llm_request).await {
            Ok((llm_response, backend_id)) => {
                let usage_event = UsageEvent {
                    tenant_id,
                    source: "inference".to_string(),
                    model: current_slug.clone(),
                    deployment_slug: Some(current_slug.clone()),
                    agent_name: None,
                    input_tokens: llm_response.input_tokens,
                    output_tokens: llm_response.output_tokens,
                    cache_read_tokens: llm_response.cache_read_tokens,
                    cache_write_tokens: llm_response.cache_write_tokens,
                    backend_id,
                    correlation_id,
                };
                let usage_recorder = state.usage.clone();
                tokio::spawn(async move {
                    if let Err(e) = usage_recorder.record(usage_event).await {
                        tracing::warn!(error = %e, "failed to record usage event");
                    }
                });

                let response_id = format!("chatcmpl-{}", Uuid::now_v7().simple());
                return Ok(ChatCompletionResponse {
                    id: response_id,
                    object: "chat.completion",
                    model: slug.clone(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChoiceMessage {
                            role: llm_response.role.to_string(),
                            content: if llm_response.content.is_empty() {
                                None
                            } else {
                                Some(llm_response.content.clone())
                            },
                            tool_calls: llm_response.tool_calls.clone(),
                            thinking: llm_response.thinking.clone(),
                        },
                        finish_reason: llm_response.finish_reason.clone(),
                    }],
                    usage: Usage {
                        prompt_tokens: llm_response.input_tokens,
                        completion_tokens: llm_response.output_tokens,
                        total_tokens: llm_response.input_tokens + llm_response.output_tokens,
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

                // Try fallback deployment if configured
                let deployment =
                    resolve_deployment(&state.db_owner, tenant_id, &current_slug).await?;
                if let Some(fallback_id) = deployment.fallback_deployment_id {
                    let fallback =
                        resolve_deployment_by_id(&state.db_owner, fallback_id).await?;
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

