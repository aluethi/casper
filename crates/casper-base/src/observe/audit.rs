use serde_json::Value as JsonValue;
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

/// A single audit log entry.
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub tenant_id: Uuid,
    pub actor: String,
    pub action: String,
    pub resource: Option<String>,
    pub detail: JsonValue,
    pub outcome: String,
    pub correlation_id: Uuid,
    pub token_id: String,
}

/// Buffered audit writer with bounded channel and batched inserts.
#[derive(Clone)]
pub struct AuditWriter {
    tx: mpsc::Sender<AuditEntry>,
}

impl AuditWriter {
    /// Create and start the audit writer background task.
    /// Returns the writer handle and a join handle for the background task.
    pub fn start(pool: PgPool, buffer_size: usize) -> (Self, tokio::task::JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(buffer_size);
        let handle = tokio::spawn(Self::background_writer(pool, rx));
        (Self { tx }, handle)
    }

    /// Log an audit entry. Never blocks the caller — drops if buffer is full.
    pub fn log(&self, entry: AuditEntry) {
        if let Err(e) = self.tx.try_send(entry) {
            tracing::warn!("Audit buffer full, dropping entry: {e}");
        }
    }

    /// Convenience: log with common fields.
    #[allow(clippy::too_many_arguments)]
    pub fn log_action(
        &self,
        tenant_id: Uuid,
        actor: &str,
        action: &str,
        resource: Option<&str>,
        detail: JsonValue,
        outcome: &str,
        correlation_id: Uuid,
        token_id: &str,
    ) {
        self.log(AuditEntry {
            tenant_id,
            actor: actor.to_string(),
            action: action.to_string(),
            resource: resource.map(|s| s.to_string()),
            detail,
            outcome: outcome.to_string(),
            correlation_id,
            token_id: token_id.to_string(),
        });
    }

    async fn background_writer(pool: PgPool, mut rx: mpsc::Receiver<AuditEntry>) {
        let mut batch: Vec<AuditEntry> = Vec::with_capacity(100);

        loop {
            // Wait for first entry or channel close
            match rx.recv().await {
                Some(entry) => batch.push(entry),
                None => {
                    // Channel closed, flush remaining
                    if !batch.is_empty() {
                        Self::flush(&pool, &batch).await;
                    }
                    break;
                }
            }

            // Drain any additional entries that are ready (up to batch size)
            while batch.len() < 100 {
                match rx.try_recv() {
                    Ok(entry) => batch.push(entry),
                    Err(_) => break,
                }
            }

            // Flush the batch
            Self::flush(&pool, &batch).await;
            batch.clear();
        }

        tracing::info!("Audit writer shutting down");
    }

    async fn flush(pool: &PgPool, entries: &[AuditEntry]) {
        for entry in entries {
            let result = sqlx::query(
                "INSERT INTO audit_log (id, tenant_id, actor, action, resource, detail, outcome, correlation_id, token_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"
            )
            .bind(Uuid::now_v7())
            .bind(entry.tenant_id)
            .bind(&entry.actor)
            .bind(&entry.action)
            .bind(&entry.resource)
            .bind(&entry.detail)
            .bind(&entry.outcome)
            .bind(entry.correlation_id)
            .bind(&entry.token_id)
            .execute(pool)
            .await;

            if let Err(e) = result {
                tracing::error!("Failed to write audit entry: {e}");
            }
        }
    }
}
