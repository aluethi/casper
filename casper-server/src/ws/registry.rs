use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use casper_base::CasperError;
use casper_base::RuntimeMetrics;
use casper_catalog::{LlmRequest, LlmResponse, MessageRole};
use dashmap::DashMap;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use casper_wire::{WsMessage, InferenceMessage};

// ── Agent Backend Connection ──────────────────────────────────────

pub struct AgentBackendConnection {
    pub id: Uuid,
    pub sender: mpsc::Sender<String>,
    pub hostname: String,
    pub connected_at: Instant,
    pub active_requests: AtomicU32,
    pub max_concurrent: u32,
    /// Maps request ID to a oneshot sender for the response.
    pub pending_requests: DashMap<String, oneshot::Sender<WsMessage>>,
}

// ── Agent Backend Registry ────────────────────────────────────────

pub struct AgentBackendRegistry {
    pub(crate) connections: DashMap<Uuid, Vec<Arc<AgentBackendConnection>>>,
    round_robin: DashMap<Uuid, AtomicU32>,
    metrics: RuntimeMetrics,
}

impl AgentBackendRegistry {
    pub fn new(metrics: RuntimeMetrics) -> Self {
        Self {
            connections: DashMap::new(),
            round_robin: DashMap::new(),
            metrics,
        }
    }

    /// Register a new connection for a backend.
    pub fn register(&self, backend_id: Uuid, conn: Arc<AgentBackendConnection>) {
        self.connections
            .entry(backend_id)
            .or_default()
            .push(conn);
        self.round_robin
            .entry(backend_id)
            .or_insert_with(|| AtomicU32::new(0));
        let count = self
            .connections
            .get(&backend_id)
            .map(|v| v.len())
            .unwrap_or(0);
        self.metrics
            .agent_backend_connections
            .with_label_values(&[&backend_id.to_string()])
            .set(count as i64);
    }

    /// Remove a connection by connection ID.
    pub fn unregister(&self, backend_id: Uuid, connection_id: Uuid) {
        if let Some(mut conns) = self.connections.get_mut(&backend_id) {
            conns.retain(|c| c.id != connection_id);
            let count = conns.len();
            self.metrics
                .agent_backend_connections
                .with_label_values(&[&backend_id.to_string()])
                .set(count as i64);
        }
    }

    /// Check if at least one connection with capacity is available.
    pub fn is_available(&self, backend_id: &Uuid) -> bool {
        self.connections
            .get(backend_id)
            .map(|conns| {
                conns
                    .iter()
                    .any(|c| c.active_requests.load(Ordering::Relaxed) < c.max_concurrent)
            })
            .unwrap_or(false)
    }

