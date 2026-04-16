use casper_wire::{
    InferenceError, InferenceMessage, InferenceResponse, InferenceUsage, Pong, Register,
    RegisterAckConfig, WsMessage,
};
use futures::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio_tungstenite::tungstenite;

// ── Connection ────────────────────────────────────────────────────

pub async fn run_connection(
    casper_url: &str,
    agent_key: &str,
    inference_url: &str,
    http_client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = url::Url::parse(casper_url)?;

    let request = tungstenite::http::Request::builder()
        .uri(casper_url)
        .header("Authorization", format!("Bearer {}", agent_key))
        .header("Host", url.host_str().unwrap_or("localhost"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .body(())?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
    tracing::info!("connected to Casper");

    let (mut ws_sink, mut ws_source) = ws_stream.split();

    // Send registration (server knows our backend_id from the key)
    let register = WsMessage::Register(Register {
        hostname: gethostname(),
        gpu_info: None,
    });
    ws_sink.send(tungstenite::Message::Text(serde_json::to_string(&register)?.into())).await?;
    tracing::info!("registration sent, waiting for ack...");

    // Wait for RegisterAck with server-pushed config
    let server_config = wait_for_ack(&mut ws_source).await?;

    let inference_base_url = inference_url.to_string();
    let max_concurrent = if server_config.max_concurrent > 0 { server_config.max_concurrent } else { 8 };

    tracing::info!(
        inference_url = %inference_base_url,
        max_concurrent = max_concurrent,
        "ready to serve requests"
    );

    let active_requests = Arc::new(AtomicU32::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent as usize));

    // Receive loop
    while let Some(msg) = ws_source.next().await {
        let text = match msg {
            Ok(tungstenite::Message::Text(t)) => t.to_string(),
            Ok(tungstenite::Message::Close(_)) => { tracing::info!("server closed connection"); break; }
            Ok(_) => continue,
            Err(e) => { tracing::error!(error = %e, "WebSocket error"); return Err(e.into()); }
        };

        let parsed: WsMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => { tracing::warn!(error = %e, "failed to parse message"); continue; }
        };

        match parsed {
            WsMessage::Ping { timestamp } => {
                let pong = WsMessage::Pong(Pong {
                    timestamp,
                    active_requests: active_requests.load(Ordering::Relaxed),
                    queue_depth: 0,
                });
                ws_sink.send(tungstenite::Message::Text(serde_json::to_string(&pong)?.into())).await?;
            }
            WsMessage::InferenceRequest(req) => {
                let client = http_client.clone();
                let base_url = inference_base_url.clone();
                let active = Arc::clone(&active_requests);
                let sem = Arc::clone(&semaphore);
                let req_id = req.id.clone();

                let (resp_tx, mut resp_rx) = tokio::sync::mpsc::channel::<String>(1);

                tokio::spawn(async move {
                    let _permit = sem.acquire().await;
                    active.fetch_add(1, Ordering::Relaxed);
                    let start = std::time::Instant::now();

                    let result = dispatch_local(&client, &base_url, &req.model, &req.messages, &req.params, req.timeout_ms).await;
                    let duration_ms = start.elapsed().as_millis() as u64;
                    active.fetch_sub(1, Ordering::Relaxed);

                    let response_msg = match result {
                        Ok((msg, usage, stop_reason)) => WsMessage::InferenceResponse(InferenceResponse {
                            id: req_id, status: "ok".to_string(), message: Some(msg), usage: Some(usage),
                            duration_ms: Some(duration_ms), stop_reason,
                        }),
                        Err(e) => WsMessage::InferenceError(InferenceError {
                            id: req_id, error: "dispatch_error".to_string(), message: Some(format!("{e}")), retryable: true,
                        }),
                    };

                    if let Ok(text) = serde_json::to_string(&response_msg) {
                        let _ = resp_tx.send(text).await;
                    }
                });

                if let Some(resp_text) = resp_rx.recv().await {
                    ws_sink.send(tungstenite::Message::Text(resp_text.into())).await?;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn wait_for_ack(
    ws_source: &mut futures::stream::SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
) -> Result<RegisterAckConfig, Box<dyn std::error::Error>> {
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(msg) = ws_source.next().await {
            if let Ok(tungstenite::Message::Text(text)) = msg {
                if let Ok(WsMessage::RegisterAck(ack)) = serde_json::from_str(&text.to_string()) {
                    if ack.status == "ok" {
                        return Ok(ack.config);
                    }
                    return Err(format!("registration rejected: {}", ack.status).into());
                }
            }
        }
        Err("connection closed before register_ack".into())
    }).await;

    match timeout {
        Ok(result) => result,
        Err(_) => Err("timed out waiting for register_ack".into()),
    }
}

// ── Local dispatch ───────────────────────────────────────────────

async fn dispatch_local(
    client: &reqwest::Client, base_url: &str,
    model: &str, messages: &[serde_json::Value], params: &serde_json::Value, timeout_ms: u64,
) -> Result<(InferenceMessage, InferenceUsage, Option<String>), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    let mut body = serde_json::json!({ "model": model, "messages": messages, "stream": false });
    if let Some(v) = params.get("max_tokens").and_then(|v| v.as_i64()) { body["max_tokens"] = serde_json::json!(v); }
    if let Some(v) = params.get("temperature").and_then(|v| v.as_f64()) { body["temperature"] = serde_json::json!(v); }

    let response = client.post(&url)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .json(&body).send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("inference server returned {status}: {text}").into());
    }

    let resp: serde_json::Value = response.json().await?;
    let choice = resp["choices"].get(0).ok_or("no choices")?;

    Ok((
        InferenceMessage {
            role: choice["message"]["role"].as_str().unwrap_or("assistant").to_string(),
            content: choice["message"]["content"].as_str().map(|s| s.to_string()),
            tool_calls: choice["message"]["tool_calls"].as_array().cloned(),
        },
        InferenceUsage {
            input_tokens: resp["usage"]["prompt_tokens"].as_i64().unwrap_or(0) as i32,
            output_tokens: resp["usage"]["completion_tokens"].as_i64().unwrap_or(0) as i32,
            cache_read_tokens: None,
            cache_write_tokens: None,
        },
        choice["finish_reason"].as_str().map(|s| s.to_string()),
    ))
}

fn gethostname() -> String {
    std::env::var("HOSTNAME").or_else(|_| std::env::var("HOST")).unwrap_or_else(|_| "unknown".to_string())
}
