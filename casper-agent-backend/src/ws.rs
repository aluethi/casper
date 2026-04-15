use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio_tungstenite::tungstenite;

// ── Protocol messages ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "register")]
    Register {
        hostname: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        gpu_info: Option<serde_json::Value>,
    },
    #[serde(rename = "register_ack")]
    RegisterAck {
        status: String,
        backend_id: uuid::Uuid,
        #[serde(default)]
        config: RegisterAckConfig,
    },
    #[serde(rename = "ping")]
    Ping { timestamp: String },
    #[serde(rename = "pong")]
    Pong {
        timestamp: String,
        active_requests: u32,
        queue_depth: u32,
    },
    #[serde(rename = "inference_request")]
    InferenceRequest {
        id: String,
        model: String,
        messages: Vec<serde_json::Value>,
        params: serde_json::Value,
        timeout_ms: u64,
    },
    #[serde(rename = "inference_response")]
    InferenceResponse {
        id: String,
        status: String,
        message: InferenceMessage,
        usage: InferenceUsage,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
    },
    #[serde(rename = "inference_error")]
    InferenceError {
        id: String,
        error: String,
        message: String,
        retryable: bool,
    },
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct RegisterAckConfig {
    #[serde(default)]
    pub inference_base_url: Option<String>,
    #[serde(default)]
    pub inference_server_type: Option<String>,
    #[serde(default)]
    pub max_concurrent: u32,
    #[serde(default)]
    pub hostname: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InferenceMessage {
    pub role: String,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InferenceUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i32>,
}

// ── Connection ────────────────────────────────────────────────────

pub async fn run_connection(
    casper_url: &str,
    agent_key: &str,
    inference_base_url_override: Option<&str>,
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

    // Send minimal registration (server already knows our backend_id from the key)
    let register = WsMessage::Register {
        hostname: gethostname(),
        gpu_info: None,
    };
    ws_sink.send(tungstenite::Message::Text(serde_json::to_string(&register)?.into())).await?;
    tracing::info!("registration sent, waiting for config from server...");

    // Wait for RegisterAck with server-pushed config
    let server_config = wait_for_ack(&mut ws_source).await?;

    // Resolve inference URL: local override > server config > default
    let inference_base_url = inference_base_url_override.map(|s| s.to_string())
        .or(server_config.inference_base_url.clone())
        .unwrap_or_else(|| "http://localhost:11434".to_string());
    let inference_type = server_config.inference_server_type.clone()
        .unwrap_or_else(|| "openai_compatible".to_string());
    let max_concurrent = if server_config.max_concurrent > 0 { server_config.max_concurrent } else { 8 };

    tracing::info!(
        backend_id = %server_config.hostname.as_deref().unwrap_or("?"),
        inference_url = %inference_base_url,
        inference_type = %inference_type,
        max_concurrent = max_concurrent,
        "configured by server"
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
                let pong = WsMessage::Pong {
                    timestamp,
                    active_requests: active_requests.load(Ordering::Relaxed),
                    queue_depth: 0,
                };
                ws_sink.send(tungstenite::Message::Text(serde_json::to_string(&pong)?.into())).await?;
            }
            WsMessage::InferenceRequest { id, model, messages, params, timeout_ms } => {
                let client = http_client.clone();
                let base_url = inference_base_url.clone();
                let srv_type = inference_type.clone();
                let active = Arc::clone(&active_requests);
                let sem = Arc::clone(&semaphore);

                let (resp_tx, mut resp_rx) = tokio::sync::mpsc::channel::<String>(1);

                tokio::spawn(async move {
                    let _permit = sem.acquire().await;
                    active.fetch_add(1, Ordering::Relaxed);
                    let start = std::time::Instant::now();

                    let result = dispatch_local(&client, &base_url, &srv_type, &model, &messages, &params, timeout_ms).await;
                    let duration_ms = start.elapsed().as_millis() as u64;
                    active.fetch_sub(1, Ordering::Relaxed);

                    let response_msg = match result {
                        Ok((msg, usage, stop_reason)) => WsMessage::InferenceResponse { id, status: "ok".to_string(), message: msg, usage, duration_ms, stop_reason },
                        Err(e) => WsMessage::InferenceError { id, error: "dispatch_error".to_string(), message: format!("{e}"), retryable: true },
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
                if let Ok(WsMessage::RegisterAck { status, config, .. }) = serde_json::from_str(&text.to_string()) {
                    if status == "ok" {
                        return Ok(config);
                    }
                    return Err(format!("registration rejected: {status}").into());
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
    client: &reqwest::Client, base_url: &str, _server_type: &str,
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
