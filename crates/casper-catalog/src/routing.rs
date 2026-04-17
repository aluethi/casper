use casper_base::CasperError;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

// ── Resolved types ────────────────────────────────────────────────

/// A fully resolved deployment ready for dispatch.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedDeployment {
    pub deployment_id: Uuid,
    pub model_id: Uuid,
    pub model_name: String,
    pub slug: String,
    pub backend_sequence: Vec<ResolvedBackend>,
    pub retry_attempts: i32,
    pub retry_backoff_ms: i32,
    pub fallback_enabled: bool,
    pub timeout_ms: i32,
    pub default_params: serde_json::Value,
    pub fallback_deployment_id: Option<Uuid>,
}

/// A resolved backend with connection info.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedBackend {
    pub id: Uuid,
    pub name: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub api_key_enc: Option<String>,
}

// ── Internal row types ────────────────────────────────────────────

type DeploymentRow = (
    Uuid,               // deployment id
    Uuid,               // model_id
    String,             // model name
    String,             // slug
    Vec<Uuid>,          // backend_sequence
    i32,                // retry_attempts
    i32,                // retry_backoff_ms
    bool,               // fallback_enabled
    i32,                // timeout_ms
    serde_json::Value,  // default_params
    Option<Uuid>,       // fallback_deployment_id
);

type BackendRow = (Uuid, String, String, Option<String>, Option<String>);

// ── Public API ────────────────────────────────────────────────────

/// Resolve a deployment by tenant + slug. Fetches the deployment, the associated
/// model, and all backends in the configured sequence.
pub async fn resolve_deployment(
    pool: &PgPool,
    tenant_id: Uuid,
    slug: &str,
) -> Result<ResolvedDeployment, CasperError> {
    // 1. Fetch deployment + model name
    let row: Option<DeploymentRow> = sqlx::query_as(
        "SELECT d.id, d.model_id, m.name, d.slug,
                d.backend_sequence, d.retry_attempts, d.retry_backoff_ms,
                d.fallback_enabled, d.timeout_ms, d.default_params,
                d.fallback_deployment_id
         FROM model_deployments d
         JOIN models m ON m.id = d.model_id
         WHERE d.tenant_id = $1 AND d.slug = $2 AND d.is_active = true AND m.is_active = true",
    )
    .bind(tenant_id)
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error resolving deployment: {e}")))?;

    let (
        deployment_id,
        model_id,
        model_name,
        deployment_slug,
        backend_sequence_ids,
        retry_attempts,
        retry_backoff_ms,
        fallback_enabled,
        timeout_ms,
        default_params,
        fallback_deployment_id,
    ) = row.ok_or_else(|| {
        CasperError::NotFound(format!("deployment '{slug}' not found or inactive"))
    })?;

    // 2. Resolve backends (including api_key_enc for dispatch)
    let backends: Vec<BackendRow> = if backend_sequence_ids.is_empty() {
        // Fall back to platform_backend_models for the model
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, pb.api_key_enc
             FROM platform_backend_models pbm
             JOIN platform_backends pb ON pb.id = pbm.backend_id
             WHERE pbm.model_id = $1 AND pb.is_active = true
             ORDER BY pbm.priority",
        )
        .bind(model_id)
        .fetch_all(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error resolving backends: {e}")))?
    } else {
        // Use the explicit backend_sequence, preserving order
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, pb.api_key_enc
             FROM unnest($1::UUID[]) WITH ORDINALITY AS s(backend_id, ord)
             JOIN platform_backends pb ON pb.id = s.backend_id
             WHERE pb.is_active = true
             ORDER BY s.ord",
        )
        .bind(&backend_sequence_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error resolving backends: {e}")))?
    };

    if backends.is_empty() {
        return Err(CasperError::Unavailable(format!(
            "no active backends for deployment '{slug}'"
        )));
    }

    let resolved_backends = backends
        .into_iter()
        .map(|(id, name, provider, base_url, api_key_enc)| ResolvedBackend {
            id,
            name,
            provider,
            base_url,
            api_key_enc,
        })
        .collect();

    Ok(ResolvedDeployment {
        deployment_id,
        model_id,
        model_name,
        slug: deployment_slug,
        backend_sequence: resolved_backends,
        retry_attempts,
        retry_backoff_ms,
        fallback_enabled,
        timeout_ms,
        default_params,
        fallback_deployment_id,
    })
}

