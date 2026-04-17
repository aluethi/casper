use std::sync::atomic::AtomicU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;
use axum::response::Response;
use casper_base::CasperError;
use dashmap::DashMap;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::protocol::WsMessage;
use super::registry::AgentBackendConnection;
use crate::AppState;

// ── WebSocket authentication ──────────────────────────────────────

async fn authenticate_agent_key(pool: &PgPool, key: &str) -> Result<Uuid, CasperError> {
    if !key.starts_with("csa-") {
        return Err(CasperError::Unauthorized);
    }

    let hash = hex::encode(Sha256::digest(key.as_bytes()));

    let row: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT abk.backend_id, abk.is_active
         FROM agent_backend_keys abk
         WHERE abk.key_hash = $1",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    let (backend_id, is_active) = row.ok_or(CasperError::Unauthorized)?;
    if !is_active {
        return Err(CasperError::Unauthorized);
    }

    let backend_active: Option<(bool,)> =
        sqlx::query_as("SELECT is_active FROM platform_backends WHERE id = $1")
            .bind(backend_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| CasperError::Internal(format!("DB error: {e}")))?;

    match backend_active {
        Some((true,)) => Ok(backend_id),
        _ => Err(CasperError::Unauthorized),
    }
}

// ── WebSocket upgrade handler ─────────────────────────────────────

/// GET /agent/connect — WebSocket upgrade with csa- key auth.
pub async fn agent_ws_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, CasperError> {
    let auth_header = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(CasperError::Unauthorized)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(CasperError::Unauthorized)?;

    let backend_id = authenticate_agent_key(&state.db_owner, token).await?;
    tracing::info!(backend_id = %backend_id, "agent backend WebSocket authenticated");

    Ok(ws.on_upgrade(move |socket| handle_agent_ws(socket, state, backend_id)))
}

// ── WebSocket session ─────────────────────────────────────────────

