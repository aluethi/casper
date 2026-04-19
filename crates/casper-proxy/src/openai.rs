use casper_base::CasperError;
use futures::StreamExt;
use serde_json::json;
use tokio::sync::mpsc;

use crate::types::{LlmRequest, LlmResponse, MessageRole, StreamEvent};

/// Whether to use Azure OpenAI's URL and auth format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenAiVariant {
    /// Standard OpenAI / compatible: `{base}/v1/chat/completions`, `Authorization: Bearer`
    Standard,
    /// Azure OpenAI: base_url IS the full endpoint, `api-key` header
    Azure,
}

/// Send a request to an OpenAI-compatible Chat Completions API and parse the response.
pub async fn call(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
) -> Result<LlmResponse, CasperError> {
    call_with_variant(client, base_url, api_key, request, OpenAiVariant::Standard).await
}

/// Azure OpenAI variant.
pub async fn call_azure(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
) -> Result<LlmResponse, CasperError> {
    call_with_variant(client, base_url, api_key, request, OpenAiVariant::Azure).await
}

async fn call_with_variant(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
    variant: OpenAiVariant,
) -> Result<LlmResponse, CasperError> {
    let mut messages = build_messages(&request.messages);

    // OpenAI uses system messages inline — if extra.system is set,
    // prepend it as a system message (this is how the agent engine passes
    // the assembled system prompt).
    if let Some(system) = request.extra.get("system").and_then(|v| v.as_str()) {
        if !system.is_empty() {
            messages.insert(0, json!({ "role": "system", "content": system }));
        }
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
    });

    if let Some(max_tokens) = request.max_tokens {
        let key = if variant == OpenAiVariant::Azure { "max_completion_tokens" } else { "max_tokens" };
        tracing::debug!(variant = ?variant, key, max_tokens, "setting max tokens param");
        body[key] = json!(max_tokens);
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }

    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
    }

    // Merge extra params (skip keys already handled above)
    if let serde_json::Value::Object(ref extra) = request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in extra {
            // "system" is handled as a message; "max_tokens" is handled via the dedicated field
            if k == "system" || k == "max_tokens" || k == "max_completion_tokens" {
                continue;
            }
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    let url = match variant {
        OpenAiVariant::Azure => {
            // Azure: base_url is the full endpoint including api-version
            // e.g. https://myresource.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-10-21
            base_url.to_string()
        }
        OpenAiVariant::Standard => {
            let base = base_url.trim_end_matches('/');
            if base.ends_with("/v1") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/v1/chat/completions")
            }
        }
    };

    let mut http_req = client
        .post(&url)
        .header("content-type", "application/json");

    http_req = match variant {
        OpenAiVariant::Azure => http_req.header("api-key", api_key),
        OpenAiVariant::Standard => http_req.header("authorization", format!("Bearer {api_key}")),
    };

    let response = http_req
        .json(&body)
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("OpenAI request failed: {e}")))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Failed to read OpenAI response: {e}")))?;

    if !status.is_success() {
        return Err(map_openai_error(status.as_u16(), &response_text));
    }

    let resp: serde_json::Value = serde_json::from_str(&response_text)
        .map_err(|e| CasperError::BadGateway(format!("Invalid OpenAI JSON: {e}")))?;

    parse_response(&resp)
}

/// Streaming variant: sends events to `tx` while accumulating the full response.
pub async fn call_stream(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
    tx: mpsc::Sender<StreamEvent>,
) -> Result<LlmResponse, CasperError> {
    call_stream_with_variant(client, base_url, api_key, request, tx, OpenAiVariant::Standard).await
}

/// Azure OpenAI streaming variant.
pub async fn call_stream_azure(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
    tx: mpsc::Sender<StreamEvent>,
) -> Result<LlmResponse, CasperError> {
    call_stream_with_variant(client, base_url, api_key, request, tx, OpenAiVariant::Azure).await
}

