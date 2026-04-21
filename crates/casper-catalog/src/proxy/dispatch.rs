use casper_base::CasperError;
use crate::{ResolvedBackend, ResolvedDeployment};
use tokio::sync::mpsc;

use super::types::{LlmRequest, LlmResponse, StreamEvent};

/// Dispatch a single LLM request to a specific backend.
/// Routes to the appropriate provider adapter based on `backend.provider`.
pub async fn dispatch(
    client: &reqwest::Client,
    backend: &ResolvedBackend,
    request: &LlmRequest,
) -> Result<LlmResponse, CasperError> {
    let api_key = backend
        .api_key_enc
        .as_deref()
        .unwrap_or("");

    let base_url = backend
        .base_url
        .as_deref()
        .ok_or_else(|| {
            CasperError::Internal(format!(
                "backend '{}' has no base_url configured",
                backend.name
            ))
        })?;

    match backend.provider.as_str() {
        "anthropic" => super::anthropic::call(client, base_url, api_key, request).await,
        "azure_openai" => super::openai::call_azure(client, base_url, api_key, request).await,
        "openai" | "openai_compatible" => {
            super::openai::call(client, base_url, api_key, request).await
        }
        "agent" => {
            Err(CasperError::Internal(
                "agent backends must be dispatched via AgentBackendRegistry".into(),
            ))
        }
        other => Err(CasperError::Internal(format!(
            "unsupported provider: {other}"
        ))),
    }
}

/// Dispatch with retry and fallback across the deployment's backend sequence.
pub async fn dispatch_with_retry<'a>(
    client: &reqwest::Client,
    deployment: &'a ResolvedDeployment,
    request: &LlmRequest,
) -> Result<(LlmResponse, &'a ResolvedBackend), CasperError> {
    let mut last_error: Option<CasperError> = None;

    for (backend_idx, backend) in deployment.backend_sequence.iter().enumerate() {
        tracing::debug!(
            backend_name = %backend.name,
            backend_provider = %backend.provider,
            backend_idx,
            "attempting dispatch"
        );

        for attempt in 0..=deployment.retry_attempts {
            if attempt > 0 {
                let delay_ms =
                    deployment.retry_backoff_ms as u64 * (1u64 << (attempt as u64 - 1));
                tracing::debug!(
                    attempt,
                    delay_ms,
                    backend_name = %backend.name,
                    "retrying after backoff"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }

            match dispatch(client, backend, request).await {
                Ok(response) => {
                    tracing::debug!(
                        backend_name = %backend.name,
                        attempt,
                        input_tokens = response.input_tokens,
                        output_tokens = response.output_tokens,
                        "dispatch succeeded"
                    );
                    return Ok((response, backend));
                }
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

        tracing::info!(
            backend_name = %backend.name,
            "all retries exhausted, falling back to next backend"
        );
    }

    Err(last_error.unwrap_or_else(|| {
        CasperError::Unavailable("all backends exhausted".into())
    }))
}

/// Streaming dispatch to a single backend.
pub async fn dispatch_stream(
    client: &reqwest::Client,
    backend: &ResolvedBackend,
    request: &LlmRequest,
    tx: mpsc::Sender<StreamEvent>,
) -> Result<LlmResponse, CasperError> {
    let api_key = backend.api_key_enc.as_deref().unwrap_or("");
    let base_url = backend.base_url.as_deref().ok_or_else(|| {
        CasperError::Internal(format!("backend '{}' has no base_url configured", backend.name))
    })?;

    match backend.provider.as_str() {
        "anthropic" => super::anthropic::call_stream(client, base_url, api_key, request, tx).await,
        "azure_openai" => super::openai::call_stream_azure(client, base_url, api_key, request, tx).await,
        "openai" | "openai_compatible" => {
            super::openai::call_stream(client, base_url, api_key, request, tx).await
        }
        "agent" => {
            let response = dispatch(client, backend, request).await?;
            if !response.content.is_empty() {
                let _ = tx.send(StreamEvent::ContentDelta { delta: response.content.clone() }).await;
            }
            Ok(response)
        }
        other => Err(CasperError::Internal(format!("unsupported provider: {other}"))),
    }
}

/// Streaming dispatch with fallback.
pub async fn dispatch_stream_with_retry<'a>(
    client: &reqwest::Client,
    deployment: &'a ResolvedDeployment,
    request: &LlmRequest,
    tx: mpsc::Sender<StreamEvent>,
) -> Result<(LlmResponse, &'a ResolvedBackend), CasperError> {
    let mut last_error: Option<CasperError> = None;

    for backend in &deployment.backend_sequence {
        let (inner_tx, mut inner_rx) = mpsc::channel::<StreamEvent>(64);

        let outer_tx = tx.clone();
        let fwd_handle = tokio::spawn(async move {
            let mut count = 0u64;
            while let Some(event) = inner_rx.recv().await {
                count += 1;
                if outer_tx.send(event).await.is_err() { break; }
            }
            count
        });

        match dispatch_stream(client, backend, request, inner_tx).await {
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
                if !deployment.fallback_enabled { break; }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| CasperError::Unavailable("all backends exhausted".into())))
}

/// Determine if an error should not be retried (4xx client errors).
pub fn is_non_retryable(err: &CasperError) -> bool {
    matches!(
        err,
        CasperError::BadRequest(_)
            | CasperError::Unauthorized
            | CasperError::Forbidden(_)
            | CasperError::NotFound(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_retryable_errors() {
        assert!(is_non_retryable(&CasperError::BadRequest("bad".into())));
        assert!(is_non_retryable(&CasperError::Unauthorized));
        assert!(is_non_retryable(&CasperError::Forbidden("denied".into())));
        assert!(is_non_retryable(&CasperError::NotFound("missing".into())));
    }

    #[test]
    fn retryable_errors() {
        assert!(!is_non_retryable(&CasperError::RateLimited));
        assert!(!is_non_retryable(&CasperError::BadGateway("err".into())));
        assert!(!is_non_retryable(&CasperError::Unavailable("err".into())));
        assert!(!is_non_retryable(&CasperError::GatewayTimeout("err".into())));
        assert!(!is_non_retryable(&CasperError::Internal("err".into())));
    }
}
