use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use casper_base::CasperError;
use casper_base::RuntimeMetrics;
use dashmap::DashMap;
use futures::Stream;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use casper_wire::WsMessage;

// ── Agent Backend Connection ──────────────────────────────────────

pub struct AgentBackendConnection {
    pub id: Uuid,
    pub sender: mpsc::Sender<String>,
    pub hostname: String,
    pub connected_at: Instant,
    pub active_requests: AtomicU32,
    pub max_concurrent: u32,
    pub pending_requests: DashMap<String, mpsc::Sender<WsMessage>>,
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

    pub fn register(&self, backend_id: Uuid, conn: Arc<AgentBackendConnection>) {
        self.connections.entry(backend_id).or_default().push(conn);
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

    /// Streaming dispatch: sends a request with `stream: true` and returns
    /// a stream of JSON chunks as the backend produces them.
    pub async fn dispatch_stream_json(
        &self,
        backend_id: Uuid,
        request: serde_json::Value,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<serde_json::Value, CasperError>> + Send>>,
        CasperError,
    > {
        let start = Instant::now();
        let bid_str = backend_id.to_string();

        let conn = self.pick_connection(&backend_id)?;
        conn.active_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics
            .agent_backend_active_requests
            .with_label_values(&[&bid_str])
            .inc();

        let request_id = format!("req-{}", Uuid::now_v7().simple());
        let (tx, rx) = mpsc::channel::<WsMessage>(64);
        conn.pending_requests.insert(request_id.clone(), tx);

        let timeout_ms = request["timeout_ms"].as_u64().unwrap_or(120_000);

        let extra = if request.get("tools").and_then(|t| t.as_array()).is_some() {
            Some(json!({ "tools": request["tools"] }))
        } else {
            None
        };

        let ws_req = WsMessage::InferenceRequest(casper_wire::InferenceRequest {
            id: request_id.clone(),
            model: request["model"].as_str().unwrap_or("").to_string(),
            messages: request["messages"].as_array().cloned().unwrap_or_default(),
            params: json!({
                "max_tokens": request["max_tokens"],
                "temperature": request["temperature"],
                "stream": true,
            }),
            extra,
            timeout_ms,
        });

        let msg_text = serde_json::to_string(&ws_req)
            .map_err(|e| CasperError::Internal(format!("failed to serialize ws request: {e}")))?;

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

        let metrics = self.metrics.clone();
        let conn_cleanup = Arc::clone(&conn);
        let req_id_cleanup = request_id.clone();

        let stream = async_stream::stream! {
            let mut rx = rx;
            let timeout = Duration::from_millis(timeout_ms);
            let deadline = tokio::time::Instant::now() + timeout;

            loop {
                match tokio::time::timeout_at(deadline, rx.recv()).await {
                    Ok(Some(WsMessage::InferenceChunk { delta, .. })) => {
                        yield Ok(json!({ "delta": delta }));
                    }
                    Ok(Some(WsMessage::InferenceThinking { delta, .. })) => {
                        yield Ok(json!({ "thinking_delta": delta }));
                    }
                    Ok(Some(WsMessage::InferenceToolCall(tc))) => {
                        yield Ok(json!({
                            "tool_call_start": {
                                "tool_index": tc.tool_index,
                                "tool_call_id": tc.tool_call_id,
                                "name": tc.name,
                            }
                        }));
                    }
                    Ok(Some(WsMessage::InferenceToolCallDelta { tool_index, arguments_delta, .. })) => {
                        yield Ok(json!({
                            "tool_call_delta": {
                                "tool_index": tool_index,
                                "arguments_delta": arguments_delta,
                            }
                        }));
                    }
                    Ok(Some(WsMessage::InferenceDone(done))) => {
                        let usage = done.usage.unwrap_or_default();
                        yield Ok(json!({
                            "done": true,
                            "usage": {
                                "input_tokens": usage.input_tokens,
                                "output_tokens": usage.output_tokens,
                            }
                        }));
                        break;
                    }
                    Ok(Some(WsMessage::InferenceResponse(resp))) => {
                        // Non-streaming fallback: backend sent full response
                        let msg = resp.message.unwrap_or(casper_wire::InferenceMessage {
                            role: "assistant".to_string(),
                            content: None,
                            tool_calls: None,
                        });
                        let usage = resp.usage.unwrap_or_default();
                        yield Ok(json!({
                            "message": {
                                "role": msg.role,
                                "content": msg.content,
                                "tool_calls": msg.tool_calls,
                            },
                            "usage": {
                                "input_tokens": usage.input_tokens,
                                "output_tokens": usage.output_tokens,
                            },
                            "stop_reason": resp.stop_reason,
                        }));
                        break;
                    }
                    Ok(Some(WsMessage::InferenceError(err))) => {
                        let detail = err.message.unwrap_or(err.error);
                        yield Err(CasperError::BadGateway(detail));
                        break;
                    }
                    Ok(Some(_)) => continue,
                    Ok(None) => {
                        yield Err(CasperError::Unavailable("agent backend connection dropped".into()));
                        break;
                    }
                    Err(_) => {
                        yield Err(CasperError::GatewayTimeout("agent backend request timed out".into()));
                        break;
                    }
                }
            }

            // Cleanup
            conn_cleanup.pending_requests.remove(&req_id_cleanup);
            conn_cleanup.active_requests.fetch_sub(1, Ordering::Relaxed);
            metrics
                .agent_backend_active_requests
                .with_label_values(&[&bid_str])
                .dec();
            let elapsed = start.elapsed().as_secs_f64();
            metrics.agent_backend_request_duration.observe(elapsed);
        };

        Ok(Box::pin(stream))
    }

    fn pick_connection(
        &self,
        backend_id: &Uuid,
    ) -> Result<Arc<AgentBackendConnection>, CasperError> {
        let conns = self
            .connections
            .get(backend_id)
            .ok_or_else(|| CasperError::Unavailable("no agent backend connections".into()))?;

        if conns.is_empty() {
            return Err(CasperError::Unavailable(
                "no agent backend connections".into(),
            ));
        }

        let counter = self
            .round_robin
            .get(backend_id)
            .ok_or_else(|| CasperError::Unavailable("no agent backend connections".into()))?;

        let len = conns.len() as u32;
        for _ in 0..len {
            let idx = counter.value().fetch_add(1, Ordering::Relaxed) % len;
            let conn = &conns[idx as usize];
            if conn.active_requests.load(Ordering::Relaxed) < conn.max_concurrent {
                return Ok(Arc::clone(conn));
            }
        }

        let idx = counter.value().fetch_add(1, Ordering::Relaxed) % len;
        Ok(Arc::clone(&conns[idx as usize]))
    }

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
                json!({
                    "connections": conns.len(),
                    "active_requests": active,
                }),
            );
        }
        serde_json::Value::Object(map)
    }
}
