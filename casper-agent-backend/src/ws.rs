use casper_wire::{
    InferenceDone, InferenceError, InferenceMessage, InferenceResponse, InferenceToolCall,
    InferenceUsage, Pong, Register, RegisterAckConfig, WsMessage,
};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
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
        .header(
            "Sec-WebSocket-Key",
            tungstenite::handshake::client::generate_key(),
        )
        .body(())?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
    tracing::info!("connected to Casper");

    let (mut ws_sink, mut ws_source) = ws_stream.split();

    // Send registration (server knows our backend_id from the key)
    let register = WsMessage::Register(Register {
        hostname: gethostname(),
        gpu_info: None,
    });
    ws_sink
        .send(tungstenite::Message::Text(
            serde_json::to_string(&register)?.into(),
        ))
        .await?;
    tracing::info!("registration sent, waiting for ack...");

    // Wait for RegisterAck with server-pushed config
    let server_config = wait_for_ack(&mut ws_source).await?;

    let inference_base_url = inference_url.to_string();
    let max_concurrent = if server_config.max_concurrent > 0 {
        server_config.max_concurrent
    } else {
        8
    };

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
            Ok(tungstenite::Message::Close(_)) => {
                tracing::info!("server closed connection");
                break;
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::error!(error = %e, "WebSocket error");
                return Err(e.into());
            }
        };

        let parsed: WsMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "failed to parse message");
                continue;
            }
        };

        match parsed {
            WsMessage::Ping { timestamp } => {
                let pong = WsMessage::Pong(Pong {
                    timestamp,
                    active_requests: active_requests.load(Ordering::Relaxed),
                    queue_depth: 0,
                });
                ws_sink
                    .send(tungstenite::Message::Text(
                        serde_json::to_string(&pong)?.into(),
                    ))
                    .await?;
            }
            WsMessage::InferenceRequest(req) => {
                let client = http_client.clone();
                let base_url = inference_base_url.clone();
                let active = Arc::clone(&active_requests);
                let sem = Arc::clone(&semaphore);
                let req_id = req.id.clone();
                let wants_stream = req
                    .params
                    .get("stream")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let (resp_tx, mut resp_rx) = tokio::sync::mpsc::channel::<String>(64);

                tokio::spawn(async move {
                    let _permit = sem.acquire().await;
                    active.fetch_add(1, Ordering::Relaxed);
                    let start = std::time::Instant::now();

                    if wants_stream {
                        let result = dispatch_local_stream(
                            &client,
                            &base_url,
                            &req.model,
                            &req.messages,
                            &req.params,
                            req.extra.as_ref(),
                            req.timeout_ms,
                            &req_id,
                            &resp_tx,
                        )
                        .await;
                        let duration_ms = start.elapsed().as_millis() as u64;
                        if let Err(e) = result {
                            let msg = WsMessage::InferenceError(InferenceError {
                                id: req_id,
                                error: "dispatch_error".to_string(),
                                message: Some(format!("{e}")),
                                retryable: true,
                            });
                            if let Ok(text) = serde_json::to_string(&msg) {
                                let _ = resp_tx.send(text).await;
                            }
                        } else {
                            let done = WsMessage::InferenceDone(InferenceDone {
                                id: req_id,
                                usage: None,
                                duration_ms: Some(duration_ms),
                            });
                            if let Ok(text) = serde_json::to_string(&done) {
                                let _ = resp_tx.send(text).await;
                            }
                        }
                    } else {
                        let result = dispatch_local(
                            &client,
                            &base_url,
                            &req.model,
                            &req.messages,
                            &req.params,
                            req.extra.as_ref(),
                            req.timeout_ms,
                        )
                        .await;
                        let duration_ms = start.elapsed().as_millis() as u64;

                        let response_msg = match result {
                            Ok((msg, usage, stop_reason)) => {
                                WsMessage::InferenceResponse(InferenceResponse {
                                    id: req_id,
                                    status: "ok".to_string(),
                                    message: Some(msg),
                                    usage: Some(usage),
                                    duration_ms: Some(duration_ms),
                                    stop_reason,
                                })
                            }
                            Err(e) => WsMessage::InferenceError(InferenceError {
                                id: req_id,
                                error: "dispatch_error".to_string(),
                                message: Some(format!("{e}")),
                                retryable: true,
                            }),
                        };

                        if let Ok(text) = serde_json::to_string(&response_msg) {
                            let _ = resp_tx.send(text).await;
                        }
                    }

                    active.fetch_sub(1, Ordering::Relaxed);
                });

                while let Some(resp_text) = resp_rx.recv().await {
                    ws_sink
                        .send(tungstenite::Message::Text(resp_text.into()))
                        .await?;
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn wait_for_ack(
    ws_source: &mut futures::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Result<RegisterAckConfig, Box<dyn std::error::Error>> {
    let timeout = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(msg) = ws_source.next().await {
            if let Ok(tungstenite::Message::Text(text)) = msg
                && let Ok(WsMessage::RegisterAck(ack)) = serde_json::from_str(text.as_ref())
            {
                if ack.status == "ok" {
                    return Ok(ack.config);
                }
                return Err(format!("registration rejected: {}", ack.status).into());
            }
        }
        Err("connection closed before register_ack".into())
    })
    .await;

    match timeout {
        Ok(result) => result,
        Err(_) => Err("timed out waiting for register_ack".into()),
    }
}

// ── Local dispatch ───────────────────────────────────────────────

async fn dispatch_local(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    messages: &[serde_json::Value],
    params: &serde_json::Value,
    extra: Option<&serde_json::Value>,
    timeout_ms: u64,
) -> Result<
    (InferenceMessage, InferenceUsage, Option<String>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    // If there's a system prompt in extra, prepend it as a system message
    let mut all_messages = Vec::new();
    if let Some(system) = extra.and_then(|e| e.get("system")).and_then(|s| s.as_str()) {
        all_messages.push(serde_json::json!({ "role": "system", "content": system }));
    }
    all_messages.extend_from_slice(messages);

    let mut body = serde_json::json!({ "model": model, "messages": all_messages, "stream": false });
    if let Some(v) = params.get("max_tokens").and_then(|v| v.as_i64()) {
        body["max_tokens"] = serde_json::json!(v);
    }
    if let Some(v) = params.get("temperature").and_then(|v| v.as_f64()) {
        body["temperature"] = serde_json::json!(v);
    }
    if let Some(tools) = extra
        .and_then(|e| e.get("tools"))
        .and_then(|t| t.as_array())
        && !tools.is_empty()
    {
        body["tools"] = serde_json::json!(tools);
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
    let choice = resp["choices"].get(0).ok_or("no choices")?;

    Ok((
        InferenceMessage {
            role: choice["message"]["role"]
                .as_str()
                .unwrap_or("assistant")
                .to_string(),
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

#[allow(clippy::too_many_arguments)]
async fn dispatch_local_stream(
    client: &reqwest::Client,
    base_url: &str,
    model: &str,
    messages: &[serde_json::Value],
    params: &serde_json::Value,
    extra: Option<&serde_json::Value>,
    timeout_ms: u64,
    req_id: &str,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    let mut all_messages = Vec::new();
    if let Some(system) = extra.and_then(|e| e.get("system")).and_then(|s| s.as_str()) {
        all_messages.push(serde_json::json!({ "role": "system", "content": system }));
    }
    all_messages.extend_from_slice(messages);

    let mut body = serde_json::json!({
        "model": model,
        "messages": all_messages,
        "stream": true,
    });
    if let Some(v) = params.get("max_tokens").and_then(|v| v.as_i64()) {
        body["max_tokens"] = serde_json::json!(v);
    }
    if let Some(v) = params.get("temperature").and_then(|v| v.as_f64()) {
        body["temperature"] = serde_json::json!(v);
    }
    if let Some(tools) = extra
        .and_then(|e| e.get("tools"))
        .and_then(|t| t.as_array())
        && !tools.is_empty()
    {
        body["tools"] = serde_json::json!(tools);
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

    let mut byte_stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut tool_acc: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();

    while let Some(chunk) = byte_stream.next().await {
        let bytes = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let data_str = match line.strip_prefix("data: ") {
                Some(d) => d,
                None => continue,
            };
            if data_str == "[DONE]" {
                return Ok(());
            }

            let data: serde_json::Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(choice) = data["choices"].as_array().and_then(|a| a.first()) {
                let delta = &choice["delta"];

                // Text content
                if let Some(c) = delta["content"].as_str()
                    && !c.is_empty()
                {
                    let msg = WsMessage::InferenceChunk {
                        id: req_id.to_string(),
                        delta: c.to_string(),
                    };
                    if let Ok(text) = serde_json::to_string(&msg) {
                        let _ = tx.send(text).await;
                    }
                }

                // Reasoning/thinking content
                if let Some(r) = delta["reasoning_content"].as_str()
                    && !r.is_empty()
                {
                    let msg = WsMessage::InferenceThinking {
                        id: req_id.to_string(),
                        delta: r.to_string(),
                    };
                    if let Ok(text) = serde_json::to_string(&msg) {
                        let _ = tx.send(text).await;
                    }
                }

                // Tool calls
                if let Some(tcs) = delta["tool_calls"].as_array() {
                    for tc in tcs {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        if let Some(id) = tc["id"].as_str() {
                            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
                            tool_acc
                                .entry(idx)
                                .or_insert_with(|| (id.to_string(), name, String::new()));
                            // Send tool call start
                            let msg = WsMessage::InferenceToolCall(InferenceToolCall {
                                id: req_id.to_string(),
                                tool_index: idx,
                                tool_call_id: id.to_string(),
                                name: tool_acc[&idx].1.clone(),
                            });
                            if let Ok(text) = serde_json::to_string(&msg) {
                                let _ = tx.send(text).await;
                            }
                        }
                        if let Some(args) = tc["function"]["arguments"].as_str()
                            && !args.is_empty()
                        {
                            if let Some(entry) = tool_acc.get_mut(&idx) {
                                entry.2.push_str(args);
                            }
                            let msg = WsMessage::InferenceToolCallDelta {
                                id: req_id.to_string(),
                                tool_index: idx,
                                arguments_delta: args.to_string(),
                            };
                            if let Ok(text) = serde_json::to_string(&msg) {
                                let _ = tx.send(text).await;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn gethostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| "unknown".to_string())
}
