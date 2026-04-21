//! Concrete LLM caller: resolves deployments, checks quotas, and dispatches
//! to HTTP backends (via casper-catalog) or WebSocket agent backends (via the
//! agent backend registry). Implements retry and fallback across the backend
//! sequence.

use std::sync::Arc;

use casper_base::CasperError;
use casper_catalog::{
    LlmCaller, LlmRequest, LlmResponse, ResolvedBackend, ResolvedDeployment, StreamEvent,
    is_non_retryable,
};
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::ws::AgentBackendRegistry;

pub struct RealLlmCaller {
    pub db: PgPool,
    pub http_client: reqwest::Client,
    pub agent_registry: Arc<AgentBackendRegistry>,
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

        let merged_extra = casper_catalog::merge_params(&deployment.default_params, &request.extra);

        let mut patched = request.clone();
        patched.model = deployment.model_name.clone();
        patched.extra = merged_extra;

        let (response, backend) =
            self.dispatch_with_retry(&deployment, &patched).await?;

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

        let merged_extra = casper_catalog::merge_params(&deployment.default_params, &request.extra);

        let mut patched = request.clone();
        patched.model = deployment.model_name.clone();
        patched.extra = merged_extra;
        patched.stream = true;

        let (response, backend) = self
            .dispatch_stream_with_retry(&deployment, &patched, tx)
            .await?;

        Ok((response, Some(backend.id)))
    }
}

impl RealLlmCaller {
    fn dispatch_single<'a>(
        &'a self,
        backend: &'a ResolvedBackend,
        request: &'a LlmRequest,
        timeout_ms: u64,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<LlmResponse, CasperError>> + Send + 'a>,
    > {
        Box::pin(async move {
            if backend.provider == "agent" {
                if !self.agent_registry.is_available(&backend.id) {
                    return Err(CasperError::Unavailable(format!(
                        "no agent backend connections for '{}'",
                        backend.name
                    )));
                }
                self.agent_registry
                    .dispatch(backend.id, request, timeout_ms)
                    .await
            } else {
                casper_catalog::dispatch(&self.http_client, backend, request).await
            }
        })
    }

    async fn dispatch_with_retry<'a>(
        &self,
        deployment: &'a ResolvedDeployment,
        request: &LlmRequest,
    ) -> Result<(LlmResponse, &'a ResolvedBackend), CasperError> {
        let mut last_error: Option<CasperError> = None;
        let timeout_ms = deployment.timeout_ms as u64;

        for (idx, backend) in deployment.backend_sequence.iter().enumerate() {
            tracing::debug!(
                backend_name = %backend.name,
                backend_provider = %backend.provider,
                idx,
                "attempting dispatch"
            );

            for attempt in 0..=deployment.retry_attempts {
                if attempt > 0 {
                    let delay_ms =
                        deployment.retry_backoff_ms as u64 * (1u64 << (attempt as u64 - 1));
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }

                match self.dispatch_single(backend, request, timeout_ms).await {
                    Ok(response) => return Ok((response, backend)),
                    Err(e) => {
                        tracing::warn!(
                            backend_name = %backend.name,
                            attempt,
                            error = %e,
                            "dispatch attempt failed"
                        );
                        if is_non_retryable(&e) {
                            return Err(e);
                        }
                        last_error = Some(e);
                    }
                }
            }

            if !deployment.fallback_enabled {
                break;
            }
        }

        Err(last_error
            .unwrap_or_else(|| CasperError::Unavailable("all backends exhausted".into())))
    }

    async fn dispatch_stream_with_retry<'a>(
        &self,
        deployment: &'a ResolvedDeployment,
        request: &LlmRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<(LlmResponse, &'a ResolvedBackend), CasperError> {
        let mut last_error: Option<CasperError> = None;
        let timeout_ms = deployment.timeout_ms as u64;

        for backend in &deployment.backend_sequence {
            if backend.provider == "agent" {
                // Agent backends don't support streaming yet; buffer and forward
                match self.dispatch_single(backend, request, timeout_ms).await {
                    Ok(response) => {
                        if !response.content.is_empty() {
                            let _ = tx
                                .send(StreamEvent::ContentDelta {
                                    delta: response.content.clone(),
                                })
                                .await;
                        }
                        return Ok((response, backend));
                    }
                    Err(e) => {
                        if is_non_retryable(&e) {
                            return Err(e);
                        }
                        last_error = Some(e);
                    }
                }
            } else {
                let (inner_tx, mut inner_rx) = mpsc::channel::<StreamEvent>(64);

                let outer_tx = tx.clone();
                let fwd_handle = tokio::spawn(async move {
                    let mut count = 0u64;
                    while let Some(event) = inner_rx.recv().await {
                        count += 1;
                        if outer_tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    count
                });

                match casper_catalog::dispatch_stream(
                    &self.http_client,
                    backend,
                    request,
                    inner_tx,
                )
                .await
                {
                    Ok(response) => {
                        let _ = fwd_handle.await;
                        return Ok((response, backend));
                    }
                    Err(e) => {
                        let events_sent = fwd_handle.await.unwrap_or(0);
                        if is_non_retryable(&e) || events_sent > 0 {
                            return Err(e);
                        }
                        last_error = Some(e);
                    }
                }
            }

            if !deployment.fallback_enabled {
                break;
            }
        }

        Err(last_error
            .unwrap_or_else(|| CasperError::Unavailable("all backends exhausted".into())))
    }
}