async fn call_stream_with_variant(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
    tx: mpsc::Sender<StreamEvent>,
    variant: OpenAiVariant,
) -> Result<LlmResponse, CasperError> {
    let mut messages = build_messages(&request.messages);

    if let Some(system) = request.extra.get("system").and_then(|v| v.as_str()) {
        if !system.is_empty() {
            messages.insert(0, json!({ "role": "system", "content": system }));
        }
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });

    if let Some(max_tokens) = request.max_tokens {
        let key = if variant == OpenAiVariant::Azure { "max_completion_tokens" } else { "max_tokens" };
        body[key] = json!(max_tokens);
    }
    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
    }

    if let serde_json::Value::Object(ref extra) = request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in extra {
            if k == "system" || k == "max_tokens" || k == "max_completion_tokens" { continue; }
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    let url = match variant {
        OpenAiVariant::Azure => base_url.to_string(),
        OpenAiVariant::Standard => {
            let base = base_url.trim_end_matches('/');
            if base.ends_with("/v1") { format!("{base}/chat/completions") }
            else { format!("{base}/v1/chat/completions") }
        }
    };

    // Override the client's global timeout — streaming responses can run for minutes
    let mut http_req = client
        .post(&url)
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(600));

    http_req = match variant {
        OpenAiVariant::Azure => http_req.header("api-key", api_key),
        OpenAiVariant::Standard => http_req.header("authorization", format!("Bearer {api_key}")),
    };

    let response = http_req
        .json(&body)
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("OpenAI stream request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(map_openai_error(status.as_u16(), &text));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    let mut content_parts = Vec::new();
    let mut thinking_parts = Vec::new();
    let mut model = String::new();
    let mut input_tokens = 0i32;
    let mut output_tokens = 0i32;
    let mut cache_read_tokens: Option<i32> = None;
    let mut finish_reason: Option<String> = None;

    // Tool call accumulation: index → (id, name, arguments_buf)
    let mut tool_acc: std::collections::HashMap<usize, (String, String, String)> = std::collections::HashMap::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| CasperError::BadGateway(format!("Stream read error: {e}")))?;
        let chunk_str = String::from_utf8_lossy(&bytes);
        buffer.push_str(&chunk_str.replace("\r\n", "\n").replace('\r', "\n"));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();

            let line = line.trim();
            if line.is_empty() || line.starts_with(':') { continue; }
            let data_str = match line.strip_prefix("data: ") {
                Some(d) => d,
                None => continue,
            };
            if data_str == "[DONE]" { continue; }

            let data: serde_json::Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if let Some(m) = data["model"].as_str() {
                if model.is_empty() { model = m.to_string(); }
            }

            // Usage (sent in the final chunk)
            if let Some(u) = data.get("usage").filter(|v| v.is_object()) {
                input_tokens = u["prompt_tokens"].as_i64().unwrap_or(0) as i32;
                output_tokens = u["completion_tokens"].as_i64().unwrap_or(0) as i32;
                cache_read_tokens = u["prompt_tokens_details"]["cached_tokens"].as_i64().map(|v| v as i32);
            }

            if let Some(choice) = data["choices"].as_array().and_then(|a| a.first()) {
                let delta = &choice["delta"];

                // Content
                if let Some(c) = delta["content"].as_str() {
                    if !c.is_empty() {
                        content_parts.push(c.to_string());
                        let _ = tx.send(StreamEvent::ContentDelta { delta: c.to_string() }).await;
                    }
                }

                // Thinking / reasoning
                if let Some(r) = delta["reasoning_content"].as_str() {
                    if !r.is_empty() {
                        thinking_parts.push(r.to_string());
                        let _ = tx.send(StreamEvent::Thinking { delta: r.to_string() }).await;
                    }
                }

                // Tool calls (streamed incrementally by index)
                if let Some(tcs) = delta["tool_calls"].as_array() {
                    for tc in tcs {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        let entry = tool_acc.entry(idx).or_insert_with(|| (String::new(), String::new(), String::new()));
                        if let Some(id) = tc["id"].as_str() { entry.0 = id.to_string(); }
                        if let Some(name) = tc["function"]["name"].as_str() { entry.1 = name.to_string(); }
                        if let Some(args) = tc["function"]["arguments"].as_str() { entry.2.push_str(args); }
                    }
                }

                // Finish reason
                if let Some(fr) = choice["finish_reason"].as_str() {
                    finish_reason = Some(fr.to_string());
                }
            }
        }
    }

    // Emit accumulated tool calls
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut indices: Vec<usize> = tool_acc.keys().copied().collect();
    indices.sort();
    for idx in indices {
        let (id, name, args) = tool_acc.remove(&idx).unwrap();
        let input: serde_json::Value = serde_json::from_str(&args).unwrap_or(json!({}));
        tool_calls.push(json!({
            "id": id,
            "type": "function",
            "function": { "name": name, "arguments": args }
        }));
        let _ = tx.send(StreamEvent::ToolCallStart { id, name, input }).await;
    }

    let content = content_parts.join("");
    let thinking = if thinking_parts.is_empty() { None } else { Some(thinking_parts.join("")) };

    Ok(LlmResponse {
        content,
        role: MessageRole::Assistant,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens: None,
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        finish_reason,
        thinking,
    })
}

/// Build OpenAI-format messages array from internal Message structs.
///
/// Handles three special cases beyond simple {role, content} messages:
/// - Assistant messages with tool_calls: content has `tool_calls` + `content` fields
/// - Tool result messages: content has `tool_call_id` + `content` fields
/// - Everything else: passed through as {role, content}
fn build_messages(messages: &[crate::types::Message]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|msg| {
            if msg.role == MessageRole::Assistant && msg.content.get("tool_calls").is_some() {
                // Assistant message with tool calls
                let mut m = json!({ "role": "assistant" });
                if let Some(c) = msg.content.get("content") {
                    if !c.is_null() {
                        m["content"] = c.clone();
                    }
                }
                if let Some(tc) = msg.content.get("tool_calls") {
                    m["tool_calls"] = tc.clone();
                }
                m
            } else if msg.role == MessageRole::Tool {
                // Tool result message
                let mut m = json!({ "role": "tool" });
                if let Some(id) = msg.content.get("tool_call_id") {
                    m["tool_call_id"] = id.clone();
                }
                if let Some(c) = msg.content.get("content") {
                    m["content"] = c.clone();
                }
                m
            } else {
                json!({
                    "role": msg.role,
                    "content": msg.content,
                })
            }
        })
        .collect()
}

