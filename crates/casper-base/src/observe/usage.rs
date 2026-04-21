use sqlx::PgPool;
use uuid::Uuid;

/// Usage event for LLM calls.
#[derive(Debug, Clone)]
pub struct UsageEvent {
    pub tenant_id: Uuid,
    pub source: String,
    pub model: String,
    pub deployment_slug: Option<String>,
    pub agent_name: Option<String>,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cache_read_tokens: Option<i32>,
    pub cache_write_tokens: Option<i32>,
    pub backend_id: Option<Uuid>,
    pub correlation_id: Uuid,
}

/// Records LLM usage events.
#[derive(Clone)]
pub struct UsageRecorder {
    pool: PgPool,
}

impl UsageRecorder {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn record(&self, event: UsageEvent) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO usage_events (id, tenant_id, source, model, deployment_slug, agent_name,
             input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, backend_id, correlation_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(Uuid::now_v7())
        .bind(event.tenant_id)
        .bind(&event.source)
        .bind(&event.model)
        .bind(&event.deployment_slug)
        .bind(&event.agent_name)
        .bind(event.input_tokens)
        .bind(event.output_tokens)
        .bind(event.cache_read_tokens)
        .bind(event.cache_write_tokens)
        .bind(event.backend_id)
        .bind(event.correlation_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
