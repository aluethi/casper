use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio_tungstenite::tungstenite;
use uuid::Uuid;

use crate::SidecarConfig;

// ── Protocol messages ─────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "register")]
    Register {
        backend_id: Uuid,
        hostname: String,
        inference_server: String,
        inference_version: String,
        models_loaded: Vec<String>,
        max_concurrent: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        gpu_info: Option<serde_json::Value>,
    },
    #[serde(rename = "register_ack")]
    RegisterAck { status: String },
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
    config: &SidecarConfig,
    http_client: &reqwest::Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = url::Url::parse(&config.casper_url)?;

    // Build WS request with auth header
    let request = tungstenite::http::Request::builder()
        .uri(config.casper_url.as_str())
        .header("Authorization", format!("Bearer {}", config.agent_key))
        .header("Host", url.host_str().unwrap_or("localhost"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())?;

    let (ws_stream, _response) =
        tokio_tungstenite::connect_async(request).await?;

    tracing::info!("connected to Casper WebSocket");

    let (mut ws_sink, mut ws_source) = ws_stream.split();

    let backend_id = config.backend_id;

    let hostname = gethostname();

    // Send registration message
    let register = WsMessage::Register {
        backend_id,
        hostname: hostname.clone(),
        inference_server: config.inference_server.server_type.clone(),
        inference_version: String::new(),
        models_loaded: vec![],
        max_concurrent: config.max_concurrent,
        gpu_info: None,
    };

    let register_text = serde_json::to_string(&register)?;
    ws_sink
        .send(tungstenite::Message::Text(register_text.into()))
        .await?;

    tracing::info!("registration message sent");

    let active_requests = Arc::new(AtomicU32::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(
        config.max_concurrent as usize,
    ));

    // Receive loop
    while let Some(msg) = ws_source.next().await {
        let msg = match msg {
            Ok(tungstenite::Message::Text(t)) => t.to_string(),
            Ok(tungstenite::Message::Close(_)) => {
                tracing::info!("server closed connection");
                break;
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::error!(error = %e, "WebSocket receive error");
                return Err(e.into());
            }
        };

        let parsed: WsMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse server message");
                continue;
            }
        };

        match parsed {
            WsMessage::RegisterAck { status } => {
                tracing::info!(status = %status, "registration acknowledged");
            }
            WsMessage::Ping { timestamp } => {
                let pong = WsMessage::Pong {
                    timestamp,
                    active_requests: active_requests.load(Ordering::Relaxed),
                    queue_depth: 0,
                };
                let pong_text = serde_json::to_string(&pong)?;
                ws_sink
                    .send(tungstenite::Message::Text(pong_text.into()))
                    .await?;
            }
            WsMessage::InferenceRequest {
                id,
                model,
                messages,
                params,
                timeout_ms,
            } => {
                let client = http_client.clone();
                let base_url = config.inference_server.base_url.clone();
                let server_type = config.inference_server.server_type.clone();
                let active = Arc::clone(&active_requests);
                let sem = Arc::clone(&semaphore);

                // We need to send the response back — clone the sink
                // via a channel approach
                let (resp_tx, mut resp_rx) = tokio::sync::mpsc::channel::<String>(1);

                tokio::spawn(async move {
                    let _permit = sem.acquire().await;
                    active.fetch_add(1, Ordering::Relaxed);

                    let start = std::time::Instant::now();
                    let result = dispatch_local(
                        &client,
                        &base_url,
                        &server_type,
                        &model,
                        &messages,
                        &params,
                        timeout_ms,
                    )
                    .await;
                    let duration_ms = start.elapsed().as_millis() as u64;

                    active.fetch_sub(1, Ordering::Relaxed);

                    let response_msg = match result {
                        Ok((msg, usage, stop_reason)) => WsMessage::InferenceResponse {
                            id,
                            status: "ok".to_string(),
                            message: msg,
                            usage,
                            duration_ms,
                            stop_reason,
                        },
                        Err(e) => WsMessage::InferenceError {
                            id,
                            error: "dispatch_error".to_string(),
                            message: format!("{e}"),
                            retryable: true,
                        },
                    };

                    if let Ok(text) = serde_json::to_string(&response_msg) {
                        let _ = resp_tx.send(text).await;
                    }
                });

                // Forward the response back through the WebSocket
                if let Some(resp_text) = resp_rx.recv().await {
                    ws_sink
                        .send(tungstenite::Message::Text(resp_text.into()))
                        .await?;
                }
            }
            _ => {
                tracing::debug!("ignoring unexpected message from server");
            }
        }
    }

    Ok(())
}

// ── Local dispatch to inference server ────────────────────────────

async fn dispatch_local(
    client: &reqwest::Client,
    base_url: &str,
    server_type: &str,
    model: &str,
    messages: &[serde_json::Value],
    params: &serde_json::Value,
    timeout_ms: u64,
) -> Result<(InferenceMessage, InferenceUsage, Option<String>), Box<dyn std::error::Error + Send + Sync>>
{
    match server_type {
        "openai_compatible" | "vllm" | "ollama" | "litellm" => {
            dispatch_openai_compatible(client, base_url, model, messages, params, timeout_ms).await
        }
        other => Err(format!("unsupported inference server type: {other}").into()),
    }
}

/// Dispatch to an OpenAI-compatible endpoint (vLLM, Ollama, LiteLLM).
async fn dispatch_openai_compatible(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    messages: &[serde_json::Value],
    params: &serde_json::Value,
    timeout_ms: u64,
) -> Result<(InferenceMessage, InferenceUsage, Option<String>), Box<dyn std::error::Error + Send + Sync>>
{
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
    });

    // Merge params
    if let Some(max_tokens) = params.get("max_tokens").and_then(|v| v.as_i64()) {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(temp) = params.get("temperature").and_then(|v| v.as_f64()) {
        body["temperature"] = serde_json::json!(temp);
    }

    let response = client
        .post(&url)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("inference server returned {status}: {text}").into());
    }

    let resp: serde_json::Value = response.json().await?;

    // Parse OpenAI-compatible response
    let choice = resp["choices"]
        .get(0)
        .ok_or("no choices in response")?;

    let content = choice["message"]["content"]
        .as_str()
        .map(|s| s.to_string());
    let role = choice["message"]["role"]
        .as_str()
        .unwrap_or("assistant")
        .to_string();
    let tool_calls = choice["message"]["tool_calls"]
        .as_array()
        .map(|arr| arr.clone());
    let finish_reason = choice["finish_reason"]
        .as_str()
        .map(|s| s.to_string());

    let usage_obj = &resp["usage"];
    let usage = InferenceUsage {
        input_tokens: usage_obj["prompt_tokens"].as_i64().unwrap_or(0) as i32,
        output_tokens: usage_obj["completion_tokens"].as_i64().unwrap_or(0) as i32,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };

    let msg = InferenceMessage {
        role,
        content,
        tool_calls,
    };

    Ok((msg, usage, finish_reason))
}

// ── Helpers ───────────────────────────────────────────────────────

fn gethostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_string())
}

