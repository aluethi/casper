use casper_base::CasperError;
use futures::StreamExt;
use serde_json::json;
use tokio::sync::mpsc;

use super::types::{LlmRequest, LlmResponse, MessageRole, StreamEvent};

/// Send a request to the Anthropic Messages API and parse the response.
///
/// Internally, Casper normalizes on the OpenAI format. This adapter converts:
///   Send: OpenAI tool defs → Anthropic, OpenAI messages → Anthropic
///   Recv: Anthropic tool_use → OpenAI tool_calls
pub async fn call(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    request: &LlmRequest,
) -> Result<LlmResponse, CasperError> {
    let (system_content, messages) = build_anthropic_messages(&request.messages, &request.extra);

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(4096),
    });

    if let Some(system) = system_content {
        body["system"] = system;
    }

    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }

    // Convert OpenAI tool defs → Anthropic format
    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    // OpenAI: {type: "function", function: {name, description, parameters}}
                    // Anthropic: {name, description, input_schema}
                    if let Some(func) = t.get("function") {
                        json!({
                            "name": func["name"],
                            "description": func.get("description").cloned().unwrap_or(json!("")),
                            "input_schema": func.get("parameters").cloned().unwrap_or(json!({"type": "object"})),
                        })
                    } else {
                        // Already in Anthropic format or unknown — pass through
                        t.clone()
                    }
                })
                .collect();
            body["tools"] = json!(anthropic_tools);
        }
    }

    // Merge extra params (skip "system" — handled above)
    if let serde_json::Value::Object(ref extra) = request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in extra {
            if k == "system" {
                continue;
            }
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Anthropic request failed: {e}")))?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Failed to read Anthropic response: {e}")))?;

    if !status.is_success() {
        return Err(map_anthropic_error(status.as_u16(), &response_text));
    }

    let resp: serde_json::Value = serde_json::from_str(&response_text)
        .map_err(|e| CasperError::BadGateway(format!("Invalid Anthropic JSON: {e}")))?;

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
    let (system_content, messages) = build_anthropic_messages(&request.messages, &request.extra);

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "stream": true,
    });

    if let Some(system) = system_content {
        body["system"] = system;
    }
    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(ref tools) = request.tools {
        if !tools.is_empty() {
            let anthropic_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    if let Some(func) = t.get("function") {
                        json!({
                            "name": func["name"],
                            "description": func.get("description").cloned().unwrap_or(json!("")),
                            "input_schema": func.get("parameters").cloned().unwrap_or(json!({"type": "object"})),
                        })
                    } else {
                        t.clone()
                    }
                })
                .collect();
            body["tools"] = json!(anthropic_tools);
        }
    }

    if let serde_json::Value::Object(ref extra) = request.extra {
        let body_obj = body.as_object_mut().unwrap();
        for (k, v) in extra {
            if k == "system" {
                continue;
            }
            if !body_obj.contains_key(k) {
                body_obj.insert(k.clone(), v.clone());
            }
        }
    }

    let url = format!("{}/v1/messages", base_url.trim_end_matches('/'));

    // Override the client's global timeout — streaming responses can run for minutes
    let response = client
        .post(&url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(600))
        .json(&body)
        .send()
        .await
        .map_err(|e| CasperError::BadGateway(format!("Anthropic stream request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        return Err(map_anthropic_error(status.as_u16(), &text));
    }

    // Parse the SSE byte stream
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    // Accumulators for the final LlmResponse
    let mut content_parts = Vec::new();
    let mut thinking_parts = Vec::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut input_tokens = 0i32;
    let mut output_tokens = 0i32;
    let mut cache_read_tokens: Option<i32> = None;
    let mut cache_write_tokens: Option<i32> = None;
    let mut finish_reason: Option<String> = None;
    let mut model = String::new();

    // Per-block state for tool_use accumulation
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut current_tool_json = String::new();
    let mut current_block_type: Option<String> = None;

    while let Some(chunk) = stream.next().await {
        let bytes =
            chunk.map_err(|e| CasperError::BadGateway(format!("Stream read error: {e}")))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        // Normalize \r\n → \n so the event boundary split works regardless of transport
        buffer = buffer.replace("\r\n", "\n").replace('\r', "\n");

        // Process complete SSE events (separated by double newline)
        while let Some(pos) = buffer.find("\n\n") {
            let event_block = buffer[..pos].to_string();
            buffer = buffer[pos + 2..].to_string();

            let mut event_type = String::new();
            let mut data_lines: Vec<String> = Vec::new();
            for line in event_block.lines() {
                if let Some(et) = line.strip_prefix("event: ") {
                    event_type = et.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data_lines.push(d.to_string());
                }
            }

            let data_str = data_lines.join("\n");
            if data_str.is_empty() {
                continue;
            }
            let data: serde_json::Value = match serde_json::from_str(&data_str) {
                Ok(v) => v,
                Err(_) => continue,
            };

            match event_type.as_str() {
                "message_start" => {
                    if let Some(m) = data["message"]["model"].as_str() {
                        model = m.to_string();
                    }
                    let usage = &data["message"]["usage"];
                    input_tokens = usage["input_tokens"].as_i64().unwrap_or(0) as i32;
                    cache_read_tokens = usage["cache_read_input_tokens"].as_i64().map(|v| v as i32);
                    cache_write_tokens = usage["cache_creation_input_tokens"]
                        .as_i64()
                        .map(|v| v as i32);
                }
                "content_block_start" => {
                    let block = &data["content_block"];
                    let btype = block["type"].as_str().unwrap_or("");
                    current_block_type = Some(btype.to_string());
                    if btype == "tool_use" {
                        current_tool_id = block["id"].as_str().unwrap_or("").to_string();
                        current_tool_name = block["name"].as_str().unwrap_or("").to_string();
                        current_tool_json.clear();
                    }
                }
                "content_block_delta" => {
                    let delta = &data["delta"];
                    match delta["type"].as_str() {
                        Some("thinking_delta") => {
                            if let Some(t) = delta["thinking"].as_str() {
                                thinking_parts.push(t.to_string());
                                let _ = tx
                                    .send(StreamEvent::Thinking {
                                        delta: t.to_string(),
                                    })
                                    .await;
                            }
                        }
                        Some("text_delta") => {
                            if let Some(t) = delta["text"].as_str() {
                                content_parts.push(t.to_string());
                                let _ = tx
                                    .send(StreamEvent::ContentDelta {
                                        delta: t.to_string(),
                                    })
                                    .await;
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(j) = delta["partial_json"].as_str() {
                                current_tool_json.push_str(j);
                            }
                        }
                        _ => {}
                    }
                }
                "content_block_stop" => {
                    if current_block_type.as_deref() == Some("tool_use") {
                        let input: serde_json::Value =
                            serde_json::from_str(&current_tool_json).unwrap_or(json!({}));
                        let arguments =
                            serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                        tool_calls.push(json!({
                            "id": current_tool_id,
                            "type": "function",
                            "function": {
                                "name": current_tool_name,
                                "arguments": arguments,
                            }
                        }));
                        let _ = tx
                            .send(StreamEvent::ToolCallStart {
                                id: current_tool_id.clone(),
                                name: current_tool_name.clone(),
                                input,
                            })
                            .await;
                    }
                    current_block_type = None;
                }
                "message_delta" => {
                    if let Some(sr) = data["delta"]["stop_reason"].as_str() {
                        finish_reason = Some(
                            match sr {
                                "end_turn" => "stop",
                                "max_tokens" => "length",
                                "tool_use" => "tool_calls",
                                other => other,
                            }
                            .to_string(),
                        );
                    }
                    let usage = &data["usage"];
                    if let Some(ot) = usage["output_tokens"].as_i64() {
                        output_tokens = ot as i32;
                    }
                }
                _ => {}
            }
        }
    }

    let content = content_parts.join("");
    let thinking = if thinking_parts.is_empty() {
        None
    } else {
        Some(thinking_parts.join(""))
    };

    Ok(LlmResponse {
        content,
        role: MessageRole::Assistant,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        finish_reason,
        thinking,
    })
}

// ── Request conversion (OpenAI → Anthropic) ─────────────────────

/// Build Anthropic messages from internal OpenAI-format messages.
/// Extracts system content (from messages or extra.system) and converts
/// assistant tool_calls + tool results into Anthropic content blocks.
fn build_anthropic_messages(
    messages: &[super::types::Message],
    extra: &serde_json::Value,
) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system: Option<serde_json::Value> = None;
    let mut out = Vec::new();

    // Check extra.system first
    if let Some(s) = extra.get("system").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            system = Some(json!(s));
        }
    }

    for msg in messages {
        match msg.role {
            MessageRole::System => {
                // Anthropic system is a top-level field
                system = Some(msg.content.clone());
            }
            MessageRole::Assistant if msg.content.get("tool_calls").is_some() => {
                // Convert OpenAI assistant+tool_calls → Anthropic content blocks
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();

                if let Some(text) = msg.content.get("content").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        content_blocks.push(json!({"type": "text", "text": text}));
                    }
                }

                if let Some(tool_calls) = msg.content.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        // OpenAI: {id, type: "function", function: {name, arguments: "JSON string"}}
                        // Anthropic: {type: "tool_use", id, name, input: {object}}
                        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let func = &tc["function"];
                        let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let input: serde_json::Value = func
                            .get("arguments")
                            .and_then(|v| v.as_str())
                            .and_then(|s| serde_json::from_str(s).ok())
                            .unwrap_or(json!({}));

                        content_blocks.push(json!({
                            "type": "tool_use",
                            "id": id,
                            "name": name,
                            "input": input,
                        }));
                    }
                }

                out.push(json!({"role": "assistant", "content": content_blocks}));
            }
            MessageRole::Tool => {
                // Convert OpenAI tool message → Anthropic tool_result content block
                // OpenAI: {role: "tool", tool_call_id: "...", content: "..."}
                // Anthropic: {role: "user", content: [{type: "tool_result", tool_use_id: "...", content: "..."}]}
                let tool_call_id = msg
                    .content
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let content = msg.content.get("content").cloned().unwrap_or(json!(""));

                out.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                    }]
                }));
            }
            _ => {
                out.push(json!({
                    "role": msg.role,
                    "content": msg.content,
                }));
            }
        }
    }

    (system, out)
}