/// Resolve a deployment by ID (used for fallback chain).
pub async fn resolve_deployment_by_id(
    pool: &PgPool,
    deployment_id: Uuid,
) -> Result<ResolvedDeployment, CasperError> {
    let row: Option<DeploymentRow> = sqlx::query_as(
        "SELECT d.id, d.model_id, m.name, d.slug,
                d.backend_sequence, d.retry_attempts, d.retry_backoff_ms,
                d.fallback_enabled, d.timeout_ms, d.default_params,
                d.fallback_deployment_id
         FROM model_deployments d
         JOIN models m ON m.id = d.model_id
         WHERE d.id = $1 AND d.is_active = true AND m.is_active = true",
    )
    .bind(deployment_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error resolving deployment: {e}")))?;

    let (
        dep_id,
        model_id,
        model_name,
        slug,
        backend_sequence_ids,
        retry_attempts,
        retry_backoff_ms,
        fallback_enabled,
        timeout_ms,
        default_params,
        fallback_dep_id,
    ) = row.ok_or_else(|| {
        CasperError::NotFound(format!("fallback deployment '{deployment_id}' not found or inactive"))
    })?;

    let backends: Vec<BackendRow> = if backend_sequence_ids.is_empty() {
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, pb.api_key_enc
             FROM platform_backend_models pbm
             JOIN platform_backends pb ON pb.id = pbm.backend_id
             WHERE pbm.model_id = $1 AND pb.is_active = true
             ORDER BY pbm.priority",
        )
        .bind(model_id)
        .fetch_all(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error resolving backends: {e}")))?
    } else {
        sqlx::query_as(
            "SELECT pb.id, pb.name, pb.provider, pb.base_url, pb.api_key_enc
             FROM unnest($1::UUID[]) WITH ORDINALITY AS s(backend_id, ord)
             JOIN platform_backends pb ON pb.id = s.backend_id
             WHERE pb.is_active = true
             ORDER BY s.ord",
        )
        .bind(&backend_sequence_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| CasperError::Internal(format!("DB error resolving backends: {e}")))?
    };

    let resolved_backends = backends
        .into_iter()
        .map(|(id, name, provider, base_url, api_key_enc)| ResolvedBackend {
            id, name, provider, base_url, api_key_enc,
        })
        .collect();

    Ok(ResolvedDeployment {
        deployment_id: dep_id,
        model_id,
        model_name,
        slug,
        backend_sequence: resolved_backends,
        retry_attempts,
        retry_backoff_ms,
        fallback_enabled,
        timeout_ms,
        default_params,
        fallback_deployment_id: fallback_dep_id,
    })
}

/// Basic quota check: verify that a quota exists and allows requests.
pub async fn check_quota(
    pool: &PgPool,
    tenant_id: Uuid,
    model_id: Uuid,
) -> Result<(), CasperError> {
    let rpm: Option<(i32,)> = sqlx::query_as(
        "SELECT requests_per_minute
         FROM model_quotas
         WHERE tenant_id = $1 AND model_id = $2",
    )
    .bind(tenant_id)
    .bind(model_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error checking quota: {e}")))?;

    match rpm {
        None => Err(CasperError::Forbidden(
            "no quota allocated for this model".into(),
        )),
        Some((rpm_val,)) if rpm_val <= 0 => Err(CasperError::RateLimited),
        Some(_) => Ok(()),
    }
}

/// Merge deployment default_params under the incoming request params.
/// Request params take precedence; default_params fill in missing fields.
pub fn merge_params(
    default_params: &serde_json::Value,
    request_extra: &serde_json::Value,
) -> serde_json::Value {
    match (default_params, request_extra) {
        (serde_json::Value::Object(defaults), serde_json::Value::Object(req)) => {
            let mut merged = defaults.clone();
            for (k, v) in req {
                merged.insert(k.clone(), v.clone());
            }
            serde_json::Value::Object(merged)
        }
        // If either side is not an object, request takes precedence
        (_, req) => req.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_params_request_overrides_defaults() {
        let defaults = serde_json::json!({
            "temperature": 0.7,
            "max_tokens": 1024,
            "top_p": 0.9
        });
        let request = serde_json::json!({
            "temperature": 0.3,
            "max_tokens": 2048
        });
        let merged = merge_params(&defaults, &request);
        assert_eq!(merged["temperature"], 0.3);
        assert_eq!(merged["max_tokens"], 2048);
        assert_eq!(merged["top_p"], 0.9);
    }

    #[test]
    fn merge_params_empty_defaults() {
        let defaults = serde_json::json!({});
        let request = serde_json::json!({"temperature": 0.5});
        let merged = merge_params(&defaults, &request);
        assert_eq!(merged["temperature"], 0.5);
    }

    #[test]
    fn merge_params_empty_request() {
        let defaults = serde_json::json!({"temperature": 0.7});
        let request = serde_json::json!({});
        let merged = merge_params(&defaults, &request);
        assert_eq!(merged["temperature"], 0.7);
    }

    #[test]
    fn merge_params_non_object_request() {
        let defaults = serde_json::json!({"temperature": 0.7});
        let request = serde_json::json!(null);
        let merged = merge_params(&defaults, &request);
        assert!(merged.is_null());
    }

    #[test]
    fn resolved_backend_debug() {
        let backend = ResolvedBackend {
            id: Uuid::nil(),
            name: "test".to_string(),
            provider: "anthropic".to_string(),
            base_url: Some("https://api.anthropic.com".to_string()),
            api_key_enc: None,
        };
        let debug = format!("{backend:?}");
        assert!(debug.contains("anthropic"));
    }
}
