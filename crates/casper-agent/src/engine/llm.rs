//! LLM caller abstraction: trait + real and mock implementations.
//!
//! The `LlmCaller` trait abstracts LLM calls so the engine can be tested
//! with mock responses. The real implementation routes through
//! `casper-catalog` (deployment resolution, quota) and `casper-proxy`
//! (dispatch with retry/fallback).

use casper_base::CasperError;
use casper_proxy::{LlmRequest, LlmResponse, StreamEvent};
#[cfg(test)]
use casper_proxy::MessageRole;
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Trait abstracting LLM calls so we can mock in tests.
#[async_trait::async_trait]
pub trait LlmCaller: Send + Sync {
    async fn call(
        &self,
        tenant_id: Uuid,
        request: &LlmRequest,
    ) -> Result<(LlmResponse, Option<Uuid>), CasperError>;

    /// Streaming variant: sends events to `tx` and returns the accumulated response.
    /// Default implementation falls back to non-streaming.
    async fn call_stream(
        &self,
        tenant_id: Uuid,
        request: &LlmRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<(LlmResponse, Option<Uuid>), CasperError> {
        let (response, backend_id) = self.call(tenant_id, request).await?;
        // Emit the buffered response as stream events
        if let Some(ref thinking) = response.thinking {
            let _ = tx.send(StreamEvent::Thinking { delta: thinking.clone() }).await;
        }
        if !response.content.is_empty() {
            let _ = tx.send(StreamEvent::ContentDelta { delta: response.content.clone() }).await;
        }
        Ok((response, backend_id))
    }
}

/// Real LLM caller that uses casper-catalog + casper-proxy.
pub struct RealLlmCaller {
    pub db: PgPool,
    pub http_client: reqwest::Client,
}

#[async_trait::async_trait]
impl LlmCaller for RealLlmCaller {
    async fn call(
        &self,
        tenant_id: Uuid,
        request: &LlmRequest,
    ) -> Result<(LlmResponse, Option<Uuid>), CasperError> {
        let deployment =
            casper_catalog::resolve_deployment(&self.db, tenant_id, &request.model).await?;
        casper_catalog::check_quota(&self.db, tenant_id, deployment.model_id).await?;

        let merged_extra =
            casper_catalog::merge_params(&deployment.default_params, &request.extra);

        let mut patched_request = request.clone();
        patched_request.model = deployment.model_name.clone();
        patched_request.extra = merged_extra;

        let (response, backend) =
            casper_proxy::dispatch_with_retry(&self.http_client, &deployment, &patched_request)
                .await?;

        Ok((response, Some(backend.id)))
    }

    async fn call_stream(
        &self,
        tenant_id: Uuid,
        request: &LlmRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<(LlmResponse, Option<Uuid>), CasperError> {
        let deployment =
            casper_catalog::resolve_deployment(&self.db, tenant_id, &request.model).await?;
        casper_catalog::check_quota(&self.db, tenant_id, deployment.model_id).await?;

        let merged_extra =
            casper_catalog::merge_params(&deployment.default_params, &request.extra);

        let mut patched_request = request.clone();
        patched_request.model = deployment.model_name.clone();
        patched_request.extra = merged_extra;
        patched_request.stream = true;

        let (response, backend) =
            casper_proxy::dispatch_stream_with_retry(&self.http_client, &deployment, &patched_request, tx)
                .await?;

        Ok((response, Some(backend.id)))
    }
}

/// Mock LLM caller for testing. Returns canned responses.
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
pub struct MockLlmCaller {
    /// Responses to return, consumed in order.
    responses: std::sync::Mutex<Vec<LlmResponse>>,
}

#[cfg(test)]
impl MockLlmCaller {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
        }
    }

    /// Create a mock that returns a simple text response.
    pub fn simple(text: &str) -> Self {
        Self::new(vec![LlmResponse {
            content: text.to_string(),
            role: MessageRole::Assistant,
            model: "mock-model".to_string(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            tool_calls: None,
            finish_reason: Some("end_turn".to_string()),
            thinking: None,
        }])
    }

    /// Create a mock that first returns a tool call, then a text response.
    pub fn with_tool_call(tool_name: &str, tool_input: serde_json::Value, final_text: &str) -> Self {
        Self::new(vec![
            LlmResponse {
                content: String::new(),
                role: MessageRole::Assistant,
                model: "mock-model".to_string(),
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                tool_calls: Some(vec![json!({
                    "type": "tool_use",
                    "id": "call_001",
                    "name": tool_name,
                    "input": tool_input,
                })]),
                finish_reason: Some("tool_use".to_string()),
                thinking: None,
            },
            LlmResponse {
                content: final_text.to_string(),
                role: MessageRole::Assistant,
                model: "mock-model".to_string(),
                input_tokens: 150,
                output_tokens: 60,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                tool_calls: None,
                finish_reason: Some("end_turn".to_string()),
                thinking: None,
            },
        ])
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl LlmCaller for MockLlmCaller {
    async fn call(
        &self,
        _tenant_id: Uuid,
        _request: &LlmRequest,
    ) -> Result<(LlmResponse, Option<Uuid>), CasperError> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            return Err(CasperError::Internal("MockLlmCaller: no more responses".into()));
        }
        Ok((responses.remove(0), None))
    }
}