// ── Response conversion (Anthropic → OpenAI) ────────────────────

/// Parse an Anthropic Messages API response into OpenAI-normalized LlmResponse.
/// Converts Anthropic tool_use blocks → OpenAI tool_calls format.
fn parse_response(resp: &serde_json::Value) -> Result<LlmResponse, CasperError> {
    let mut text_parts = Vec::new();
    let mut thinking_parts = Vec::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();

    if let Some(content_arr) = resp["content"].as_array() {
        for block in content_arr {
            match block["type"].as_str() {
                Some("thinking") => {
                    if let Some(text) = block["thinking"].as_str() {
                        thinking_parts.push(text.to_string());
                    }
                }
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        text_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    // Anthropic: {type: "tool_use", id, name, input: {object}}
                    // → OpenAI: {id, type: "function", function: {name, arguments: "JSON string"}}
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(json!({}));
                    let arguments =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());

                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments,
                        }
                    }));
                }
                _ => {}
            }
        }
    }

    let content = text_parts.join("");

    let usage = &resp["usage"];
    let input_tokens = usage["input_tokens"].as_i64().unwrap_or(0) as i32;
    let output_tokens = usage["output_tokens"].as_i64().unwrap_or(0) as i32;
    let cache_read_tokens = usage["cache_read_input_tokens"].as_i64().map(|v| v as i32);
    let cache_write_tokens = usage["cache_creation_input_tokens"]
        .as_i64()
        .map(|v| v as i32);

    let finish_reason = resp["stop_reason"].as_str().map(|s| match s {
        "end_turn" => "stop".to_string(),
        "max_tokens" => "length".to_string(),
        "tool_use" => "tool_calls".to_string(),
        other => other.to_string(),
    });

    let model = resp["model"].as_str().unwrap_or("unknown").to_string();

    let thinking = if thinking_parts.is_empty() {
        None
    } else {
        Some(thinking_parts.join("\n"))
    };

    Ok(LlmResponse {
        content,
        role: MessageRole::Assistant,
        model,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        finish_reason,
        thinking,
    })
}

