//! LLM caller trait: abstracts deployment resolution, quota checks, and dispatch.
//!
//! Implementations live in downstream crates (e.g. `casper-server`) where
//! concrete infrastructure (HTTP clients, WebSocket registries) is available.

use casper_base::CasperError;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::proxy::types::{LlmRequest, LlmResponse, StreamEvent};

/// Trait abstracting LLM calls so consumers (agent engine, inference service)
/// are decoupled from dispatch details (HTTP, WebSocket, mock).
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
        if let Some(ref thinking) = response.thinking {
            let _ = tx
                .send(StreamEvent::Thinking {
                    delta: thinking.clone(),
                })
                .await;
        }
        if !response.content.is_empty() {
            let _ = tx
                .send(StreamEvent::ContentDelta {
                    delta: response.content.clone(),
                })
                .await;
        }
        Ok((response, backend_id))
    }
}
