use axum::{Json, Router, extract::State, routing::{get, post}};
use casper_base::{CasperError, Scope};
use casper_base::scope::has_scope;
use casper_catalog::{check_quota, merge_params, resolve_deployment};
use casper_observe::UsageEvent;
use casper_proxy::{LlmRequest, Message, dispatch_with_retry};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;
use crate::auth::ScopeGuard;

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

// ── Handlers ──────────────────────────────────────────────────────

/// POST /v1/chat/completions — Proxy an LLM request through a deployment.
async fn chat_completions(
    State(state): State<AppState>,
    guard: ScopeGuard,
    Json(body): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, CasperError> {
    let slug = &body.model;
    let tenant_id = guard.0.tenant_id.0;

    // 1. Check scope: inference:{slug}:call
    let scope_str = format!("inference:{slug}:call");
    guard.require(&scope_str)?;

    if body.stream {
        return Err(CasperError::BadRequest(
            "streaming not yet supported; set stream=false".into(),
        ));
    }

    // 2. Resolve deployment
    let deployment =
        resolve_deployment(&state.db_owner, tenant_id, slug).await?;

    // 3. Check quota
    check_quota(&state.db_owner, tenant_id, deployment.model_id).await?;

    // 4. Build LlmRequest from ChatCompletionRequest + deployment.default_params
    let merged_extra = merge_params(&deployment.default_params, &body.extra);

    let messages: Vec<Message> = body
        .messages
        .iter()
        .map(|m| {
            let role = m["role"]
                .as_str()
                .unwrap_or("user")
                .to_string();
            let content = m.get("content").cloned().unwrap_or(serde_json::Value::Null);
            Message { role, content }
        })
        .collect();

    // Apply defaults for max_tokens and temperature if not in request but in defaults
    let max_tokens = body
        .max_tokens
        .or_else(|| {
            deployment.default_params["max_tokens"]
                .as_i64()
                .map(|v| v as i32)
        });

    let temperature = body
        .temperature
        .or_else(|| {
            deployment.default_params["temperature"].as_f64()
        });

    let llm_request = LlmRequest {
        messages,
        model: deployment.model_name.clone(),
        max_tokens,
        temperature,
        stream: false,
        tools: body.tools,
        extra: merged_extra,
    };

    // 5. Dispatch with retry/fallback
    let http_client = &state.http_client;
    let (llm_response, used_backend) =
        dispatch_with_retry(http_client, &deployment, &llm_request).await?;

    // 6. Record usage event (fire-and-forget)
    let usage_event = UsageEvent {
        tenant_id,
        source: "inference".to_string(),
        model: deployment.model_name.clone(),
        deployment_slug: Some(deployment.slug.clone()),
        agent_name: None,
        input_tokens: llm_response.input_tokens,
        output_tokens: llm_response.output_tokens,
        cache_read_tokens: llm_response.cache_read_tokens,
        cache_write_tokens: llm_response.cache_write_tokens,
        backend_id: Some(used_backend.id),
        correlation_id: guard.0.correlation_id.0,
    };

    let usage_recorder = state.usage.clone();
    tokio::spawn(async move {
        if let Err(e) = usage_recorder.record(usage_event).await {
            tracing::warn!(error = %e, "failed to record usage event");
        }
    });

    // 7. Build OpenAI-compatible response
    let response_id = format!("chatcmpl-{}", Uuid::now_v7().simple());

    let choice_message = ChoiceMessage {
        role: llm_response.role.clone(),
        content: if llm_response.content.is_empty() {
            None
        } else {
            Some(llm_response.content.clone())
        },
        tool_calls: llm_response.tool_calls.clone(),
    };

    let response = ChatCompletionResponse {
        id: response_id,
        object: "chat.completion",
        model: slug.clone(),
        choices: vec![Choice {
            index: 0,
            message: choice_message,
            finish_reason: llm_response.finish_reason.clone(),
        }],
        usage: Usage {
            prompt_tokens: llm_response.input_tokens,
            completion_tokens: llm_response.output_tokens,
            total_tokens: llm_response.input_tokens + llm_response.output_tokens,
        },
    };

    Ok(Json(response))
}

/// GET /v1/models — List models (deployments) accessible to the caller.
async fn list_models(
    State(state): State<AppState>,
    guard: ScopeGuard,
) -> Result<Json<ModelsListResponse>, CasperError> {
    let tenant_id = guard.0.tenant_id.0;

    // Determine which deployments the caller can see based on scopes.
    // If they have inference:call (two-part) → all active deployments
    // If they have inference:{slug}:call → only matching deployments
    let has_broad_scope = has_scope(
        &guard.0.scopes,
        &Scope::parse("inference:call").unwrap(),
    );

    type DeploymentSlugRow = (String, String); // (slug, provider)

    let rows: Vec<DeploymentSlugRow> = sqlx::query_as(
        "SELECT d.slug, m.provider
         FROM model_deployments d
         JOIN models m ON m.id = d.model_id
         WHERE d.tenant_id = $1 AND d.is_active = true AND m.is_active = true
         ORDER BY d.slug",
    )
    .bind(tenant_id)
    .fetch_all(&state.db_owner)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let data: Vec<ModelEntry> = rows
        .into_iter()
        .filter(|(slug, _provider)| {
            if has_broad_scope {
                return true;
            }
            // Check if caller has specific scope for this slug
            let scope_str = format!("inference:{slug}:call");
            if let Ok(scope) = Scope::parse(&scope_str) {
                has_scope(&guard.0.scopes, &scope)
            } else {
                false
            }
        })
        .map(|(slug, provider)| ModelEntry {
            id: slug,
            object: "model",
            owned_by: provider,
        })
        .collect();

    Ok(Json(ModelsListResponse {
        object: "list",
        data,
    }))
}

// ── Router ────────────────────────────────────────────────────────

pub fn inference_router() -> Router<AppState> {
    Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/models", get(list_models))
}