/// Parse an OpenAI Chat Completions response into our LlmResponse.
fn parse_response(resp: &serde_json::Value) -> Result<LlmResponse, CasperError> {
    let choice = resp["choices"]
        .as_array()
        .and_then(|arr| arr.first())
        .ok_or_else(|| CasperError::BadGateway("OpenAI response has no choices".into()))?;

    let message = &choice["message"];

    let content = message["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    let role: MessageRole = message["role"]
        .as_str()
        .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_string())).ok())
        .unwrap_or(MessageRole::Assistant);

    // Extract thinking / reasoning content (OpenAI o-series, DeepSeek, etc.)
    let thinking = message.get("reasoning_content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Extract tool calls if present
    let tool_calls = message["tool_calls"]
        .as_array()
        .map(|arr| arr.to_vec())
        .filter(|arr| !arr.is_empty());

    let finish_reason = choice["finish_reason"]
        .as_str()
        .map(|s| s.to_string());

    // Usage
    let usage = &resp["usage"];
    let input_tokens = usage["prompt_tokens"].as_i64().unwrap_or(0) as i32;
    let output_tokens = usage["completion_tokens"].as_i64().unwrap_or(0) as i32;

    // Some OpenAI-compatible APIs include cache token info
    let cache_read_tokens = usage["prompt_tokens_details"]["cached_tokens"]
        .as_i64()
        .map(|v| v as i32);

    let model = resp["model"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    Ok(LlmResponse {
        content,
        role,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens: None,
        tool_calls,
        finish_reason,
        thinking,
    })
}

/// Map OpenAI HTTP status codes to CasperError variants.
fn map_openai_error(status: u16, body: &str) -> CasperError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| body.chars().take(500).collect());

    match status {
        401 => CasperError::BadGateway(format!("OpenAI auth error: {message}")),
        429 => CasperError::RateLimited,
        500..=599 => CasperError::BadGateway(format!("OpenAI server error ({status}): {message}")),
        _ => CasperError::BadGateway(format!("OpenAI error ({status}): {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn build_messages_preserves_system() {
        let messages = vec![
            Message {
                role: MessageRole::System,
                content: json!("You are helpful"),
            },
            Message {
                role: MessageRole::User,
                content: json!("Hi"),
            },
        ];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0]["role"], "system");
        assert_eq!(built[1]["role"], "user");
    }

    #[test]
    fn build_messages_assistant_with_tool_calls() {
        let messages = vec![Message {
            role: MessageRole::Assistant,
            content: json!({
                "content": "Let me check.",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "search", "arguments": "{\"q\":\"test\"}"}
                }]
            }),
        }];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["role"], "assistant");
        assert_eq!(built[0]["content"], "Let me check.");
        assert_eq!(built[0]["tool_calls"][0]["function"]["name"], "search");
    }

    #[test]
    fn build_messages_tool_result() {
        let messages = vec![Message {
            role: MessageRole::Tool,
            content: json!({"tool_call_id": "call_1", "content": "result data"}),
        }];

        let built = build_messages(&messages);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["role"], "tool");
        assert_eq!(built[0]["tool_call_id"], "call_1");
        assert_eq!(built[0]["content"], "result data");
    }

    #[test]
    fn parse_openai_response() {
        let resp = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 8,
                "total_tokens": 28
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "Hello! How can I help?");
        assert_eq!(llm.role, MessageRole::Assistant);
        assert_eq!(llm.model, "gpt-4o");
        assert_eq!(llm.input_tokens, 20);
        assert_eq!(llm.output_tokens, 8);
        assert!(llm.cache_read_tokens.is_none());
        assert_eq!(llm.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn parse_openai_tool_call_response() {
        let resp = json!({
            "id": "chatcmpl-def456",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\":\"Zurich\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "prompt_tokens": 30,
                "completion_tokens": 15,
                "total_tokens": 45
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "");
        assert!(llm.tool_calls.is_some());
        assert_eq!(llm.tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(llm.finish_reason, Some("tool_calls".to_string()));
    }

    #[test]
    fn parse_openai_cached_tokens() {
        let resp = json!({
            "id": "chatcmpl-ghi789",
            "object": "chat.completion",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Cached response"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "total_tokens": 110,
                "prompt_tokens_details": {
                    "cached_tokens": 80
                }
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.cache_read_tokens, Some(80));
    }

    #[test]
    fn parse_empty_choices_fails() {
        let resp = json!({
            "id": "chatcmpl-err",
            "object": "chat.completion",
            "choices": [],
            "usage": {}
        });

        let result = parse_response(&resp);
        assert!(result.is_err());
    }

    #[test]
    fn error_mapping() {
        let err = map_openai_error(429, r#"{"error":{"message":"rate limited"}}"#);
        assert!(matches!(err, CasperError::RateLimited));

        let err = map_openai_error(500, r#"{"error":{"message":"server error"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));
    }
}