    /// Dispatch a request to an agent backend (round-robin).
    pub async fn dispatch(
        &self,
        backend_id: Uuid,
        request: &LlmRequest,
        timeout_ms: u64,
    ) -> Result<LlmResponse, CasperError> {
        let start = Instant::now();
        let bid_str = backend_id.to_string();

        let conn = self.pick_connection(&backend_id)?;
        conn.active_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .agent_backend_active_requests
            .with_label_values(&[&bid_str])
            .inc();

        let request_id = format!("req-{}", Uuid::now_v7().simple());
        let (tx, rx) = oneshot::channel::<WsMessage>();
        conn.pending_requests.insert(request_id.clone(), tx);

        // Build the WS inference request
        let ws_req = WsMessage::InferenceRequest(casper_wire::InferenceRequest {
            id: request_id.clone(),
            model: request.model.clone(),
            messages: request
                .messages
                .iter()
                .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
                .collect(),
            params: serde_json::json!({
                "max_tokens": request.max_tokens,
                "temperature": request.temperature,
                "stream": false,
            }),
            timeout_ms,
        });

        let msg_text = serde_json::to_string(&ws_req).map_err(|e| {
            CasperError::Internal(format!("failed to serialize ws request: {e}"))
        })?;

        // Send the request over WebSocket
        if conn.sender.send(msg_text).await.is_err() {
            conn.pending_requests.remove(&request_id);
            conn.active_requests.fetch_sub(1, Ordering::Relaxed);
            self.metrics
                .agent_backend_active_requests
                .with_label_values(&[&bid_str])
                .dec();
            self.metrics
                .agent_backend_errors
                .with_label_values(&[&bid_str, "disconnected"])
                .inc();
            return Err(CasperError::Unavailable(
                "agent backend connection closed".into(),
            ));
        }

        // Wait for response with timeout
        let timeout = Duration::from_millis(timeout_ms);
        let result = tokio::time::timeout(timeout, rx).await;

        conn.active_requests.fetch_sub(1, Ordering::Relaxed);
        self.metrics
            .agent_backend_active_requests
            .with_label_values(&[&bid_str])
            .dec();

        let elapsed = start.elapsed().as_secs_f64();
        self.metrics.agent_backend_request_duration.observe(elapsed);

        match result {
            Ok(Ok(WsMessage::InferenceResponse(resp))) => {
                let msg = resp.message.unwrap_or(InferenceMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: None,
                });
                let usage = resp.usage.unwrap_or_default();

                let role: MessageRole = serde_json::from_value(
                    serde_json::Value::String(msg.role),
                ).unwrap_or(MessageRole::Assistant);

                Ok(LlmResponse {
                    content: msg.content.unwrap_or_default(),
                    role,
                    model: request.model.clone(),
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cache_read_tokens: usage.cache_read_tokens,
                    cache_write_tokens: usage.cache_write_tokens,
                    tool_calls: msg.tool_calls,
                    finish_reason: resp.stop_reason.or(Some("stop".to_string())),
                    thinking: None,
                })
            }
            Ok(Ok(WsMessage::InferenceError(err))) => {
                let error = err.error;
                let message = err.message;
                let retryable = err.retryable;
                self.metrics
                    .agent_backend_errors
                    .with_label_values(&[&bid_str, &error])
                    .inc();
                let detail = message.unwrap_or(error.clone());
                if retryable {
                    Err(CasperError::Unavailable(detail))
                } else {
                    Err(CasperError::BadGateway(detail))
                }
            }
            Ok(Ok(_)) => {
                self.metrics
                    .agent_backend_errors
                    .with_label_values(&[&bid_str, "unexpected_message"])
                    .inc();
                Err(CasperError::Internal(
                    "unexpected message type from agent backend".into(),
                ))
            }
            Ok(Err(_)) => {
                self.metrics
                    .agent_backend_errors
                    .with_label_values(&[&bid_str, "disconnected"])
                    .inc();
                Err(CasperError::Unavailable(
                    "agent backend connection dropped".into(),
                ))
            }
            Err(_) => {
                conn.pending_requests.remove(&request_id);
                self.metrics
                    .agent_backend_errors
                    .with_label_values(&[&bid_str, "timeout"])
                    .inc();
                Err(CasperError::GatewayTimeout(
                    "agent backend request timed out".into(),
                ))
            }
        }
    }

    fn pick_connection(
        &self,
        backend_id: &Uuid,
    ) -> Result<Arc<AgentBackendConnection>, CasperError> {
        let conns = self.connections.get(backend_id).ok_or_else(|| {
            CasperError::Unavailable("no agent backend connections".into())
        })?;

        if conns.is_empty() {
            return Err(CasperError::Unavailable(
                "no agent backend connections".into(),
            ));
        }

        let counter = self.round_robin.get(backend_id).ok_or_else(|| {
            CasperError::Unavailable("no agent backend connections".into())
        })?;

        let len = conns.len() as u32;
        for _ in 0..len {
            let idx = counter.value().fetch_add(1, Ordering::Relaxed) % len;
            let conn = &conns[idx as usize];
            if conn.active_requests.load(Ordering::Relaxed) < conn.max_concurrent {
                return Ok(Arc::clone(conn));
            }
        }

        // All at capacity — pick first anyway (will queue)
        let idx = counter.value().fetch_add(1, Ordering::Relaxed) % len;
        Ok(Arc::clone(&conns[idx as usize]))
    }

    /// Get status summary for health endpoint.
    pub fn health_status(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for entry in self.connections.iter() {
            let backend_id = entry.key();
            let conns = entry.value();
            let active: u32 = conns
                .iter()
                .map(|c| c.active_requests.load(Ordering::Relaxed))
                .sum();
            map.insert(
                backend_id.to_string(),
                serde_json::json!({
                    "connections": conns.len(),
                    "active_requests": active,
                }),
            );
        }
        serde_json::Value::Object(map)
    }
}
