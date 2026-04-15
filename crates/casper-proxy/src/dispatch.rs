use casper_base::CasperError;
use casper_catalog::{ResolvedBackend, ResolvedDeployment};

use crate::types::{LlmRequest, LlmResponse};

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
        "anthropic" => crate::anthropic::call(client, base_url, api_key, request).await,
        "openai" | "azure_openai" | "openai_compatible" => {
            crate::openai::call(client, base_url, api_key, request).await
        }
        other => Err(CasperError::Internal(format!(
            "unsupported provider: {other}"
        ))),
    }
}

/// Dispatch with retry and fallback across the deployment's backend sequence.
///
/// For each backend in the sequence:
///   - Retry up to `retry_attempts` times with exponential backoff
///   - On final failure and `fallback_enabled`: try the next backend
///   - If all backends exhausted: return 503
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
                // Exponential backoff: base_ms * 2^(attempt-1)
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

                    // Don't retry on client errors (bad request, auth issues from our side)
                    if is_non_retryable(&e) {
                        return Err(e);
                    }

                    last_error = Some(e);
                }
            }
        }

        // All retry attempts exhausted for this backend.
        // If fallback is not enabled, stop here.
        if !deployment.fallback_enabled {
            break;
        }

        tracing::info!(
            backend_name = %backend.name,
            "all retries exhausted, falling back to next backend"
        );
    }

    // All backends exhausted
    Err(last_error.unwrap_or_else(|| {
        CasperError::Unavailable("all backends exhausted".into())
    }))
}

/// Determine if an error should not be retried.
fn is_non_retryable(err: &CasperError) -> bool {
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
