use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use casper_base::CasperError;
use casper_llm::{CompletionRequest, CompletionResponse, ContentBlock, LlmProvider};
use futures::Stream;
use sqlx::PgPool;
use uuid::Uuid;

use super::routing::{ResolvedBackend, ResolvedDeployment, check_quota, resolve_deployment};
use crate::ws::AgentBackendRegistry;

// ── Provider factory ────────────────────────────────────────────

fn create_provider(
    client: reqwest::Client,
    backend: &ResolvedBackend,
    agent_registry: &Arc<AgentBackendRegistry>,
) -> Result<Box<dyn LlmProvider>, CasperError> {
    use casper_llm::proxy::anthropic::AnthropicProvider;
    use casper_llm::proxy::local::LocalLlmProvider;
    use casper_llm::proxy::openai::OpenAiProvider;

    match backend.provider.as_str() {
        "anthropic" => {
            let api_key = backend.api_key_enc.clone().unwrap_or_default();
            let base_url = require_base_url(backend)?;
            Ok(Box::new(AnthropicProvider::new(client, base_url, api_key)))
        }
        "azure_openai" => {
            let api_key = backend.api_key_enc.clone().unwrap_or_default();
            let base_url = require_base_url(backend)?;
            Ok(Box::new(OpenAiProvider::azure(client, base_url, api_key)))
        }
        "openai" | "openai_compatible" => {
            let api_key = backend.api_key_enc.clone().unwrap_or_default();
            let base_url = require_base_url(backend)?;
            Ok(Box::new(OpenAiProvider::standard(
                client, base_url, api_key,
            )))
        }
        "agent" => {
            let backend_id = backend.id;
            let registry = Arc::clone(agent_registry);

            if !registry.is_available(&backend_id) {
                return Err(CasperError::Unavailable(format!(
                    "no agent backend connections for '{}'",
                    backend.name
                )));
            }

            Ok(Box::new(LocalLlmProvider::new(move |request_json| {
                let registry = Arc::clone(&registry);
                Box::pin(async move { registry.dispatch_json(backend_id, request_json).await })
            })))
        }
        other => Err(CasperError::Internal(format!(
            "unsupported provider: {other}"
        ))),
    }
}

fn require_base_url(backend: &ResolvedBackend) -> Result<String, CasperError> {
    backend.base_url.clone().ok_or_else(|| {
        CasperError::Internal(format!(
            "backend '{}' has no base_url configured",
            backend.name
        ))
    })
}

pub fn is_non_retryable(err: &CasperError) -> bool {
    matches!(
        err,
        CasperError::BadRequest(_)
            | CasperError::Unauthorized
            | CasperError::Forbidden(_)
            | CasperError::NotFound(_)
    )
}

// ── Routed provider (shared, tenant-agnostic) ───────────────────

pub struct RoutedProvider {
    pub db: PgPool,
    pub http_client: reqwest::Client,
    pub agent_registry: Arc<AgentBackendRegistry>,
}

impl RoutedProvider {
    pub fn for_tenant(self: &Arc<Self>, tenant_id: Uuid) -> TenantProvider {
        TenantProvider {
            inner: Arc::clone(self),
            tenant_id,
        }
    }

    async fn dispatch_single(
        &self,
        backend: &ResolvedBackend,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, CasperError> {
        let provider = create_provider(self.http_client.clone(), backend, &self.agent_registry)?;
        provider.complete(request.clone()).await
    }

    async fn dispatch_with_retry<'a>(
        &self,
        deployment: &'a ResolvedDeployment,
        request: &CompletionRequest,
    ) -> Result<(CompletionResponse, &'a ResolvedBackend), CasperError> {
        let mut last_error: Option<CasperError> = None;

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
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                match self.dispatch_single(backend, request).await {
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

        Err(last_error.unwrap_or_else(|| CasperError::Unavailable("all backends exhausted".into())))
    }

    async fn dispatch_stream_with_fallback(
        &self,
        deployment: &ResolvedDeployment,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>
    {
        let mut last_error: Option<CasperError> = None;

        for backend in &deployment.backend_sequence {
            let provider =
                match create_provider(self.http_client.clone(), backend, &self.agent_registry) {
                    Ok(p) => p,
                    Err(e) => {
                        last_error = Some(e);
                        if !deployment.fallback_enabled {
                            break;
                        }
                        continue;
                    }
                };

            match provider.complete_stream(request.clone()).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    if is_non_retryable(&e) {
                        return Err(e);
                    }
                    last_error = Some(e);
                }
            }

            if !deployment.fallback_enabled {
                break;
            }
        }

        Err(last_error.unwrap_or_else(|| CasperError::Unavailable("all backends exhausted".into())))
    }
}

// ── Tenant-scoped provider ──────────────────────────────────────

pub struct TenantProvider {
    inner: Arc<RoutedProvider>,
    tenant_id: Uuid,
}

#[async_trait::async_trait]
impl LlmProvider for TenantProvider {
    fn name(&self) -> &str {
        "routed"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CasperError> {
        let model_slug = request.model.as_deref().unwrap_or("default");
        let deployment = resolve_deployment(&self.inner.db, self.tenant_id, model_slug).await?;
        check_quota(&self.inner.db, self.tenant_id, deployment.model_id).await?;

        let mut patched = request;
        patched.model = Some(deployment.model_name.clone());

        let (response, _backend) = self
            .inner
            .dispatch_with_retry(&deployment, &patched)
            .await?;
        Ok(response)
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ContentBlock, CasperError>> + Send>>, CasperError>
    {
        let model_slug = request.model.as_deref().unwrap_or("default");
        let deployment = resolve_deployment(&self.inner.db, self.tenant_id, model_slug).await?;
        check_quota(&self.inner.db, self.tenant_id, deployment.model_id).await?;

        let mut patched = request;
        patched.model = Some(deployment.model_name.clone());

        self.inner
            .dispatch_stream_with_fallback(&deployment, patched)
            .await
    }
}