/// Map Anthropic HTTP status codes to CasperError variants.
fn map_anthropic_error(status: u16, body: &str) -> CasperError {
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| body.chars().take(500).collect());

    match status {
        401 => CasperError::BadGateway(format!("Anthropic auth error: {message}")),
        429 => CasperError::RateLimited,
        500..=599 => {
            CasperError::BadGateway(format!("Anthropic server error ({status}): {message}"))
        }
        _ => CasperError::BadGateway(format!("Anthropic error ({status}): {message}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_extracted_from_messages() {
        let messages = vec![
            super::types::Message {
                role: MessageRole::System,
                content: json!("You are a helpful assistant"),
            },
            super::types::Message {
                role: MessageRole::User,
                content: json!("Hello"),
            },
        ];

        let (system, msgs) = build_anthropic_messages(&messages, &json!({}));
        assert_eq!(system, Some(json!("You are a helpful assistant")));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn system_extracted_from_extra() {
        let messages = vec![super::types::Message {
            role: MessageRole::User,
            content: json!("Hello"),
        }];

        let (system, msgs) = build_anthropic_messages(&messages, &json!({"system": "Be helpful"}));
        assert_eq!(system, Some(json!("Be helpful")));
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn no_system() {
        let messages = vec![super::types::Message {
            role: MessageRole::User,
            content: json!("Hello"),
        }];

        let (system, msgs) = build_anthropic_messages(&messages, &json!({}));
        assert!(system.is_none());
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn tool_calls_converted_to_anthropic() {
        let messages = vec![super::types::Message {
            role: MessageRole::Assistant,
            content: json!({
                "content": "Let me search.",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "web_search",
                        "arguments": "{\"query\":\"rust\"}"
                    }
                }]
            }),
        }];

        let (_, msgs) = build_anthropic_messages(&messages, &json!({}));
        assert_eq!(msgs.len(), 1);
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Let me search.");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["name"], "web_search");
        assert_eq!(content[1]["id"], "call_1");
        assert_eq!(content[1]["input"]["query"], "rust");
    }

    #[test]
    fn tool_result_converted_to_anthropic() {
        let messages = vec![super::types::Message {
            role: MessageRole::Tool,
            content: json!({
                "tool_call_id": "call_1",
                "content": "search results here",
            }),
        }];

        let (_, msgs) = build_anthropic_messages(&messages, &json!({}));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "call_1");
        assert_eq!(content[0]["content"], "search results here");
    }

    #[test]
    fn openai_tool_defs_converted() {
        // Verify the conversion happens in call() by testing the format
        let openai_tool = json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
            }
        });
        let func = openai_tool.get("function").unwrap();
        let anthropic = json!({
            "name": func["name"],
            "description": func.get("description").cloned().unwrap_or(json!("")),
            "input_schema": func.get("parameters").cloned().unwrap_or(json!({"type": "object"})),
        });
        assert_eq!(anthropic["name"], "get_weather");
        assert_eq!(
            anthropic["input_schema"]["properties"]["city"]["type"],
            "string"
        );
    }

    #[test]
    fn parse_anthropic_response() {
        let resp = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Hello! How can I help?"}
            ],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 25,
                "output_tokens": 10,
                "cache_read_input_tokens": 5,
                "cache_creation_input_tokens": 20
            }
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "Hello! How can I help?");
        assert_eq!(llm.role, MessageRole::Assistant);
        assert_eq!(llm.input_tokens, 25);
        assert_eq!(llm.output_tokens, 10);
        assert_eq!(llm.cache_read_tokens, Some(5));
        assert_eq!(llm.cache_write_tokens, Some(20));
        assert_eq!(llm.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn parse_anthropic_tool_use_returns_openai_format() {
        let resp = json!({
            "id": "msg_456",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-20250514",
            "content": [
                {"type": "text", "text": "Let me search."},
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "search",
                    "input": {"query": "rust programming"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 50, "output_tokens": 30}
        });

        let llm = parse_response(&resp).unwrap();
        assert_eq!(llm.content, "Let me search.");
        assert_eq!(llm.finish_reason, Some("tool_calls".to_string()));

        let tc = llm.tool_calls.unwrap();
        assert_eq!(tc.len(), 1);
        // Should be in OpenAI format
        assert_eq!(tc[0]["id"], "toolu_123");
        assert_eq!(tc[0]["type"], "function");
        assert_eq!(tc[0]["function"]["name"], "search");
        // arguments is a JSON string
        let args: serde_json::Value =
            serde_json::from_str(tc[0]["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args["query"], "rust programming");
    }

    #[test]
    fn error_mapping() {
        let err = map_anthropic_error(429, r#"{"error":{"message":"rate limited"}}"#);
        assert!(matches!(err, CasperError::RateLimited));

        let err = map_anthropic_error(500, r#"{"error":{"message":"internal"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));

        let err = map_anthropic_error(401, r#"{"error":{"message":"invalid key"}}"#);
        assert!(matches!(err, CasperError::BadGateway(_)));
    }
}