async fn handle_agent_ws(socket: WebSocket, state: AppState, backend_id: Uuid) {
    let (mut ws_sink, mut ws_stream) = socket.split();
    let connection_id = Uuid::now_v7();
    let (msg_tx, mut msg_rx) = mpsc::channel::<String>(256);

    // Wait for registration message
    let registered = tokio::time::timeout(
        Duration::from_secs(10),
        wait_for_registration(&mut ws_stream, backend_id),
    )
    .await;

    let (hostname, max_concurrent) = match registered {
        Ok(Ok(reg)) => reg,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "agent registration failed");
            return;
        }
        Err(_) => {
            tracing::warn!("agent registration timed out");
            return;
        }
    };

    let conn = Arc::new(AgentBackendConnection {
        id: connection_id,
        sender: msg_tx,
        hostname: hostname.clone(),
        connected_at: Instant::now(),
        active_requests: AtomicU32::new(0),
        max_concurrent,
        pending_requests: DashMap::new(),
    });

    state.agent_registry.register(backend_id, Arc::clone(&conn));
    tracing::info!(
        backend_id = %backend_id,
        hostname = %hostname,
        connection_id = %connection_id,
        "agent backend registered"
    );

    // Load max_concurrent from backend extra_config (if configured server-side)
    let extra_config: serde_json::Value = sqlx::query_scalar(
        "SELECT extra_config FROM platform_backends WHERE id = $1"
    )
    .bind(backend_id)
    .fetch_optional(&state.db_owner)
    .await
    .ok()
    .flatten()
    .unwrap_or(serde_json::json!({}));

    let ack_config = super::protocol::RegisterAckConfig {
        max_concurrent: extra_config.get("max_concurrent").and_then(|v| v.as_u64()).unwrap_or(max_concurrent as u64) as u32,
        hostname: Some(hostname.clone()),
    };

    // Send register_ack with platform config (no inference URLs — that's sidecar-local)
    let ack = serde_json::to_string(&WsMessage::RegisterAck {
        status: "ok".to_string(),
        backend_id,
        config: ack_config,
    }).unwrap();
    let _ = send_text(&mut ws_sink, &ack).await;

    // Audit connect
    state.audit.log_action(
        Uuid::nil(),
        "system",
        "agent_backend.connect",
        Some(&backend_id.to_string()),
        serde_json::json!({ "hostname": &hostname, "connection_id": connection_id.to_string() }),
        "success",
        Uuid::nil(),
        "agent-backend",
    );

    // Spawn outgoing message forwarder (mpsc -> WS sink)
    let send_handle = tokio::spawn(async move {
        use futures::SinkExt;
        while let Some(text) = msg_rx.recv().await {
            if ws_sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    // Spawn heartbeat pinger
    let ping_sender = conn.sender.clone();
    let ping_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let ping = serde_json::to_string(&WsMessage::Ping {
                timestamp: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
            if ping_sender.send(ping).await.is_err() {
                break;
            }
        }
    });

    // Receive loop
    let timeout_check = Duration::from_secs(90);
    let conn_recv = Arc::clone(&conn);

    loop {
        let recv = tokio::time::timeout(timeout_check, next_text(&mut ws_stream)).await;

        match recv {
            Ok(Some(Ok(text))) => match serde_json::from_str::<WsMessage>(&text) {
                Ok(WsMessage::Pong { .. }) => { /* heartbeat OK */ }
                Ok(msg @ WsMessage::InferenceResponse { .. })
                | Ok(msg @ WsMessage::InferenceError { .. }) => {
                    let req_id = match &msg {
                        WsMessage::InferenceResponse { id, .. } => id.clone(),
                        WsMessage::InferenceError { id, .. } => id.clone(),
                        _ => unreachable!(),
                    };
                    if let Some((_, tx)) = conn_recv.pending_requests.remove(&req_id) {
                        let _ = tx.send(msg);
                    }
                }
                Ok(_) => tracing::debug!("ignoring unexpected message type from agent"),
                Err(e) => tracing::warn!(error = %e, "failed to parse agent message"),
            },
            Ok(Some(Err(e))) => {
                tracing::warn!(error = %e, "agent WebSocket error");
                break;
            }
            Ok(None) => {
                tracing::info!("agent WebSocket closed");
                break;
            }
            Err(_) => {
                tracing::warn!(
                    backend_id = %backend_id,
                    hostname = %hostname,
                    "agent backend heartbeat timeout (90s)"
                );
                break;
            }
        }
    }

    // Cleanup
    ping_handle.abort();
    send_handle.abort();
    state.agent_registry.unregister(backend_id, connection_id);
    conn.pending_requests.clear();

    tracing::info!(
        backend_id = %backend_id,
        hostname = %hostname,
        connection_id = %connection_id,
        "agent backend disconnected"
    );

    state.audit.log_action(
        Uuid::nil(),
        "system",
        "agent_backend.disconnect",
        Some(&backend_id.to_string()),
        serde_json::json!({
            "hostname": &hostname,
            "connection_id": connection_id.to_string(),
            "duration_secs": conn.connected_at.elapsed().as_secs(),
        }),
        "success",
        Uuid::nil(),
        "agent-backend",
    );
}

// ── Helpers ───────────────────────────────────────────────────────

async fn wait_for_registration(
    ws_stream: &mut futures::stream::SplitStream<WebSocket>,
    expected_backend_id: Uuid,
) -> Result<(String, u32), CasperError> {
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let parsed: WsMessage = serde_json::from_str(&text).map_err(|e| {
                    CasperError::BadRequest(format!("invalid registration message: {e}"))
                })?;
                match parsed {
                    WsMessage::Register {
                        backend_id,
                        hostname,
                        max_concurrent,
                        ..
                    } => {
                        if backend_id != expected_backend_id {
                            return Err(CasperError::Forbidden(
                                "backend_id does not match authenticated key".into(),
                            ));
                        }
                        return Ok((hostname, max_concurrent));
                    }
                    _ => {
                        return Err(CasperError::BadRequest(
                            "expected register message".into(),
                        ));
                    }
                }
            }
            Ok(Message::Close(_)) => {
                return Err(CasperError::BadRequest(
                    "connection closed before registration".into(),
                ));
            }
            Ok(_) => continue,
            Err(e) => return Err(CasperError::Internal(format!("WebSocket error: {e}"))),
        }
    }
    Err(CasperError::BadRequest(
        "connection closed before registration".into(),
    ))
}

async fn next_text(
    ws_stream: &mut futures::stream::SplitStream<WebSocket>,
) -> Option<Result<String, axum::Error>> {
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Text(text)) => return Some(Ok(text.to_string())),
            Ok(Message::Close(_)) => return None,
            Ok(_) => continue,
            Err(e) => return Some(Err(e)),
        }
    }
    None
}

async fn send_text(
    ws_sink: &mut futures::stream::SplitSink<WebSocket, Message>,
    text: &str,
) -> Result<(), axum::Error> {
    use futures::SinkExt;
    ws_sink.send(Message::Text(text.to_string().into())).await
}
